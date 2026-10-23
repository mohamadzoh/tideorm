//! Cache invalidations an open transaction replays when it commits, and the
//! in-memory state it restores if it does not.
//!
//! A write invalidates the cached reads of its table at once, but until the
//! transaction commits, every other connection still reads the rows it
//! replaced. A cached read made meanwhile would serve them for its whole TTL,
//! so each transaction records what its writes invalidated and invalidates it
//! again once it has committed.
//!
//! State kept outside the database, such as an entity manager's ids and clean
//! snapshots, is only true once the transaction that wrote it commits. Such
//! state registers how to undo itself, which runs when the transaction rolls
//! back, fails to commit, or is dropped unfinished.
//!
//! A transaction's record travels with its connection, and its executor marks
//! the failures of the statements it runs there, so a statement on an outer
//! transaction, run from inside another database's, marks the transaction it
//! ran on.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;

use super::QueryCache;

/// What the writes inside one open transaction invalidated, and what to undo
/// if it never commits.
#[derive(Default)]
pub(crate) struct PendingInvalidations {
    /// The record of the transaction a savepoint is open in, which it hands
    /// its rollback work and its aborts to.
    enclosing: Option<Record>,
    tables: HashSet<String>,
    databases: HashSet<Arc<str>>,
    all: bool,
    undo: Vec<Box<dyn FnOnce() + Send>>,
    /// Whether a statement failed inside the transaction, which on PostgreSQL
    /// aborts it.
    statement_failed: bool,
    /// Why the transaction can no longer run statements or commit, if it can't.
    aborted: Option<Abort>,
    /// How many transactions, this one included, are open on the connection.
    depth: usize,
    /// The connection, held by whatever runs on it, shared by every
    /// transaction open on it (see [`Gate`]).
    gate: Gate,
    /// Held by a savepoint open in this transaction until it ends, so another
    /// opened beside it waits (see `Executor::begin`); one opened inside that
    /// savepoint takes the savepoint's own.
    savepoint_slot: Gate,
    /// Savepoints cancelled while something else held the connection, which
    /// could not be rolled back then; dropping them rolls them back, once
    /// the connection is free.
    deferred: Vec<Box<dyn Send>>,
    /// The seed lock survives savepoint completion until the enclosing
    /// transaction commits or rolls back. Its mutex also serializes seed
    /// operations sharing that transaction. The boxed guard releases on drop.
    seed_lock: SeedLock,
}

type SeedLock = Arc<futures_util::lock::Mutex<Option<Box<dyn Send>>>>;

/// A transaction's record, shared by the transaction, its connection's
/// executor and the savepoints opened in it.
pub(crate) type Record = Arc<Mutex<PendingInvalidations>>;

/// A transaction's connection, held by each statement, begin, commit and
/// rollback for as long as it runs there. The driver locks the connection
/// too, and panics when a transaction is dropped while another future holds
/// that lock; with every user waiting here first, the driver's lock is never
/// waited for, so a future cancelled while it waits has started nothing, and
/// one dropped holding the gate releases the driver's lock first.
pub(crate) type Gate = Arc<futures_util::lock::Mutex<()>>;

/// Why an open transaction can no longer run statements or commit: it was
/// rolled back underneath TideORM, and what follows would run in autocommit
/// mode, outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Abort {
    /// A statement deadlocked, and the server rolled the whole transaction back.
    Deadlock,
    /// A failed statement made SQLite roll the whole transaction back.
    SqliteRollback,
    /// A savepoint failed, or was cancelled, while it started, or was
    /// cancelled while it ended: the driver then rolls back at the enclosing
    /// transaction's depth, or no longer knows the savepoint's state.
    Savepoint,
    /// A savepoint was cancelled while another statement of the connection
    /// ran, so it could not be rolled back then; the statements that would
    /// follow would run inside it.
    Cancelled,
    /// A savepoint's handle leaked outside its scope, so it stays open until
    /// the handle drops: the statements that would follow would run inside
    /// it, and a commit would release it in place of the savepoint or
    /// transaction committing.
    Leaked,
}

impl Abort {
    /// What happened to a transaction aborted this way.
    fn reason(self) -> &'static str {
        match self {
            Self::Deadlock => {
                "a statement inside it deadlocked, and the database rolled the whole transaction \
                 back"
            }
            Self::SqliteRollback => {
                "a failed statement made SQLite roll the whole transaction back"
            }
            Self::Savepoint => {
                "a nested transaction failed or was cancelled while it started, or was cancelled \
                 while it ended, which ends the enclosing transaction"
            }
            Self::Cancelled => {
                "a nested transaction was cancelled while another statement of its connection was \
                 running, so it could not be rolled back"
            }
            Self::Leaked => {
                "a nested transaction's handle leaked outside its scope, so its savepoint stays \
                 open until that handle drops"
            }
        }
    }

    /// The error a transaction aborted this way commits with.
    pub(crate) fn commit_error(self) -> String {
        format!(
            "the transaction cannot commit: {}; further TideORM statements on this transaction \
             are blocked",
            self.reason()
        )
    }

    /// The error a statement on a transaction aborted this way fails with.
    pub(crate) fn statement_error(self) -> String {
        format!(
            "the statement was not run: {}; further TideORM statements on this transaction are \
             blocked, so retry the whole transaction",
            self.reason()
        )
    }

    /// Whether the abort ended every transaction open on the connection, so
    /// the enclosing ones learn of it. A cancelled savepoint is rolled back
    /// with the transaction it was open in, which leaves the ones around that
    /// one as they were.
    fn ends_the_connection_transaction(self) -> bool {
        !matches!(self, Self::Cancelled)
    }
}

impl Drop for PendingInvalidations {
    /// A record dropped before [`replay`](Self::replay) belongs to a
    /// transaction that did not commit. An abort ended every enclosing
    /// transaction too, so they learn of it. Savepoints it still holds roll
    /// back now if the connection is free, and otherwise go to the enclosing
    /// transaction, which cannot go on.
    fn drop(&mut self) {
        while let Some(undo) = self.undo.pop() {
            undo();
        }
        if let Some(abort) = self.aborted
            && abort.ends_the_connection_transaction()
            && let Some(enclosing) = &self.enclosing
        {
            enclosing.lock().abort(abort);
        }
        let deferred = std::mem::take(&mut self.deferred);
        if deferred.is_empty() {
            return;
        }
        match &self.enclosing {
            Some(enclosing) => Self::defer_or_drop(enclosing, deferred),
            None => drop(deferred),
        }
    }
}

thread_local! {
    /// The innermost transaction open in the current poll, installed around
    /// each poll by the transaction scope like its connection is.
    static CURRENT: RefCell<Option<Record>> =
        const { RefCell::new(None) };
}

impl PendingInvalidations {
    /// The record of a transaction begun on its own, or, as a savepoint, in
    /// the transaction `enclosing` records.
    pub(crate) fn new(enclosing: Option<Record>) -> Self {
        let (depth, gate, seed_lock) = match &enclosing {
            Some(enclosing) => {
                let enclosing = enclosing.lock();
                (
                    enclosing.depth + 1,
                    enclosing.gate.clone(),
                    enclosing.seed_lock.clone(),
                )
            }
            None => (1, Gate::default(), SeedLock::default()),
        };
        Self {
            enclosing,
            tables: HashSet::new(),
            databases: HashSet::new(),
            all: false,
            undo: Vec::new(),
            statement_failed: false,
            aborted: None,
            depth,
            gate,
            savepoint_slot: Gate::default(),
            deferred: Vec::new(),
            seed_lock,
        }
    }

    pub(crate) fn seed_lock(&self) -> SeedLock {
        self.seed_lock.clone()
    }

    /// The transaction's connection, to hold while something runs on it.
    pub(crate) fn gate(&self) -> Gate {
        self.gate.clone()
    }

    /// What a savepoint opened in this transaction holds until it ends.
    pub(crate) fn savepoint_slot(&self) -> Gate {
        self.savepoint_slot.clone()
    }

    /// Drop `savepoints`, open in the transaction `record` records, now if
    /// its connection is free, which rolls them back; otherwise hold them
    /// until the transaction takes them back with
    /// [`take_deferred`](Self::take_deferred), and refuse to let it go on,
    /// since what it would run next would run inside them.
    pub(crate) fn defer_or_drop(record: &Record, savepoints: Vec<Box<dyn Send>>) {
        let gate = record.lock().gate();
        // The driver's lock is free whenever the gate is.
        let Some(_free) = gate.try_lock() else {
            let mut record = record.lock();
            record.abort(Abort::Cancelled);
            record.deferred.extend(savepoints);
            return;
        };
        drop(savepoints);
    }

    /// The savepoints held for the transaction, to drop while it holds its
    /// connection.
    pub(crate) fn take_deferred(&mut self) -> Vec<Box<dyn Send>> {
        std::mem::take(&mut self.deferred)
    }

    pub(super) fn table(&mut self, table: &str) {
        self.tables.insert(table.to_string());
    }

    pub(super) fn all(&mut self) {
        self.all = true;
    }

    pub(super) fn database(&mut self, database: Arc<str>) {
        self.databases.insert(database);
    }

    /// Whether a statement failed inside the transaction.
    pub(crate) fn statement_failed(&self) -> bool {
        self.statement_failed
    }

    /// Why the transaction, or one it enclosed, was rolled back underneath
    /// TideORM, if it was.
    pub(crate) fn aborted(&self) -> Option<Abort> {
        self.aborted
    }

    /// Mark the transaction aborted, keeping the first reason.
    pub(crate) fn abort(&mut self, abort: Abort) {
        self.aborted.get_or_insert(abort);
    }

    /// Note that a statement failed inside the transaction, and whether it
    /// deadlocked, which ends the whole transaction.
    pub(crate) fn note_failed_statement(&mut self, deadlocked: bool) {
        self.statement_failed = true;
        if deadlocked {
            self.abort(Abort::Deadlock);
        }
    }

    /// Take back an [`Abort::SqliteRollback`] noted while it was being
    /// checked: SQLite kept the transaction open after all.
    pub(crate) fn clear_sqlite_rollback(&mut self) {
        if self.aborted == Some(Abort::SqliteRollback) {
            self.aborted = None;
        }
    }

    /// How many transactions, this one included, are open on its connection.
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    /// Invalidate again what the transaction's writes invalidated, now that it
    /// has committed. Inside an enclosing transaction this records it there
    /// as well, to be replayed when that one commits. Only a savepoint hands
    /// it undo steps: an independent transaction's commit is already final.
    pub(crate) fn replay(mut self) {
        let undo = std::mem::take(&mut self.undo);
        if let Some(enclosing) = &self.enclosing {
            enclosing.lock().undo.extend(undo);
        }
        self.replay_invalidations();
    }

    /// Invalidate again what the transaction's writes invalidated, recording
    /// it in the enclosing transaction, or in none: the transaction around
    /// this scope may be another database's.
    pub(crate) fn replay_invalidations(&self) {
        let _scope = replace(self.enclosing.clone());

        let cache = QueryCache::global();
        if self.all {
            cache.clear();
        } else {
            for database in &self.databases {
                cache.invalidate_database(database.clone());
            }
            for table in &self.tables {
                cache.invalidate_model(table);
            }
        }
    }
}

/// Restores the enclosing transaction's record when dropped.
pub(crate) struct PendingGuard(Option<Record>);

impl Drop for PendingGuard {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

/// Record the current poll's invalidations in `pending`.
pub(crate) fn install(pending: &Record) -> PendingGuard {
    replace(Some(pending.clone()))
}

/// Record the current poll's invalidations in `record`, or, with `None`, in
/// no transaction: statements on a pool run outside every transaction.
pub(crate) fn replace(record: Option<Record>) -> PendingGuard {
    PendingGuard(CURRENT.with(|slot| slot.replace(record)))
}

/// Run `undo` if the innermost open transaction does not commit. Outside a
/// transaction nothing can roll back, and `undo` is dropped unrun.
#[cfg(any(feature = "entity-manager", feature = "dirty-tracking"))]
pub(crate) fn undo_on_rollback(undo: impl FnOnce() + Send + 'static) {
    record(|pending| pending.undo.push(Box::new(undo)));
}

/// Note an invalidation against the innermost open transaction, if any.
pub(super) fn record(note: impl FnOnce(&mut PendingInvalidations)) {
    CURRENT.with(|slot| {
        if let Some(pending) = slot.borrow().as_ref() {
            note(&mut pending.lock());
        }
    });
}

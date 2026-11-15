//! Savepoints: a statement on the enclosing transaction while one is open,
//! savepoints cancelled, leaked or opened side by side, and one opened inside
//! another database's transaction.

use crate::fixtures::{file_database, memory_database, slow_count};
use tideorm::prelude::*;

/// A statement on a transaction while a savepoint is open in it waits for the
/// savepoint to end. It used to run inside the savepoint, and was rolled back
/// with it though the transaction it ran on committed.
#[tokio::test]
async fn a_statement_on_the_transaction_waits_for_its_open_savepoint() {
    let db = memory_database("CREATE TABLE parent_rows(id INTEGER PRIMARY KEY)").await;
    let handle = db.clone();
    db.transaction(async move |_| {
        let inner = handle.clone();
        let (ready, open) = tokio::sync::oneshot::channel();
        let nested = handle.transaction(async move |_| {
            inner.exec_raw("INSERT INTO parent_rows VALUES (1)").await?;
            ready.send(()).unwrap();
            // The biased join polls the parent's statement before this
            // resumes, so the savepoint is still open when it is sent.
            tokio::task::yield_now().await;
            Err::<(), _>(tideorm::Error::query("roll the savepoint back"))
        });
        let parent = async {
            open.await.unwrap();
            handle.exec_raw("INSERT INTO parent_rows VALUES (2)").await
        };
        let (nested, parent) = tokio::join!(biased; nested, parent);
        assert!(nested.is_err(), "the savepoint should have failed");
        parent?;
        Ok(())
    })
    .await
    .unwrap();
    let rows = db
        .query_raw_json("SELECT id FROM parent_rows ORDER BY id")
        .await
        .unwrap();
    let ids: Vec<i64> = rows.iter().map(|row| row["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, [2]);
}

/// Once a savepoint has finished its statement and is idle, cancelling its
/// closure rolls back only its own work. Wait for that boundary explicitly:
/// cancelling after an arbitrary poll can instead hit begin or commit, whose
/// outcomes the other cancellation tests cover.
#[tokio::test]
async fn a_cancelled_idle_savepoint_rolls_back_only_its_own_work() {
    let db = memory_database("CREATE TABLE cancel_rows(id INTEGER PRIMARY KEY)").await;
    let handle = db.clone();
    db.transaction(async move |_| {
        handle.exec_raw("INSERT INTO cancel_rows VALUES (1)").await?;
        let inner_handle = handle.clone();
        let (ready, idle) = tokio::sync::oneshot::channel();
        let mut inner = Box::pin(handle.transaction(async move |_| {
            inner_handle
                .exec_raw("INSERT INTO cancel_rows VALUES (2)")
                .await?;
            ready.send(()).unwrap();
            std::future::pending::<tideorm::Result<()>>().await
        }));
        tokio::select! {
            outcome = &mut inner => panic!("the nested transaction ended before cancellation: {outcome:?}"),
            ready = idle => ready.unwrap(),
        }
        drop(inner);
        handle.exec_raw("INSERT INTO cancel_rows VALUES (3)").await?;
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(
        db.query_raw_json("SELECT id FROM cancel_rows ORDER BY id")
            .await
            .unwrap(),
        vec![json!({"id": 1}), json!({"id": 3})]
    );
}

/// A savepoint of A's transaction opened inside B's belongs to A: a handle on
/// B used inside it runs on B's transaction, not on A's savepoint.
#[tokio::test]
async fn a_savepoint_opened_inside_another_databases_transaction_keeps_its_database() {
    let (a, _a_file) = file_database("savepoint-outer").await;
    let (b, _b_file) = file_database("savepoint-inner").await;
    b.exec_raw("CREATE TABLE inner_rows(id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    let (a_handle, b_handle) = (a.clone(), b.clone());
    a.transaction(async move |_| {
        let (a_inner, b_inner) = (a_handle.clone(), b_handle.clone());
        b_handle
            .transaction(async move |_| {
                a_inner
                    .transaction(async move |_| {
                        b_inner.exec_raw("INSERT INTO inner_rows VALUES (1)").await
                    })
                    .await
            })
            .await
    })
    .await
    .unwrap();
    let rows = b.query_raw_json("SELECT id FROM inner_rows").await.unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    drop((a, b));
}

/// A nested transaction cancelled while another statement of its connection
/// runs is rolled back once that statement ends, before anything else runs
/// there, and the enclosing transaction goes on. The driver used to panic
/// trying to roll it back at once.
#[tokio::test]
async fn a_nested_transaction_cancelled_mid_statement_rolls_back_after_it() {
    use std::time::Duration;
    let (db, _file) = file_database("cancelled-mid-statement").await;
    db.exec_raw("CREATE TABLE mid_rows(id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    let handle = db.clone();
    db.transaction(async move |_| {
        handle.exec_raw("INSERT INTO mid_rows VALUES (1)").await?;
        let inner = handle.clone();
        let nested = tokio::time::timeout(
            Duration::from_millis(150),
            handle.transaction(async move |_| {
                inner.exec_raw("INSERT INTO mid_rows VALUES (2)").await?;
                tokio::time::sleep(Duration::from_secs(10)).await;
                Ok(())
            }),
        );
        let slow = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            handle
                .query_raw_json(&format!("SELECT {} AS n", slow_count(20_000_000)))
                .await
        };
        let (nested, slow) = tokio::join!(nested, slow);
        assert!(nested.is_err(), "the nested transaction was not cancelled");
        slow?;
        handle.exec_raw("INSERT INTO mid_rows VALUES (3)").await?;
        Ok(())
    })
    .await
    .unwrap();
    let rows = db
        .query_raw_json("SELECT id FROM mid_rows ORDER BY id")
        .await
        .unwrap();
    let ids: Vec<i64> = rows.iter().map(|row| row["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, [1, 3]);
    drop(db);
}

/// A nested transaction whose handle leaked outside its closure is rolled
/// back, and the enclosing transaction refuses to go on. It used to release
/// the open savepoint as if it were its own and report a commit, while the
/// transaction stayed open until the stray handle dropped and rolled
/// everything back.
#[tokio::test]
async fn a_leaked_savepoint_aborts_the_enclosing_transaction() {
    let db = memory_database("CREATE TABLE leak_rows(id INTEGER PRIMARY KEY)").await;
    let stray = std::sync::Arc::new(std::sync::Mutex::new(None));
    let (handle, keep) = (db.clone(), stray.clone());
    let result: tideorm::Result<()> = db
        .transaction(async move |_| {
            handle.exec_raw("INSERT INTO leak_rows VALUES (1)").await?;
            let leaked = handle
                .transaction(async move |_| {
                    *keep.lock().unwrap() = Some(tideorm::database::__current_db()?);
                    Ok(())
                })
                .await;
            assert!(leaked.is_err(), "the leak was not reported");
            assert!(
                handle
                    .exec_raw("INSERT INTO leak_rows VALUES (2)")
                    .await
                    .is_err()
            );
            Ok(())
        })
        .await;
    let error = result.unwrap_err().to_string();
    assert!(error.contains("leaked"), "{error}");
    drop(stray.lock().unwrap().take());
    let rows = db.query_raw_json("SELECT id FROM leak_rows").await.unwrap();
    assert!(rows.is_empty(), "{rows:?}");
}

/// A nested transaction or a batch insert cancelled while it waits for the
/// connection another statement holds never started, so nothing panics and
/// the enclosing transaction commits what it wrote.
#[tokio::test]
async fn a_savepoint_cancelled_while_it_waits_for_the_connection_never_starts() {
    use std::time::Duration;

    #[tideorm::model(table = "waiting_rows")]
    struct WaitingRow {
        #[tideorm(primary_key)]
        id: i64,
    }

    for batch in [false, true] {
        let (db, _file) = file_database("cancelled-while-waiting").await;
        db.exec_raw("CREATE TABLE waiting_rows(id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        let handle = db.clone();
        let result: tideorm::Result<()> = db
            .transaction(async move |_| {
                handle
                    .exec_raw("INSERT INTO waiting_rows VALUES (1)")
                    .await?;
                let slow_sql = format!("SELECT {} AS n", slow_count(20_000_000));
                let slow = handle.query_raw_json(&slow_sql);
                let waiting = async {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    let savepoint = async {
                        if batch {
                            WaitingRow::insert_all(vec![WaitingRow { id: 2 }, WaitingRow { id: 3 }])
                                .await
                                .map(drop)
                        } else {
                            handle.transaction(async |_| Ok(())).await
                        }
                    };
                    tokio::time::timeout(Duration::from_millis(100), savepoint).await
                };
                let (slow, waiting) = tokio::join!(slow, waiting);
                assert!(waiting.is_err(), "the savepoint did not wait");
                slow?;
                Ok(())
            })
            .await;
        result.unwrap();
        let rows = db
            .query_raw_json("SELECT id FROM waiting_rows")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        drop(db);
    }
}

/// Two savepoints opened side by side in one transaction take turns. The
/// driver names a savepoint by how many are open on its connection, so one that
/// ended while a later one was still open ended that one in its place, and
/// the later one's rollback then undid the rows the first had committed.
#[tokio::test]
async fn side_by_side_savepoints_in_one_transaction_take_turns() {
    use std::time::Duration;

    #[tideorm::model(table = "side_by_side_rows")]
    struct SideRow {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        email: String,
    }
    let row = |email: &str| SideRow {
        id: 0,
        email: email.to_string(),
    };

    let db = memory_database(
        "CREATE TABLE side_by_side_rows(id INTEGER PRIMARY KEY AUTOINCREMENT, \
         email TEXT NOT NULL UNIQUE)",
    )
    .await;
    let handle = db.clone();
    let result: tideorm::Result<()> = db
        .transaction(async move |_| {
            // The first opens, writes, and waits while the second opens;
            // it commits while the second, which then fails, is open.
            let first = handle.transaction(async move |_| {
                SideRow::create(row("a@x")).await?;
                tokio::time::sleep(Duration::from_millis(20)).await;
                SideRow::create(row("b@x")).await.map(drop)
            });
            let second = async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                handle
                    .transaction(async move |_| {
                        SideRow::create(row("c@x")).await?;
                        tokio::time::sleep(Duration::from_millis(40)).await;
                        SideRow::create(row("c@x")).await.map(drop)
                    })
                    .await
            };
            let (written, refused) = tokio::join!(first, second);
            written?;
            assert!(refused.is_err(), "the duplicate was written");
            Ok(())
        })
        .await;
    result.unwrap();
    let mut emails: Vec<String> = db
        .query_raw_json("SELECT email FROM side_by_side_rows")
        .await
        .unwrap()
        .iter()
        .map(|row| row["email"].as_str().unwrap().to_string())
        .collect();
    emails.sort();
    assert_eq!(emails, ["a@x", "b@x"]);
}

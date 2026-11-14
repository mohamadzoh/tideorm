//! The rules that read several models at once: how deep owned relations nest,
//! whether they cycle, and whether every foreign key is indexed. Each model's
//! derive writes a [`ModelNode`] and the `const` items calling the checks
//! here and panicking with the message they return, so a broken rule fails
//! `cargo check`.

use super::message::Message;

/// A model as the relation rules read it at compile time. Each model's
/// derive writes one in a `static`, which its `__rules_node()` returns.
#[doc(hidden)]
pub struct ModelNode {
    /// Tells the model apart from another of the same name: [`__model_id`]
    /// of where it is declared.
    pub id: u64,
    /// The model's type, without its module path.
    pub name: &'static str,
    /// Its table, `schema.table` when it names a schema.
    pub table: &'static str,
    /// The models whose rows it owns: the related models of its `has_one`,
    /// `has_many`, `morph_one` and `morph_many`.
    pub owned: &'static [&'static ModelNode],
    /// The columns its primary key and its indexes lead with, each also under
    /// its field's name, which a relation's `foreign_key` may name.
    pub leading_columns: &'static [&'static str],
    /// Whether its project's rules require every foreign key it holds to lead
    /// an index.
    pub indexes_foreign_keys: bool,
}

/// The id of the model declared at `place`: its FNV-1a hash.
#[doc(hidden)]
pub const fn __model_id(place: &str) -> u64 {
    let bytes = place.as_bytes();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut at = 0;
    while at < bytes.len() {
        hash = (hash ^ bytes[at] as u64).wrapping_mul(0x0000_0100_0000_01b3);
        at += 1;
    }
    hash
}

/// What is broken when `model`'s owned relations lead back to it.
#[doc(hidden)]
pub const fn __check_relation_cycles(model: &'static ModelNode) -> Option<Message> {
    let mut walk = Walk::new();
    if !walk.run(model, true) {
        return None;
    }
    let mut message = Message::new();
    message
        .push(model.name)
        .push("'s relations lead back to it: ");
    walk.push_chain(&mut message);
    message.push(" → ").push(model.name);
    Some(*message.rule("forbid_relation_cycles"))
}

/// What is broken when `model`'s owned relations nest deeper than `max`.
#[doc(hidden)]
pub const fn __check_relation_depth(model: &'static ModelNode, max: usize) -> Option<Message> {
    let mut walk = Walk::new();
    walk.run(model, false);
    let mut message = Message::new();
    message.push(model.name);
    if walk.endless[0] {
        message.push("'s relations reach a cycle, so they nest without end");
    } else if walk.depth[0] > max {
        message
            .push("'s relations nest ")
            .number(walk.depth[0])
            .push(" deep (");
        walk.push_deepest(&mut message);
        message.push("), over the limit of ").number(max);
    } else {
        return None;
    }
    Some(*message.rule("max_relation_depth"))
}

/// What is broken when `column`, a foreign key `holder` holds, leads none of
/// its indexes while its project requires it to.
#[doc(hidden)]
pub const fn __check_foreign_key(holder: &'static ModelNode, column: &str) -> Option<Message> {
    if !holder.indexes_foreign_keys {
        return None;
    }
    let mut at = 0;
    while at < holder.leading_columns.len() {
        if crate::model::__str_eq(holder.leading_columns[at], column) {
            return None;
        }
        at += 1;
    }
    let mut message = Message::new();
    message
        .push(holder.name)
        .push("'s foreign key `")
        .push(column)
        .push("` leads no index");
    Some(*message.rule("require_foreign_key_index"))
}

// shortcut: a walk reaches at most this many models, failing the build past
// it; raise it if a project's relations ever reach more.
const MAX_MODELS: usize = 1024;
const SLOTS: usize = 2 * MAX_MODELS;
const EMPTY: usize = usize::MAX;
const NEW: u8 = 0;
const ON_PATH: u8 = 1;
const DONE: u8 = 2;

/// A depth-first walk of the owned relations from one model, which is the
/// walk's model 0. A model is numbered as the walk reaches it.
struct Walk {
    models: [Option<&'static ModelNode>; MAX_MODELS],
    count: usize,
    /// Model numbers by id, open addressing.
    slots: [usize; SLOTS],
    state: [u8; MAX_MODELS],
    /// The longest owned chain below each model, and the next model on it.
    depth: [usize; MAX_MODELS],
    next: [usize; MAX_MODELS],
    /// Whether a cycle is reachable from each model, which makes it endless.
    endless: [bool; MAX_MODELS],
    /// The path from model 0: each model and the next relation to follow.
    path: [(usize, usize); MAX_MODELS],
    length: usize,
}

impl Walk {
    const fn new() -> Self {
        Self {
            models: [None; MAX_MODELS],
            count: 0,
            slots: [EMPTY; SLOTS],
            state: [NEW; MAX_MODELS],
            depth: [0; MAX_MODELS],
            next: [EMPTY; MAX_MODELS],
            endless: [false; MAX_MODELS],
            path: [(0, 0); MAX_MODELS],
            length: 0,
        }
    }

    const fn model(&self, number: usize) -> &'static ModelNode {
        match self.models[number] {
            Some(model) => model,
            None => panic!("every numbered model is held"),
        }
    }

    /// `model`'s number, numbering it when the walk first reaches it.
    const fn number(&mut self, model: &'static ModelNode) -> usize {
        let mut slot = (model.id % SLOTS as u64) as usize;
        loop {
            let number = self.slots[slot];
            if number == EMPTY {
                break;
            }
            if self.model(number).id == model.id {
                return number;
            }
            slot = (slot + 1) % SLOTS;
        }
        if self.count == MAX_MODELS {
            panic!("the project rules reach more than 1024 models through owned relations");
        }
        let number = self.count;
        self.count += 1;
        self.models[number] = Some(model);
        self.slots[slot] = number;
        number
    }

    /// Walk every model `start` owns, directly or not. With `to_start`, stop
    /// at the first relation back to `start`, the path then leading to it,
    /// and say whether there was one.
    const fn run(&mut self, start: &'static ModelNode, to_start: bool) -> bool {
        let first = self.number(start);
        self.state[first] = ON_PATH;
        self.path[0] = (first, 0);
        self.length = 1;
        while self.length > 0 {
            let (at, relation) = self.path[self.length - 1];
            let model = self.model(at);
            if relation == model.owned.len() {
                self.length -= 1;
                self.state[at] = DONE;
                if self.length > 0 {
                    self.reach(self.path[self.length - 1].0, at);
                }
                continue;
            }
            self.path[self.length - 1].1 = relation + 1;
            let owned = model.owned[relation];
            // A relation to the model's own table (a tree's children) nests
            // data, not the schema.
            if crate::model::__str_eq(owned.table, model.table) {
                continue;
            }
            let next = self.number(owned);
            match self.state[next] {
                NEW => {
                    self.state[next] = ON_PATH;
                    self.path[self.length] = (next, 0);
                    self.length += 1;
                }
                ON_PATH => {
                    self.endless[at] = true;
                    if to_start && next == first {
                        return true;
                    }
                }
                _ => self.reach(at, next),
            }
        }
        false
    }

    /// Count `to`, which `from` owns, in `from`'s depth.
    const fn reach(&mut self, from: usize, to: usize) {
        if self.endless[to] {
            self.endless[from] = true;
        } else if self.depth[to] + 1 > self.depth[from] {
            self.depth[from] = self.depth[to] + 1;
            self.next[from] = to;
        }
    }

    /// The names on the path, `A → B → C`.
    const fn push_chain(&self, message: &mut Message) {
        let mut at = 0;
        while at < self.length {
            if at > 0 {
                message.push(" → ");
            }
            message.push(self.model(self.path[at].0).name);
            at += 1;
        }
    }

    /// The names on the deepest chain from model 0, `A → B → C`.
    const fn push_deepest(&self, message: &mut Message) {
        let mut at = 0;
        message.push(self.model(at).name);
        while self.next[at] != EMPTY {
            at = self.next[at];
            message.push(" → ").push(self.model(at).name);
        }
    }
}

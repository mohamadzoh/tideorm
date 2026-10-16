//! Bulk `update_all()` assignments, arithmetic and JSON setters: an invalid value
//! fails before any row changes, and every row stays readable by its model.

#[path = "batch_updates/assignments.rs"]
mod assignments;

#[path = "batch_updates/arithmetic.rs"]
mod arithmetic;

#[path = "batch_updates/json.rs"]
mod json;

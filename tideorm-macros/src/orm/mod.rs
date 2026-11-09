//! Expansion code of the `tideorm::orm` derives, covering
//! TideORM's feature set (with this crate's `postgres` feature, PostgreSQL
//! arrays). The `#[proc_macro*]` entry points are at the crate root.

pub(crate) mod derives;
pub(crate) mod enum_iter;
pub(crate) mod from_query_result;

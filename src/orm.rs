//! Hidden TideORM ORM facade.
//!
//! Macro-generated code should target this module rather than binding directly
//! to engine-specific paths. That gives TideORM one public compatibility seam for the
//! current ORM engine while the runtime is decoupled piece by piece.

#![allow(unused_imports)]

pub use sea_orm::*;
pub use sea_orm::{DbBackend as OrmBackend, DbErr as OrmError};
pub use sea_orm::{entity, sea_query, sqlx};

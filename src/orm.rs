//! Hidden TideORM ORM facade.
//!
//! Macro-generated code should target this module rather than binding directly
//! to engine-specific paths. That gives TideORM one public compatibility seam for the
//! current ORM engine: the in-repository `tideorm-engine` crate, plus the derive macros
//! from `tideorm-macros`. The derives read `#[orm(...)]` attributes and their
//! expansions name `::tideorm::orm::` paths.

pub use tideorm_engine::DbErr as OrmError;
pub use tideorm_engine::*;

macro_rules! export_derives {
    () => {
        pub use tideorm_macros::{DeriveActiveEnum, DeriveValueType, EnumIter};
    };
}

export_derives!();

pub use entity::prelude;

/// Entities and models, with the derive macros in the prelude.
pub mod entity {
    pub use tideorm_engine::entity::*;

    /// Everything an entity definition names, derive macros included.
    pub mod prelude {
        pub use tideorm_engine::entity::prelude::*;
        export_derives!();
    }
}

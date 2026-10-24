//! `#[tideorm::scopes]` carries each `cfg` and `cfg_attr(.., cfg(..))` of a
//! scope method, parameter or impl block onto the code it generates for it.
//! Dropped, the generated code would call a method or forward an argument
//! that is configured out, or name a type that does not exist, so this file
//! compiles only if every one is carried.

use tideorm::prelude::*;

#[tideorm::model(table = "conditional_scope_models")]
struct ScopeModel {
    #[tideorm(primary_key)]
    id: i64,
}

#[tideorm::scopes]
impl ScopeModel {
    #[cfg(any())]
    pub fn disabled(query: QueryBuilder<Self>, value: UnavailableType) -> QueryBuilder<Self> {
        query
    }

    #[cfg_attr(all(), cfg(any()))]
    pub fn conditionally_disabled(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query
    }

    #[cfg_attr(all(), cfg_attr(all(), cfg(any()), inline))]
    pub fn nested_disabled(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query
    }

    #[cfg(test)]
    #[cfg_attr(all(), inline)]
    pub fn enabled(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query.where_eq(Self::ID, 7)
    }

    pub fn optional_parameter(
        query: QueryBuilder<Self>,
        #[cfg(any())] unavailable: UnavailableType,
        #[cfg_attr(all(), cfg(any()))] also_unavailable: UnavailableType,
    ) -> QueryBuilder<Self> {
        query
    }
}

#[test]
fn conditional_scopes_keep_enabled_methods_and_remove_disabled_parameters() {
    let _: QueryBuilder<ScopeModel> = ScopeModel::query().enabled().optional_parameter();
}

#[tideorm::scopes]
#[cfg(any())]
impl UnavailableModel {
    pub fn disabled_impl(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query
    }
}

#[tideorm::scopes]
#[cfg_attr(all(), cfg_attr(all(), cfg(any())))]
impl AnotherUnavailableModel {
    pub fn nested_disabled_impl(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query
    }
}

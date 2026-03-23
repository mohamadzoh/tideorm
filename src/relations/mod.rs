//! Model Relations System
//!
//! This module provides model relations using SeaORM-style field declarations.
//! Relations are defined as struct fields with attributes, following SeaORM's pattern.
//!
//! ## Supported Relations
//!
//! - `HasOne<E>`: One-to-one relation (e.g., User has_one Profile)
//! - `HasMany<E>`: One-to-many relation (e.g., User has_many Posts)
//! - `BelongsTo<E>`: Inverse of HasOne/HasMany (e.g., Post belongs_to User)
//!
//! ## Defining Relations Inside Models
//!
//! Relations are declared as fields in your model struct using `#[tideorm(...)]` relation attributes:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! #[tideorm::model(table = "users")]
//! #[derive(Clone, Debug, Serialize, Deserialize)]
//! pub struct User {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub name: String,
//!     pub email: String,
//!
//!     // Relations defined as fields
//!     #[tideorm(has_one = "Profile", foreign_key = "user_id")]
//!     pub profile: HasOne<Profile>,
//!
//!     #[tideorm(has_many = "Post", foreign_key = "user_id")]
//!     pub posts: HasMany<Post>,
//! }
//!
//! #[tideorm::model(table = "posts")]
//! pub struct Post {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub user_id: i64,
//!     pub title: String,
//!
//!     #[tideorm(belongs_to = "User", foreign_key = "user_id")]
//!     pub author: BelongsTo<User>,
//! }
//!
//! #[tideorm::model(table = "profiles")]
//! pub struct Profile {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub user_id: i64,
//!     pub bio: String,
//!
//!     #[tideorm(belongs_to = "User", foreign_key = "user_id")]
//!     pub user: BelongsTo<User>,
//! }
//!
//! // Loading relations:
//! let user = User::find(1).await?;
//! let posts = user.posts.load().await?;
//! let profile = user.profile.load().await?;
//!
//! let post = Post::find(1).await?;
//! let author = post.author.load().await?;
//! ```
//!
//! ## Many-to-Many Relations
//!
//! ```rust,ignore
//! #[tideorm::model(table = "users")]
//! pub struct User {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub name: String,
//!
//!     #[tideorm(has_many_through = "Role", pivot = "user_roles", foreign_key = "user_id", related_key = "role_id")]
//!     pub roles: HasManyThrough<Role, UserRole>,
//! }
//!
//! // Load roles for a user
//! let roles = user.roles.load().await?;
//! ```
//!
//! ## Relation Constraints
//!
//! You can add constraints to relation queries:
//!
//! ```rust,ignore
//! // Load only published posts
//! let published_posts = user.posts.load_with(|query| {
//!     query
//!         .where_eq("published", true)
//!         .order_by("created_at", Order::Desc)
//!         .limit(10)
//! }).await?;
//! ```

#[allow(missing_docs)]
mod direct;
#[allow(missing_docs)]
mod eager;
mod helpers;
#[allow(missing_docs)]
mod many_to_many;
#[allow(missing_docs)]
mod metadata;
#[allow(missing_docs)]
mod polymorphic;
#[allow(missing_docs)]
mod self_referencing;

#[cfg(test)]
pub(crate) use crate::internal::Value;
#[cfg(test)]
pub(crate) use helpers::build_self_ref_tree_sql;

pub use direct::{BelongsTo, HasMany, HasOne};
pub use eager::{
    EagerLoadExt, EagerLoadModel, EagerQueryBuilder, RelationConstraints, RelationExt,
    RelationLoader, RelationPath, RelationTree, WithRelations,
};
pub use many_to_many::{HasManyThrough, WithPivot};
pub use metadata::{RelationInfo, RelationType};
pub use polymorphic::{MorphMany, MorphOne, MorphResult, MorphResult3, MorphResult4, MorphTo};
pub use self_referencing::{SelfRef, SelfRefMany};

#[cfg(test)]
#[path = "../testing/relations_tests.rs"]
mod tests;

pub(crate) fn require_scalar_relation_key<'a>(
    value: &'a serde_json::Value,
    context: &str,
) -> crate::error::Result<&'a serde_json::Value> {
    if value.is_array() || value.is_object() {
        return Err(crate::error::Error::invalid_query(format!(
            "{} only supports scalar relation keys; composite primary keys require an explicit single-column relation key or a custom query",
            context
        )));
    }

    Ok(value)
}

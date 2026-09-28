//! Entity-manager scenarios every backend target runs.
//!
//! The including target supplies `mod backend` with
//! `async fn connect() -> tideorm::Result<Option<Arc<Database>>>`: it installs the
//! global database and returns `None` when the backend is switched off. Tables
//! are built with the migration `Schema`, so one DDL definition serves all three
//! backends.

use std::sync::Arc;

use tideorm::Database;
use tideorm::migration::Schema;
use tideorm::prelude::*;

use super::backend;

#[tideorm::model(table = "entity_manager_test_users")]
struct EntityManagerUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_many = "EntityManagerPost", foreign_key = "user_id")]
    posts: HasMany<EntityManagerPost>,
}

#[tideorm::model(table = "entity_manager_test_posts")]
struct EntityManagerPost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,
}

#[tideorm::model(table = "entity_manager_code_users")]
struct EntityManagerCodeUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    code: String,
    name: String,

    #[tideorm(
        has_many = "EntityManagerCodePost",
        foreign_key = "user_code",
        local_key = "code"
    )]
    posts: HasMany<EntityManagerCodePost>,
}

#[tideorm::model(table = "entity_manager_code_posts")]
struct EntityManagerCodePost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_code: String,
    title: String,
}

#[tideorm::model(table = "entity_manager_slug_users")]
struct EntityManagerSlugUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_many = "EntityManagerSlugPost", foreign_key = "user_id")]
    posts: HasMany<EntityManagerSlugPost>,
}

#[tideorm::model(table = "entity_manager_slug_posts")]
struct EntityManagerSlugPost {
    #[tideorm(primary_key)]
    slug: String,
    user_id: i64,
    title: String,
}

#[tideorm::model(table = "entity_manager_api_keys")]
struct EntityManagerApiKey {
    #[tideorm(primary_key)]
    key: String,
    label: String,
    active: bool,
}

#[tideorm::model(table = "entity_manager_team_memberships")]
struct EntityManagerTeamMembership {
    #[tideorm(primary_key)]
    team_id: i64,
    #[tideorm(primary_key)]
    member_id: i64,
    role: String,
}

#[tideorm::model(table = "entity_manager_composite_users")]
struct EntityManagerCompositeUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_many = "EntityManagerCompositePost", foreign_key = "user_id")]
    posts: HasMany<EntityManagerCompositePost>,
}

#[tideorm::model(table = "entity_manager_composite_posts")]
struct EntityManagerCompositePost {
    #[tideorm(primary_key)]
    user_id: i64,
    #[tideorm(primary_key)]
    slug: String,
    title: String,
}

#[tideorm::model(table = "entity_manager_aggregate_users")]
struct EntityManagerAggregateUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_one = "EntityManagerAggregateProfile", foreign_key = "user_id")]
    profile: HasOne<EntityManagerAggregateProfile>,

    #[tideorm(has_many = "EntityManagerAggregatePost", foreign_key = "user_id")]
    posts: HasMany<EntityManagerAggregatePost>,
}

#[tideorm::model(table = "entity_manager_aggregate_profiles")]
struct EntityManagerAggregateProfile {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    bio: String,
}

#[tideorm::model(table = "entity_manager_aggregate_posts")]
struct EntityManagerAggregatePost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,

    #[tideorm(belongs_to = "EntityManagerAggregateUser", foreign_key = "user_id")]
    author: BelongsTo<EntityManagerAggregateUser>,

    #[tideorm(
        has_many_through = "EntityManagerAggregateTag",
        pivot = "entity_manager_aggregate_post_tags",
        foreign_key = "post_id",
        related_key = "tag_id"
    )]
    tags: HasManyThrough<EntityManagerAggregateTag, EntityManagerAggregatePostTag>,
}

#[tideorm::model(table = "entity_manager_aggregate_tags")]
struct EntityManagerAggregateTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "entity_manager_aggregate_post_tags")]
struct EntityManagerAggregatePostTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    post_id: i64,
    tag_id: i64,
}

/// Raised by [`EntityManagerFlaggedUser`]'s callback once the flush reaches a
/// user named "second", which is where the cancellation scenario drops it.
static SECOND_SAVE_STARTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[tideorm::model(table = "entity_manager_test_users")]
struct EntityManagerFlaggedUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

impl Callbacks for EntityManagerFlaggedUser {
    fn before_save(&mut self) -> tideorm::Result<()> {
        if self.name == "second" {
            SECOND_SAVE_STARTED.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
}

const TABLES: [&str; 15] = [
    "entity_manager_test_users",
    "entity_manager_test_posts",
    "entity_manager_code_users",
    "entity_manager_code_posts",
    "entity_manager_slug_users",
    "entity_manager_slug_posts",
    "entity_manager_api_keys",
    "entity_manager_team_memberships",
    "entity_manager_composite_users",
    "entity_manager_composite_posts",
    "entity_manager_aggregate_users",
    "entity_manager_aggregate_posts",
    "entity_manager_aggregate_profiles",
    "entity_manager_aggregate_tags",
    "entity_manager_aggregate_post_tags",
];

/// Connect the backend and recreate every table, so each test starts empty.
/// `None` means the backend is switched off and the test should pass vacuously.
async fn setup_database() -> tideorm::Result<Option<Arc<Database>>> {
    let Some(db) = backend::connect().await? else {
        return Ok(None);
    };

    let mut schema = Schema::new(db.backend());
    for table in TABLES {
        schema.drop_table_if_exists(table).await?;
    }

    for table in [
        "entity_manager_test_users",
        "entity_manager_slug_users",
        "entity_manager_composite_users",
        "entity_manager_aggregate_users",
        "entity_manager_aggregate_tags",
    ] {
        schema
            .create_table(table, |t| {
                t.id();
                t.string("name").not_null();
            })
            .await?;
    }
    for table in [
        "entity_manager_test_posts",
        "entity_manager_aggregate_posts",
    ] {
        schema
            .create_table(table, |t| {
                t.id();
                t.big_integer("user_id").not_null();
                t.string("title").not_null();
            })
            .await?;
    }
    schema
        .create_table("entity_manager_code_users", |t| {
            t.id();
            t.string("code").not_null().unique();
            t.string("name").not_null();
        })
        .await?;
    schema
        .create_table("entity_manager_code_posts", |t| {
            t.id();
            t.string("user_code").not_null();
            t.string("title").not_null();
        })
        .await?;
    schema
        .create_table("entity_manager_slug_posts", |t| {
            t.string("slug").primary_key();
            t.big_integer("user_id").not_null();
            t.string("title").not_null();
        })
        .await?;
    schema
        .create_table("entity_manager_api_keys", |t| {
            t.string("key").primary_key();
            t.string("label").not_null();
            t.boolean("active").not_null();
        })
        .await?;
    schema
        .create_table("entity_manager_team_memberships", |t| {
            t.big_integer("team_id").not_null();
            t.big_integer("member_id").not_null();
            t.string("role").not_null();
            t.primary_key(&["team_id", "member_id"]);
        })
        .await?;
    schema
        .create_table("entity_manager_composite_posts", |t| {
            t.big_integer("user_id").not_null();
            t.string("slug").not_null();
            t.string("title").not_null();
            t.primary_key(&["user_id", "slug"]);
        })
        .await?;
    schema
        .create_table("entity_manager_aggregate_profiles", |t| {
            t.id();
            t.big_integer("user_id").not_null().unique();
            t.string("bio").not_null();
        })
        .await?;
    schema
        .create_table("entity_manager_aggregate_post_tags", |t| {
            t.id();
            t.big_integer("post_id").not_null();
            t.big_integer("tag_id").not_null();
        })
        .await?;

    Ok(Some(db))
}

async fn seed_user_with_posts(
    count: usize,
) -> tideorm::Result<(EntityManagerUser, Vec<EntityManagerPost>)> {
    let user = EntityManagerUser {
        id: 0,
        name: "Alice".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let mut posts = Vec::with_capacity(count);
    for index in 0..count {
        let post = EntityManagerPost {
            id: 0,
            user_id: user.id,
            title: format!("post-{index}"),
        }
        .save()
        .await?;
        posts.push(post);
    }

    Ok((user, posts))
}

/// Run `work` with the global profiler counting, and return its output with
/// the number of statements it sent.
async fn count_queries<T>(
    work: impl std::future::Future<Output = tideorm::Result<T>>,
) -> tideorm::Result<(T, u64)> {
    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);
    let output = work.await;
    let queries = GlobalProfiler::stats().total_queries;
    GlobalProfiler::disable();
    GlobalProfiler::reset();
    Ok((output?, queries))
}

#[tokio::test]
async fn tracked_deletion_emits_delete() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (saved_user, posts) = seed_user_with_posts(3).await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerUser>(saved_user.id)
        .await?
        .expect("entity_manager user should exist");
    entity_manager.load(&mut user.posts).await?;

    let removed_id = posts[1].id;
    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .retain(|post| post.id != removed_id);

    entity_manager.save(&user).await?;

    let remaining = EntityManagerPost::query_with(db.as_ref())
        .where_eq("user_id", user.id)
        .count()
        .await?;
    assert_eq!(remaining, 2);
    assert!(
        EntityManagerPost::find_with(removed_id, db.as_ref())
            .await?
            .is_none()
    );

    Ok(())
}

#[tokio::test]
async fn identity_map_no_duplicate_queries() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (saved_user, _) = seed_user_with_posts(0).await?;

    let entity_manager = EntityManager::new(db);
    let ((first, second), queries) = count_queries(async {
        let first = entity_manager
            .find::<EntityManagerUser>(saved_user.id)
            .await?
            .expect("first lookup should return a user");
        let second = entity_manager
            .find::<EntityManagerUser>(saved_user.id)
            .await?
            .expect("second lookup should return a user");
        Ok((first, second))
    })
    .await?;

    assert_eq!(queries, 1);
    assert_eq!(first.id, second.id);
    assert_eq!(first.name, second.name);
    Ok(())
}

#[tokio::test]
async fn entity_managers_are_isolated() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (saved_user, posts) = seed_user_with_posts(3).await?;

    let entity_manager_a = EntityManager::new(db.clone());
    let entity_manager_b = EntityManager::new(db.clone());

    let mut user_a = entity_manager_a
        .find::<EntityManagerUser>(saved_user.id)
        .await?
        .expect("entity_manager A user should exist");
    let mut user_b = entity_manager_b
        .find::<EntityManagerUser>(saved_user.id)
        .await?
        .expect("entity_manager B user should exist");

    entity_manager_a.load(&mut user_a.posts).await?;
    entity_manager_b.load(&mut user_b.posts).await?;

    let remove_from_a = posts[2].id;
    let remove_from_b = posts[0].id;

    user_a
        .posts
        .as_mut()
        .expect("entity_manager A posts should be loaded")
        .retain(|post| post.id != remove_from_a);
    user_b
        .posts
        .as_mut()
        .expect("entity_manager B posts should be loaded")
        .retain(|post| post.id != remove_from_b);

    entity_manager_a.save(&user_a).await?;
    entity_manager_b.save(&user_b).await?;

    let remaining = EntityManagerPost::query_with(db.as_ref())
        .where_eq("user_id", saved_user.id)
        .order_by("id", Order::Asc)
        .get()
        .await?;

    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, posts[1].id);

    Ok(())
}

#[tokio::test]
async fn repeated_save_with_same_new_child_does_not_duplicate() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (saved_user, _) = seed_user_with_posts(0).await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerUser>(saved_user.id)
        .await?
        .expect("entity_manager user should exist");
    entity_manager.load(&mut user.posts).await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .push(EntityManagerPost {
            id: 0,
            user_id: 0,
            title: "only-once".to_string(),
        });

    let user = entity_manager.save(&user).await?;
    let _user = entity_manager.save(&user).await?;

    let saved_posts = EntityManagerPost::query_with(db.as_ref())
        .where_eq("user_id", saved_user.id)
        .order_by("id", Order::Asc)
        .get()
        .await?;

    assert_eq!(saved_posts.len(), 1);
    assert_eq!(saved_posts[0].title, "only-once");

    Ok(())
}

#[tokio::test]
async fn repeated_save_with_same_new_root_does_not_duplicate() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let user = EntityManagerUser {
        id: 0,
        name: "root-once".to_string(),
        posts: Default::default(),
    };

    let user = entity_manager.save(&user).await?;
    let _user = entity_manager.save(&user).await?;

    let saved_users = EntityManagerUser::query_with(db.as_ref())
        .order_by("id", Order::Asc)
        .get()
        .await?;

    assert_eq!(saved_users.len(), 1);
    assert_eq!(saved_users[0].name, "root-once");

    Ok(())
}

#[tokio::test]
async fn edited_existing_child_is_saved() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (saved_user, posts) = seed_user_with_posts(1).await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerUser>(saved_user.id)
        .await?
        .expect("entity_manager user should exist");
    entity_manager.load(&mut user.posts).await?;

    user.posts.as_mut().expect("loaded posts should be mutable")[0].title =
        "edited-title".to_string();

    let user = entity_manager.save(&user).await?;
    let cached_posts = user.posts.get_cached().expect("posts should stay loaded");
    assert_eq!(cached_posts[0].title, "edited-title");

    let saved_post = EntityManagerPost::find_with(posts[0].id, db.as_ref())
        .await?
        .expect("saved post should exist");
    assert_eq!(saved_post.title, "edited-title");

    Ok(())
}

#[tokio::test]
async fn identical_new_children_are_persisted_separately() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (saved_user, _) = seed_user_with_posts(0).await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerUser>(saved_user.id)
        .await?
        .expect("entity_manager user should exist");
    entity_manager.load(&mut user.posts).await?;

    let posts = user.posts.as_mut().expect("loaded posts should be mutable");
    posts.push(EntityManagerPost {
        id: 0,
        user_id: 0,
        title: "same-title".to_string(),
    });
    posts.push(EntityManagerPost {
        id: 0,
        user_id: 0,
        title: "same-title".to_string(),
    });

    let user = entity_manager.save(&user).await?;
    let cached_posts = user.posts.get_cached().expect("posts should stay loaded");
    assert_eq!(cached_posts.len(), 2);
    assert!(cached_posts.iter().all(|post| post.id > 0));
    assert_ne!(cached_posts[0].id, cached_posts[1].id);

    let saved_posts = EntityManagerPost::query_with(db.as_ref())
        .where_eq("user_id", saved_user.id)
        .order_by("id", Order::Asc)
        .get()
        .await?;

    assert_eq!(saved_posts.len(), 2);
    assert_eq!(saved_posts[0].title, "same-title");
    assert_eq!(saved_posts[1].title, "same-title");

    Ok(())
}

#[tokio::test]
async fn identical_new_roots_are_persisted_separately() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let first = EntityManagerUser {
        id: 0,
        name: "same-root".to_string(),
        posts: Default::default(),
    };
    let second = EntityManagerUser {
        id: 0,
        name: "same-root".to_string(),
        posts: Default::default(),
    };

    let _first = entity_manager.save(&first).await?;
    let _second = entity_manager.save(&second).await?;

    let saved_users = EntityManagerUser::query_with(db.as_ref())
        .where_eq("name", "same-root")
        .order_by("id", Order::Asc)
        .get()
        .await?;

    assert_eq!(saved_users.len(), 2);
    assert_ne!(saved_users[0].id, saved_users[1].id);

    Ok(())
}

#[tokio::test]
async fn string_local_key_is_used_for_new_children() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let created = EntityManagerCodeUser {
        id: 0,
        code: "user-code-1".to_string(),
        name: "Code User".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerCodeUser>(created.id)
        .await?
        .expect("code user should exist");
    entity_manager.load(&mut user.posts).await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .push(EntityManagerCodePost {
            id: 0,
            user_code: String::new(),
            title: "uses-code".to_string(),
        });

    let user = entity_manager.save(&user).await?;
    let cached_posts = user.posts.get_cached().expect("posts should stay loaded");
    assert_eq!(cached_posts.len(), 1);
    assert_eq!(cached_posts[0].user_code, "user-code-1");

    let saved_posts = EntityManagerCodePost::query_with(db.as_ref())
        .where_eq("user_code", "user-code-1")
        .order_by("id", Order::Asc)
        .get()
        .await?;

    assert_eq!(saved_posts.len(), 1);
    assert_eq!(saved_posts[0].title, "uses-code");

    Ok(())
}

#[tokio::test]
async fn natural_key_child_delete_uses_model_primary_key() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let user = EntityManagerSlugUser {
        id: 0,
        name: "Slug User".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    EntityManagerSlugPost {
        slug: "slug-a".to_string(),
        user_id: user.id,
        title: "A".to_string(),
    }
    .save()
    .await?;
    EntityManagerSlugPost {
        slug: "slug-b".to_string(),
        user_id: user.id,
        title: "B".to_string(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerSlugUser>(user.id)
        .await?
        .expect("slug user should exist");
    entity_manager.load(&mut user.posts).await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .retain(|post| post.slug != "slug-b");

    entity_manager.save(&user).await?;

    let remaining = EntityManagerSlugPost::query_with(db.as_ref())
        .where_eq("user_id", user.id)
        .order_by("slug", Order::Asc)
        .get()
        .await?;

    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].slug, "slug-a");
    assert!(
        EntityManagerSlugPost::find_with("slug-b".to_string(), db.as_ref())
            .await?
            .is_none()
    );

    Ok(())
}

#[tokio::test]
async fn natural_key_root_uses_entity_manager_identity_map() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let created = EntityManagerApiKey {
        key: "api-key-1".to_string(),
        label: "Primary key".to_string(),
        active: true,
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db);
    let ((first, second), queries) = count_queries(async {
        let first = entity_manager
            .find::<EntityManagerApiKey>(created.key.clone())
            .await?
            .expect("natural-key model should exist");
        let second = entity_manager
            .find::<EntityManagerApiKey>(created.key.clone())
            .await?
            .expect("natural-key model should exist on second lookup");
        Ok((first, second))
    })
    .await?;

    assert_eq!(queries, 1);
    assert_eq!(first.key, second.key);
    assert_eq!(first.label, second.label);
    Ok(())
}

#[tokio::test]
async fn composite_key_root_uses_entity_manager_identity_map() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let created = EntityManagerTeamMembership {
        team_id: 10,
        member_id: 7,
        role: "admin".to_string(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db);
    let ((first, second), queries) = count_queries(async {
        let first = entity_manager
            .find::<EntityManagerTeamMembership>((created.team_id, created.member_id))
            .await?
            .expect("composite-key model should exist");
        let second = entity_manager
            .find::<EntityManagerTeamMembership>((created.team_id, created.member_id))
            .await?
            .expect("composite-key model should exist on second lookup");
        Ok((first, second))
    })
    .await?;

    assert_eq!(queries, 1);
    assert_eq!(first.team_id, second.team_id);
    assert_eq!(first.member_id, second.member_id);
    assert_eq!(first.role, second.role);
    Ok(())
}

#[tokio::test]
async fn composite_key_child_delete_uses_model_primary_key() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let user = EntityManagerCompositeUser {
        id: 0,
        name: "Composite User".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    EntityManagerCompositePost {
        user_id: user.id,
        slug: "slug-a".to_string(),
        title: "A".to_string(),
    }
    .save()
    .await?;
    EntityManagerCompositePost {
        user_id: user.id,
        slug: "slug-b".to_string(),
        title: "B".to_string(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerCompositeUser>(user.id)
        .await?
        .expect("composite parent should exist");
    entity_manager.load(&mut user.posts).await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .retain(|post| post.slug != "slug-b");

    entity_manager.save(&user).await?;

    let remaining = EntityManagerCompositePost::query_with(db.as_ref())
        .where_eq("user_id", user.id)
        .order_by("slug", Order::Asc)
        .get()
        .await?;

    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].slug, "slug-a");
    assert!(
        EntityManagerCompositePost::find_with((user.id, "slug-b".to_string()), db.as_ref())
            .await?
            .is_none()
    );

    Ok(())
}

#[tokio::test]
async fn composite_key_child_insert_is_saved() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let user = EntityManagerCompositeUser {
        id: 0,
        name: "Composite Insert User".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerCompositeUser>(user.id)
        .await?
        .expect("composite parent should exist");
    entity_manager.load(&mut user.posts).await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .push(EntityManagerCompositePost {
            user_id: 0,
            slug: "slug-insert".to_string(),
            title: "Inserted".to_string(),
        });

    let user = entity_manager.save(&user).await?;
    let cached_posts = user.posts.get_cached().expect("posts should stay loaded");
    assert_eq!(cached_posts.len(), 1);
    assert_eq!(cached_posts[0].user_id, user.id);
    assert_eq!(cached_posts[0].slug, "slug-insert");
    assert_eq!(cached_posts[0].title, "Inserted");

    let saved =
        EntityManagerCompositePost::find_with((user.id, "slug-insert".to_string()), db.as_ref())
            .await?
            .expect("inserted composite child should exist");
    assert_eq!(saved.title, "Inserted");

    Ok(())
}

#[tokio::test]
async fn composite_key_child_update_is_saved() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let user = EntityManagerCompositeUser {
        id: 0,
        name: "Composite Update User".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    EntityManagerCompositePost {
        user_id: user.id,
        slug: "slug-update".to_string(),
        title: "Before".to_string(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerCompositeUser>(user.id)
        .await?
        .expect("composite parent should exist");
    entity_manager.load(&mut user.posts).await?;

    let posts = user.posts.as_mut().expect("loaded posts should be mutable");
    assert_eq!(posts.len(), 1);
    posts[0].title = "After".to_string();

    let user = entity_manager.save(&user).await?;
    let cached_posts = user.posts.get_cached().expect("posts should stay loaded");
    assert_eq!(cached_posts[0].title, "After");

    let saved =
        EntityManagerCompositePost::find_with((user.id, "slug-update".to_string()), db.as_ref())
            .await?
            .expect("updated composite child should exist");
    assert_eq!(saved.title, "After");

    Ok(())
}

#[tokio::test]
async fn hasone_insert_update_delete_is_synced() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let created = EntityManagerAggregateUser {
        id: 0,
        name: "Aggregate User".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerAggregateUser>(created.id)
        .await?
        .expect("aggregate user should exist");
    entity_manager.load(&mut user.profile).await?;
    assert!(user.profile.get_cached().is_none());

    user.profile.set_cached(Some(EntityManagerAggregateProfile {
        id: 0,
        user_id: 0,
        bio: "Bio One".to_string(),
    }));

    let mut user = entity_manager.save(&user).await?;
    let profile = user
        .profile
        .get_cached()
        .expect("profile should be inserted");
    assert!(profile.id > 0);
    assert_eq!(profile.user_id, user.id);
    assert_eq!(profile.bio, "Bio One");

    let saved_profile = EntityManagerAggregateProfile::query_with(db.as_ref())
        .where_eq("user_id", user.id)
        .first()
        .await?
        .expect("profile row should exist");
    assert_eq!(saved_profile.bio, "Bio One");

    user.profile
        .as_mut()
        .expect("profile should stay loaded")
        .bio = "Bio Two".to_string();

    let mut user = entity_manager.save(&user).await?;
    assert_eq!(
        user.profile
            .get_cached()
            .expect("profile should remain loaded")
            .bio,
        "Bio Two"
    );

    let saved_profile = EntityManagerAggregateProfile::query_with(db.as_ref())
        .where_eq("user_id", user.id)
        .first()
        .await?
        .expect("updated profile row should exist");
    assert_eq!(saved_profile.bio, "Bio Two");

    user.profile.clear();

    let user = entity_manager.save(&user).await?;
    assert!(user.profile.get_cached().is_none());
    assert!(
        EntityManagerAggregateProfile::query_with(db.as_ref())
            .where_eq("user_id", user.id)
            .first()
            .await?
            .is_none()
    );

    Ok(())
}

/// Replacing a `has_one` child with a new one deletes the old row before it
/// inserts the new one, so a unique foreign key (`user_id` here) takes it.
#[tokio::test]
async fn replacing_a_has_one_child_deletes_the_old_row_first() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let created = EntityManagerAggregateUser {
        id: 0,
        name: "Aggregate User".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .save()
    .await?;
    EntityManagerAggregateProfile {
        id: 0,
        user_id: created.id,
        bio: "Old Bio".to_string(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerAggregateUser>(created.id)
        .await?
        .expect("aggregate user should exist");
    entity_manager.load(&mut user.profile).await?;
    user.profile.set_cached(Some(EntityManagerAggregateProfile {
        id: 0,
        user_id: 0,
        bio: "New Bio".to_string(),
    }));

    let user = entity_manager.save(&user).await?;
    let profiles = EntityManagerAggregateProfile::query_with(db.as_ref())
        .where_eq("user_id", user.id)
        .get()
        .await?;
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].bio, "New Bio");
    assert_eq!(
        user.profile.get_cached().map(|profile| profile.id),
        Some(profiles[0].id)
    );

    Ok(())
}

#[tokio::test]
async fn entity_manager_save_rolls_back_root_when_relation_sync_fails() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let cached_user = EntityManagerUser {
        id: 0,
        name: "Cached User".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;
    let created = EntityManagerAggregateUser {
        id: 0,
        name: "Aggregate User".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .save()
    .await?;
    let original_profile = EntityManagerAggregateProfile {
        id: 0,
        user_id: created.id,
        bio: "Existing Bio".to_string(),
    }
    .save()
    .await?;
    // The replacement profile below takes a bio another user's profile holds,
    // so writing it fails after the old profile was deleted.
    Database::execute(
        "CREATE UNIQUE INDEX entity_manager_aggregate_profiles_bio ON entity_manager_aggregate_profiles (bio)",
    )
    .await?;
    let other = EntityManagerAggregateUser {
        id: 0,
        name: "Other User".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .save()
    .await?;
    EntityManagerAggregateProfile {
        id: 0,
        user_id: other.id,
        bio: "Conflicting Bio".to_string(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let _cached_user = entity_manager
        .find::<EntityManagerUser>(cached_user.id)
        .await?
        .expect("cached user should load into the identity map");
    let mut user = entity_manager
        .find::<EntityManagerAggregateUser>(created.id)
        .await?
        .expect("aggregate user should exist");
    entity_manager.load(&mut user.profile).await?;

    user.name = "Rolled Back Name".to_string();
    user.profile.set_cached(Some(EntityManagerAggregateProfile {
        id: 0,
        user_id: 0,
        bio: "Conflicting Bio".to_string(),
    }));

    assert!(entity_manager.save(&user).await.is_err());

    let persisted_user = EntityManagerAggregateUser::find_with(created.id, db.as_ref())
        .await?
        .expect("aggregate user should still exist");
    assert_eq!(persisted_user.name, "Aggregate User");

    let profiles = EntityManagerAggregateProfile::query_with(db.as_ref())
        .where_eq("user_id", created.id)
        .order_by("id", Order::Asc)
        .get()
        .await?;
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].id, original_profile.id);
    assert_eq!(profiles[0].bio, "Existing Bio");

    let (cached_again, queries) =
        count_queries(entity_manager.find::<EntityManagerUser>(cached_user.id)).await?;
    let cached_again =
        cached_again.expect("cached user should remain in the identity map after rollback");

    assert_eq!(cached_again.name, "Cached User");
    assert_eq!(queries, 0);

    Ok(())
}

#[tokio::test]
async fn belongs_to_load_through_the_entity_manager_reuses_the_cached_parent() -> tideorm::Result<()>
{
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let user = EntityManagerAggregateUser {
        id: 0,
        name: "Cached Author".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .save()
    .await?;
    let first_post = EntityManagerAggregatePost {
        id: 0,
        user_id: user.id,
        title: "Post A".to_string(),
        author: Default::default(),
        tags: Default::default(),
    }
    .save()
    .await?;
    let second_post = EntityManagerAggregatePost {
        id: 0,
        user_id: user.id,
        title: "Post B".to_string(),
        author: Default::default(),
        tags: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db);
    let _cached_user = entity_manager
        .find::<EntityManagerAggregateUser>(user.id)
        .await?
        .expect("cached parent should exist");
    let mut first = entity_manager
        .find::<EntityManagerAggregatePost>(first_post.id)
        .await?
        .expect("first post should exist");
    let mut second = entity_manager
        .find::<EntityManagerAggregatePost>(second_post.id)
        .await?
        .expect("second post should exist");

    let ((first_author, second_author), queries) = count_queries(async {
        let first_author = entity_manager
            .load(&mut first.author)
            .await?
            .expect("first author should load from entity_manager");
        let second_author = entity_manager
            .load(&mut second.author)
            .await?
            .expect("second author should load from entity_manager");
        Ok((first_author, second_author))
    })
    .await?;

    assert_eq!(queries, 0);
    assert_eq!(first_author.id, user.id);
    assert_eq!(second_author.id, user.id);
    Ok(())
}

#[tokio::test]
async fn nested_has_many_through_changes_are_synced_from_root_save() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let user = EntityManagerAggregateUser {
        id: 0,
        name: "Graph User".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .save()
    .await?;
    let post = EntityManagerAggregatePost {
        id: 0,
        user_id: user.id,
        title: "Graph Post".to_string(),
        author: Default::default(),
        tags: Default::default(),
    }
    .save()
    .await?;
    let old_tag = EntityManagerAggregateTag {
        id: 0,
        name: "old-tag".to_string(),
    }
    .save()
    .await?;
    EntityManagerAggregatePostTag {
        id: 0,
        post_id: post.id,
        tag_id: old_tag.id,
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerAggregateUser>(user.id)
        .await?
        .expect("graph user should exist");
    entity_manager.load(&mut user.posts).await?;

    let posts = user.posts.as_mut().expect("posts should be loaded");
    assert_eq!(posts.len(), 1);
    entity_manager.load(&mut posts[0].tags).await?;

    let tags = posts[0].tags.as_mut().expect("tags should be loaded");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "old-tag");
    tags.clear();
    tags.push(EntityManagerAggregateTag {
        id: 0,
        name: "new-tag".to_string(),
    });

    let user = entity_manager.save(&user).await?;
    let saved_post = &user.posts.get_cached().expect("posts should remain loaded")[0];
    let saved_tags = saved_post
        .tags
        .get_cached()
        .expect("tags should remain loaded");
    assert_eq!(saved_tags.len(), 1);
    assert_eq!(saved_tags[0].name, "new-tag");
    assert!(saved_tags[0].id > 0);

    let pivots = EntityManagerAggregatePostTag::query_with(db.as_ref())
        .where_eq("post_id", saved_post.id)
        .order_by("id", Order::Asc)
        .get()
        .await?;
    assert_eq!(pivots.len(), 1);
    assert_eq!(pivots[0].tag_id, saved_tags[0].id);
    assert_eq!(
        EntityManagerAggregatePostTag::query_with(db.as_ref())
            .where_eq("post_id", saved_post.id)
            .where_eq("tag_id", old_tag.id)
            .count()
            .await?,
        0
    );
    assert!(
        EntityManagerAggregateTag::find_with(old_tag.id, db.as_ref())
            .await?
            .is_some()
    );

    Ok(())
}

#[tokio::test]
async fn entity_manager_facade_find_load_and_save_supports_all_relation_helpers()
-> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let user = EntityManagerAggregateUser {
        id: 0,
        name: "Facade User".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .save()
    .await?;
    EntityManagerAggregateProfile {
        id: 0,
        user_id: user.id,
        bio: "Initial Bio".to_string(),
    }
    .save()
    .await?;
    let post = EntityManagerAggregatePost {
        id: 0,
        user_id: user.id,
        title: "Facade Post".to_string(),
        author: Default::default(),
        tags: Default::default(),
    }
    .save()
    .await?;
    let old_tag = EntityManagerAggregateTag {
        id: 0,
        name: "old-tag".to_string(),
    }
    .save()
    .await?;
    EntityManagerAggregatePostTag {
        id: 0,
        post_id: post.id,
        tag_id: old_tag.id,
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = entity_manager
        .find::<EntityManagerAggregateUser>(user.id)
        .await?
        .expect("facade user should exist");
    let user_id = user.id;

    let profile_bio = entity_manager
        .load(&mut user.profile)
        .await?
        .map(|profile| profile.bio.clone())
        .expect("profile should load through entity_manager facade");
    assert_eq!(profile_bio, "Initial Bio");

    let posts_len = entity_manager.load(&mut user.posts).await?.len();
    assert_eq!(posts_len, 1);

    {
        let post = &mut user.posts.as_mut().expect("posts should be loaded")[0];
        let author_id = entity_manager
            .load(&mut post.author)
            .await?
            .map(|author| author.id)
            .expect("belongs_to relation should load through entity_manager facade");
        assert_eq!(author_id, user_id);

        let tag_names: Vec<_> = entity_manager
            .load(&mut post.tags)
            .await?
            .iter()
            .map(|tag| tag.name.clone())
            .collect();
        assert_eq!(tag_names, vec!["old-tag".to_string()]);

        post.tags.as_mut().expect("tags should be loaded").clear();
        post.tags
            .as_mut()
            .expect("tags should stay loaded")
            .push(EntityManagerAggregateTag {
                id: 0,
                name: "new-tag".to_string(),
            });
    }

    user.profile.as_mut().expect("profile should be loaded").bio = "Updated Bio".to_string();

    let user = entity_manager.save(&user).await?;
    assert_eq!(
        user.profile
            .get_cached()
            .map(|profile| profile.bio.as_str()),
        Some("Updated Bio")
    );

    let saved_post = &user.posts.get_cached().expect("posts should stay loaded")[0];
    let saved_tags = saved_post
        .tags
        .get_cached()
        .expect("tags should stay loaded");
    assert_eq!(saved_tags.len(), 1);
    assert_eq!(saved_tags[0].name, "new-tag");
    assert!(saved_tags[0].id > 0);

    let saved_profile = EntityManagerAggregateProfile::query_with(db.as_ref())
        .where_eq("user_id", user.id)
        .first()
        .await?
        .expect("profile row should still exist");
    assert_eq!(saved_profile.bio, "Updated Bio");

    let pivots = EntityManagerAggregatePostTag::query_with(db.as_ref())
        .where_eq("post_id", saved_post.id)
        .order_by("id", Order::Asc)
        .get()
        .await?;
    assert_eq!(pivots.len(), 1);
    assert_eq!(pivots[0].tag_id, saved_tags[0].id);
    assert_eq!(
        EntityManagerAggregatePostTag::query_with(db.as_ref())
            .where_eq("post_id", saved_post.id)
            .where_eq("tag_id", old_tag.id)
            .count()
            .await?,
        0
    );

    Ok(())
}

#[tokio::test]
async fn entity_manager_persist_and_flush_inserts_new_root() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let managed = entity_manager.persist(EntityManagerUser {
        id: 0,
        name: "Managed Insert".to_string(),
        posts: Default::default(),
    });

    entity_manager.flush().await?;

    let saved = managed.get();
    assert!(saved.id > 0);

    let persisted = EntityManagerUser::find_with(saved.id, db.as_ref())
        .await?
        .expect("managed insert should be flushed");
    assert_eq!(persisted.name, "Managed Insert");

    Ok(())
}

#[tokio::test]
async fn entity_manager_find_managed_and_flush_updates_existing_root() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let saved = EntityManagerUser {
        id: 0,
        name: "Before Update".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let managed = entity_manager
        .find_managed::<EntityManagerUser>(saved.id)
        .await?
        .expect("managed entity should load");

    managed.edit(|user| user.name = "After Update".to_string());
    entity_manager.flush().await?;

    let updated = EntityManagerUser::find_with(saved.id, db.as_ref())
        .await?
        .expect("updated user should exist");
    assert_eq!(updated.name, "After Update");

    Ok(())
}

#[tokio::test]
async fn entity_manager_flush_persists_relation_only_changes_without_root_update()
-> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let saved_user = EntityManagerAggregateUser {
        id: 0,
        name: "Managed Aggregate".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .save()
    .await?;
    EntityManagerAggregateProfile {
        id: 0,
        user_id: saved_user.id,
        bio: "Before Relation Flush".to_string(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let managed = entity_manager
        .find_managed::<EntityManagerAggregateUser>(saved_user.id)
        .await?
        .expect("managed aggregate should load");

    let mut aggregate = managed.get();
    entity_manager.load(&mut aggregate.profile).await?;
    managed.replace(aggregate);
    managed.edit(|user| {
        user.profile.as_mut().expect("profile should be loaded").bio =
            "After Relation Flush".to_string();
    });

    let ((), queries) = count_queries(entity_manager.flush()).await?;
    assert_eq!(queries, 1);

    let refreshed_user = EntityManagerAggregateUser::find_with(saved_user.id, db.as_ref())
        .await?
        .expect("aggregate user should still exist");
    assert_eq!(refreshed_user.name, "Managed Aggregate");

    let refreshed_profile = EntityManagerAggregateProfile::query_with(db.as_ref())
        .where_eq("user_id", saved_user.id)
        .first()
        .await?
        .expect("profile should still exist");
    assert_eq!(refreshed_profile.bio, "After Relation Flush");

    Ok(())
}

#[tokio::test]
async fn entity_manager_merge_and_flush_updates_existing_root() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let saved = EntityManagerUser {
        id: 0,
        name: "Before Merge".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let merged = entity_manager.merge(EntityManagerUser {
        id: saved.id,
        name: "After Merge".to_string(),
        posts: Default::default(),
    })?;

    entity_manager.flush().await?;

    assert_eq!(merged.get().name, "After Merge");

    let updated = EntityManagerUser::find_with(saved.id, db.as_ref())
        .await?
        .expect("merged user should exist");
    assert_eq!(updated.name, "After Merge");

    Ok(())
}

#[tokio::test]
async fn entity_manager_flush_rolls_back_all_managed_writes_on_error() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let cached_user = EntityManagerUser {
        id: 0,
        name: "Cached User".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;
    let _cached_user = entity_manager
        .find::<EntityManagerUser>(cached_user.id)
        .await?
        .expect("cached user should load into the identity map");
    let first = entity_manager.persist(EntityManagerCodeUser {
        id: 0,
        code: "duplicate".to_string(),
        name: "First".to_string(),
        posts: Default::default(),
    });
    let second = entity_manager.persist(EntityManagerCodeUser {
        id: 0,
        code: "duplicate".to_string(),
        name: "Second".to_string(),
        posts: Default::default(),
    });

    assert!(entity_manager.flush().await.is_err());
    assert_eq!(
        EntityManagerCodeUser::query_with(db.as_ref())
            .count()
            .await?,
        0
    );
    assert_eq!(first.state(), EntityState::New);
    assert_eq!(second.state(), EntityState::New);
    assert_eq!(first.get().id, 0);
    assert_eq!(second.get().id, 0);

    let (cached_again, queries) =
        count_queries(entity_manager.find::<EntityManagerUser>(cached_user.id)).await?;
    let cached_again =
        cached_again.expect("cached user should remain in the identity map after failed flush");

    assert_eq!(cached_again.name, "Cached User");
    assert_eq!(queries, 0);

    second.edit(|user| user.code = "unique".to_string());
    entity_manager.flush().await?;

    assert!(first.get().id > 0);
    assert!(second.get().id > 0);
    assert_eq!(
        EntityManagerCodeUser::query_with(db.as_ref())
            .count()
            .await?,
        2
    );

    Ok(())
}

/// A managed entity whose primary key was changed is not saved: writing it
/// under the new key overwrote whichever row held that key.
#[tokio::test]
async fn entity_manager_refuses_to_save_a_changed_primary_key() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let bystander = EntityManagerUser {
        id: 0,
        name: "Bystander".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;
    let moved = EntityManagerUser {
        id: 0,
        name: "Moved".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let managed = entity_manager
        .find_managed::<EntityManagerUser>(moved.id)
        .await?
        .expect("managed entity should load");
    managed.edit(|user| {
        user.id = bystander.id;
        user.name = "Overwritten".to_string();
    });

    let error = entity_manager
        .flush()
        .await
        .expect_err("a changed primary key must not be saved");
    assert!(error.to_string().contains("primary key"), "{error}");
    for (id, name) in [(bystander.id, "Bystander"), (moved.id, "Moved")] {
        let row = EntityManagerUser::find_with(id, db.as_ref())
            .await?
            .expect("row should still exist");
        assert_eq!(row.name, name);
    }

    Ok(())
}

/// A flush dropped part way rolls its transaction back, and the context too:
/// the entities it had already written kept their ids and `Managed` state, so
/// the next flush never inserted them.
#[tokio::test]
async fn entity_manager_cancelled_flush_leaves_the_context_unflushed() -> tideorm::Result<()> {
    use std::future::Future;
    use std::sync::atomic::Ordering;
    use std::task::Poll;

    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let first = entity_manager.persist(EntityManagerFlaggedUser {
        id: 0,
        name: "first".to_string(),
    });
    let second = entity_manager.persist(EntityManagerFlaggedUser {
        id: 0,
        name: "second".to_string(),
    });

    SECOND_SAVE_STARTED.store(false, Ordering::SeqCst);
    let mut flush = Box::pin(entity_manager.flush());
    std::future::poll_fn(|cx| match flush.as_mut().poll(cx) {
        Poll::Ready(result) => panic!("the flush ended before it was cancelled: {result:?}"),
        Poll::Pending if SECOND_SAVE_STARTED.load(Ordering::SeqCst) => Poll::Ready(()),
        Poll::Pending => Poll::Pending,
    })
    .await;
    drop(flush);

    assert_eq!(first.state(), EntityState::New);
    assert_eq!(first.get().id, 0);
    assert_eq!(second.state(), EntityState::New);

    entity_manager.flush().await?;
    assert!(first.get().id > 0);
    assert!(second.get().id > 0);
    assert_eq!(
        EntityManagerFlaggedUser::query_with(db.as_ref())
            .count()
            .await?,
        2
    );

    Ok(())
}

/// A flush inside a transaction that then rolls back leaves the context as it
/// was before the flush, since none of what it wrote was committed.
#[tokio::test]
async fn entity_manager_flush_is_undone_by_an_enclosing_rollback() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let managed = entity_manager.persist(EntityManagerUser {
        id: 0,
        name: "Rolled Back".to_string(),
        posts: Default::default(),
    });

    let flushing = entity_manager.clone();
    let outcome: tideorm::Result<()> = db
        .transaction(move |_| {
            Box::pin(async move {
                flushing.flush().await?;
                Err(Error::query("the enclosing work failed"))
            })
        })
        .await;
    assert!(outcome.is_err());
    assert_eq!(managed.state(), EntityState::New);
    assert_eq!(managed.get().id, 0);
    assert_eq!(EntityManagerUser::query_with(db.as_ref()).count().await?, 0);

    // Committed, the same flush keeps what it wrote.
    let flushing = entity_manager.clone();
    db.transaction(move |_| Box::pin(async move { flushing.flush().await }))
        .await?;
    assert_eq!(managed.state(), EntityState::Managed);
    let id = managed.get().id;
    assert!(id > 0);
    assert!(
        EntityManagerUser::find_with(id, db.as_ref())
            .await?
            .is_some()
    );

    Ok(())
}

#[tokio::test]
async fn entity_manager_remove_and_detach_control_flush_lifecycle() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let removable = EntityManagerUser {
        id: 0,
        name: "Remove Me".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;
    let detachable = EntityManagerUser {
        id: 0,
        name: "Detach Me".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let removable_managed = entity_manager
        .find_managed::<EntityManagerUser>(removable.id)
        .await?
        .expect("removable entity should load");
    let detachable_managed = entity_manager
        .find_managed::<EntityManagerUser>(detachable.id)
        .await?
        .expect("detachable entity should load");

    entity_manager.remove(&removable_managed);
    entity_manager.detach(&detachable_managed);
    detachable_managed.edit(|user| user.name = "Detached Update".to_string());

    entity_manager.flush().await?;

    assert!(
        EntityManagerUser::find_with(removable.id, db.as_ref())
            .await?
            .is_none()
    );

    let unchanged = EntityManagerUser::find_with(detachable.id, db.as_ref())
        .await?
        .expect("detached entity should remain in the database");
    assert_eq!(unchanged.name, "Detach Me");

    Ok(())
}

#[tokio::test]
async fn entity_manager_clear_drops_cached_identity_and_managed_state() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let saved = EntityManagerUser {
        id: 0,
        name: "Before Clear".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let initial = entity_manager
        .find::<EntityManagerUser>(saved.id)
        .await?
        .expect("user should load into the entity manager");
    assert_eq!(initial.name, "Before Clear");

    EntityManagerUser {
        id: saved.id,
        name: "After Clear".to_string(),
        posts: Default::default(),
    }
    .update()
    .await?;

    let stale = entity_manager
        .find::<EntityManagerUser>(saved.id)
        .await?
        .expect("identity map should still return the cached entity before clear");
    assert_eq!(stale.name, "Before Clear");

    let managed = entity_manager
        .find_managed::<EntityManagerUser>(saved.id)
        .await?
        .expect("managed entity should load before clear");
    managed.edit(|user| user.name = "Detached By Clear".to_string());

    entity_manager.clear();
    entity_manager.flush().await?;

    let fresh = entity_manager
        .find::<EntityManagerUser>(saved.id)
        .await?
        .expect("cleared entity manager should reload from the database");
    assert_eq!(fresh.name, "After Clear");

    let persisted = EntityManagerUser::find_with(saved.id, db.as_ref())
        .await?
        .expect("user should still exist after clear");
    assert_eq!(persisted.name, "After Clear");

    Ok(())
}

#[tokio::test]
async fn hasmany_without_entity_manager_unchanged() -> tideorm::Result<()> {
    let Some(_db) = setup_database().await? else {
        return Ok(());
    };
    let (saved_user, posts) = seed_user_with_posts(2).await?;

    let user = EntityManagerUser::find(saved_user.id)
        .await?
        .expect("user should exist in default path");
    let loaded_posts = user.posts.load().await?;

    assert_eq!(loaded_posts.len(), 2);
    assert_eq!(loaded_posts[0].user_id, saved_user.id);
    assert_eq!(loaded_posts[1].id, posts[1].id);

    Ok(())
}

/// A new root read from JSON saves the children it carries. Its relation was
/// built under the placeholder key `0`, and saving the root rebuilt it under
/// the stored key without them.
#[tokio::test]
async fn a_new_root_read_from_json_saves_its_children() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let entity_manager = EntityManager::new(db.clone());
    let user: EntityManagerUser = serde_json::from_value(serde_json::json!({
        "id": 0,
        "name": "from json",
        "posts": [{ "id": 0, "user_id": 0, "title": "first" }]
    }))
    .expect("the user deserializes");

    let saved = entity_manager.save(&user).await?;

    let titles: Vec<String> = EntityManagerPost::query_with(db.as_ref())
        .where_eq("user_id", saved.id)
        .pluck("title")
        .await?;
    assert_eq!(titles, vec!["first".to_string()]);
    Ok(())
}

/// Persisting an entity the context was about to remove takes the removal
/// back and writes it; the removal won, deleting the row.
#[tokio::test]
async fn persisting_a_removed_entity_writes_it() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (user, _) = seed_user_with_posts(0).await?;
    let entity_manager = EntityManager::new(db.clone());
    let managed = entity_manager
        .find_managed::<EntityManagerUser>(user.id)
        .await?
        .expect("the user is managed");

    entity_manager.remove(&managed);
    entity_manager.persist(EntityManagerUser {
        id: user.id,
        name: "fresh".into(),
        ..Default::default()
    });
    entity_manager.flush().await?;

    let stored = EntityManagerUser::find_with(user.id, db.as_ref()).await?;
    assert_eq!(stored.map(|user| user.name).as_deref(), Some("fresh"));
    Ok(())
}

/// A managed handle to a row that another path of the context saves moves
/// onto what was stored, so flushing it later writes only its own edits:
/// it wrote the whole row it had loaded back over the newer save.
#[tokio::test]
async fn a_managed_handle_keeps_what_another_save_stored() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let user = EntityManagerCodeUser {
        id: 0,
        code: "original".into(),
        name: "original".into(),
        ..Default::default()
    }
    .save()
    .await?;
    let entity_manager = EntityManager::new(db.clone());
    let managed = entity_manager
        .find_managed::<EntityManagerCodeUser>(user.id)
        .await?
        .expect("the user is managed");

    let mut copy = EntityManagerCodeUser::find_with(user.id, db.as_ref())
        .await?
        .expect("the user exists");
    copy.name = "saved elsewhere".into();
    entity_manager.save(&copy).await?;
    managed.edit(|user| user.code = "edited".into());
    entity_manager.flush().await?;

    let stored = EntityManagerCodeUser::find_with(user.id, db.as_ref())
        .await?
        .expect("the user exists");
    assert_eq!(
        (stored.code.as_str(), stored.name.as_str()),
        ("edited", "saved elsewhere")
    );
    Ok(())
}

#[tideorm::model(table = "entity_manager_note_owners")]
struct EntityManagerNoteOwner {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_many = "EntityManagerSecretNote", foreign_key = "owner_id")]
    notes: HasMany<EntityManagerSecretNote>,
}

/// A child whose own `Serialize` skips a field; the skipped field is still a
/// column.
#[tideorm::model(table = "entity_manager_secret_notes")]
#[derive(serde::Serialize)]
struct EntityManagerSecretNote {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    owner_id: i64,
    label: String,
    #[serde(skip_serializing)]
    secret: String,
}

/// An edit to a field the child's own serde leaves out is saved: the change
/// check compared the JSON, which does not carry the field.
#[tokio::test]
async fn an_edit_to_a_field_serde_skips_is_saved() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let mut schema = Schema::new(db.backend());
    schema
        .drop_table_if_exists("entity_manager_secret_notes")
        .await?;
    schema
        .drop_table_if_exists("entity_manager_note_owners")
        .await?;
    schema
        .create_table("entity_manager_note_owners", |t| {
            t.id();
            t.string("name").not_null();
        })
        .await?;
    schema
        .create_table("entity_manager_secret_notes", |t| {
            t.id();
            t.big_integer("owner_id").not_null();
            t.string("label").not_null();
            t.string("secret").not_null();
        })
        .await?;
    let owner = EntityManagerNoteOwner {
        id: 0,
        name: "owner".into(),
        ..Default::default()
    }
    .save()
    .await?;
    EntityManagerSecretNote {
        id: 0,
        owner_id: owner.id,
        label: "note".into(),
        secret: "old".into(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut owner = entity_manager
        .find::<EntityManagerNoteOwner>(owner.id)
        .await?
        .expect("the owner exists");
    entity_manager.load(&mut owner.notes).await?;
    let mut notes = owner
        .notes
        .get_cached()
        .map(|notes| notes.to_vec())
        .unwrap_or_default();
    notes[0].secret = "new".into();
    owner.notes.set_cached(notes);
    entity_manager.save(&owner).await?;

    let secrets: Vec<String> = EntityManagerSecretNote::query_with(db.as_ref())
        .pluck("secret")
        .await?;
    assert_eq!(secrets, vec!["new".to_string()]);
    Ok(())
}

/// Two flushes of one manager run one after the other, so an entity waiting
/// to be inserted is inserted once. Both used to see it unwritten and insert
/// a row each.
#[tokio::test]
async fn concurrent_flushes_insert_a_new_entity_once() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let managed = entity_manager.persist(EntityManagerUser {
        id: 0,
        name: "flushed once".to_string(),
        posts: Default::default(),
    });
    let (first, second) = tokio::join!(entity_manager.flush(), entity_manager.flush());
    first?;
    second?;

    assert!(managed.get().id > 0);
    let rows = EntityManagerUser::query_with(db.as_ref())
        .where_eq("name", "flushed once")
        .count()
        .await?;
    assert_eq!(rows, 1);
    Ok(())
}

static EDIT_WHILE_SAVING: std::sync::Mutex<Option<Managed<EntityManagerEditedTag>>> =
    std::sync::Mutex::new(None);

#[tideorm::model(table = "entity_manager_aggregate_tags")]
struct EntityManagerEditedTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

impl Callbacks for EntityManagerEditedTag {
    fn after_save(&self) -> tideorm::Result<()> {
        if let Some(managed) = EDIT_WHILE_SAVING.lock().unwrap().take() {
            managed.edit(|tag| tag.name = "edited while saving".to_string());
        }
        Ok(())
    }
}

/// An edit made to a managed entity while a flush writes it stays, over the
/// row as stored, and the next flush writes it. The flush replaced the entity
/// with what it had written and marked it clean.
#[tokio::test]
async fn an_edit_made_while_a_flush_writes_the_entity_is_kept() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let managed = entity_manager.persist(EntityManagerEditedTag {
        id: 0,
        name: "persisted".to_string(),
    });
    *EDIT_WHILE_SAVING.lock().unwrap() = Some(managed.clone());
    entity_manager.flush().await?;

    let held = managed.get();
    assert!(held.id > 0, "the entity keeps the key its insert gave it");
    assert_eq!(held.name, "edited while saving");
    let stored = EntityManagerEditedTag::find_with(held.id, db.as_ref())
        .await?
        .expect("the row was inserted");
    assert_eq!(stored.name, "persisted");

    entity_manager.flush().await?;
    let stored = EntityManagerEditedTag::find_with(held.id, db.as_ref())
        .await?
        .expect("the row is there");
    assert_eq!(stored.name, "edited while saving");
    Ok(())
}

/// Two lookups of one row share the handle the first one filed, and the
/// second leaves its edits alone. It reset the handle to the row as it read
/// it, dropping an edit made in between.
#[tokio::test]
async fn a_lookup_finishing_second_keeps_the_edits_of_the_first() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let saved = EntityManagerUser {
        id: 0,
        name: "loaded".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;
    let entity_manager = EntityManager::new(db.clone());
    let (first, second) = tokio::join!(
        async {
            let managed = entity_manager
                .find_managed::<EntityManagerUser>(saved.id)
                .await?
                .expect("the row is there");
            managed.edit(|user| user.name = "edited".to_string());
            Ok::<_, tideorm::Error>(managed)
        },
        entity_manager.find_managed::<EntityManagerUser>(saved.id),
    );
    let first = first?;
    let second = second?.expect("the row is there");

    assert_eq!(first.get().name, "edited");
    assert_eq!(second.get().name, "edited");
    Ok(())
}

#[tideorm::model(table = "entity_manager_nodes")]
struct EntityManagerNode {
    #[tideorm(primary_key)]
    id: i64,
    parent_id: Option<i64>,
    name: String,

    #[tideorm(foreign_key = "parent_id")]
    parent: SelfRef<EntityManagerNode>,
}

/// Rows of a self-referencing table flush in row order: a parent is inserted
/// before the child that references it and deleted after it, whatever order
/// they were handed to the manager in. The table order cannot say it, since
/// the table depends on itself.
#[tokio::test]
async fn self_referencing_rows_insert_parents_first_and_delete_children_first()
-> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    Database::execute("DROP TABLE IF EXISTS entity_manager_nodes").await?;
    Database::execute(
        "CREATE TABLE entity_manager_nodes (
            id BIGINT NOT NULL PRIMARY KEY,
            parent_id BIGINT NULL,
            name VARCHAR(100) NOT NULL,
            FOREIGN KEY (parent_id) REFERENCES entity_manager_nodes (id)
        )",
    )
    .await?;
    let node = |id: i64, parent_id: Option<i64>, name: &str| EntityManagerNode {
        id,
        parent_id,
        name: name.to_string(),
        ..Default::default()
    };

    let entity_manager = EntityManager::new(db.clone());
    let leaf = entity_manager.persist(node(3, Some(2), "leaf"));
    let branch = entity_manager.persist(node(2, Some(1), "branch"));
    let root = entity_manager.persist(node(1, None, "root"));
    entity_manager.flush().await?;
    assert_eq!(EntityManagerNode::count().await?, 3);

    entity_manager.remove(&root);
    entity_manager.remove(&branch);
    entity_manager.remove(&leaf);
    entity_manager.flush().await?;
    assert_eq!(EntityManagerNode::count().await?, 0);

    Database::execute("DROP TABLE entity_manager_nodes").await?;
    Ok(())
}

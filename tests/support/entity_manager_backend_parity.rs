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

#[tokio::test]
async fn tracked_deletion_emits_delete() -> tideorm::Result<()> {
    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (saved_user, posts) = seed_user_with_posts(3).await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user = EntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager)
        .await?
        .expect("entity_manager user should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    let removed_id = posts[1].id;
    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .retain(|post| post.id != removed_id);

    save_with_entity_manager(&user, &entity_manager).await?;

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

    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let entity_manager = EntityManager::new(db);
    let first = EntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager)
        .await?
        .expect("first lookup should return a user");
    let second = EntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager)
        .await?
        .expect("second lookup should return a user");

    let stats = GlobalProfiler::stats();
    assert_eq!(stats.total_queries, 1);
    assert_eq!(first.id, second.id);
    assert_eq!(first.name, second.name);

    GlobalProfiler::disable();
    GlobalProfiler::reset();
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

    let mut user_a = EntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager_a)
        .await?
        .expect("entity_manager A user should exist");
    let mut user_b = EntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager_b)
        .await?
        .expect("entity_manager B user should exist");

    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(
        &mut user_a.posts,
        &entity_manager_a,
    )
    .await?;
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(
        &mut user_b.posts,
        &entity_manager_b,
    )
    .await?;

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

    save_with_entity_manager(&user_a, &entity_manager_a).await?;
    save_with_entity_manager(&user_b, &entity_manager_b).await?;

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
    let mut user = EntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager)
        .await?
        .expect("entity_manager user should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .push(EntityManagerPost {
            id: 0,
            user_id: 0,
            title: "only-once".to_string(),
        });

    let user = save_with_entity_manager(&user, &entity_manager).await?;
    let _user = save_with_entity_manager(&user, &entity_manager).await?;

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

    let user = save_with_entity_manager(&user, &entity_manager).await?;
    let _user = save_with_entity_manager(&user, &entity_manager).await?;

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
    let mut user = EntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager)
        .await?
        .expect("entity_manager user should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    user.posts.as_mut().expect("loaded posts should be mutable")[0].title =
        "edited-title".to_string();

    let user = save_with_entity_manager(&user, &entity_manager).await?;
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
    let mut user = EntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager)
        .await?
        .expect("entity_manager user should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

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

    let user = save_with_entity_manager(&user, &entity_manager).await?;
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

    let _first = save_with_entity_manager(&first, &entity_manager).await?;
    let _second = save_with_entity_manager(&second, &entity_manager).await?;

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
    let mut user = EntityManagerCodeUser::find_in_entity_manager(created.id, &entity_manager)
        .await?
        .expect("code user should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .push(EntityManagerCodePost {
            id: 0,
            user_code: String::new(),
            title: "uses-code".to_string(),
        });

    let user = save_with_entity_manager(&user, &entity_manager).await?;
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
    let mut user = EntityManagerSlugUser::find_in_entity_manager(user.id, &entity_manager)
        .await?
        .expect("slug user should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .retain(|post| post.slug != "slug-b");

    save_with_entity_manager(&user, &entity_manager).await?;

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

    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let entity_manager = EntityManager::new(db);
    let first = EntityManagerApiKey::find_in_entity_manager(created.key.clone(), &entity_manager)
        .await?
        .expect("natural-key model should exist");
    let second = EntityManagerApiKey::find_in_entity_manager(created.key.clone(), &entity_manager)
        .await?
        .expect("natural-key model should exist on second lookup");

    let stats = GlobalProfiler::stats();
    assert_eq!(stats.total_queries, 1);
    assert_eq!(first.key, second.key);
    assert_eq!(first.label, second.label);

    GlobalProfiler::disable();
    GlobalProfiler::reset();
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

    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let entity_manager = EntityManager::new(db);
    let first = EntityManagerTeamMembership::find_in_entity_manager(
        (created.team_id, created.member_id),
        &entity_manager,
    )
    .await?
    .expect("composite-key model should exist");
    let second = EntityManagerTeamMembership::find_in_entity_manager(
        (created.team_id, created.member_id),
        &entity_manager,
    )
    .await?
    .expect("composite-key model should exist on second lookup");

    let stats = GlobalProfiler::stats();
    assert_eq!(stats.total_queries, 1);
    assert_eq!(first.team_id, second.team_id);
    assert_eq!(first.member_id, second.member_id);
    assert_eq!(first.role, second.role);

    GlobalProfiler::disable();
    GlobalProfiler::reset();
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
    let mut user = EntityManagerCompositeUser::find_in_entity_manager(user.id, &entity_manager)
        .await?
        .expect("composite parent should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .retain(|post| post.slug != "slug-b");

    save_with_entity_manager(&user, &entity_manager).await?;

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
    let mut user = EntityManagerCompositeUser::find_in_entity_manager(user.id, &entity_manager)
        .await?
        .expect("composite parent should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .push(EntityManagerCompositePost {
            user_id: 0,
            slug: "slug-insert".to_string(),
            title: "Inserted".to_string(),
        });

    let user = save_with_entity_manager(&user, &entity_manager).await?;
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
    let mut user = EntityManagerCompositeUser::find_in_entity_manager(user.id, &entity_manager)
        .await?
        .expect("composite parent should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    let posts = user.posts.as_mut().expect("loaded posts should be mutable");
    assert_eq!(posts.len(), 1);
    posts[0].title = "After".to_string();

    let user = save_with_entity_manager(&user, &entity_manager).await?;
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
    let mut user = EntityManagerAggregateUser::find_in_entity_manager(created.id, &entity_manager)
        .await?
        .expect("aggregate user should exist");
    user.profile.load_in_entity_manager(&entity_manager).await?;
    assert!(user.profile.get_cached().is_none());

    user.profile.set_cached(Some(EntityManagerAggregateProfile {
        id: 0,
        user_id: 0,
        bio: "Bio One".to_string(),
    }));

    let mut user = save_with_entity_manager(&user, &entity_manager).await?;
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

    let mut user = save_with_entity_manager(&user, &entity_manager).await?;
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

    let user = save_with_entity_manager(&user, &entity_manager).await?;
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

    let entity_manager = EntityManager::new(db.clone());
    let _cached_user = entity_manager
        .find::<EntityManagerUser>(cached_user.id)
        .await?
        .expect("cached user should load into the identity map");
    let mut user = EntityManagerAggregateUser::find_in_entity_manager(created.id, &entity_manager)
        .await?
        .expect("aggregate user should exist");
    user.profile.load_in_entity_manager(&entity_manager).await?;

    user.name = "Rolled Back Name".to_string();
    user.profile.set_cached(Some(EntityManagerAggregateProfile {
        id: 0,
        user_id: 0,
        bio: "Conflicting Bio".to_string(),
    }));

    assert!(
        save_with_entity_manager(&user, &entity_manager)
            .await
            .is_err()
    );

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

    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let cached_again = entity_manager
        .find::<EntityManagerUser>(cached_user.id)
        .await?
        .expect("cached user should remain in the identity map after rollback");
    let stats = GlobalProfiler::stats();

    assert_eq!(cached_again.name, "Cached User");
    assert_eq!(stats.total_queries, 0);

    GlobalProfiler::disable();
    GlobalProfiler::reset();

    Ok(())
}

#[tokio::test]
async fn belongs_to_load_in_entity_manager_reuses_cached_parent() -> tideorm::Result<()> {
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
    let _cached_user = EntityManagerAggregateUser::find_in_entity_manager(user.id, &entity_manager)
        .await?
        .expect("cached parent should exist");
    let mut first =
        EntityManagerAggregatePost::find_in_entity_manager(first_post.id, &entity_manager)
            .await?
            .expect("first post should exist");
    let mut second =
        EntityManagerAggregatePost::find_in_entity_manager(second_post.id, &entity_manager)
            .await?
            .expect("second post should exist");

    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let first_author = first
        .author
        .load_in_entity_manager(&entity_manager)
        .await?
        .expect("first author should load from entity_manager");
    let second_author = second
        .author
        .load_in_entity_manager(&entity_manager)
        .await?
        .expect("second author should load from entity_manager");

    let stats = GlobalProfiler::stats();
    assert_eq!(stats.total_queries, 0);
    assert_eq!(first_author.id, user.id);
    assert_eq!(second_author.id, user.id);

    GlobalProfiler::disable();
    GlobalProfiler::reset();
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
    let mut user = EntityManagerAggregateUser::find_in_entity_manager(user.id, &entity_manager)
        .await?
        .expect("graph user should exist");
    user.posts.load_in_entity_manager(&entity_manager).await?;

    let posts = user.posts.as_mut().expect("posts should be loaded");
    assert_eq!(posts.len(), 1);
    posts[0]
        .tags
        .load_in_entity_manager(&entity_manager)
        .await?;

    let tags = posts[0].tags.as_mut().expect("tags should be loaded");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "old-tag");
    tags.clear();
    tags.push(EntityManagerAggregateTag {
        id: 0,
        name: "new-tag".to_string(),
    });

    let user = save_with_entity_manager(&user, &entity_manager).await?;
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
    aggregate
        .profile
        .load_in_entity_manager(&entity_manager)
        .await?;
    managed.replace(aggregate);
    managed.edit(|user| {
        user.profile.as_mut().expect("profile should be loaded").bio =
            "After Relation Flush".to_string();
    });

    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    entity_manager.flush().await?;

    let stats = GlobalProfiler::stats();
    assert_eq!(stats.total_queries, 1);

    GlobalProfiler::disable();
    GlobalProfiler::reset();

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

    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let cached_again = entity_manager
        .find::<EntityManagerUser>(cached_user.id)
        .await?
        .expect("cached user should remain in the identity map after failed flush");
    let stats = GlobalProfiler::stats();

    assert_eq!(cached_again.name, "Cached User");
    assert_eq!(stats.total_queries, 0);

    GlobalProfiler::disable();
    GlobalProfiler::reset();

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

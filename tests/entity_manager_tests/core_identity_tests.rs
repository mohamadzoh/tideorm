use super::*;

#[tokio::test]
async fn tracked_deletion_emits_delete() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;
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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn identity_map_no_duplicate_queries() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;
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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_managers_are_isolated() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;
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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn repeated_save_with_same_new_child_does_not_duplicate() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;
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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn repeated_save_with_same_new_root_does_not_duplicate() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn edited_existing_child_is_saved() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;
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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn identical_new_children_are_persisted_separately() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;
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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn identical_new_roots_are_persisted_separately() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn string_local_key_is_used_for_new_children() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

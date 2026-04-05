use super::*;

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn natural_key_child_delete_uses_model_primary_key() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn natural_key_root_uses_entity_manager_identity_map() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn composite_key_root_uses_entity_manager_identity_map() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn composite_key_child_delete_uses_model_primary_key() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn composite_key_child_insert_is_saved() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn composite_key_child_update_is_saved() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

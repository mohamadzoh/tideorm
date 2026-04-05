use super::*;

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn hasone_insert_update_delete_is_synced() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_manager_save_rolls_back_root_when_relation_sync_fails() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn belongs_to_load_in_entity_manager_reuses_cached_parent() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn nested_has_many_through_changes_are_synced_from_root_save() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_manager_facade_find_load_and_save_supports_all_relation_helpers()
-> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

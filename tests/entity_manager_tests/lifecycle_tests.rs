use super::*;

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_manager_persist_and_flush_inserts_new_root() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_manager_find_managed_and_flush_updates_existing_root() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_manager_flush_persists_relation_only_changes_without_root_update()
-> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_manager_merge_and_flush_updates_existing_root() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_manager_flush_rolls_back_all_managed_writes_on_error() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_manager_remove_and_detach_control_flush_lifecycle() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn entity_manager_clear_drops_cached_identity_and_managed_state() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let db = setup_database().await?;

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
    let _guard = test_lock().lock_owned().await;
    let _db = setup_database().await?;
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

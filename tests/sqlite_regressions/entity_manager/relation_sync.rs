//! Relation syncs diff each copy against what it loaded, NULL keys load
//! nothing, and saved aggregates query the manager's database.

use std::sync::Arc;

use super::Pause;
use crate::fixtures::{global_memory_database, memory_database};
use tideorm::entity_manager::EntityManager;
use tideorm::prelude::*;

#[tideorm::model(table = "sync_users")]
struct SyncUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    #[tideorm(has_many = "SyncPost", foreign_key = "user_id")]
    posts: HasMany<SyncPost>,
}

#[tideorm::model(table = "sync_posts")]
struct SyncPost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,
}

/// Set to hold the next save of a `SyncUser` until told to go on.
static PAUSE: Pause = Pause::new();

impl tideorm::callbacks::Callbacks for SyncUser {
    fn before_save(&mut self) -> tideorm::Result<()> {
        PAUSE.wait();
        Ok(())
    }
}

const SYNC_TABLES: &str = "CREATE TABLE sync_users(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT); \
     CREATE TABLE sync_posts(id INTEGER PRIMARY KEY AUTOINCREMENT, user_id INTEGER, title TEXT)";

/// The sync tables holding user 1 and its `posts`, rows of `sync_posts`.
async fn sync_database(posts: &str) -> Database {
    memory_database(&format!(
        "{SYNC_TABLES}; INSERT INTO sync_users VALUES (1, 'ada'); \
         INSERT INTO sync_posts VALUES {posts}"
    ))
    .await
}

async fn titles(db: &Database) -> Vec<String> {
    SyncPost::query_with(db)
        .order_asc(SyncPost::ID)
        .get()
        .await
        .unwrap()
        .into_iter()
        .map(|post| post.title)
        .collect()
}

/// Two copies of one aggregate: the copy loaded before another one added a
/// child must not delete it when saved, and still deletes what it removed.
#[tokio::test]
async fn a_stale_copy_deletes_only_the_children_it_removed() {
    let db = sync_database("(1, 1, 'p1'), (2, 1, 'p2')").await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let mut first = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    let mut second = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    manager.load(&mut first.posts).await.unwrap();
    manager.load(&mut second.posts).await.unwrap();

    first.posts.as_mut().unwrap().push(SyncPost {
        id: 0,
        user_id: 0,
        title: "p3".into(),
    });
    manager.save(&first).await.unwrap();
    second.name = "renamed".into();
    manager.save(&second).await.unwrap();
    assert_eq!(titles(&db).await, ["p1", "p2", "p3"]);

    second
        .posts
        .as_mut()
        .unwrap()
        .retain(|post| post.title != "p2");
    manager.save(&second).await.unwrap();
    assert_eq!(titles(&db).await, ["p1", "p3"]);
}

#[tideorm::model(table = "sync_articles")]
struct SyncArticle {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    title: String,
    #[tideorm(
        has_many_through = "SyncLabel",
        pivot = "sync_article_labels",
        foreign_key = "article_id",
        related_key = "label_id"
    )]
    labels: HasManyThrough<SyncLabel, SyncArticleLabel>,
}

#[tideorm::model(table = "sync_labels")]
struct SyncLabel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "sync_article_labels")]
struct SyncArticleLabel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    article_id: i64,
    label_id: i64,
}

/// The `HasManyThrough` twin of the test above: a copy loaded before another
/// attached a label must not detach it when saved, and still detaches the one
/// it removed.
#[tokio::test]
async fn a_stale_copy_detaches_only_the_labels_it_removed() {
    let db = memory_database(
        "CREATE TABLE sync_articles(id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT); \
         CREATE TABLE sync_labels(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT); \
         CREATE TABLE sync_article_labels(id INTEGER PRIMARY KEY AUTOINCREMENT, \
             article_id INTEGER, label_id INTEGER); \
         INSERT INTO sync_articles VALUES (1, 'a'); \
         INSERT INTO sync_labels VALUES (1, 'l1'), (2, 'l2'), (3, 'l3'); \
         INSERT INTO sync_article_labels VALUES (1, 1, 1), (2, 1, 2)",
    )
    .await;
    let attached = || async {
        SyncArticleLabel::query_with(&db)
            .order_asc("label_id")
            .get()
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.label_id)
            .collect::<Vec<_>>()
    };
    let manager = EntityManager::new(Arc::new(db.clone()));
    let mut first = manager.find::<SyncArticle>(1).await.unwrap().unwrap();
    let mut second = manager.find::<SyncArticle>(1).await.unwrap().unwrap();
    manager.load(&mut first.labels).await.unwrap();
    manager.load(&mut second.labels).await.unwrap();

    let label = manager.find::<SyncLabel>(3).await.unwrap().unwrap();
    first.labels.as_mut().unwrap().push(label);
    manager.save(&first).await.unwrap();
    second.title = "renamed".into();
    manager.save(&second).await.unwrap();
    assert_eq!(attached().await, [1, 2, 3]);

    second
        .labels
        .as_mut()
        .unwrap()
        .retain(|label| label.id != 2);
    manager.save(&second).await.unwrap();
    assert_eq!(attached().await, [1, 3]);
}

/// The aggregate a save returns carries the relation as saved, so removing a
/// child it added then deletes that child.
#[tokio::test]
async fn a_saved_aggregate_deletes_a_child_it_added_then_removed() {
    let db = sync_database("(1, 1, 'p1')").await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let mut user = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    manager.load(&mut user.posts).await.unwrap();
    user.posts.as_mut().unwrap().push(SyncPost {
        id: 0,
        user_id: 0,
        title: "p2".into(),
    });
    let mut user = manager.save(&user).await.unwrap();
    assert_eq!(titles(&db).await, ["p1", "p2"]);

    user.posts
        .as_mut()
        .unwrap()
        .retain(|post| post.title != "p2");
    manager.save(&user).await.unwrap();
    assert_eq!(titles(&db).await, ["p1"]);
}

/// A flush whose transaction rolls back leaves the entity's relations as they
/// were before it, so the next flush still deletes the child it removed and
/// inserts the one it added, and a new owner's children are inserted with it.
#[tokio::test]
async fn a_rolled_back_flush_leaves_the_relations_to_flush_again() {
    let db = sync_database("(1, 1, 'p1'), (2, 1, 'p2')").await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let user = manager.find_managed::<SyncUser>(1).await.unwrap().unwrap();
    let mut value = user.get();
    manager.load(&mut value.posts).await.unwrap();
    let posts = value.posts.as_mut().unwrap();
    posts.retain(|post| post.title != "p1");
    posts.push(SyncPost {
        id: 0,
        user_id: 0,
        title: "p3".into(),
    });
    user.replace(value);
    let mut owner = SyncUser {
        id: 0,
        name: "bob".into(),
        posts: Default::default(),
    };
    owner.posts.set_cached(vec![SyncPost {
        id: 0,
        user_id: 0,
        title: "bob's post".into(),
    }]);
    let owner = manager.persist(owner);

    let flushing = manager.clone();
    let rolled_back = db
        .transaction(|_| {
            Box::pin(async move {
                flushing.flush().await?;
                Err::<(), _>(tideorm::Error::query("roll back"))
            })
        })
        .await;
    assert!(rolled_back.is_err());
    assert_eq!(titles(&db).await, ["p1", "p2"]);
    assert_eq!(owner.get().id, 0);

    manager.flush().await.unwrap();
    let mut written = titles(&db).await;
    written.sort();
    assert_eq!(written, ["bob's post", "p2", "p3"]);
    let bob = SyncUser::query_with(&db)
        .where_eq("name", "bob")
        .first()
        .await
        .unwrap()
        .unwrap();
    let bobs = SyncPost::query_with(&db)
        .where_eq("user_id", bob.id)
        .count()
        .await
        .unwrap();
    assert_eq!(bobs, 1);
}

/// A manager's unit of work commits on its own database, whatever
/// transaction of another database encloses it: that one rolling back leaves
/// the identity map as the commit left it, and a handle found inside it
/// managed.
#[tokio::test]
async fn another_databases_rollback_leaves_the_managers_work() {
    let other = memory_database("CREATE TABLE unrelated(id INTEGER PRIMARY KEY)").await;
    let db = sync_database("(1, 1, 'p1'), (2, 1, 'p2')").await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let mut user = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    user.name = "saved".into();
    let found = Arc::new(std::sync::Mutex::new(None));
    let (saving, keep) = (manager.clone(), found.clone());
    let rolled_back = other
        .transaction(move |_| {
            Box::pin(async move {
                saving.save(&user).await?;
                let handle = saving.find_managed::<SyncPost>(1).await?;
                *keep.lock().unwrap() = handle;
                Err::<(), _>(tideorm::Error::query("roll back"))
            })
        })
        .await;
    assert!(rolled_back.is_err());

    let user = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    assert_eq!(user.name, "saved");
    let post = found.lock().unwrap().take().unwrap();
    assert_eq!(post.state(), tideorm::entity_manager::EntityState::Managed);
    post.edit(|post| post.title = "edited".into());
    manager.flush().await.unwrap();
    assert_eq!(titles(&db).await, ["edited", "p2"]);
}

/// A relation edited while a flush writes its entity cannot be merged with
/// the rows the flush wrote, and used to be replaced by them: the child added
/// meanwhile was never inserted. The flush now fails and rolls back, which
/// keeps the edit, and the next flush writes everything once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_relation_edited_during_a_flush_is_kept() {
    let db = sync_database("(1, 1, 'p1'), (2, 1, 'p2')").await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let user = manager.find_managed::<SyncUser>(1).await.unwrap().unwrap();
    let mut value = user.get();
    manager.load(&mut value.posts).await.unwrap();
    value.name = "renamed".into();
    user.replace(value);

    let (task, resume_tx) = PAUSE.flush(&manager).await;
    user.edit(|user| {
        user.posts.as_mut().unwrap().push(SyncPost {
            id: 0,
            user_id: 0,
            title: "p3".into(),
        })
    });
    resume_tx.send(()).unwrap();
    let error = task.await.unwrap().unwrap_err().to_string();
    assert!(error.contains("edited while"), "{error}");
    assert_eq!(titles(&db).await, ["p1", "p2"]);

    manager.flush().await.unwrap();
    assert_eq!(titles(&db).await, ["p1", "p2", "p3"]);
    let stored = SyncUser::find_with(1, &db).await.unwrap().unwrap();
    assert_eq!(stored.name, "renamed");
}

/// A copy the manager hands out holds no relation it did not load: saving
/// it leaves a child another save changed meanwhile alone, and loading its
/// relation reads the children other copies added.
#[tokio::test]
async fn a_handed_out_copy_does_not_carry_stale_children() {
    let db = sync_database("(1, 1, 'p1'), (2, 1, 'p2')").await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let mut user = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    manager.load(&mut user.posts).await.unwrap();
    manager.save(&user).await.unwrap();

    let mut post = manager.find::<SyncPost>(1).await.unwrap().unwrap();
    post.title = "p1 edited".into();
    manager.save(&post).await.unwrap();

    let mut renamed = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    renamed.name = "renamed".into();
    manager.save(&renamed).await.unwrap();
    assert_eq!(titles(&db).await, ["p1 edited", "p2"]);

    let mut stale = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    manager.load(&mut stale.posts).await.unwrap();
    let mut adding = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    manager.load(&mut adding.posts).await.unwrap();
    adding.posts.as_mut().unwrap().push(SyncPost {
        id: 0,
        user_id: 0,
        title: "p3".into(),
    });
    manager.save(&adding).await.unwrap();
    stale.name = "again".into();
    manager.save(&stale).await.unwrap();
    let mut reader = manager.find::<SyncUser>(1).await.unwrap().unwrap();
    manager.load(&mut reader.posts).await.unwrap();
    let mut loaded: Vec<String> = reader
        .posts
        .get_cached()
        .unwrap()
        .iter()
        .map(|post| post.title.clone())
        .collect();
    loaded.sort();
    assert_eq!(loaded, ["p1 edited", "p2", "p3"]);
}

/// An aggregate a manager inserted, and its new children, load relations from
/// the manager's database, not the global one: through `save`, and through
/// `persist` and `flush`.
#[tokio::test]
async fn an_inserted_aggregate_loads_from_the_managers_database() {
    let db = memory_database(SYNC_TABLES).await;
    let _global = global_memory_database(&format!(
        "{SYNC_TABLES}; INSERT INTO sync_users VALUES (1, 'global'), (2, 'global'); \
         INSERT INTO sync_posts VALUES (1, 1, 'global'), (2, 2, 'global')"
    ))
    .await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let new_user = |name: &str| {
        let mut user = SyncUser {
            id: 0,
            name: name.into(),
            posts: Default::default(),
        };
        user.posts.set_cached(vec![SyncPost {
            id: 0,
            user_id: 0,
            title: format!("{name}'s post"),
        }]);
        user
    };

    let saved = manager.save(&new_user("ada")).await.unwrap();
    let posts = saved.posts.load().await.unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title, "ada's post");

    let managed = manager.persist(new_user("bob"));
    manager.flush().await.unwrap();
    let bob = managed.get();
    assert_eq!(bob.id, 2);
    let posts = bob.posts.load().await.unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title, "bob's post");
    Database::reset_global();
}

#[tideorm::model(table = "null_key_accounts")]
struct Account {
    #[tideorm(primary_key)]
    id: i64,
    code: Option<String>,
    #[tideorm(has_one = "Badge", foreign_key = "account_code", local_key = "code")]
    badge: HasOne<Badge>,
}

#[tideorm::model(table = "null_key_badges")]
struct Badge {
    #[tideorm(primary_key)]
    id: i64,
    account_code: Option<String>,
    label: String,
}

#[tideorm::model(table = "null_key_tickets")]
struct Ticket {
    #[tideorm(primary_key)]
    id: i64,
    holder_code: Option<String>,
    #[tideorm(
        belongs_to = "Account",
        foreign_key = "holder_code",
        owner_key = "code"
    )]
    holder: BelongsTo<Account>,
}

/// A NULL key has no related row: the manager must not hand back a tracked
/// row whose key is NULL too, and saving must not delete that row.
#[tokio::test]
async fn a_null_key_loads_nothing_through_the_entity_manager() {
    let db = memory_database(
        "CREATE TABLE null_key_accounts(id INTEGER PRIMARY KEY, code TEXT); \
         CREATE TABLE null_key_badges(id INTEGER PRIMARY KEY, account_code TEXT, label TEXT); \
         CREATE TABLE null_key_tickets(id INTEGER PRIMARY KEY, holder_code TEXT); \
         INSERT INTO null_key_accounts VALUES (1, NULL); \
         INSERT INTO null_key_badges VALUES (9, NULL, 'orphan'); \
         INSERT INTO null_key_tickets VALUES (5, NULL)",
    )
    .await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    manager.find::<Badge>(9).await.unwrap().unwrap();
    let mut account = manager.find::<Account>(1).await.unwrap().unwrap();
    assert!(manager.load(&mut account.badge).await.unwrap().is_none());
    account.badge.clear();
    manager.save(&account).await.unwrap();
    assert!(Badge::find_with(9, &db).await.unwrap().is_some());

    let mut ticket = manager.find::<Ticket>(5).await.unwrap().unwrap();
    assert!(manager.load(&mut ticket.holder).await.unwrap().is_none());
}

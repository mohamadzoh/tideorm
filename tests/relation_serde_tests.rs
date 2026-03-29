use tideorm::relations::{HasManyThrough, MorphMany};
use tideorm::{BelongsTo, HasMany, HasOne};

#[tideorm::model(table = "profiles")]
struct Profile {
    #[tideorm(primary_key)]
    pub id: i64,
    pub user_id: i64,
    pub bio: String,
}

#[tideorm::model(table = "posts")]
struct Post {
    #[tideorm(primary_key)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
}

#[tideorm::model(table = "articles")]
struct Article {
    #[tideorm(primary_key)]
    pub id: i64,
    pub user_id: i64,

    #[tideorm(belongs_to = "User", foreign_key = "user_id")]
    pub author: BelongsTo<User>,
}

#[tideorm::model(table = "roles")]
struct Role {
    #[tideorm(primary_key)]
    pub id: i64,
    pub name: String,
}

#[tideorm::model(table = "user_roles")]
struct UserRole {
    #[tideorm(primary_key)]
    pub id: i64,
    pub user_id: i64,
    pub role_id: i64,
}

#[tideorm::model(table = "teams")]
struct Team {
    #[tideorm(primary_key)]
    #[tideorm(column = "team_uuid")]
    pub uuid: String,

    #[tideorm(
        has_many = "TeamMember",
        foreign_key = "team_uuid",
        local_key = "team_uuid"
    )]
    pub members: HasMany<TeamMember>,

    #[tideorm(morph_name = "labelable")]
    pub labels: MorphMany<Role>,
}

#[tideorm::model(table = "team_members")]
struct TeamMember {
    #[tideorm(primary_key)]
    pub id: i64,

    #[tideorm(column = "team_uuid")]
    pub team_ref: String,

    #[tideorm(
        belongs_to = "Team",
        foreign_key = "team_uuid",
        owner_key = "team_uuid"
    )]
    pub team: BelongsTo<Team>,
}

#[tideorm::model(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,

    #[tideorm(has_one = "Profile", foreign_key = "user_id")]
    pub profile: HasOne<Profile>,

    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,

    #[tideorm(
        has_many_through = "Role",
        pivot = "user_roles",
        foreign_key = "user_id",
        related_key = "role_id"
    )]
    pub roles: HasManyThrough<Role, UserRole>,
}

#[test]
fn generated_serde_skips_relation_fields() {
    let user = User {
        id: 1,
        email: "alice@example.com".to_string(),
        name: "Alice".to_string(),
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    };

    let value = serde_json::to_value(&user).unwrap();

    assert_eq!(
        value,
        serde_json::json!({
            "id": 1,
            "email": "alice@example.com",
            "name": "Alice"
        })
    );
    assert!(value.get("profile").is_none());
    assert!(value.get("posts").is_none());
}

#[test]
fn generated_serde_restores_relation_fields_with_defaults() {
    let user: User = serde_json::from_value(serde_json::json!({
        "id": 1,
        "email": "alice@example.com",
        "name": "Alice"
    }))
    .unwrap();

    assert_eq!(user.id, 1);
    assert_eq!(user.email, "alice@example.com");
    assert_eq!(user.name, "Alice");

    let reserialized = serde_json::to_value(&user).unwrap();
    assert_eq!(
        reserialized,
        serde_json::json!({
            "id": 1,
            "email": "alice@example.com",
            "name": "Alice"
        })
    );
}

#[test]
fn with_relations_initializes_supported_relation_wrappers() {
    let user = User {
        id: 7,
        email: "alice@example.com".to_string(),
        name: "Alice".to_string(),
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    }
    .with_relations();

    assert_eq!(user.profile.foreign_key, "user_id");
    assert_eq!(user.profile.local_key, "id");
    assert_eq!(user.posts.foreign_key, "user_id");
    assert_eq!(user.posts.local_key, "id");
    assert_eq!(user.roles.foreign_key, "user_id");
    assert_eq!(user.roles.related_key, "role_id");
    assert_eq!(user.roles.local_key, "id");
    assert_eq!(user.roles.related_local_key, "id");
    assert_eq!(user.roles.pivot_table, "user_roles");

    let article = Article {
        id: 1,
        user_id: 7,
        author: Default::default(),
    }
    .with_relations();

    assert_eq!(article.author.foreign_key, "user_id");
    assert_eq!(article.author.owner_key, "id");

    let team = Team {
        uuid: "team-7".to_string(),
        members: Default::default(),
        labels: Default::default(),
    }
    .with_relations();

    assert_eq!(team.members.foreign_key, "team_uuid");
    assert_eq!(team.members.local_key, "team_uuid");
    assert_eq!(team.labels.morph_name, "labelable");
    assert_eq!(team.labels.local_key, "id");

    let member = TeamMember {
        id: 9,
        team_ref: "team-7".to_string(),
        team: Default::default(),
    }
    .with_relations();

    assert_eq!(member.team.foreign_key, "team_uuid");
    assert_eq!(member.team.owner_key, "team_uuid");
}

#[test]
fn generated_related_impls_expose_relation_defs() {
    use tideorm::internal::InternalModel;
    use tideorm::orm::{Related, RelationType};

    let posts = <<User as InternalModel>::Entity as Related<<Post as InternalModel>::Entity>>::to();
    assert_eq!(posts.rel_type, RelationType::HasMany);

    let profile =
        <<User as InternalModel>::Entity as Related<<Profile as InternalModel>::Entity>>::to();
    assert_eq!(profile.rel_type, RelationType::HasOne);

    let author =
        <<Article as InternalModel>::Entity as Related<<User as InternalModel>::Entity>>::to();
    assert_eq!(author.rel_type, RelationType::HasOne);

    let roles_via =
        <<User as InternalModel>::Entity as Related<<Role as InternalModel>::Entity>>::via();
    assert!(roles_via.is_some());
}

#[tokio::test]
async fn eager_loaded_caches_back_wrapper_loads() {
    let mut user = User {
        id: 7,
        email: "alice@example.com".to_string(),
        name: "Alice".to_string(),
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    }
    .with_relations();

    user.profile.set_cached(Some(Profile {
        id: 1,
        user_id: 7,
        bio: "Hello".to_string(),
    }));
    user.posts.set_cached(vec![Post {
        id: 2,
        user_id: 7,
        title: "Cached".to_string(),
    }]);

    let profile = user.profile.load().await.unwrap();
    let posts = user.posts.load().await.unwrap();

    assert_eq!(profile.unwrap().bio, "Hello");
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title, "Cached");
}

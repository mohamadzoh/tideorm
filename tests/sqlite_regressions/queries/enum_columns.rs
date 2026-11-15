//! An enum column stores its `string_value`, which is not the name serde
//! gives its variant: `pluck()` and `value()` read it as the model does.

use crate::fixtures::memory_database;
use tideorm::prelude::*;

#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    tideorm::orm::EnumIter,
    tideorm::orm::DeriveActiveEnum,
    serde::Serialize,
    serde::Deserialize,
)]
#[orm(rs_type = "String", db_type = "String(StringLen::None)")]
enum Mood {
    #[default]
    #[orm(string_value = "happy")]
    Happy,
    #[orm(string_value = "sad")]
    Sad,
}

#[tideorm::model(table = "enum_feelings")]
struct Feeling {
    #[tideorm(primary_key)]
    id: i64,
    mood: Mood,
    later: Option<Mood>,
}

#[tokio::test]
async fn an_enum_column_plucks_as_its_variants() {
    let db = memory_database(
        "CREATE TABLE enum_feelings (id INTEGER PRIMARY KEY, mood TEXT NOT NULL, later TEXT);
        INSERT INTO enum_feelings VALUES (1, 'happy', 'sad'), (2, 'sad', NULL);",
    )
    .await;
    let query = || Feeling::query_with(&db).order_asc("id");
    assert_eq!(
        query().pluck(Feeling::MOOD).await.unwrap(),
        [Mood::Happy, Mood::Sad]
    );
    assert_eq!(
        query().pluck(Feeling::LATER).await.unwrap(),
        [Some(Mood::Sad), None]
    );
    assert_eq!(
        query().value(Feeling::MOOD).await.unwrap(),
        Some(Mood::Happy)
    );
    // A column named by string reads as asked: the stored text as a String.
    let stored: Vec<String> = query().pluck("mood").await.unwrap();
    assert_eq!(stored, ["happy", "sad"]);
    let moods: Vec<Mood> = query().pluck("mood").await.unwrap();
    assert_eq!(moods, [Mood::Happy, Mood::Sad]);
    let extremes = (
        Feeling::query_with(&db).min::<Mood>("mood").await.unwrap(),
        Feeling::query_with(&db).max::<Mood>("mood").await.unwrap(),
    );
    assert_eq!(extremes, (Some(Mood::Happy), Some(Mood::Sad)));
}

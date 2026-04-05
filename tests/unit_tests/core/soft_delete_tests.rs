use chrono::Utc;

#[test]
fn test_deleted_at_timestamp() {
    let now = Utc::now();
    let later = now + chrono::Duration::seconds(1);
    assert!(later > now);
}

#[test]
fn test_optional_deleted_at() {
    let deleted_at: Option<chrono::DateTime<Utc>> = None;
    assert!(deleted_at.is_none());

    let deleted_at: Option<chrono::DateTime<Utc>> = Some(Utc::now());
    assert!(deleted_at.is_some());
}

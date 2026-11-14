use super::{bounded_index_name, is_reserved, is_snake_case};

#[test]
fn snake_case_is_lowercase_words_joined_once() {
    for name in ["users", "user_roles", "s_3_objects", "a1"] {
        assert!(is_snake_case(name), "{name}");
    }
    for name in [
        "Users",
        "userRoles",
        "_users",
        "users_",
        "user__roles",
        "1users",
        "",
    ] {
        assert!(!is_snake_case(name), "{name}");
    }
}

#[test]
fn reserved_words_match_in_any_case() {
    assert!(is_reserved("order"));
    assert!(is_reserved("User"));
    assert!(!is_reserved("orders"));
}

#[test]
fn index_names_are_bounded_as_the_runtime_bounds_them() {
    assert_eq!(
        bounded_index_name("idx_users_email".into()),
        "idx_users_email"
    );
    let long = format!("idx_{}_email", "a".repeat(70));
    let bounded = bounded_index_name(long.clone());
    assert_eq!(bounded.len(), 63);
    assert!(bounded.starts_with(&long[..54]));
    // `tideorm::migration::ddl::bounded_index_name` of the same name.
    assert_eq!(&bounded[54..], "_59c31944");
}

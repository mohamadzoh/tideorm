// =============================================================================
// BATCH UPDATE VALUE TESTS
// =============================================================================

#[cfg(test)]
mod batch_update_value_tests {
    use serde_json::json;
    use tideorm::model::UpdateValue;

    #[test]
    fn test_update_value_variants() {
        // Test all UpdateValue variants can be created
        let _value = UpdateValue::Value(json!("hello"));
        let _trusted_raw = UpdateValue::UnsafeRaw("NOW()".to_string());
        let _inc = UpdateValue::Increment(5);
        let _dec = UpdateValue::Decrement(3);
        let _mul = UpdateValue::Multiply(2.5);
        let _div = UpdateValue::Divide(4.0);
        let _append = UpdateValue::ArrayAppend(json!("item"));
        let _remove = UpdateValue::ArrayRemove(json!("item"));
        let _json_set = UpdateValue::JsonSet("$.path".to_string(), json!("value"));
        let _coalesce = UpdateValue::Coalesce(json!("default"));
    }

    #[test]
    fn test_update_value_value() {
        let value = UpdateValue::Value(json!("hello"));
        match value {
            UpdateValue::Value(v) => assert_eq!(v, json!("hello")),
            _ => panic!("Expected UpdateValue::Value"),
        }
    }

    #[test]
    fn test_update_value_increment() {
        let value = UpdateValue::Increment(5);
        match value {
            UpdateValue::Increment(n) => assert_eq!(n, 5),
            _ => panic!("Expected UpdateValue::Increment"),
        }
    }

    #[test]
    fn test_update_value_decrement() {
        let value = UpdateValue::Decrement(3);
        match value {
            UpdateValue::Decrement(n) => assert_eq!(n, 3),
            _ => panic!("Expected UpdateValue::Decrement"),
        }
    }

    #[test]
    fn test_update_value_multiply() {
        let value = UpdateValue::Multiply(2.5);
        match value {
            UpdateValue::Multiply(n) => assert!((n - 2.5).abs() < f64::EPSILON),
            _ => panic!("Expected UpdateValue::Multiply"),
        }
    }

    #[test]
    fn test_update_value_divide() {
        let value = UpdateValue::Divide(4.0);
        match value {
            UpdateValue::Divide(n) => assert!((n - 4.0).abs() < f64::EPSILON),
            _ => panic!("Expected UpdateValue::Divide"),
        }
    }

    #[test]
    fn test_update_value_unsafe_raw() {
        let value = UpdateValue::UnsafeRaw("NOW()".to_string());
        match value {
            UpdateValue::UnsafeRaw(s) => assert_eq!(s, "NOW()"),
            _ => panic!("Expected UpdateValue::UnsafeRaw"),
        }
    }

    #[test]
    fn test_update_value_array_append() {
        let value = UpdateValue::ArrayAppend(json!("new_item"));
        match value {
            UpdateValue::ArrayAppend(v) => assert_eq!(v, json!("new_item")),
            _ => panic!("Expected UpdateValue::ArrayAppend"),
        }
    }

    #[test]
    fn test_update_value_array_remove() {
        let value = UpdateValue::ArrayRemove(json!("old_item"));
        match value {
            UpdateValue::ArrayRemove(v) => assert_eq!(v, json!("old_item")),
            _ => panic!("Expected UpdateValue::ArrayRemove"),
        }
    }

    #[test]
    fn test_update_value_json_set() {
        let value = UpdateValue::JsonSet("$.path".to_string(), json!("value"));
        match value {
            UpdateValue::JsonSet(path, val) => {
                assert_eq!(path, "$.path");
                assert_eq!(val, json!("value"));
            }
            _ => panic!("Expected UpdateValue::JsonSet"),
        }
    }

    #[test]
    fn test_update_value_coalesce() {
        let value = UpdateValue::Coalesce(json!("default"));
        match value {
            UpdateValue::Coalesce(v) => assert_eq!(v, json!("default")),
            _ => panic!("Expected UpdateValue::Coalesce"),
        }
    }

    #[test]
    fn test_update_value_clone() {
        let original = UpdateValue::Increment(10);
        let cloned = original.clone();
        match cloned {
            UpdateValue::Increment(n) => assert_eq!(n, 10),
            _ => panic!("Expected UpdateValue::Increment"),
        }
    }

    #[test]
    fn test_update_value_debug() {
        let value = UpdateValue::UnsafeRaw("test_expr".to_string());
        let debug_str = format!("{:?}", value);
        assert!(debug_str.contains("UnsafeRaw"));
        assert!(debug_str.contains("test_expr"));
    }
}

// =============================================================================
// RELATION CONSTRAINTS TESTS
// =============================================================================

#[cfg(test)]
mod relation_constraints_tests {
    use tideorm::query::Order;
    use tideorm::relations::RelationConstraints;

    #[test]
    fn test_relation_constraints_new() {
        let constraints = RelationConstraints::new();
        assert!(constraints.conditions.is_empty());
        assert!(constraints.order_by.is_none());
        assert!(constraints.limit.is_none());
        assert!(constraints.offset.is_none());
        assert!(!constraints.with_trashed);
    }

    #[test]
    fn test_relation_constraints_where_eq() {
        let constraints = RelationConstraints::new().where_eq("status", "active");

        assert_eq!(constraints.conditions.len(), 1);
        assert_eq!(constraints.conditions[0].0, "status");
        assert_eq!(constraints.conditions[0].1, serde_json::json!("active"));
    }

    #[test]
    fn test_relation_constraints_multiple_where() {
        let constraints = RelationConstraints::new()
            .where_eq("status", "active")
            .where_eq("verified", true);

        assert_eq!(constraints.conditions.len(), 2);
    }

    #[test]
    fn test_relation_constraints_order_by_asc() {
        let constraints = RelationConstraints::new().order_by("created_at", Order::Asc);

        let (col, order) = constraints.order_by.unwrap();
        assert_eq!(col, "created_at");
        match order {
            Order::Asc => {}
            _ => panic!("Expected Order::Asc"),
        }
    }

    #[test]
    fn test_relation_constraints_order_by_desc() {
        let constraints = RelationConstraints::new().order_by("created_at", Order::Desc);

        let (col, order) = constraints.order_by.unwrap();
        assert_eq!(col, "created_at");
        match order {
            Order::Desc => {}
            _ => panic!("Expected Order::Desc"),
        }
    }

    #[test]
    fn test_relation_constraints_limit() {
        let constraints = RelationConstraints::new().limit(10);

        assert_eq!(constraints.limit, Some(10));
    }

    #[test]
    fn test_relation_constraints_offset() {
        let constraints = RelationConstraints::new().offset(20);

        assert_eq!(constraints.offset, Some(20));
    }

    #[test]
    fn test_relation_constraints_with_trashed() {
        let constraints = RelationConstraints::new().with_trashed();

        assert!(constraints.with_trashed);
    }

    #[test]
    fn test_relation_constraints_chained() {
        let constraints = RelationConstraints::new()
            .where_eq("status", "published")
            .where_eq("visible", true)
            .order_by("created_at", Order::Desc)
            .limit(10)
            .offset(0)
            .with_trashed();

        assert_eq!(constraints.conditions.len(), 2);
        assert!(constraints.order_by.is_some());
        assert_eq!(constraints.limit, Some(10));
        assert_eq!(constraints.offset, Some(0));
        assert!(constraints.with_trashed);
    }
}

#[cfg(test)]
mod hashed_type_tests {
    use tideorm::types::Hashed;

    #[test]
    fn test_hashed_uses_argon2_format() {
        let hashed = Hashed::new("secret123");
        assert!(hashed.hash().starts_with("$argon2"));
    }

    #[test]
    fn test_hashed_verify_accepts_matching_password() {
        let hashed = Hashed::new("secret123");
        assert!(hashed.verify("secret123"));
        assert!(!hashed.verify("wrong-password"));
    }

    #[test]
    fn test_hashed_is_salted() {
        let first = Hashed::new("secret123");
        let second = Hashed::new("secret123");

        assert_ne!(first.hash(), second.hash());
        assert!(first.verify("secret123"));
        assert!(second.verify("secret123"));
    }

    #[test]
    fn test_hashed_verify_rejects_non_argon2_hashes() {
        let hashed = Hashed::from_hash("legacy-hash-value".to_string());

        assert!(!hashed.verify("secret123"));
        assert!(!hashed.verify("wrong-password"));
    }
}

// =============================================================================
// ATTRIBUTE CASTING TESTS
// =============================================================================

#[cfg(test)]
mod attribute_casting_tests {
    use std::sync::Once;

    use serde_json::json;
    use tideorm::tokenization::TokenConfig;
    use tideorm::types::{
        CastType, CastValue, Collection, CommaSeparated, Encrypted, Hashed, WithDefault,
    };

    static ENCRYPTED_INIT: Once = Once::new();

    fn init_encrypted_test_key() {
        ENCRYPTED_INIT.call_once(|| {
            TokenConfig::set_encryption_key("test-encryption-key-for-unit-tests-32");
        });
    }

    // Encrypted type tests
    #[test]
    fn test_encrypted_new() {
        let encrypted = Encrypted::new("secret".to_string());
        assert_eq!(encrypted.get(), "secret");
    }

    #[test]
    fn test_encrypted_into_inner() {
        let encrypted = Encrypted::new("secret".to_string());
        let inner: String = encrypted.into_inner();
        assert_eq!(inner, "secret");
    }

    #[test]
    fn test_encrypted_clone() {
        let encrypted = Encrypted::new("secret".to_string());
        let cloned = encrypted.clone();
        assert_eq!(cloned.get(), "secret");
    }

    #[test]
    fn test_encrypted_inner() {
        let encrypted = Encrypted::new("secret".to_string());
        assert_eq!(encrypted.inner(), "secret");
    }

    #[test]
    fn test_encrypted_from() {
        let encrypted: Encrypted<String> = "secret".to_string().into();
        assert_eq!(encrypted.get(), "secret");
    }

    #[test]
    fn test_encrypted_serializes_to_ciphertext() {
        init_encrypted_test_key();

        let encrypted = Encrypted::new("secret".to_string());
        let serialized = serde_json::to_value(&encrypted).unwrap();

        let ciphertext = serialized.as_str().unwrap();
        assert!(ciphertext.starts_with("enc::"));
        assert_ne!(ciphertext, "secret");
        assert!(!ciphertext.contains("secret"));
    }

    #[test]
    fn test_encrypted_round_trips_from_ciphertext() {
        init_encrypted_test_key();

        let encrypted = Encrypted::new("secret".to_string());
        let serialized = serde_json::to_value(&encrypted).unwrap();
        let round_trip: Encrypted<String> = serde_json::from_value(serialized).unwrap();

        assert_eq!(round_trip.get(), "secret");
    }

    #[test]
    fn test_encrypted_rejects_plaintext_payloads() {
        let err =
            serde_json::from_value::<Encrypted<String>>(serde_json::json!("secret")).unwrap_err();

        assert!(
            err.to_string()
                .contains("Encrypted fields must use the encrypted payload format")
        );
    }

    #[test]
    fn test_encrypted_rejects_tampered_ciphertext() {
        init_encrypted_test_key();

        let encrypted = Encrypted::new("secret".to_string());
        let serialized = serde_json::to_value(&encrypted).unwrap();
        let ciphertext = serialized.as_str().unwrap();
        let mut chars: Vec<char> = ciphertext.chars().collect();
        let tamper_index = chars.len().saturating_sub(4);
        chars[tamper_index] = if chars[tamper_index] == 'A' { 'B' } else { 'A' };
        let tampered: String = chars.into_iter().collect();

        let err =
            serde_json::from_value::<Encrypted<String>>(serde_json::json!(tampered)).unwrap_err();
        assert!(
            err.to_string().contains("Failed to decrypt field payload")
                || err.to_string().contains("Invalid encrypted field payload")
        );
    }

    // Hashed type tests
    #[test]
    fn test_hashed_new() {
        let hashed = Hashed::new("password123");
        // Hashed value is stored as hash, not plain text
        assert!(!hashed.hash().is_empty());
    }

    #[test]
    fn test_hashed_verify() {
        let hashed = Hashed::new("password123");
        assert!(hashed.verify("password123"));
        assert!(!hashed.verify("wrongpassword"));
    }

    #[test]
    fn test_hashed_from_str() {
        let hashed: Hashed = "password123".into();
        assert!(hashed.verify("password123"));
    }

    #[test]
    fn test_hashed_from_string() {
        let hashed: Hashed = "password123".to_string().into();
        assert!(hashed.verify("password123"));
    }

    #[test]
    fn test_hashed_serialize_redacts_raw_hash() {
        let hashed = Hashed::new("password123");

        let serialized = serde_json::to_value(&hashed).unwrap();

        assert_eq!(serialized, serde_json::json!("***HASHED***"));
        assert_ne!(serialized, serde_json::json!(hashed.hash()));
    }

    #[test]
    fn test_hashed_nested_serialization_does_not_leak_argon2_hash() {
        #[derive(serde::Serialize)]
        struct UserPayload {
            password: Hashed,
        }

        let payload = UserPayload {
            password: Hashed::new("password123"),
        };

        let serialized = serde_json::to_value(&payload).unwrap();
        let password = serialized
            .get("password")
            .and_then(serde_json::Value::as_str)
            .unwrap();

        assert_eq!(password, "***HASHED***");
        assert!(!password.starts_with("$argon2"));
    }

    #[test]
    fn test_hashed_deserialize_rejects_redacted_payload() {
        let err = serde_json::from_value::<Hashed>(serde_json::json!("***HASHED***")).unwrap_err();

        assert!(err.to_string().contains("redacted serialization format"));
    }

    // CommaSeparated type tests
    #[test]
    fn test_comma_separated_new() {
        let values = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let cs = CommaSeparated::new(values);
        assert_eq!(cs.to_string(), "a,b,c");
    }

    #[test]
    fn test_comma_separated_from_string() {
        let cs = CommaSeparated::<String>::from_string("a,b,c");
        let values = cs.values();
        assert_eq!(values, &["a", "b", "c"]);
    }

    #[test]
    fn test_comma_separated_empty() {
        let cs = CommaSeparated::<String>::new(vec![]);
        assert_eq!(cs.to_string(), "");
        assert!(cs.is_empty());
    }

    #[test]
    fn test_comma_separated_single_item() {
        let cs = CommaSeparated::new(vec!["single".to_string()]);
        assert_eq!(cs.to_string(), "single");
    }

    #[test]
    fn test_comma_separated_integers() {
        let cs = CommaSeparated::new(vec![1i32, 2, 3, 4, 5]);
        assert_eq!(cs.to_string(), "1,2,3,4,5");
    }

    #[test]
    fn test_comma_separated_push() {
        let mut cs = CommaSeparated::new(vec!["a".to_string(), "b".to_string()]);
        cs.push("c".to_string());
        assert_eq!(cs.values(), &["a", "b", "c"]);
    }

    #[test]
    fn test_comma_separated_contains() {
        let cs = CommaSeparated::new(vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        assert!(cs.contains(&"b".to_string()));
        assert!(!cs.contains(&"d".to_string()));
    }

    #[test]
    fn test_comma_separated_len() {
        let cs = CommaSeparated::new(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(cs.len(), 2);
    }

    #[test]
    fn test_comma_separated_is_empty() {
        let empty = CommaSeparated::<String>::new(vec![]);
        let non_empty = CommaSeparated::new(vec!["a".to_string()]);
        assert!(empty.is_empty());
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_comma_separated_from_vec() {
        let cs: CommaSeparated<String> = vec!["x".to_string(), "y".to_string()].into();
        assert_eq!(cs.len(), 2);
    }

    // Collection type tests
    #[test]
    fn test_collection_new() {
        let collection = Collection::<i32>::new();
        assert!(collection.is_empty());
    }

    #[test]
    fn test_collection_from_vec() {
        let collection = Collection::from_vec(vec![1, 2, 3]);
        assert_eq!(collection.count(), 3);
    }

    #[test]
    fn test_collection_add() {
        let mut collection = Collection::new();
        collection.add(1);
        collection.add(2);
        assert_eq!(collection.count(), 2);
    }

    #[test]
    fn test_collection_to_vec() {
        let collection = Collection::from_vec(vec![1, 2, 3]);
        let vec = collection.to_vec();
        assert_eq!(vec, vec![1, 2, 3]);
    }

    #[test]
    fn test_collection_all() {
        let collection = Collection::from_vec(vec![1, 2, 3]);
        assert_eq!(collection.all(), &[1, 2, 3]);
    }

    #[test]
    fn test_collection_first() {
        let collection = Collection::from_vec(vec![1, 2, 3]);
        assert_eq!(collection.first(), Some(&1));

        let empty = Collection::<i32>::new();
        assert_eq!(empty.first(), None);
    }

    #[test]
    fn test_collection_last() {
        let collection = Collection::from_vec(vec![1, 2, 3]);
        assert_eq!(collection.last(), Some(&3));
    }

    #[test]
    fn test_collection_filter() {
        let collection = Collection::from_vec(vec![1, 2, 3, 4, 5]);
        let even = collection.filter(|x| *x % 2 == 0);
        assert_eq!(even.to_vec(), vec![2, 4]);
    }

    #[test]
    fn test_collection_map() {
        let collection = Collection::from_vec(vec![1, 2, 3]);
        let doubled: Collection<i32> = collection.map(|x| x * 2);
        assert_eq!(doubled.to_vec(), vec![2, 4, 6]);
    }

    #[test]
    fn test_collection_find() {
        let collection = Collection::from_vec(vec![1, 2, 3, 4, 5]);
        let found = collection.find(|x| *x == 3);
        assert_eq!(found, Some(&3));

        let not_found = collection.find(|x| *x == 10);
        assert_eq!(not_found, None);
    }

    #[test]
    fn test_collection_any() {
        let collection = Collection::from_vec(vec![1, 2, 3]);
        assert!(collection.any(|x| *x == 2));
        assert!(!collection.any(|x| *x == 5));
    }

    #[test]
    fn test_collection_every() {
        let collection = Collection::from_vec(vec![2, 4, 6]);
        assert!(collection.every(|x| *x % 2 == 0));
        assert!(!collection.every(|x| *x > 3));
    }

    #[test]
    fn test_collection_take() {
        let collection = Collection::from_vec(vec![1, 2, 3, 4, 5]);
        let taken = collection.take(3);
        assert_eq!(taken.to_vec(), vec![1, 2, 3]);
    }

    #[test]
    fn test_collection_skip() {
        let collection = Collection::from_vec(vec![1, 2, 3, 4, 5]);
        let skipped = collection.skip(2);
        assert_eq!(skipped.to_vec(), vec![3, 4, 5]);
    }

    // CastType tests
    #[test]
    fn test_cast_type_from_str() {
        assert_eq!(CastType::parse_str("string"), Some(CastType::String));
        assert_eq!(CastType::parse_str("integer"), Some(CastType::Integer));
        assert_eq!(CastType::parse_str("float"), Some(CastType::Float));
        assert_eq!(CastType::parse_str("boolean"), Some(CastType::Boolean));
        assert_eq!(CastType::parse_str("json"), Some(CastType::Json));
        assert_eq!(CastType::parse_str("array"), Some(CastType::Array));
        assert_eq!(CastType::parse_str("datetime"), Some(CastType::DateTime));
        assert_eq!(CastType::parse_str("date"), Some(CastType::Date));
        assert_eq!(CastType::parse_str("time"), Some(CastType::Time));
        assert_eq!(CastType::parse_str("uuid"), Some(CastType::Uuid));
        assert_eq!(CastType::parse_str("decimal"), Some(CastType::Decimal));
        assert_eq!(CastType::parse_str("encrypted"), Some(CastType::Encrypted));
        assert_eq!(CastType::parse_str("hashed"), Some(CastType::Hashed));
        assert_eq!(
            CastType::parse_str("comma_separated"),
            Some(CastType::CommaSeparated)
        );
        assert_eq!(
            CastType::parse_str("collection"),
            Some(CastType::Collection)
        );
        assert_eq!(CastType::parse_str("unknown"), None);
    }

    #[test]
    fn test_cast_type_display() {
        assert_eq!(CastType::String.to_string(), "string");
        assert_eq!(CastType::Integer.to_string(), "integer");
        assert_eq!(CastType::Boolean.to_string(), "boolean");
    }

    // CastValue tests
    #[test]
    fn test_cast_value_to_string() {
        let result = CastValue::cast(&json!(123), CastType::String);
        assert_eq!(result.unwrap(), json!("123"));
    }

    #[test]
    fn test_cast_value_to_integer() {
        let result = CastValue::cast(&json!("42"), CastType::Integer);
        assert_eq!(result.unwrap(), json!(42));

        let result2 = CastValue::cast(&json!(3.14), CastType::Integer);
        assert_eq!(result2.unwrap(), json!(3));
    }

    #[test]
    fn test_cast_value_to_float() {
        let result = CastValue::cast(&json!("3.14"), CastType::Float);
        assert_eq!(result.unwrap(), json!(3.14));

        let result2 = CastValue::cast(&json!(42), CastType::Float);
        assert_eq!(result2.unwrap(), json!(42.0));
    }

    #[test]
    fn test_cast_value_to_boolean() {
        assert_eq!(
            CastValue::cast(&json!("true"), CastType::Boolean).unwrap(),
            json!(true)
        );
        assert_eq!(
            CastValue::cast(&json!("false"), CastType::Boolean).unwrap(),
            json!(false)
        );
        assert_eq!(
            CastValue::cast(&json!(1), CastType::Boolean).unwrap(),
            json!(true)
        );
        assert_eq!(
            CastValue::cast(&json!(0), CastType::Boolean).unwrap(),
            json!(false)
        );
        assert_eq!(
            CastValue::cast(&json!("1"), CastType::Boolean).unwrap(),
            json!(true)
        );
        assert_eq!(
            CastValue::cast(&json!("0"), CastType::Boolean).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn test_cast_value_to_array_from_array() {
        let result = CastValue::cast(&json!([1, 2, 3]), CastType::Array);
        assert_eq!(result.unwrap(), json!([1, 2, 3]));
    }

    #[test]
    fn test_cast_value_json_passthrough() {
        let value = json!({"key": "value"});
        let result = CastValue::cast(&value, CastType::Json);
        assert_eq!(result.unwrap(), value);
    }

    #[test]
    fn test_cast_value_decimal_passthrough() {
        let result = CastValue::cast(&json!(3.14159), CastType::Decimal);
        assert_eq!(result.unwrap(), json!(3.14159));
    }

    #[test]
    fn test_cast_value_parse_comma_separated() {
        let result = CastValue::parse_comma_separated("a,b,c");
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_cast_value_format_comma_separated() {
        let result = CastValue::format_comma_separated(&["a", "b", "c"]);
        assert_eq!(result, "a,b,c");
    }

    // WithDefault tests
    #[test]
    fn test_with_default_none() {
        let wd: WithDefault<i32> = WithDefault::none();
        assert!(wd.is_none());
        assert!(!wd.is_some());
    }

    #[test]
    fn test_with_default_some() {
        let wd = WithDefault::some(42);
        assert!(wd.is_some());
        assert!(!wd.is_none());
    }

    #[test]
    fn test_with_default_unwrap_or() {
        let wd_none: WithDefault<i32> = WithDefault::none();
        assert_eq!(wd_none.unwrap_or(0), 0);

        let wd_some = WithDefault::some(42);
        assert_eq!(wd_some.unwrap_or(0), 42);
    }

    #[test]
    fn test_with_default_unwrap_or_else() {
        let wd: WithDefault<i32> = WithDefault::none();
        assert_eq!(wd.unwrap_or_else(|| 100), 100);

        let wd_some = WithDefault::some(42);
        assert_eq!(wd_some.unwrap_or_else(|| 100), 42);
    }

    #[test]
    fn test_with_default_into_option() {
        let wd = WithDefault::some("hello".to_string());
        assert_eq!(wd.into_option(), Some("hello".to_string()));

        let wd_none: WithDefault<String> = WithDefault::none();
        assert_eq!(wd_none.into_option(), None);
    }
}

#[cfg(test)]
mod soft_delete_query_tests {
    use tideorm::prelude::*;

    #[tideorm::model(
        table = "query_soft_delete_override",
        soft_delete,
        deleted_at_column = "archived_on"
    )]
    struct CustomSoftDeleteModel {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
        archived_on: Option<chrono::DateTime<chrono::Utc>>,
    }

    #[test]
    fn test_soft_delete_query_uses_overridden_column() {
        let sql = CustomSoftDeleteModel::query().build_sql_preview();
        assert!(sql.contains("\"archived_on\" IS NULL"));
        assert!(!sql.contains("\"deleted_at\" IS NULL"));
    }

    #[test]
    fn test_only_trashed_query_uses_overridden_column() {
        let sql = CustomSoftDeleteModel::query()
            .only_trashed()
            .build_sql_preview();
        assert!(sql.contains("\"archived_on\" IS NOT NULL"));
        assert!(!sql.contains("\"deleted_at\" IS NOT NULL"));
    }
}

// =============================================================================
// RELATION FIELD TYPE TESTS (SeaORM-style)
// =============================================================================

#[cfg(test)]
mod relation_field_type_tests {
    use serde_json::json;
    use tideorm::relations::{
        BelongsTo, HasMany, HasManyThrough, HasOne, MorphMany, MorphOne, RelationConstraints,
    };

    #[tideorm::model(table = "relation_field_test_models")]
    struct RelationFieldTestModel {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }

    #[tideorm::model(table = "relation_field_test_pivots")]
    struct RelationFieldPivotTestModel {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
    }

    // =========================================================================
    // HasOne TESTS
    // =========================================================================

    #[test]
    fn test_has_one_default_has_none_cached() {
        let relation = HasOne::<RelationFieldTestModel>::default();

        assert_eq!(relation.foreign_key, "");
        assert_eq!(relation.local_key, "");
        assert!(relation.get_cached().is_none());
    }

    #[tokio::test]
    async fn test_has_one_default_fails_loudly_when_unconfigured() {
        let relation = HasOne::<RelationFieldTestModel>::default().with_parent_pk(json!(1));

        let err = relation.load().await.unwrap_err();
        assert!(
            err.to_string()
                .contains("HasOne relation is not configured")
        );
    }

    // =========================================================================
    // HasMany TESTS
    // =========================================================================

    #[test]
    fn test_has_many_default_has_none_cached() {
        let relation = HasMany::<RelationFieldTestModel>::default();

        assert_eq!(relation.foreign_key, "");
        assert_eq!(relation.local_key, "");
        assert!(relation.get_cached().is_none());
    }

    #[tokio::test]
    async fn test_has_many_default_fails_loudly_when_unconfigured() {
        let relation = HasMany::<RelationFieldTestModel>::default().with_parent_pk(json!(1));

        let err = relation.load().await.unwrap_err();
        assert!(
            err.to_string()
                .contains("HasMany relation is not configured")
        );
    }

    // =========================================================================
    // BelongsTo TESTS
    // =========================================================================

    #[test]
    fn test_belongs_to_default_has_none_cached() {
        let relation = BelongsTo::<RelationFieldTestModel>::default();

        assert_eq!(relation.foreign_key, "");
        assert_eq!(relation.owner_key, "");
        assert!(relation.get_cached().is_none());
    }

    #[tokio::test]
    async fn test_belongs_to_default_fails_loudly_when_unconfigured() {
        let relation = BelongsTo::<RelationFieldTestModel>::default().with_fk_value(json!(1));

        let err = relation.load().await.unwrap_err();
        assert!(
            err.to_string()
                .contains("BelongsTo relation is not configured")
        );
    }

    #[test]
    fn test_has_many_through_default_has_none_cached() {
        let relation =
            HasManyThrough::<RelationFieldTestModel, RelationFieldPivotTestModel>::default();

        assert_eq!(relation.foreign_key, "");
        assert_eq!(relation.related_key, "");
        assert_eq!(relation.local_key, "");
        assert_eq!(relation.related_local_key, "");
        assert_eq!(relation.pivot_table, "");
        assert!(relation.get_cached().is_none());
    }

    #[tokio::test]
    async fn test_has_many_through_default_fails_loudly_when_unconfigured() {
        let relation =
            HasManyThrough::<RelationFieldTestModel, RelationFieldPivotTestModel>::default()
                .with_parent_pk(json!(1));

        let err = relation.load().await.unwrap_err();
        assert!(
            err.to_string()
                .contains("HasManyThrough relation is not configured")
        );
    }

    #[test]
    fn test_morph_one_default_has_none_cached() {
        let relation = MorphOne::<RelationFieldTestModel>::default();

        assert_eq!(relation.morph_name, "");
        assert_eq!(relation.local_key, "");
        assert!(relation.get_cached().is_none());
    }

    #[tokio::test]
    async fn test_morph_one_default_fails_loudly_when_unconfigured() {
        let relation = MorphOne::<RelationFieldTestModel>::default();

        let err = relation.load().await.unwrap_err();
        assert!(
            err.to_string()
                .contains("MorphOne relation is not configured")
        );
    }

    #[test]
    fn test_morph_many_default_has_none_cached() {
        let relation = MorphMany::<RelationFieldTestModel>::default();

        assert_eq!(relation.morph_name, "");
        assert_eq!(relation.local_key, "");
        assert!(relation.get_cached().is_none());
    }

    #[tokio::test]
    async fn test_morph_many_default_fails_loudly_when_unconfigured() {
        let relation = MorphMany::<RelationFieldTestModel>::default();

        let err = relation.load().await.unwrap_err();
        assert!(
            err.to_string()
                .contains("MorphMany relation is not configured")
        );
    }

    // =========================================================================
    // RelationConstraints TESTS
    // =========================================================================

    #[test]
    fn test_relation_constraints_default() {
        let constraints = RelationConstraints::default();
        assert!(constraints.conditions.is_empty());
        assert!(constraints.order_by.is_none());
        assert!(constraints.limit.is_none());
        assert!(constraints.offset.is_none());
    }

    #[test]
    fn test_relation_constraints_with_where() {
        let constraints = RelationConstraints::default().where_eq("status", json!("active"));

        assert_eq!(constraints.conditions.len(), 1);
        assert_eq!(constraints.conditions[0].0, "status");
        assert_eq!(constraints.conditions[0].1, json!("active"));
    }

    #[test]
    fn test_relation_constraints_chained() {
        use tideorm::query::Order;
        let constraints = RelationConstraints::default()
            .where_eq("active", json!(true))
            .where_eq("published", json!(true))
            .order_by("created_at", Order::Desc)
            .limit(10)
            .offset(5);

        assert_eq!(constraints.conditions.len(), 2);
        assert_eq!(
            constraints.order_by,
            Some(("created_at".to_string(), Order::Desc))
        );
        assert_eq!(constraints.limit, Some(10));
        assert_eq!(constraints.offset, Some(5));
    }

    #[test]
    fn test_relation_constraints_order_asc() {
        use tideorm::query::Order;
        let constraints = RelationConstraints::default().order_by("name", Order::Asc);

        assert_eq!(constraints.order_by, Some(("name".to_string(), Order::Asc)));
    }

    #[test]
    fn test_relation_constraints_clone() {
        let constraints = RelationConstraints::default()
            .where_eq("status", json!("active"))
            .limit(5);

        let cloned = constraints.clone();
        assert_eq!(cloned.conditions.len(), 1);
        assert_eq!(cloned.limit, Some(5));
    }
}

// =============================================================================
// ADVANCED RELATIONS TESTS
// =============================================================================

#[cfg(test)]
mod advanced_relations_tests {
    use tideorm::relations::{
        MorphResult, MorphResult3, MorphResult4, RelationInfo, RelationPath, RelationTree,
        RelationType, WithPivot,
    };

    // =========================================================================
    // RELATION TYPE TESTS
    // =========================================================================

    #[test]
    fn test_relation_type_display_has_many_through() {
        assert_eq!(
            format!("{}", RelationType::HasManyThrough),
            "has_many_through"
        );
    }

    #[test]
    fn test_relation_type_display_morph_to() {
        assert_eq!(format!("{}", RelationType::MorphTo), "morph_to");
    }

    #[test]
    fn test_relation_type_display_morph_one() {
        assert_eq!(format!("{}", RelationType::MorphOne), "morph_one");
    }

    #[test]
    fn test_relation_type_display_morph_many() {
        assert_eq!(format!("{}", RelationType::MorphMany), "morph_many");
    }

    #[test]
    fn test_relation_type_equality() {
        assert_eq!(RelationType::HasManyThrough, RelationType::HasManyThrough);
        assert_ne!(RelationType::HasManyThrough, RelationType::HasMany);
        assert_ne!(RelationType::MorphOne, RelationType::MorphMany);
    }

    // =========================================================================
    // RELATION INFO BUILDER TESTS
    // =========================================================================

    #[test]
    fn test_relation_info_belongs_to_builder() {
        let info = RelationInfo::belongs_to("author", "users", "user_id", "id");

        assert_eq!(info.name, "author");
        assert_eq!(info.relation_type, RelationType::BelongsTo);
        assert_eq!(info.related_table, "users");
        assert_eq!(info.foreign_key, "user_id");
        assert_eq!(info.local_key, "id");
        assert!(info.pivot_table.is_none());
    }

    #[test]
    fn test_relation_info_has_one_builder() {
        let info = RelationInfo::has_one("profile", "profiles", "user_id", "id");

        assert_eq!(info.name, "profile");
        assert_eq!(info.relation_type, RelationType::HasOne);
    }

    #[test]
    fn test_relation_info_has_many_builder() {
        let info = RelationInfo::has_many("posts", "posts", "user_id", "id");

        assert_eq!(info.name, "posts");
        assert_eq!(info.relation_type, RelationType::HasMany);
    }

    #[test]
    fn test_relation_info_has_many_through_builder() {
        let info = RelationInfo::has_many_through(
            "roles",
            "roles",
            "user_roles",
            "user_id",
            "role_id",
            "id",
        );

        assert_eq!(info.name, "roles");
        assert_eq!(info.relation_type, RelationType::HasManyThrough);
        assert_eq!(info.pivot_table, Some("user_roles".to_string()));
    }

    #[test]
    fn test_relation_info_morph_one_builder() {
        let info =
            RelationInfo::morph_one("image", "images", "imageable_type", "imageable_id", "id");

        assert_eq!(info.name, "image");
        assert_eq!(info.relation_type, RelationType::MorphOne);
        assert_eq!(info.morph_type_column, Some("imageable_type".to_string()));
        assert_eq!(info.morph_id_column, Some("imageable_id".to_string()));
    }

    #[test]
    fn test_relation_info_morph_many_builder() {
        let info = RelationInfo::morph_many(
            "comments",
            "comments",
            "commentable_type",
            "commentable_id",
            "id",
        );

        assert_eq!(info.name, "comments");
        assert_eq!(info.relation_type, RelationType::MorphMany);
    }

    // =========================================================================
    // RELATION PATH TESTS (for nested eager loading)
    // =========================================================================

    #[test]
    fn test_relation_path_simple() {
        let path = RelationPath::parse("posts");

        assert_eq!(path.full_path, "posts");
        assert_eq!(path.segments.len(), 1);
        assert_eq!(path.root(), "posts");
        assert!(!path.is_nested());
        assert_eq!(path.depth(), 1);
        assert!(path.nested().is_none());
    }

    #[test]
    fn test_relation_path_nested() {
        let path = RelationPath::parse("posts.comments");

        assert_eq!(path.full_path, "posts.comments");
        assert_eq!(path.segments.len(), 2);
        assert_eq!(path.root(), "posts");
        assert!(path.is_nested());
        assert_eq!(path.depth(), 2);

        let nested = path.nested().unwrap();
        assert_eq!(nested.full_path, "comments");
        assert_eq!(nested.root(), "comments");
        assert!(!nested.is_nested());
    }

    #[test]
    fn test_relation_path_deeply_nested() {
        let path = RelationPath::parse("posts.comments.author");

        assert_eq!(path.depth(), 3);
        assert!(path.is_nested());

        let nested1 = path.nested().unwrap();
        assert_eq!(nested1.root(), "comments");
        assert!(nested1.is_nested());

        let nested2 = nested1.nested().unwrap();
        assert_eq!(nested2.root(), "author");
        assert!(!nested2.is_nested());
    }

    #[test]
    fn test_relation_path_empty() {
        let path = RelationPath::parse("");

        assert_eq!(path.depth(), 1);
        assert_eq!(path.root(), "");
    }

    // =========================================================================
    // RELATION TREE TESTS
    // =========================================================================

    #[test]
    fn test_relation_tree_new() {
        let tree = RelationTree::new();

        assert!(tree.is_empty());
        assert!(tree.roots().is_empty());
    }

    #[test]
    fn test_relation_tree_add_simple_path() {
        let mut tree = RelationTree::new();
        tree.add_path(&RelationPath::parse("posts"));

        assert!(!tree.is_empty());
        let roots = tree.roots();
        assert_eq!(roots.len(), 1);
        assert!(roots.contains(&"posts".to_string()));
        assert!(!tree.has_nested("posts"));
    }

    #[test]
    fn test_relation_tree_add_nested_path() {
        let mut tree = RelationTree::new();
        tree.add_path(&RelationPath::parse("posts.comments"));

        let roots = tree.roots();
        assert_eq!(roots.len(), 1);
        assert!(tree.has_nested("posts"));

        let nested = tree.get_nested("posts").unwrap();
        assert!(nested.roots().contains(&"comments".to_string()));
    }

    #[test]
    fn test_relation_tree_multiple_paths() {
        let mut tree = RelationTree::new();
        tree.add_path(&RelationPath::parse("posts"));
        tree.add_path(&RelationPath::parse("profile"));
        tree.add_path(&RelationPath::parse("posts.comments"));
        tree.add_path(&RelationPath::parse("posts.comments.author"));

        let roots = tree.roots();
        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&"posts".to_string()));
        assert!(roots.contains(&"profile".to_string()));

        // Profile has no nested
        assert!(!tree.has_nested("profile"));

        // Posts has nested comments
        assert!(tree.has_nested("posts"));
        let posts_nested = tree.get_nested("posts").unwrap();
        assert!(posts_nested.roots().contains(&"comments".to_string()));

        // Comments has nested author
        assert!(posts_nested.has_nested("comments"));
    }

    // =========================================================================
    // MORPH RESULT TESTS
    // =========================================================================

    #[derive(Debug, Clone, PartialEq)]
    struct Post {
        id: i32,
        title: String,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Video {
        id: i32,
        url: String,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Image {
        id: i32,
        path: String,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Audio {
        id: i32,
        file: String,
    }

    #[test]
    fn test_morph_result_type_a() {
        let post = Post {
            id: 1,
            title: "Hello".to_string(),
        };
        let result: MorphResult<Post, Video> = MorphResult::TypeA(post.clone());

        assert!(result.is_type_a());
        assert!(!result.is_type_b());
        assert!(!result.is_unknown());
        assert_eq!(result.as_type_a(), Some(&post));
        assert_eq!(result.as_type_b(), None);
    }

    #[test]
    fn test_morph_result_type_b() {
        let video = Video {
            id: 1,
            url: "http://example.com".to_string(),
        };
        let result: MorphResult<Post, Video> = MorphResult::TypeB(video.clone());

        assert!(!result.is_type_a());
        assert!(result.is_type_b());
        assert_eq!(result.as_type_b(), Some(&video));
    }

    #[test]
    fn test_morph_result_unknown() {
        let result: MorphResult<Post, Video> =
            MorphResult::Unknown(serde_json::json!({"type": "document"}));

        assert!(!result.is_type_a());
        assert!(!result.is_type_b());
        assert!(result.is_unknown());
    }

    #[test]
    fn test_morph_result_into_type_a() {
        let post = Post {
            id: 1,
            title: "Hello".to_string(),
        };
        let result: MorphResult<Post, Video> = MorphResult::TypeA(post.clone());

        assert_eq!(result.into_type_a(), Some(post));
    }

    #[test]
    fn test_morph_result_into_type_b() {
        let video = Video {
            id: 1,
            url: "http://example.com".to_string(),
        };
        let result: MorphResult<Post, Video> = MorphResult::TypeB(video.clone());

        assert_eq!(result.into_type_b(), Some(video));
    }

    #[test]
    fn test_morph_result3() {
        let _result: MorphResult3<Post, Video, Image> = MorphResult3::TypeA(Post {
            id: 1,
            title: "Test".to_string(),
        });
        let _result: MorphResult3<Post, Video, Image> = MorphResult3::TypeB(Video {
            id: 1,
            url: "url".to_string(),
        });
        let _result: MorphResult3<Post, Video, Image> = MorphResult3::TypeC(Image {
            id: 1,
            path: "path".to_string(),
        });
        let _result: MorphResult3<Post, Video, Image> =
            MorphResult3::Unknown(serde_json::json!({}));
    }

    #[test]
    fn test_morph_result4() {
        let _result: MorphResult4<Post, Video, Image, Audio> = MorphResult4::TypeA(Post {
            id: 1,
            title: "Test".to_string(),
        });
        let _result: MorphResult4<Post, Video, Image, Audio> = MorphResult4::TypeB(Video {
            id: 1,
            url: "url".to_string(),
        });
        let _result: MorphResult4<Post, Video, Image, Audio> = MorphResult4::TypeC(Image {
            id: 1,
            path: "path".to_string(),
        });
        let _result: MorphResult4<Post, Video, Image, Audio> = MorphResult4::TypeD(Audio {
            id: 1,
            file: "file".to_string(),
        });
        let _result: MorphResult4<Post, Video, Image, Audio> =
            MorphResult4::Unknown(serde_json::json!({}));
    }

    // =========================================================================
    // WITH PIVOT TESTS
    // =========================================================================

    #[derive(Debug, Clone)]
    struct Role {
        id: i32,
        name: String,
    }

    #[derive(Debug, Clone)]
    struct UserRolePivot {
        assigned_at: String,
        role_level: i32,
    }

    #[test]
    fn test_with_pivot_creation() {
        let role = Role {
            id: 1,
            name: "Admin".to_string(),
        };
        let pivot = UserRolePivot {
            assigned_at: "2024-01-01".to_string(),
            role_level: 10,
        };

        let with_pivot = WithPivot::new(role.clone(), pivot.clone());

        assert_eq!(with_pivot.model.id, 1);
        assert_eq!(with_pivot.model.name, "Admin");
        assert_eq!(with_pivot.pivot.assigned_at, "2024-01-01");
        assert_eq!(with_pivot.pivot.role_level, 10);
    }

    #[test]
    fn test_with_pivot_deref() {
        let role = Role {
            id: 1,
            name: "Admin".to_string(),
        };
        let pivot = UserRolePivot {
            assigned_at: "2024-01-01".to_string(),
            role_level: 10,
        };

        let with_pivot = WithPivot::new(role, pivot);

        // Test Deref - can access model fields directly
        assert_eq!(with_pivot.id, 1);
        assert_eq!(with_pivot.name, "Admin");
    }

    #[test]
    fn test_with_pivot_into_parts() {
        let role = Role {
            id: 1,
            name: "Admin".to_string(),
        };
        let pivot = UserRolePivot {
            assigned_at: "2024-01-01".to_string(),
            role_level: 10,
        };

        let with_pivot = WithPivot::new(role, pivot);
        let (model, pivot) = with_pivot.into_parts();

        assert_eq!(model.id, 1);
        assert_eq!(pivot.role_level, 10);
    }
}

// =============================================================================

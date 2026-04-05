use super::*;

#[test]
fn relation_ext_get_field_value_reads_single_fields_by_field_or_column_name() {
    let model = RelationExtLookupModel {
        id: 7,
        account_id: 41,
    }
    .with_relations();

    assert_eq!(model.get_field_value("account_id").unwrap(), json!(41));
    assert_eq!(model.get_field_value("owner_id").unwrap(), json!(41));
    assert_eq!(model.get_field_value("id").unwrap(), json!(7));
}

#[test]
fn relation_ext_get_field_value_falls_back_to_serialized_relation_fields() {
    let mut parent = RelationExtParentModel {
        id: 42,
        name: "parent".to_string(),
        child: Default::default(),
    }
    .with_relations();

    parent.child.set_cached(Some(RelationExtChildModel {
        id: 7,
        parent_id: 42,
        label: "child".to_string(),
    }));

    assert_eq!(
        parent.get_field_value("child").unwrap(),
        json!({ "id": 7, "parent_id": 42, "label": "child" })
    );
}

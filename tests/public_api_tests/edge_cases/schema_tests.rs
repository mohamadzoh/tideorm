use tideorm::config::DatabaseType;
use tideorm::schema::{ColumnSchema, SchemaGenerator, TableSchemaBuilder};

#[test]
fn test_column_schema_is_nullable_by_default() {
    let col = ColumnSchema::new("name", "TEXT");
    assert!(col.nullable);
}

#[test]
fn test_schema_generator_emits_every_added_table() {
    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);

    for i in 0..5 {
        let schema = TableSchemaBuilder::new(format!("table_{}", i))
            .column(ColumnSchema::new("id", "BIGINT").primary_key())
            .build();
        generator.add_table(schema);
    }

    let sql = generator.generate();
    for i in 0..5 {
        assert!(
            sql.contains(&format!("table_{i}")),
            "missing table_{i}: {sql}"
        );
    }
}

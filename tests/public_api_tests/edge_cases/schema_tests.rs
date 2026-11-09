use tideorm::config::DatabaseType;
use tideorm::schema::{ColumnSchema, SchemaGenerator, TableSchemaBuilder};

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
    let tables: String = (0..5)
        .map(|i| {
            format!(
                "CREATE TABLE IF NOT EXISTS \"table_{i}\" (\n    \"id\" BIGINT,\n    PRIMARY KEY (\"id\")\n);\n\n"
            )
        })
        .collect();
    let (_, statements) = sql.split_once(" UTC\n\n").expect("the header ends");
    assert_eq!(statements, tables);
}

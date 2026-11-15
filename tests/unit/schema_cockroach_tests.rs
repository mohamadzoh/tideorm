use super::{unqualified, without_public_schema};

#[test]
fn view_tables_lose_the_database_name() {
    let view = "CREATE VIEW public.v (id) AS SELECT t.id, 'app.public.t' AS lit \
                FROM app.public.t JOIN \"app\".s2.x AS u ON u.id = t.id \
                JOIN other.public.t AS o ON o.id = t.id";
    assert_eq!(
        unqualified(view, "app"),
        "CREATE VIEW public.v (id) AS SELECT t.id, 'app.public.t' AS lit \
         FROM public.t JOIN s2.x AS u ON u.id = t.id \
         JOIN other.public.t AS o ON o.id = t.id"
    );
}

#[test]
fn two_part_names_stay_and_a_quoted_database_goes() {
    let sql = "SELECT t.id FROM app.t, \"My DB\".public.u";
    assert_eq!(unqualified(sql, "app"), sql);
    assert_eq!(
        unqualified(sql, "My DB"),
        "SELECT t.id FROM app.t, public.u"
    );
}

#[test]
fn the_public_schema_keeps_only_its_comment() {
    let mut plain = "CREATE SCHEMA public;".to_owned();
    assert!(!without_public_schema(&mut plain));
    let mut bare = "CREATE SCHEMA public".to_owned();
    assert!(!without_public_schema(&mut bare));
    // CockroachDB writes a comment as an escape string.
    let comment = r"COMMENT ON SCHEMA public IS e'pub \'c\'';";
    let mut commented = format!("CREATE SCHEMA public;\n{comment}");
    assert!(without_public_schema(&mut commented));
    assert_eq!(commented, comment);
    let mut other = "CREATE SCHEMA public_v2;".to_owned();
    assert!(without_public_schema(&mut other));
    assert_eq!(other, "CREATE SCHEMA public_v2;");
}

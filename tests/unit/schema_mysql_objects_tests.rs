use super::localize;

#[test]
fn insert_and_replace_localize_optional_into_and_modifiers() {
    for function in ["INSERT(src.name, 1, 1, 'x')", "REPLACE(src.name, 'x', 'y')"] {
        assert_eq!(
            localize(
                &format!(
                    "SELECT {function}
         FROM src.items AS src"
                ),
                "src",
                "",
                false
            ),
            format!(
                "SELECT {function}
         FROM items AS src"
            )
        );
    }

    for verb in ["INSERT", "REPLACE"] {
        for modifier in ["", "LOW_PRIORITY ", "DELAYED ", "HIGH_PRIORITY IGNORE "] {
            for into in ["", "INTO "] {
                let sql = format!("{verb} {modifier}{into}/* table */ `src`.`items` VALUES(7)");
                let expected = format!("{verb} {modifier}{into}/* table */ `items` VALUES(7)");
                assert_eq!(localize(&sql, "src", "", false), expected);
            }
        }
    }
    assert_eq!(
        localize(
            "INSERT src.items SELECT 'src.items' FROM external.items",
            "src",
            "",
            false
        ),
        "INSERT items SELECT 'src.items' FROM external.items"
    );
}

#[test]
fn comments_and_qualified_delete_targets_are_portable() {
    for whitespace in ["\t", "\r", "\x0b", "\x0c", " "] {
        let sql =
            format!("BEGIN --{whitespace}don't parse this quote\nDELETE src.a FROM src.a; END");
        let expected = format!("BEGIN --{whitespace}don't parse this quote\nDELETE a FROM a; END");
        assert_eq!(localize(&sql, "src", "", false), expected);
    }
    for (sql, expected) in [
        (
            "DELETE LOW_PRIORITY IGNORE src.a, src.b FROM src.a JOIN src.b",
            "DELETE LOW_PRIORITY IGNORE a, b FROM a JOIN b",
        ),
        (
            "DELETE FROM src.a, src.b USING src.a JOIN src.b",
            "DELETE FROM a, b USING a JOIN b",
        ),
        ("DELETE `src`.`a` FROM `src`.`a`", "DELETE `a` FROM `a`"),
    ] {
        assert_eq!(localize(sql, "src", "", false), expected);
    }
}

#[test]
fn local_database_relations_are_rebound_without_rewriting_aliases_or_literals() {
    assert_eq!(
        localize("DELETE FROM SRC.a", "src", "", true),
        "DELETE FROM a"
    );
    assert_eq!(
        localize("DELETE FROM SRC.a", "src", "", false),
        "DELETE FROM SRC.a"
    );

    for (sql, expected) in [
        ("TRUNCATE src.a", "TRUNCATE a"),
        (
            "RENAME TABLE src.a TO src.b, src.c TO src.d",
            "RENAME TABLE a TO b, c TO d",
        ),
        ("DROP INDEX idx ON src.a", "DROP INDEX idx ON a"),
        (
            "LOCK TABLES src.a WRITE, src.b READ",
            "LOCK TABLES a WRITE, b READ",
        ),
        (
            "DELETE x,y FROM src.a x, src.b y",
            "DELETE x,y FROM a x, b y",
        ),
        (
            "UPDATE src.a x, src.b y SET x.id=y.id",
            "UPDATE a x, b y SET x.id=y.id",
        ),
        (
            "SELECT src.id, src.name
         FROM src.a AS src",
            "SELECT src.id, src.name
         FROM a AS src",
        ),
        (
            "SELECT COALESCE(src.id, src.other)
         FROM src.a src",
            "SELECT COALESCE(src.id, src.other)
         FROM a src",
        ),
        (
            "SELECT *
         FROM (src.a x, src.b y) JOIN src.c z ON x.id=z.id",
            "SELECT *
         FROM (a x, b y) JOIN c z ON x.id=z.id",
        ),
        (
            "SELECT *
         FROM src.a x, (SELECT *
         FROM src.b y, src.c z) d, src.e",
            "SELECT *
         FROM a x, (SELECT *
         FROM b y, c z) d, e",
        ),
        (
            "CREATE TRIGGER src.guard BEFORE INSERT ON src.a FOR EACH ROW SET NEW.id=1",
            "CREATE TRIGGER guard BEFORE INSERT ON a FOR EACH ROW SET NEW.id=1",
        ),
        (
            "SELECT src.a.id, src.f(1), 'src.a'
         FROM src.a /* src.b */",
            "SELECT a.id, f(1), 'src.a'
         FROM a /* src.b */",
        ),
        (
            "ALTER TABLE src.a ADD FOREIGN KEY (id) REFERENCES src.b(id)",
            "ALTER TABLE a ADD FOREIGN KEY (id) REFERENCES b(id)",
        ),
    ] {
        assert_eq!(localize(sql, "src", "", false), expected, "{sql}");
    }
}

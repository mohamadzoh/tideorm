//! JSON containment for SQLite, which has no `@>` or `<@` operator.
//!
//! PostgreSQL's reading is rebuilt from `json_each` rows: an object candidate
//! needs every member contained under the same key, an array candidate needs
//! every element contained by some element, and a scalar has to equal the
//! value — or, at the top level only, be an element of an array. Keys and
//! scalars are bound as parameters in the order their `?` appear.

use crate::internal::Value;

/// One JSON node the test inspects: the column itself, or a `json_each` row.
struct Node {
    /// The node's JSON text.
    json: String,
    /// Its type as `json_type` and `json_each.type` spell it.
    kind: String,
    /// Its value as an SQL scalar.
    atom: String,
}

impl Node {
    fn row(alias: &str) -> Self {
        Self {
            json: format!("{alias}.value"),
            kind: format!("{alias}.type"),
            atom: format!("{alias}.atom"),
        }
    }
}

/// SQL testing that the JSON document in `column_sql` contains `candidate`.
pub(crate) fn sqlite_json_contains(
    column_sql: &str,
    candidate: &serde_json::Value,
) -> (String, Vec<Value>) {
    let root = Node {
        json: column_sql.to_string(),
        kind: format!("json_type({column_sql})"),
        atom: format!("json_extract({column_sql}, '$')"),
    };
    let mut builder = Containment::default();
    let sql = builder.contains(&root, candidate, true);
    (sql, builder.values)
}

/// SQL testing that the JSON document in `column_sql` is contained by
/// `container`: the same test with the known document on the other side.
pub(crate) fn sqlite_json_contained_by(
    column_sql: &str,
    container: &serde_json::Value,
) -> (String, Vec<Value>) {
    let root = Node {
        json: column_sql.to_string(),
        kind: format!("json_type({column_sql})"),
        atom: format!("json_extract({column_sql}, '$')"),
    };
    let mut builder = Containment::default();
    let sql = builder.contained_by(&root, container, true);
    (sql, builder.values)
}

#[derive(Default)]
struct Containment {
    values: Vec<Value>,
    aliases: usize,
}

impl Containment {
    fn alias(&mut self) -> String {
        self.aliases += 1;
        format!("tide_json_{}", self.aliases)
    }

    fn contains(&mut self, node: &Node, candidate: &serde_json::Value, top_level: bool) -> String {
        use serde_json::Value as Json;

        match candidate {
            Json::Object(members) => {
                let mut checks = vec![format!("{} = 'object'", node.kind)];
                for (key, value) in members {
                    let alias = self.alias();
                    self.values.push(Value::from(key.clone()));
                    let member = self.contains(&Node::row(&alias), value, false);
                    checks.push(format!(
                        "EXISTS (SELECT 1 FROM json_each({}) AS {alias} WHERE {alias}.key = ? AND {member})",
                        node.json
                    ));
                }
                format!("({})", checks.join(" AND "))
            }
            Json::Array(elements) => {
                let mut checks = vec![format!("{} = 'array'", node.kind)];
                for element in elements {
                    checks.push(self.some_element(node, element));
                }
                format!("({})", checks.join(" AND "))
            }
            scalar if top_level => {
                let equal = self.scalar_equals(node, scalar);
                let element = self.some_element(node, scalar);
                format!("({equal} OR ({} = 'array' AND {element}))", node.kind)
            }
            scalar => self.scalar_equals(node, scalar),
        }
    }

    /// `container` contains `node`: every member of an object node sits under
    /// a key the container has and is contained by its value there, every
    /// element of an array node is contained by some element of the container,
    /// and a scalar node equals the container, or, at the top level only, an
    /// element of it.
    fn contained_by(
        &mut self,
        node: &Node,
        container: &serde_json::Value,
        top_level: bool,
    ) -> String {
        use serde_json::Value as Json;

        match container {
            Json::Object(members) => {
                let alias = self.alias();
                let row = Node::row(&alias);
                let mut placed = Vec::new();
                for (key, value) in members {
                    self.values.push(Value::from(key.clone()));
                    let member = self.contained_by(&row, value, false);
                    placed.push(format!("({alias}.key = ? AND {member})"));
                }
                format!(
                    "({} = 'object' AND NOT EXISTS (SELECT 1 FROM json_each({}) AS {alias} WHERE NOT {}))",
                    node.kind,
                    node.json,
                    any_of(placed)
                )
            }
            Json::Array(elements) => {
                let alias = self.alias();
                let row = Node::row(&alias);
                let placed: Vec<String> = elements
                    .iter()
                    .map(|element| self.contained_by(&row, element, false))
                    .collect();
                let array = format!(
                    "({} = 'array' AND NOT EXISTS (SELECT 1 FROM json_each({}) AS {alias} WHERE NOT {}))",
                    node.kind,
                    node.json,
                    any_of(placed)
                );
                if !top_level {
                    return array;
                }
                let scalars: Vec<String> = elements
                    .iter()
                    .filter(|element| !element.is_array() && !element.is_object())
                    .map(|element| self.scalar_equals(node, element))
                    .collect();
                format!("({array} OR {})", any_of(scalars))
            }
            scalar => self.scalar_equals(node, scalar),
        }
    }

    /// Some element of the array `node` contains `candidate`.
    fn some_element(&mut self, node: &Node, candidate: &serde_json::Value) -> String {
        let alias = self.alias();
        let element = self.contains(&Node::row(&alias), candidate, false);
        format!(
            "EXISTS (SELECT 1 FROM json_each({}) AS {alias} WHERE {element})",
            node.json
        )
    }

    fn scalar_equals(&mut self, node: &Node, scalar: &serde_json::Value) -> String {
        use serde_json::Value as Json;

        match scalar {
            Json::Null => format!("{} = 'null'", node.kind),
            Json::Bool(true) => format!("{} = 'true'", node.kind),
            Json::Bool(false) => format!("{} = 'false'", node.kind),
            Json::Number(number) => {
                self.values.push(
                    number
                        .as_i64()
                        .map_or_else(|| Value::from(number.as_f64()), Value::from),
                );
                format!(
                    "({} IN ('integer', 'real') AND {} = ?)",
                    node.kind, node.atom
                )
            }
            Json::String(text) => {
                self.values.push(Value::from(text.clone()));
                format!("({} = 'text' AND {} = ?)", node.kind, node.atom)
            }
            Json::Array(_) | Json::Object(_) => unreachable!("containers are not scalars"),
        }
    }
}

/// `alternatives` joined with OR, parenthesized; an empty list is false.
fn any_of(alternatives: Vec<String>) -> String {
    if alternatives.is_empty() {
        "(0)".to_string()
    } else {
        format!("({})", alternatives.join(" OR "))
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/query_sqlite_json_tests.rs"]
mod tests;

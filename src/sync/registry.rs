use parking_lot::RwLock;
use regex::Regex;

use super::{ModelSchema, SyncRegistry};

pub(super) static MODEL_SCHEMAS: RwLock<Vec<ModelSchema>> = RwLock::new(Vec::new());

/// Distributed registration emitted by `#[tideorm::model]` for sync discovery and column metadata.
#[doc(hidden)]
pub struct CompiledModelRegistration {
    pub source_path: &'static str,
    pub sync_schema: fn() -> ModelSchema,
    /// The model's table, which a query joining it names.
    pub table_name: fn() -> &'static str,
    pub schema_name: fn() -> Option<&'static str>,
    /// Physical column names used to resolve wildcard projections.
    pub column_names: fn() -> &'static [&'static str],
    /// Encryption metadata used by projections of a joined model.
    pub encrypted_column: fn(&str) -> crate::Result<Option<crate::model::EncryptedColumn>>,
    /// The model's type for one of its columns, which binds a filter value on
    /// that column when another model's query joins the table.
    pub column_type: ColumnTypeOf,
}

/// A model's column-type lookup, as its registration carries it.
#[doc(hidden)]
pub type ColumnTypeOf = fn(&str) -> Option<crate::orm::ColumnType>;

inventory::collect!(CompiledModelRegistration);

/// The column type a compiled model declares for `column` of `table`, including
/// its schema when qualified: `None` when no model matches that identity.
pub(crate) fn registered_column_type(table: &str, column: &str) -> Option<crate::orm::ColumnType> {
    let (schema, table) = table
        .rsplit_once('.')
        .map_or((None, table), |(schema, table)| (Some(schema), table));
    inventory::iter::<CompiledModelRegistration>
        .into_iter()
        .find(|model| (model.table_name)() == table && (model.schema_name)() == schema)
        .and_then(|model| (model.column_type)(column))
}

/// Columns of the compiled model for a table in the requested schema.
pub(crate) fn registered_column_names(
    schema: Option<&str>,
    table: &str,
) -> &'static [&'static str] {
    inventory::iter::<CompiledModelRegistration>
        .into_iter()
        .find(|model| (model.table_name)() == table && (model.schema_name)() == schema)
        .map_or(&[], |model| (model.column_names)())
}

/// Resolve encryption metadata without conflating same-named tables in different schemas.
pub(crate) fn registered_encrypted_column(
    schema: Option<&str>,
    table: &str,
    column: &str,
) -> crate::Result<Option<crate::model::EncryptedColumn>> {
    for model in inventory::iter::<CompiledModelRegistration> {
        if (model.table_name)() == table
            && (model.schema_name)() == schema
            && let Some(source) = (model.encrypted_column)(column)?
        {
            return Ok(Some(source));
        }
    }
    Ok(None)
}

pub(super) fn register_compiled_models_matching(pattern: &str) -> usize {
    let Some(compiled_pattern) = compile_glob_pattern(pattern) else {
        return 0;
    };

    let mut registered = 0;

    for model in inventory::iter::<CompiledModelRegistration> {
        if glob_path_matches(&compiled_pattern, model.source_path) {
            SyncRegistry::register_schema((model.sync_schema)());
            registered += 1;
        }
    }

    registered
}

fn compile_glob_pattern(pattern: &str) -> Option<Regex> {
    let normalized_pattern = normalize_path(pattern);
    let regex = glob_to_regex(&normalized_pattern);

    Regex::new(&regex).ok()
}

fn glob_path_matches(pattern: &Regex, candidate: &str) -> bool {
    let normalized_candidate = normalize_path(candidate);
    pattern.is_match(&normalized_candidate)
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::from("^");
    let mut chars = pattern.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        regex.push_str("(?:[^/]+/)*");
                    } else {
                        regex.push_str(".*");
                    }
                } else {
                    regex.push_str("[^/]*");
                }
            }
            '?' => regex.push_str("[^/]"),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }

    regex.push('$');
    regex
}

/// Trait for models that can be synced with the database
///
/// This trait is automatically implemented by TideORM's model macros, which
/// describe the model's table as a `ModelSchema`.
pub trait SyncModel {
    /// Get the schema for this model
    fn sync_schema() -> ModelSchema;

    /// Register this model for synchronization
    fn register_for_sync() {
        SyncRegistry::register_schema(Self::sync_schema());
    }
}

/// Trait for registering one model or several at once
///
/// This is implemented for every model and for tuples of up to 16 model types.
/// Used by `TideConfig::models::<User>()` and `TideConfig::models::<(User, Post)>()`.
pub trait RegisterModels {
    /// Register the model, or every model in the tuple
    fn register_all();
}

impl<M: SyncModel> RegisterModels for M {
    fn register_all() {
        M::register_for_sync();
    }
}

impl RegisterModels for () {
    fn register_all() {}
}

macro_rules! impl_register_models_tuples {
    ($first:ident) => {
        impl<$first: SyncModel> RegisterModels for ($first,) {
            fn register_all() {
                $first::register_for_sync();
            }
        }
    };
    ($first:ident, $($rest:ident),+) => {
        impl_register_models_tuples!($($rest),+);

        impl<$first: SyncModel, $($rest: SyncModel),+> RegisterModels for ($first, $($rest),+) {
            fn register_all() {
                $first::register_for_sync();
                $($rest::register_for_sync();)+
            }
        }
    };
}

impl_register_models_tuples!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16
);

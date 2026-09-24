use parking_lot::RwLock;
use regex::Regex;

use super::{ModelSchema, SyncRegistry};

pub(super) static MODEL_SCHEMAS: RwLock<Vec<ModelSchema>> = RwLock::new(Vec::new());

/// Distributed registration emitted by `#[tideorm::model]` for source-path-based sync discovery.
#[doc(hidden)]
pub struct CompiledModelRegistration {
    pub source_path: &'static str,
    pub sync_schema: fn() -> ModelSchema,
}

inventory::collect!(CompiledModelRegistration);

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

/// Trait for registering multiple models at once
///
/// This is implemented for tuples of up to 16 model types.
/// Used by `TideConfig::models::<(Model1, Model2, ...)>()`.
pub trait RegisterModels {
    /// Register all models in this tuple
    fn register_all();
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

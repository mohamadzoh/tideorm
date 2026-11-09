use super::{Chunks, MYSQL_MAX_STATEMENT_BYTES, Order, Strategy, held_keys, next_chunk, strategy};
use crate::config::DatabaseType;
use crate::internal::InternalModel;

#[tideorm::model(table = "batch_insert_counters")]
struct Counter {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    label: String,
}

#[tideorm::model(table = "batch_insert_tokens")]
struct Token {
    #[tideorm(primary_key)]
    id: uuid::Uuid,
    label: String,
}

#[tideorm::model(table = "batch_insert_codes")]
struct Code {
    #[tideorm(primary_key)]
    code: String,
    label: String,
}

fn active_models<M: InternalModel>(models: Vec<M>) -> Vec<M::ActiveModel> {
    models
        .into_iter()
        .map(|model| model.try_into_active_model().expect("the model converts"))
        .collect()
}

fn counters() -> Vec<<Counter as InternalModel>::ActiveModel> {
    active_models(
        (0..3)
            .map(|i| Counter {
                id: 0,
                label: format!("counter {i}"),
            })
            .collect(),
    )
}

fn tokens(labels: &[&str]) -> Vec<Token> {
    labels
        .iter()
        .map(|label| Token {
            id: uuid::Uuid::new_v4(),
            label: label.to_string(),
        })
        .collect()
}

fn codes() -> Vec<<Code as InternalModel>::ActiveModel> {
    active_models(
        ["a", "b"]
            .into_iter()
            .map(|code| Code {
                code: code.to_string(),
                label: format!("code {code}"),
            })
            .collect(),
    )
}

/// The strategy `insert_many` picks for `models`.
fn plan<M: InternalModel + crate::model::Model>(
    backend: DatabaseType,
    models: &[M::ActiveModel],
) -> Strategy {
    strategy::<M>(backend, models, held_keys::<M>(models))
}

/// The strategy `insert_many` picks for `models` on MariaDB.
fn plan_mariadb<M: InternalModel + crate::model::Model>(models: &[M::ActiveModel]) -> Strategy {
    strategy::<M>(DatabaseType::MariaDB, models, held_keys::<M>(models))
}

#[test]
fn postgres_batches_take_the_rows_back_in_input_order() {
    assert!(matches!(
        plan::<Counter>(DatabaseType::Postgres, &counters()),
        Strategy::Returning(Order::AsReturned)
    ));
}

#[test]
fn mariadb_batches_take_the_rows_back_in_input_order() {
    let tokens = active_models(tokens(&["a", "b"]));
    for strategy in [
        plan_mariadb::<Counter>(&counters()),
        plan_mariadb::<Token>(&tokens),
        plan_mariadb::<Code>(&codes()),
    ] {
        assert!(matches!(strategy, Strategy::Returning(Order::AsReturned)));
    }
}

#[test]
fn mysql_batches_read_client_keys_back_without_returning() {
    let tokens = active_models(tokens(&["a", "b"]));
    assert!(matches!(
        plan::<Token>(DatabaseType::MySQL, &tokens),
        Strategy::InsertThenSelect(keys) if keys.len() == 2
    ));
    assert!(matches!(
        plan::<Counter>(DatabaseType::MySQL, &counters()),
        Strategy::PerRow
    ));
    // A MySQL `CHAR` key can come back without its trailing spaces.
    assert!(matches!(
        plan::<Code>(DatabaseType::MySQL, &codes()),
        Strategy::PerRow
    ));
}

#[test]
fn sqlite_batches_match_rows_back_by_key() {
    assert!(matches!(
        plan::<Counter>(DatabaseType::SQLite, &counters()),
        Strategy::Returning(Order::ByIncreasingKey)
    ));
    let tokens = active_models(tokens(&["a", "b"]));
    assert!(matches!(
        plan::<Token>(DatabaseType::SQLite, &tokens),
        Strategy::Returning(Order::ByClientKey(keys)) if keys.len() == 2
    ));
    assert!(matches!(
        plan::<Code>(DatabaseType::SQLite, &codes()),
        Strategy::Returning(Order::ByClientKey(_))
    ));
}

#[test]
fn a_key_two_models_share_is_left_to_the_database_row_by_row() {
    let mut twins = tokens(&["first", "second"]);
    twins[1].id = twins[0].id;
    let twins = active_models(twins);
    assert!(matches!(
        plan::<Token>(DatabaseType::SQLite, &twins),
        Strategy::PerRow
    ));
    assert!(matches!(
        plan::<Token>(DatabaseType::MySQL, &twins),
        Strategy::PerRow
    ));
}

#[test]
fn a_mysql_batch_of_large_rows_is_split_by_size() {
    let half = "x".repeat(MYSQL_MAX_STATEMENT_BYTES / 2);
    let big = tokens(&[half.as_str(), half.as_str(), half.as_str(), "small"]);
    let mut remaining = active_models(big).into_iter().peekable();

    let by_size = |rows| Chunks {
        rows,
        limit_bytes: true,
    };
    let sizes: Vec<usize> = std::iter::from_fn(|| {
        let chunk = next_chunk::<Token, _>(&mut remaining, by_size(1_000));
        (!chunk.is_empty()).then_some(chunk.len())
    })
    .collect();
    // Two halves and their keys pass the budget, so each large row goes alone
    // and the small one joins the last.
    assert_eq!(sizes, vec![1, 1, 2]);

    let small = active_models(tokens(&["a", "b", "c", "d", "e"]));
    let mut remaining = small.into_iter().peekable();
    assert_eq!(next_chunk::<Token, _>(&mut remaining, by_size(2)).len(), 2);
    assert_eq!(next_chunk::<Token, _>(&mut remaining, by_size(10)).len(), 3);
    assert!(next_chunk::<Token, _>(&mut remaining, by_size(10)).is_empty());

    // PostgreSQL and SQLite count only parameters.
    let big = tokens(&[half.as_str(), half.as_str(), half.as_str()]);
    let mut remaining = active_models(big).into_iter().peekable();
    let by_count = Chunks {
        rows: 1_000,
        limit_bytes: false,
    };
    assert_eq!(next_chunk::<Token, _>(&mut remaining, by_count).len(), 3);
}

#[test]
fn batch_chunks_use_the_enabled_drivers_parameter_limits() {
    use crate::orm::DbBackend;
    for (backend, enabled, expected) in [
        (DbBackend::Postgres, cfg!(feature = "postgres"), 65_535),
        (DbBackend::MySQL, cfg!(feature = "mysql"), 65_535),
        (DbBackend::SQLite, cfg!(feature = "sqlite"), 32_766),
    ] {
        if enabled {
            assert_eq!(backend.max_bind_parameters().unwrap(), expected);
            assert_eq!(super::rows_per_statement(backend, 2).unwrap(), expected / 2);
            assert_eq!(super::rows_per_statement(backend, 0).unwrap(), expected);
            assert_eq!(super::rows_per_statement(backend, expected + 1).unwrap(), 1);
        } else {
            assert!(backend.max_bind_parameters().is_err());
            assert!(super::rows_per_statement(backend, 2).is_err());
        }
    }
}

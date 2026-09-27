use super::{MYSQL_MAX_STATEMENT_BYTES, Order, Strategy, mysql_chunk, strategy};
use crate::internal::{Backend, InternalModel};

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

#[test]
fn postgres_batches_take_the_rows_back_in_input_order() {
    assert!(matches!(
        strategy::<Counter>(Backend::Postgres, &counters()),
        Strategy::Returning(Order::AsReturned)
    ));
}

#[test]
fn mysql_and_mariadb_batches_never_ask_the_engine_for_returning() {
    // MariaDB is reported as the MySQL backend, and the engine renders no
    // `RETURNING` for it.
    let tokens = active_models(tokens(&["a", "b"]));
    assert!(matches!(
        strategy::<Token>(Backend::MySql, &tokens),
        Strategy::InsertThenSelect(keys) if keys.len() == 2
    ));
    assert!(matches!(
        strategy::<Counter>(Backend::MySql, &counters()),
        Strategy::PerRow
    ));
    // A MySQL `CHAR` key can come back without its trailing spaces.
    assert!(matches!(
        strategy::<Code>(Backend::MySql, &codes()),
        Strategy::PerRow
    ));
}

#[test]
fn sqlite_batches_match_rows_back_by_key() {
    assert!(matches!(
        strategy::<Counter>(Backend::Sqlite, &counters()),
        Strategy::Returning(Order::ByIncreasingKey)
    ));
    let tokens = active_models(tokens(&["a", "b"]));
    assert!(matches!(
        strategy::<Token>(Backend::Sqlite, &tokens),
        Strategy::Returning(Order::ByClientKey(keys)) if keys.len() == 2
    ));
    assert!(matches!(
        strategy::<Code>(Backend::Sqlite, &codes()),
        Strategy::Returning(Order::ByClientKey(_))
    ));
}

#[test]
fn a_key_two_models_share_is_left_to_the_database_row_by_row() {
    let mut twins = tokens(&["first", "second"]);
    twins[1].id = twins[0].id;
    let twins = active_models(twins);
    assert!(matches!(
        strategy::<Token>(Backend::Sqlite, &twins),
        Strategy::PerRow
    ));
    assert!(matches!(
        strategy::<Token>(Backend::MySql, &twins),
        Strategy::PerRow
    ));
}

#[test]
fn a_mysql_batch_of_large_rows_is_split_by_size() {
    let half = "x".repeat(MYSQL_MAX_STATEMENT_BYTES / 2);
    let big = tokens(&[half.as_str(), half.as_str(), half.as_str(), "small"]);
    let mut remaining = active_models(big).into_iter().peekable();

    let sizes: Vec<usize> = std::iter::from_fn(|| {
        let chunk = mysql_chunk::<Token, _>(&mut remaining, 1_000);
        (!chunk.is_empty()).then_some(chunk.len())
    })
    .collect();
    // Two halves and their keys pass the budget, so each large row goes alone
    // and the small one joins the last.
    assert_eq!(sizes, vec![1, 1, 2]);

    let small = active_models(tokens(&["a", "b", "c", "d", "e"]));
    let mut remaining = small.into_iter().peekable();
    assert_eq!(mysql_chunk::<Token, _>(&mut remaining, 2).len(), 2);
    assert_eq!(mysql_chunk::<Token, _>(&mut remaining, 10).len(), 3);
    assert!(mysql_chunk::<Token, _>(&mut remaining, 10).is_empty());
}

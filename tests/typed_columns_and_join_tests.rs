//! Typed column conditions (`Column<T>` operators) and flat-join result
//! consolidation (`JoinResultConsolidator`).

use tideorm::columns::{ColumnEq, ColumnIn, ColumnLike, ColumnNullable, ColumnOrd};
use tideorm::query::{ConditionValue, Operator};

#[path = "typed_columns_and_join_tests/typed_columns.rs"]
mod typed_columns;

#[path = "typed_columns_and_join_tests/join_consolidation.rs"]
mod join_consolidation;

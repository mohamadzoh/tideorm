//! The SQL of bulk arithmetic (`increment`, `decrement`, `multiply`,
//! `divide`), whose result must stay readable by the model.

use super::*;

/// An SQLite expression that fails with "integer overflow" when evaluated. A
/// `CASE` that reaches it aborts the whole UPDATE, rows already changed
/// included, which is how bulk arithmetic refuses a result the model cannot read.
const SQLITE_ABORT: &str = "abs(-9223372036854775808)";

/// `factor` as the decimal it is written as (`0.1`, not the binary fraction
/// nearest to it), when a `Decimal` holds it.
fn exact_decimal(factor: f64) -> Option<rust_decimal::Decimal> {
    use std::str::FromStr;

    rust_decimal::Decimal::from_str(&factor.to_string())
        .ok()
        .map(|exact| exact.normalize())
}

/// `col` scaled by `factor` in SQLite's integer arithmetic: multiplied by the
/// factor's numerator and divided by its denominator (the other way round to
/// divide), with the quotient rounded half away from zero, as `ROUND` does.
///
/// The column is divided first, `(col / d) * n` plus its remainder's share
/// `(col % d) * n / d` rounded, so no intermediate product outgrows the
/// result: SQLite turns an integer overflow into a REAL, which would lose
/// the low digits of a large value. `None` when the remainder's share could
/// overflow on its own.
fn sqlite_integer_scale(
    col: &str,
    factor: rust_decimal::Decimal,
    divide: bool,
) -> Option<BoundSql> {
    let numerator = i64::try_from(factor.mantissa()).ok()?;
    let denominator = 10_i64.checked_pow(factor.scale())?;
    let common = i64::try_from(gcd(numerator.unsigned_abs(), denominator.unsigned_abs())).ok()?;
    let (numerator, denominator) = (numerator / common, denominator / common);
    let (times, over) = if divide {
        (denominator, numerator)
    } else {
        (numerator, denominator)
    };
    // The divisor stays positive, so its half rounds away from zero.
    let (times, over) = if over < 0 {
        (times.checked_neg()?, over.checked_neg()?)
    } else {
        (times, over)
    };

    // The remainder is smaller than the divisor, so its share stays within
    // `(over - 1) * |times| + over / 2`.
    (over - 1)
        .checked_mul(times.checked_abs()?)?
        .checked_add(over / 2)?;

    let integer = |value: i64| Value::BigInt(Some(value));
    let scaled = |mut operand: BoundSql| {
        if times != 1 {
            operand.push(" * ").push(integer(times));
        }
        operand
    };
    if over == 1 {
        return Some(scaled(BoundSql::sql(col)));
    }
    // `(col / over)` or `(col % over)`, scaled.
    let part = |operator: &str| {
        let mut part = BoundSql::sql(format!("({col} {operator} "));
        part.push(integer(over)).push(")");
        scaled(part)
    };
    // The remainder's share takes the sign of `col * times`, and its half
    // rounds away from zero.
    let half = over / 2;
    let (below_zero, otherwise) = if times < 0 {
        (half, -half)
    } else {
        (-half, half)
    };
    let mut sum = part("/");
    sum.push(" + (")
        .push(part("%"))
        .push(format!(" + CASE WHEN {col} < 0 THEN "))
        .push(integer(below_zero))
        .push(" ELSE ")
        .push(integer(otherwise))
        .push(" END) / ")
        .push(integer(over));
    Some(sum)
}

/// `col <operator> operand`, the operand bound.
fn applied(col: &str, operator: &str, operand: Value) -> BoundSql {
    let mut applied = BoundSql::sql(format!("{col} {operator} "));
    applied.push(operand);
    applied
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Round a finite, in-range SQLite REAL to binary32, using ties-to-even.
/// Eight binary-search steps find its spacing (never below 2^-149), so no
/// SQLite math extension or per-connection function registration is required.
fn sqlite_f32_round(expression: BoundSql) -> BoundSql {
    let factors = (0..8)
        .map(|step| format!("WHEN {step} THEN {:e}", 2_f64.powi(128 >> step)))
        .collect::<Vec<_>>()
        .join(" ");
    let factor = format!("(CASE step {factors} END)");
    let minimum = 2_f64.powi(-149);
    let mut rounded = BoundSql::sql("(WITH RECURSIVE input(value) AS (SELECT ");
    rounded.push(expression).push(format!(
        "), \
         spacing(step, quantum) AS (VALUES(0, {minimum:e}) UNION ALL \
         SELECT step + 1, CASE WHEN abs(value) >= quantum * {factor} * 8388608.0 \
         THEN quantum * {factor} ELSE quantum END FROM spacing, input WHERE step < 8), \
         scaled(value, quantum, units) AS \
         (SELECT value, quantum, abs(value) / quantum FROM input, spacing WHERE step = 8), \
         parts(value, quantum, units, whole) AS \
         (SELECT value, quantum, units, CAST(units AS INTEGER) FROM scaled) \
         SELECT CASE WHEN value IS NULL THEN NULL ELSE \
         (CASE WHEN value < 0 THEN -1.0 ELSE 1.0 END) * quantum * \
         (whole + CASE WHEN units - whole > 0.5 OR \
         (units - whole = 0.5 AND whole % 2 = 1) THEN 1 ELSE 0 END) END FROM parts)"
    ));
    rounded
}

impl<M: Model> BatchUpdateBuilder<M> {
    /// SQLite has one integer storage width; enforce the model's narrower type
    /// too. `kind` is the column's type.
    pub(super) fn sqlite_integer_bounds(column: &str, kind: Option<&ColumnType>) -> (i64, i64) {
        if let Some(bounds) = M::__sqlite_integer_bounds(column) {
            return bounds;
        }
        match kind {
            Some(ColumnType::TinyInteger) => (i8::MIN.into(), i8::MAX.into()),
            Some(ColumnType::SmallInteger) => (i16::MIN.into(), i16::MAX.into()),
            Some(ColumnType::Integer) => (i32::MIN.into(), i32::MAX.into()),
            Some(ColumnType::TinyUnsigned) => (0, u8::MAX.into()),
            Some(ColumnType::SmallUnsigned) => (0, u16::MAX.into()),
            Some(ColumnType::Unsigned) => (0, u32::MAX.into()),
            Some(ColumnType::BigUnsigned) => (0, i64::MAX),
            _ => (i64::MIN, i64::MAX),
        }
    }

    /// One SQLite integer expression, used three times, each binding values
    /// of its own, kept only when its result is an integer within the
    /// column's range. SQLite promotes overflowing integer arithmetic to
    /// REAL, and casting that REAL clamps it silently, so anything else fails
    /// the statement: abs(i64::MIN) raises SQLite's integer-overflow error and
    /// rolls back every row of this UPDATE.
    fn sqlite_integer_checked(
        column: &str,
        kind: Option<&ColumnType>,
        expression: BoundSql,
    ) -> BoundSql {
        let (min, max) = Self::sqlite_integer_bounds(column, kind);
        let mut checked = BoundSql::sql("CASE WHEN typeof(");
        checked
            .push(expression.clone())
            .push(") IN ('integer', 'null') AND COALESCE((")
            .push(expression.clone())
            .push(format!(") BETWEEN {min} AND {max}, 1) THEN "))
            .push(expression)
            .push(format!(" ELSE {SQLITE_ABORT} END"));
        checked
    }

    /// Keep SQLite arithmetic within the model's readable Float/Double/Decimal range.
    /// SQLite otherwise accepts infinity (or a value too wide for f32), which
    /// cannot round-trip through JSON and typed reads. The error aborts the
    /// entire statement, including rows already updated; NULL stays NULL.
    /// `kind` is the column's type.
    fn sqlite_float_arithmetic(
        kind: Option<&ColumnType>,
        col: &str,
        operator: &str,
        operand: Value,
    ) -> Option<BoundSql> {
        let maximum = match kind {
            Some(ColumnType::Float) => f64::from(f32::MAX),
            Some(ColumnType::Double) => f64::MAX,
            Some(ColumnType::Decimal(_) | ColumnType::Money(_)) => {
                crate::internal::SQLITE_DECIMAL_MAX
            }
            _ => return None,
        };
        let mut expression = BoundSql::sql(format!("CASE WHEN {col} IS NULL OR ("));
        expression
            .push(applied(col, operator, operand.clone()))
            .push(format!(") BETWEEN -{maximum:e} AND {maximum:e} THEN "))
            .push(applied(col, operator, operand))
            .push(format!(" ELSE {SQLITE_ABORT} END"));
        Some(if maximum == f64::from(f32::MAX) {
            sqlite_f32_round(expression)
        } else {
            expression
        })
    }

    /// `col` multiplied, or with `divide` divided, by `factor`.
    ///
    /// An integer column is scaled exactly and rounded half away from zero,
    /// since an `f64` holds integers exactly only up to 2^53: PostgreSQL,
    /// MySQL and MariaDB take the factor as a decimal, and SQLite, which has
    /// no decimal arithmetic, as a fraction of two integers. SQLite would
    /// otherwise store the REAL product, after which the model cannot read
    /// the row. A decimal column takes the factor as a decimal too, where the
    /// backend has one, and floating-point columns take an `f64`. `kind` is
    /// the column's type.
    pub(super) fn scaled(
        column: &str,
        kind: Option<&ColumnType>,
        col: &str,
        db_type: DatabaseType,
        factor: f64,
        divide: bool,
    ) -> Result<BoundSql> {
        if !factor.is_finite() || (divide && factor == 0.0) {
            return Err(Error::query(
                "bulk arithmetic requires a finite factor and a nonzero divisor",
            ));
        }
        let integral = kind.is_some_and(crate::internal::is_integer);
        let decimal = matches!(kind, Some(ColumnType::Decimal(_) | ColumnType::Money(_)));
        let operator = if divide { "/" } else { "*" };

        match (db_type, exact_decimal(factor)) {
            (DatabaseType::SQLite, Some(exact)) if integral => {
                if let Some(scaled) = sqlite_integer_scale(col, exact, divide) {
                    return Ok(Self::sqlite_integer_checked(column, kind, scaled));
                }
            }
            (DatabaseType::Postgres | DatabaseType::MySQL | DatabaseType::MariaDB, Some(exact))
                if integral || decimal =>
            {
                return Ok(applied(col, operator, Value::Decimal(Some(exact))));
            }
            _ => {}
        }

        if integral && db_type == DatabaseType::SQLite {
            return Err(Error::query(
                "SQLite cannot scale this integer column exactly with the requested factor",
            ));
        }
        let factor = Value::Double(Some(factor));
        if db_type == DatabaseType::SQLite
            && let Some(expression) =
                Self::sqlite_float_arithmetic(kind, col, operator, factor.clone())
        {
            return Ok(expression);
        }
        Ok(applied(col, operator, factor))
    }

    /// `col` plus or minus `by`, as `operator` says. On SQLite the result has
    /// to stay readable by the model: within its float range, as
    /// [`sqlite_float_arithmetic`](Self::sqlite_float_arithmetic) keeps it,
    /// or an integer within its integer range. `kind` is the column's type.
    pub(super) fn stepped(
        column: &str,
        kind: Option<&ColumnType>,
        col: &str,
        db_type: DatabaseType,
        operator: &str,
        by: i64,
    ) -> BoundSql {
        let operand = Value::BigInt(Some(by));
        let sqlite = db_type == DatabaseType::SQLite;
        if sqlite
            && let Some(expression) =
                Self::sqlite_float_arithmetic(kind, col, operator, operand.clone())
        {
            return expression;
        }
        let expression = applied(col, operator, operand);
        if !(sqlite && kind.is_some_and(crate::internal::is_integer)) {
            return expression;
        }
        Self::sqlite_integer_checked(column, kind, expression)
    }
}

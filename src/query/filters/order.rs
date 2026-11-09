//! The direction of a sort.

/// Sort order for queries
///
/// It reads from a request parameter: `"asc".parse::<Order>()`, or as a
/// `Deserialize` field, in any case (`asc`, `DESC`, `ascending`). It
/// serializes as `"asc"` or `"desc"` and displays as the SQL keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Order {
    /// Ascending order (A-Z, 1-9). The default.
    #[default]
    Asc,
    /// Descending order (Z-A, 9-1)
    Desc,
}

impl Order {
    /// The direction as the statement builder takes it.
    pub(crate) fn to_builder(self) -> crate::orm::sql_builder::Order {
        match self {
            Order::Asc => crate::orm::sql_builder::Order::Asc,
            Order::Desc => crate::orm::sql_builder::Order::Desc,
        }
    }
}

impl std::fmt::Display for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Order {
    type Err = crate::error::Error;

    fn from_str(order: &str) -> crate::error::Result<Self> {
        match order.trim().to_ascii_lowercase().as_str() {
            "asc" | "ascending" => Ok(Order::Asc),
            "desc" | "descending" => Ok(Order::Desc),
            _ => Err(crate::error::Error::query(format!(
                "unknown sort order '{order}': expected 'asc' or 'desc'"
            ))),
        }
    }
}

impl serde::Serialize for Order {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Order::Asc => "asc",
            Order::Desc => "desc",
        })
    }
}

impl<'de> serde::Deserialize<'de> for Order {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let order = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        order.parse().map_err(serde::de::Error::custom)
    }
}

/// [`Order`] under a second name, for a crate whose own `Order` type (an
/// `Order` model, say) shadows the prelude's: `SortOrder::Desc`.
pub type SortOrder = Order;

impl Order {
    /// The SQL keyword for this direction: `ASC` or `DESC`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }

    /// The other direction.
    #[must_use]
    pub fn reverse(self) -> Self {
        match self {
            Order::Asc => Order::Desc,
            Order::Desc => Order::Asc,
        }
    }
}

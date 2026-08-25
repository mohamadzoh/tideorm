//! Static descriptions of declared relations.

/// A description of one relation declared on a model.
///
/// This is a descriptor, not a loader. Nothing in TideORM's own read paths
/// consults it: relations are loaded through the wrapper stored in the model
/// field ([`HasMany`](crate::relations::HasMany) and friends). `RelationInfo`
/// exists so generated code, schema tooling, and diagnostics can talk about a
/// relation as data, without naming its Rust types.
///
/// Build one with the constructor matching the relation kind rather than a
/// struct literal. Each fills in only the columns its [`RelationType`] actually
/// uses and leaves the rest `None`, and which fields carry meaning depends
/// entirely on that type.
#[derive(Debug, Clone)]
pub struct RelationInfo {
    /// Name of the model field the relation was declared on — the same string
    /// `.with("posts")` and the eager loader match against.
    pub name: String,
    /// Which kind of relation this is. It determines which of the remaining
    /// fields are populated.
    pub relation_type: RelationType,
    /// Table of the model on the far side of the relation.
    pub related_table: String,
    /// Column carrying the link. Which table it lives on depends on the kind:
    /// the related table for `HasOne`/`HasMany`, the declaring model's own table
    /// for `BelongsTo`, and the pivot table (the half pointing back at the
    /// owner) for `HasManyThrough`. Left empty for the morph kinds, which link
    /// through [`morph_type_column`](Self::morph_type_column) and
    /// [`morph_id_column`](Self::morph_id_column) instead.
    pub foreign_key: String,
    /// Column that [`foreign_key`](Self::foreign_key) is compared against,
    /// usually a primary key. For `BelongsTo` this is the column on the
    /// *related* table that the declaring model's foreign key points at.
    pub local_key: String,
    /// Join table. Set only for `HasManyThrough`.
    pub pivot_table: Option<String>,
    /// Pivot column pointing at the related table — the other half of a pivot
    /// row from [`foreign_key`](Self::foreign_key). Set only for
    /// `HasManyThrough`.
    ///
    /// Before 0.10 this key was smuggled through
    /// [`morph_type_column`](Self::morph_type_column); that field now means only
    /// what its name says and is `None` for a through-relation.
    pub related_key: Option<String>,
    /// Discriminator column holding the owner's table name. Set only for the
    /// morph kinds.
    pub morph_type_column: Option<String>,
    /// Column holding the owner's key. Set only for the morph kinds.
    pub morph_id_column: Option<String>,
}

impl RelationInfo {
    /// Describe an inverse relation: `foreign_key` lives on the declaring
    /// model's own table and points at `local_key` on `related_table`.
    pub fn belongs_to(name: &str, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::BelongsTo,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: None,
            related_key: None,
            morph_type_column: None,
            morph_id_column: None,
        }
    }

    /// Describe a one-to-one relation: `foreign_key` lives on `related_table`
    /// and points back at `local_key` on the declaring model.
    ///
    /// Identical in shape to [`has_many`](Self::has_many); only the
    /// [`RelationType`] differs, and with it whether a loader expects at most
    /// one row or many.
    pub fn has_one(name: &str, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::HasOne,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: None,
            related_key: None,
            morph_type_column: None,
            morph_id_column: None,
        }
    }

    /// Describe a one-to-many relation: `foreign_key` lives on `related_table`
    /// and points back at `local_key` on the declaring model.
    pub fn has_many(name: &str, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::HasMany,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: None,
            related_key: None,
            morph_type_column: None,
            morph_id_column: None,
        }
    }

    /// Describe a many-to-many relation joined through `pivot_table`.
    ///
    /// The two pivot columns are distinct and not interchangeable:
    /// `foreign_key` points back at the declaring model (matching `local_key`),
    /// while `related_key` points at `related_table`. Both are recorded — the
    /// latter in [`related_key`](Self::related_key), which is the only
    /// constructor that populates it.
    pub fn has_many_through(
        name: &str,
        related_table: &str,
        pivot_table: &str,
        foreign_key: &str,
        related_key: &str,
        local_key: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::HasManyThrough,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: Some(pivot_table.to_string()),
            related_key: Some(related_key.to_string()),
            morph_type_column: None,
            morph_id_column: None,
        }
    }

    /// Describe a polymorphic one-to-one relation.
    ///
    /// `type_column` and `id_column` both live on `related_table` and are read
    /// as a pair: the discriminator holds the owner's table name, the id column
    /// holds `local_key`'s value. [`foreign_key`](Self::foreign_key) is left
    /// empty because a morph relation has no single plain foreign key.
    pub fn morph_one(
        name: &str,
        related_table: &str,
        type_column: &str,
        id_column: &str,
        local_key: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::MorphOne,
            related_table: related_table.to_string(),
            foreign_key: String::new(),
            local_key: local_key.to_string(),
            pivot_table: None,
            related_key: None,
            morph_type_column: Some(type_column.to_string()),
            morph_id_column: Some(id_column.to_string()),
        }
    }

    /// Describe a polymorphic one-to-many relation. Same column layout as
    /// [`morph_one`](Self::morph_one), matching many rows instead of one.
    pub fn morph_many(
        name: &str,
        related_table: &str,
        type_column: &str,
        id_column: &str,
        local_key: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::MorphMany,
            related_table: related_table.to_string(),
            foreign_key: String::new(),
            local_key: local_key.to_string(),
            pivot_table: None,
            related_key: None,
            morph_type_column: Some(type_column.to_string()),
            morph_id_column: Some(id_column.to_string()),
        }
    }
}

/// The kind of a declared relation, and the discriminator that says which
/// [`RelationInfo`] fields are populated.
///
/// The [`Display`](std::fmt::Display) rendering is the snake_case spelling used
/// by the `#[tideorm(has_many = "..")]` attributes, so it can be dropped into a
/// diagnostic and still name something the user wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationType {
    /// Inverse side; the foreign key lives on the declaring model.
    BelongsTo,
    /// One related row, keyed by a foreign key on the related table.
    HasOne,
    /// Many related rows, keyed by a foreign key on the related table.
    HasMany,
    /// Many related rows reached through a pivot table.
    HasManyThrough,
    /// Polymorphic inverse side.
    ///
    /// [`RelationInfo`] has no constructor for this variant: the target table is
    /// only known per row, from the discriminator column, so there is nothing
    /// static to describe. It exists so code matching on relation kinds stays
    /// exhaustive.
    MorphTo,
    /// One related row keyed by a `(type, id)` discriminator pair.
    MorphOne,
    /// Many related rows keyed by a `(type, id)` discriminator pair.
    MorphMany,
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelationType::BelongsTo => write!(f, "belongs_to"),
            RelationType::HasOne => write!(f, "has_one"),
            RelationType::HasMany => write!(f, "has_many"),
            RelationType::HasManyThrough => write!(f, "has_many_through"),
            RelationType::MorphTo => write!(f, "morph_to"),
            RelationType::MorphOne => write!(f, "morph_one"),
            RelationType::MorphMany => write!(f, "morph_many"),
        }
    }
}

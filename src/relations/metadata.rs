#[derive(Debug, Clone)]
pub struct RelationInfo {
    pub name: String,
    pub relation_type: RelationType,
    pub related_table: String,
    pub foreign_key: String,
    pub local_key: String,
    pub pivot_table: Option<String>,
    pub morph_type_column: Option<String>,
    pub morph_id_column: Option<String>,
}

impl RelationInfo {
    pub fn belongs_to(name: &str, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::BelongsTo,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: None,
            morph_type_column: None,
            morph_id_column: None,
        }
    }

    pub fn has_one(name: &str, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::HasOne,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: None,
            morph_type_column: None,
            morph_id_column: None,
        }
    }

    pub fn has_many(name: &str, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::HasMany,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: None,
            morph_type_column: None,
            morph_id_column: None,
        }
    }

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
            morph_type_column: Some(related_key.to_string()),
            morph_id_column: None,
        }
    }

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
            morph_type_column: Some(type_column.to_string()),
            morph_id_column: Some(id_column.to_string()),
        }
    }

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
            morph_type_column: Some(type_column.to_string()),
            morph_id_column: Some(id_column.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationType {
    BelongsTo,
    HasOne,
    HasMany,
    HasManyThrough,
    MorphTo,
    MorphOne,
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

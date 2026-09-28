use super::{AttachmentError, FileAttachment, FilesData};

/// Trait for models with file attachments
///
/// **You implement this yourself** — the derive does not generate it. Declaring
/// `has_one_files` / `has_many_files` populates [`ModelMeta`](crate::model::ModelMeta)
/// so the field names are queryable, but the read/write half needs a body only you
/// can supply: it has to know which column holds the payload. Store it in a JSON
/// column (`files: Option<Json>` by convention) and forward
/// `get_files_data` / `set_files_data` to it.
///
/// A generated impl would force a column-naming convention the attribute never
/// promised, and would conflict with every impl already written by hand.
///
/// # Trust boundary
///
/// File keys are stored exactly as given. See [`FileAttachment`] for what
/// TideORM does and does not check, and screen untrusted keys with
/// [`FileAttachment::is_safe_key`] before calling `attach()`.
pub trait HasAttachments {
    /// Get the list of hasOne file relations
    fn has_one_files() -> Vec<&'static str>;

    /// Get the list of hasMany file relations
    fn has_many_files() -> Vec<&'static str>;

    /// Get all file relation names
    fn all_file_relations() -> Vec<&'static str> {
        let mut relations = Self::has_one_files();
        relations.extend(Self::has_many_files());
        relations
    }

    /// Check if a relation is hasOne
    fn is_has_one_relation(relation: &str) -> bool {
        Self::has_one_files().contains(&relation)
    }

    /// Check if a relation is hasMany
    fn is_has_many_relation(relation: &str) -> bool {
        Self::has_many_files().contains(&relation)
    }

    /// Get the current files data from the model
    fn get_files_data(&self) -> Result<FilesData, AttachmentError>;

    /// Set the files data on the model
    fn set_files_data(&mut self, data: FilesData) -> Result<(), AttachmentError>;

    /// Attach one file to a relation.
    ///
    /// For `hasOne` relations this replaces the previous attachment. For
    /// `hasMany` relations it appends another entry.
    ///
    /// The relation name is validated; `file_key` is not. Screen untrusted keys
    /// with [`FileAttachment::is_safe_key`] first.
    fn attach(&mut self, relation: &str, file_key: &str) -> Result<(), AttachmentError> {
        self.attach_with_metadata(relation, FileAttachment::new(file_key))
    }

    /// Attach one file using fully prepared attachment metadata.
    fn attach_with_metadata(
        &mut self,
        relation: &str,
        attachment: FileAttachment,
    ) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;
        let has_one = Self::is_has_one_relation(relation);
        edit_files(self, |files| {
            if has_one {
                files.set_one(relation, attachment);
            } else {
                files.add_many(relation, attachment);
            }
        })
    }

    /// Attach multiple files to a `hasMany` relation.
    fn attach_many(&mut self, relation: &str, file_keys: Vec<&str>) -> Result<(), AttachmentError> {
        require_has_many::<Self>(relation, ", use attach() instead")?;
        edit_files(self, |files| {
            for key in file_keys {
                files.add_many(relation, FileAttachment::new(key));
            }
        })
    }

    /// Remove attachments from a relation.
    ///
    /// For `hasOne`, pass `None` to clear the attachment, or `Some(key)` to
    /// clear it only while it is that file. For `hasMany`, pass `Some(key)` to
    /// remove one entry or `None` to clear the whole relation.
    fn detach(&mut self, relation: &str, file_key: Option<&str>) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;
        let has_one = Self::is_has_one_relation(relation);
        edit_files(self, |files| {
            if has_one {
                let attached = files.get_one(relation);
                if file_key.is_none_or(|key| attached.is_some_and(|file| file.key == key)) {
                    files.remove_one(relation);
                }
            } else if let Some(key) = file_key {
                files.remove_from_many(relation, key);
            } else {
                files.clear_many(relation);
            }
        })
    }

    /// Remove multiple keys from a `hasMany` relation.
    fn detach_many(&mut self, relation: &str, file_keys: Vec<&str>) -> Result<(), AttachmentError> {
        require_has_many::<Self>(relation, "")?;
        edit_files(self, |files| {
            for key in file_keys {
                files.remove_from_many(relation, key);
            }
        })
    }

    /// Replace the current relation contents with a new list of file keys.
    fn sync(&mut self, relation: &str, file_keys: Vec<&str>) -> Result<(), AttachmentError> {
        let attachments = file_keys.into_iter().map(FileAttachment::new).collect();
        self.sync_with_metadata(relation, attachments)
    }

    /// Replace the current relation contents with pre-built attachment metadata.
    ///
    /// A `hasOne` relation keeps the first attachment, or is cleared when there
    /// is none.
    fn sync_with_metadata(
        &mut self,
        relation: &str,
        attachments: Vec<FileAttachment>,
    ) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;
        let has_one = Self::is_has_one_relation(relation);
        edit_files(self, |files| {
            if has_one {
                match attachments.into_iter().next() {
                    Some(first) => files.set_one(relation, first),
                    None => files.remove_one(relation),
                }
            } else {
                files.clear_many(relation);
                for attachment in attachments {
                    files.add_many(relation, attachment);
                }
            }
        })
    }

    /// Return the single attachment for a `hasOne` relation.
    fn get_file(&self, relation: &str) -> Result<Option<FileAttachment>, AttachmentError> {
        Ok(self.get_files_data()?.get_one(relation))
    }

    /// Return all attachments for a `hasMany` relation.
    fn get_files(&self, relation: &str) -> Result<Vec<FileAttachment>, AttachmentError> {
        Ok(self.get_files_data()?.get_many(relation))
    }

    /// Check if a relation has any files
    fn has_files(&self, relation: &str) -> Result<bool, AttachmentError> {
        Ok(self.count_files(relation)? > 0)
    }

    /// Count files in a relation
    fn count_files(&self, relation: &str) -> Result<usize, AttachmentError> {
        Ok(self.get_files_data()?.count_files(relation))
    }

    /// Validate that a relation exists
    fn validate_relation(&self, relation: &str) -> Result<(), AttachmentError> {
        if !Self::all_file_relations().contains(&relation) {
            return Err(AttachmentError::InvalidRelation(format!(
                "Unknown file relation: '{}'. Available: {:?}",
                relation,
                Self::all_file_relations()
            )));
        }
        Ok(())
    }
}

/// Read `model`'s attachments, change them with `edit`, and write them back.
fn edit_files<M: HasAttachments + ?Sized>(
    model: &mut M,
    edit: impl FnOnce(&mut FilesData),
) -> Result<(), AttachmentError> {
    let mut files = model.get_files_data()?;
    edit(&mut files);
    model.set_files_data(files)
}

/// Refuse a relation that is not `M`'s `hasMany`, `hint` saying what to use.
fn require_has_many<M: HasAttachments + ?Sized>(
    relation: &str,
    hint: &str,
) -> Result<(), AttachmentError> {
    if M::is_has_many_relation(relation) {
        return Ok(());
    }
    Err(AttachmentError::InvalidRelation(format!(
        "'{relation}' is not a hasMany relation{hint}"
    )))
}

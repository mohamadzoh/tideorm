use super::{AttachmentError, FileAttachment, FilesData};

/// Trait for models with file attachments
///
/// This trait is usually macro-generated from file attachment configuration.
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

        let mut files = self.get_files_data()?;

        if Self::is_has_one_relation(relation) {
            files.set_one(relation, attachment);
        } else {
            files.add_many(relation, attachment);
        }

        self.set_files_data(files)
    }

    /// Attach multiple files to a `hasMany` relation.
    fn attach_many(&mut self, relation: &str, file_keys: Vec<&str>) -> Result<(), AttachmentError> {
        if !Self::is_has_many_relation(relation) {
            return Err(AttachmentError::InvalidRelation(format!(
                "'{}' is not a hasMany relation, use attach() instead",
                relation
            )));
        }

        let mut files = self.get_files_data()?;

        for key in file_keys {
            files.add_many(relation, FileAttachment::new(key));
        }

        self.set_files_data(files)
    }

    /// Remove attachments from a relation.
    ///
    /// For `hasOne`, pass `None` to clear the attachment. For `hasMany`, pass
    /// `Some(key)` to remove one entry or `None` to clear the whole relation.
    fn detach(&mut self, relation: &str, file_key: Option<&str>) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;

        let mut files = self.get_files_data()?;

        if Self::is_has_one_relation(relation) {
            files.remove_one(relation);
        } else if let Some(key) = file_key {
            files.remove_from_many(relation, key);
        } else {
            files.clear_many(relation);
        }

        self.set_files_data(files)
    }

    /// Remove multiple keys from a `hasMany` relation.
    fn detach_many(&mut self, relation: &str, file_keys: Vec<&str>) -> Result<(), AttachmentError> {
        if !Self::is_has_many_relation(relation) {
            return Err(AttachmentError::InvalidRelation(format!(
                "'{}' is not a hasMany relation",
                relation
            )));
        }

        let mut files = self.get_files_data()?;

        for key in file_keys {
            files.remove_from_many(relation, key);
        }

        self.set_files_data(files)
    }

    /// Replace the current relation contents with a new list of file keys.
    fn sync(&mut self, relation: &str, file_keys: Vec<&str>) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;

        let mut files = self.get_files_data()?;

        if Self::is_has_one_relation(relation) {
            if file_keys.is_empty() {
                files.remove_one(relation);
            } else {
                files.set_one(relation, FileAttachment::new(file_keys[0]));
            }
        } else {
            files.clear_many(relation);
            for key in file_keys {
                files.add_many(relation, FileAttachment::new(key));
            }
        }

        self.set_files_data(files)
    }

    /// Replace the current relation contents with pre-built attachment metadata.
    fn sync_with_metadata(
        &mut self,
        relation: &str,
        attachments: Vec<FileAttachment>,
    ) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;

        let mut files = self.get_files_data()?;

        if Self::is_has_one_relation(relation) {
            if attachments.is_empty() {
                files.remove_one(relation);
            } else if let Some(first) = attachments.into_iter().next() {
                files.set_one(relation, first);
            }
        } else {
            files.clear_many(relation);
            for attachment in attachments {
                files.add_many(relation, attachment);
            }
        }

        self.set_files_data(files)
    }

    /// Return the single attachment for a `hasOne` relation.
    fn get_file(&self, relation: &str) -> Result<Option<FileAttachment>, AttachmentError> {
        let files = self.get_files_data()?;
        Ok(files.get_one(relation))
    }

    /// Return all attachments for a `hasMany` relation.
    fn get_files(&self, relation: &str) -> Result<Vec<FileAttachment>, AttachmentError> {
        let files = self.get_files_data()?;
        Ok(files.get_many(relation))
    }

    /// Check if a relation has any files
    fn has_files(&self, relation: &str) -> Result<bool, AttachmentError> {
        let files = self.get_files_data()?;
        Ok(files.has_files(relation))
    }

    /// Count files in a relation
    fn count_files(&self, relation: &str) -> Result<usize, AttachmentError> {
        let files = self.get_files_data()?;
        Ok(files.count_files(relation))
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

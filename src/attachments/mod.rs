//! File Attachments System
//!
//! This module stores file references inside a JSON or JSONB model column.
//!
//! Use it when the database needs to keep attachment metadata such as keys,
//! filenames, and timestamps, but the actual file bytes live somewhere else.
//!
//! The two supported shapes are:
//! - single-file relations such as `thumbnail` or `avatar`
//! - multi-file relations such as `images` or `documents`
//!
//! If attachment calls appear to succeed but nothing is persisted, the usual
//! cause is that the model was not saved after mutating the in-memory `files`
//! payload.
//!
//! Typical workflow:
//! - declare the `files` JSON or JSONB column plus `#[tideorm(has_one_file = ...)]` or `#[tideorm(has_many_files = ...)]`
//! - use `attach()` or `attach_many()` to append metadata-backed file references
//! - use `detach()` or `sync()` when the relation should be removed or replaced wholesale
//! - save the model afterward so the updated payload is persisted
//!
//! ## File Metadata
//!
//! Each attachment stores:
//! - `key`: The file path/key
//! - `filename`: Extracted filename
//! - `created_at`: Timestamp when attached
//! - Additional fields can be added via `attach_with_metadata`

mod error;
mod file_attachment;
mod files_data;
mod has_attachments;

pub use error::AttachmentError;
pub use file_attachment::FileAttachment;
pub use files_data::FilesData;
pub use has_attachments::HasAttachments;

#[cfg(test)]
#[path = "../../tests/unit/attachments_tests.rs"]
mod tests;

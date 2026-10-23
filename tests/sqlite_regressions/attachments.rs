//! File attachments stored in a model's JSON column.

use crate::fixtures::global_memory_database;
use serde_json::json;
use tideorm::attachments::{AttachmentError, FileAttachment, FilesData, HasAttachments};
use tideorm::prelude::*;

#[tideorm::model(table = "attachment_galleries", has_many_files = "photos")]
struct Gallery {
    #[tideorm(primary_key)]
    id: i64,
    files: Option<serde_json::Value>,
}

impl HasAttachments for Gallery {
    fn has_one_files() -> Vec<&'static str> {
        vec![]
    }

    fn has_many_files() -> Vec<&'static str> {
        vec!["photos"]
    }

    fn get_files_data(&self) -> Result<FilesData, AttachmentError> {
        Ok(self
            .files
            .as_ref()
            .map(FilesData::from_json)
            .unwrap_or_default())
    }

    fn set_files_data(&mut self, data: FilesData) -> Result<(), AttachmentError> {
        self.files = Some(data.to_json());
        Ok(())
    }
}

fn keys(gallery: &Gallery) -> Vec<String> {
    gallery
        .get_files("photos")
        .unwrap()
        .into_iter()
        .map(|file| file.key)
        .collect()
}

/// A hasMany relation holding one attachment rather than a list, as a
/// relation that was hasOne once stored it, was wiped by the next attach or
/// detach, and failed to read.
#[tokio::test]
async fn a_has_many_relation_holding_one_attachment_keeps_it() {
    global_memory_database("CREATE TABLE attachment_galleries(id INTEGER PRIMARY KEY, files TEXT)")
        .await;
    let single = FileAttachment::new("gallery/first.jpg").to_json();
    Gallery::create(Gallery {
        id: 1,
        files: Some(json!({ "photos": single })),
    })
    .await
    .unwrap();

    let mut gallery = Gallery::find(1).await.unwrap().unwrap();
    assert_eq!(gallery.count_files("photos").unwrap(), 1);
    assert_eq!(keys(&gallery), ["gallery/first.jpg"]);

    gallery.attach("photos", "gallery/second.jpg").unwrap();
    let mut gallery = gallery.update().await.unwrap();
    assert_eq!(keys(&gallery), ["gallery/first.jpg", "gallery/second.jpg"]);

    gallery.files = Some(json!({ "photos": single }));
    gallery.detach("photos", Some("gallery/other.jpg")).unwrap();
    let gallery = gallery.update().await.unwrap();
    assert_eq!(keys(&gallery), ["gallery/first.jpg"]);
    assert_eq!(gallery.files.unwrap()["photos"], json!([single]));
}

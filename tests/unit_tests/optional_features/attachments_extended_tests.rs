// EXTENDED ATTACHMENTS TESTS
// =============================================================================

#![cfg(feature = "attachments")]

use serde::{Deserialize, Serialize};
use tideorm::attachments::{AttachmentError, FileAttachment, FilesData, HasAttachments};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TestProduct {
    id: i64,
    name: String,
    files: Option<serde_json::Value>,
}

impl HasAttachments for TestProduct {
    fn has_one_files() -> Vec<&'static str> {
        vec!["thumbnail", "cover"]
    }

    fn has_many_files() -> Vec<&'static str> {
        vec!["images", "documents"]
    }

    fn get_files_data(&self) -> Result<FilesData, AttachmentError> {
        match &self.files {
            Some(json) => Ok(FilesData::from_json(json)),
            None => Ok(FilesData::new()),
        }
    }

    fn set_files_data(&mut self, data: FilesData) -> Result<(), AttachmentError> {
        self.files = Some(data.to_json());
        Ok(())
    }
}

impl TestProduct {
    fn new(id: i64, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            files: None,
        }
    }
}

#[test]
fn test_has_attachments_attach_single() {
    let mut product = TestProduct::new(1, "Test Product");

    product.attach("thumbnail", "uploads/thumb.jpg").unwrap();

    let thumb = product.get_file("thumbnail").unwrap().unwrap();
    assert_eq!(thumb.key, "uploads/thumb.jpg");
    assert_eq!(thumb.filename, "thumb.jpg");
}

#[test]
fn test_has_attachments_attach_replaces_has_one() {
    let mut product = TestProduct::new(1, "Test Product");

    product.attach("thumbnail", "uploads/old.jpg").unwrap();
    product.attach("thumbnail", "uploads/new.jpg").unwrap();

    let thumb = product.get_file("thumbnail").unwrap().unwrap();
    assert_eq!(thumb.key, "uploads/new.jpg");
    assert_eq!(product.count_files("thumbnail").unwrap(), 1);
}

#[test]
fn test_has_attachments_attach_many() {
    let mut product = TestProduct::new(1, "Test Product");

    product
        .attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"])
        .unwrap();

    let images = product.get_files("images").unwrap();
    assert_eq!(images.len(), 3);
    assert_eq!(images[0].key, "img1.jpg");
    assert_eq!(images[1].key, "img2.jpg");
    assert_eq!(images[2].key, "img3.jpg");
}

#[test]
fn test_has_attachments_attach_many_accumulates() {
    let mut product = TestProduct::new(1, "Test Product");

    product.attach("images", "img1.jpg").unwrap();
    product.attach("images", "img2.jpg").unwrap();
    product
        .attach_many("images", vec!["img3.jpg", "img4.jpg"])
        .unwrap();

    assert_eq!(product.count_files("images").unwrap(), 4);
}

#[test]
fn test_has_attachments_detach_specific() {
    let mut product = TestProduct::new(1, "Test Product");

    product
        .attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"])
        .unwrap();
    product.detach("images", Some("img2.jpg")).unwrap();

    let images = product.get_files("images").unwrap();
    assert_eq!(images.len(), 2);
    assert!(images.iter().all(|f| f.key != "img2.jpg"));
}

#[test]
fn test_has_attachments_detach_all() {
    let mut product = TestProduct::new(1, "Test Product");

    product
        .attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"])
        .unwrap();
    product.detach("images", None).unwrap();

    assert!(!product.has_files("images").unwrap());
    assert_eq!(product.count_files("images").unwrap(), 0);
}

#[test]
fn test_has_attachments_detach_has_one() {
    let mut product = TestProduct::new(1, "Test Product");

    product.attach("thumbnail", "thumb.jpg").unwrap();
    assert!(product.has_files("thumbnail").unwrap());

    product.detach("thumbnail", None).unwrap();
    assert!(!product.has_files("thumbnail").unwrap());
}

#[test]
fn test_has_attachments_sync_replaces_all() {
    let mut product = TestProduct::new(1, "Test Product");

    product
        .attach_many("images", vec!["old1.jpg", "old2.jpg", "old3.jpg"])
        .unwrap();
    product
        .sync("images", vec!["new1.jpg", "new2.jpg"])
        .unwrap();

    let images = product.get_files("images").unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].key, "new1.jpg");
    assert_eq!(images[1].key, "new2.jpg");
}

#[test]
fn test_has_attachments_sync_empty_clears() {
    let mut product = TestProduct::new(1, "Test Product");

    product
        .attach_many("images", vec!["img1.jpg", "img2.jpg"])
        .unwrap();
    product.sync("images", vec![]).unwrap();

    assert!(!product.has_files("images").unwrap());
}

#[test]
fn test_has_attachments_sync_has_one() {
    let mut product = TestProduct::new(1, "Test Product");

    product.attach("thumbnail", "old.jpg").unwrap();
    product.sync("thumbnail", vec!["new.jpg"]).unwrap();

    let thumb = product.get_file("thumbnail").unwrap().unwrap();
    assert_eq!(thumb.key, "new.jpg");
}

#[test]
fn test_has_attachments_with_metadata() {
    let mut product = TestProduct::new(1, "Test Product");

    let attachment = FileAttachment::with_metadata(
        "uploads/doc.pdf",
        Some("My Document.pdf"),
        Some(1024 * 1024),
        Some("application/pdf"),
    );

    product
        .attach_with_metadata("documents", attachment)
        .unwrap();

    let docs = product.get_files("documents").unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(
        docs[0].original_filename,
        Some("My Document.pdf".to_string())
    );
    assert_eq!(docs[0].size, Some(1024 * 1024));
    assert_eq!(docs[0].mime_type, Some("application/pdf".to_string()));
}

#[test]
fn test_has_attachments_invalid_relation() {
    let mut product = TestProduct::new(1, "Test Product");

    let result = product.attach("unknown_relation", "file.jpg");
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.to_string().contains("Unknown file relation"));
}

#[test]
fn test_has_attachments_attach_many_on_has_one() {
    let mut product = TestProduct::new(1, "Test Product");

    let result = product.attach_many("thumbnail", vec!["img1.jpg", "img2.jpg"]);
    assert!(result.is_err());
}

#[test]
fn test_has_attachments_multiple_relations() {
    let mut product = TestProduct::new(1, "Test Product");

    product.attach("thumbnail", "thumb.jpg").unwrap();
    product.attach("cover", "cover.jpg").unwrap();
    product
        .attach_many("images", vec!["img1.jpg", "img2.jpg"])
        .unwrap();
    product
        .attach_many("documents", vec!["doc1.pdf", "doc2.pdf"])
        .unwrap();

    assert_eq!(product.count_files("thumbnail").unwrap(), 1);
    assert_eq!(product.count_files("cover").unwrap(), 1);
    assert_eq!(product.count_files("images").unwrap(), 2);
    assert_eq!(product.count_files("documents").unwrap(), 2);
}

#[test]
fn test_has_attachments_json_persistence() {
    let mut product = TestProduct::new(1, "Test Product");

    product.attach("thumbnail", "thumb.jpg").unwrap();
    product
        .attach_many("images", vec!["img1.jpg", "img2.jpg"])
        .unwrap();

    let json = serde_json::to_string(&product).unwrap();
    let loaded: TestProduct = serde_json::from_str(&json).unwrap();

    assert_eq!(loaded.count_files("thumbnail").unwrap(), 1);
    assert_eq!(loaded.count_files("images").unwrap(), 2);

    let thumb = loaded.get_file("thumbnail").unwrap().unwrap();
    assert_eq!(thumb.key, "thumb.jpg");
}

#[test]
fn test_file_attachment_deep_path() {
    let attachment = FileAttachment::new("uploads/2024/01/15/user_123/profile/avatar.png");
    assert_eq!(attachment.filename, "avatar.png");
    assert_eq!(
        attachment.key,
        "uploads/2024/01/15/user_123/profile/avatar.png"
    );
}

#[test]
fn test_file_attachment_unicode_filename() {
    let attachment = FileAttachment::new("uploads/æ–‡æ¡£/å›¾ç‰‡.jpg");
    assert_eq!(attachment.filename, "å›¾ç‰‡.jpg");
}

#[test]
fn test_file_attachment_special_characters() {
    let attachment = FileAttachment::new("uploads/file with spaces (1).pdf");
    assert_eq!(attachment.filename, "file with spaces (1).pdf");
}

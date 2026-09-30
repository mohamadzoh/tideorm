use super::*;

#[test]
fn test_file_attachment_creation() {
    let attachment = FileAttachment::new("uploads/2024/01/image.jpg");
    assert_eq!(attachment.key, "uploads/2024/01/image.jpg");
    assert_eq!(attachment.filename, "image.jpg");
}

#[test]
fn test_file_attachment_with_metadata() {
    let attachment = FileAttachment::with_metadata(
        "uploads/doc.pdf",
        Some("My Document.pdf"),
        Some(1024),
        Some("application/pdf"),
    );
    assert_eq!(
        attachment.original_filename,
        Some("My Document.pdf".to_string())
    );
    assert_eq!(attachment.size, Some(1024));
    assert_eq!(attachment.mime_type, Some("application/pdf".to_string()));
}

#[test]
fn test_files_data_has_one() {
    let mut files = FilesData::new();
    files.set_one("thumbnail", FileAttachment::new("thumb.jpg"));

    assert!(files.has_files("thumbnail"));
    assert_eq!(files.count_files("thumbnail"), 1);

    let thumb = files.get_one("thumbnail").unwrap();
    assert_eq!(thumb.key, "thumb.jpg");

    files.remove_one("thumbnail");
    assert!(!files.has_files("thumbnail"));
}

#[test]
fn test_file_attachment_filename_uses_backslash_separators_too() {
    let attachment = FileAttachment::new("uploads\\2024\\01\\image.jpg");
    assert_eq!(attachment.filename, "image.jpg");

    let mixed = FileAttachment::new("uploads/2024\\report.pdf");
    assert_eq!(mixed.filename, "report.pdf");

    let bare = FileAttachment::new("image.jpg");
    assert_eq!(bare.filename, "image.jpg");
}

#[test]
fn test_is_safe_key_accepts_plain_relative_keys() {
    assert!(FileAttachment::is_safe_key("image.jpg"));
    assert!(FileAttachment::is_safe_key("uploads/2024/01/image.jpg"));
    assert!(FileAttachment::is_safe_key("uploads/my file (1).jpg"));
}

#[test]
fn test_is_safe_key_rejects_traversal_and_absolute_and_url_keys() {
    for key in [
        "",
        "   ",
        "../secret",
        "uploads/../../etc/passwd",
        "uploads\\..\\..\\windows\\system32",
        "/etc/passwd",
        "\\\\server\\share\\file",
        "C:\\Windows\\system32",
        "C:relative",
        "https://evil.example/x.png",
        "//evil.example/x.png",
        "uploads/a\0b.jpg",
    ] {
        assert!(
            !FileAttachment::is_safe_key(key),
            "'{}' should be rejected as an unsafe storage key",
            key.escape_debug()
        );
    }
}

#[test]
fn test_files_data_has_many() {
    let mut files = FilesData::new();
    files.add_many("images", FileAttachment::new("img1.jpg"));
    files.add_many("images", FileAttachment::new("img2.jpg"));

    assert!(files.has_files("images"));
    assert_eq!(files.count_files("images"), 2);

    let images = files.get_many("images");
    assert_eq!(images.len(), 2);

    files.remove_from_many("images", "img1.jpg");
    assert_eq!(files.count_files("images"), 1);

    files.clear_many("images");
    assert!(!files.has_files("images"));
}

#[test]
fn metadata_named_like_a_field_round_trips_or_yields_to_the_field() {
    // An unset `size` leaves the name to the metadata, which reads back.
    let unset = FileAttachment::new("a.png")
        .add_metadata("size", "12KB")
        .add_metadata("width", 10);
    let read: FileAttachment =
        serde_json::from_value(serde_json::to_value(&unset).unwrap()).unwrap();
    assert_eq!(read.size, None);
    assert_eq!(read.metadata.get("size"), Some(&serde_json::json!("12KB")));
    assert_eq!(read.metadata.get("width"), Some(&serde_json::json!(10)));

    // A set field keeps its name; the metadata entry is not stored.
    let set = FileAttachment::with_metadata("b.png", None, Some(5), None)
        .add_metadata("size", "12KB")
        .add_metadata("key", "other.png");
    let json = serde_json::to_value(&set).unwrap();
    assert_eq!(json["key"], "b.png");
    assert_eq!(json["size"], 5);
    let read: FileAttachment = serde_json::from_value(json).unwrap();
    assert_eq!((read.key.as_str(), read.size), ("b.png", Some(5)));
    assert!(read.metadata.is_empty(), "{:?}", read.metadata);
}

struct AvatarHolder {
    files: FilesData,
}

impl HasAttachments for AvatarHolder {
    fn has_one_files() -> Vec<&'static str> {
        vec!["avatar"]
    }

    fn has_many_files() -> Vec<&'static str> {
        vec![]
    }

    fn get_files_data(&self) -> Result<FilesData, AttachmentError> {
        Ok(self.files.clone())
    }

    fn set_files_data(&mut self, data: FilesData) -> Result<(), AttachmentError> {
        self.files = data;
        Ok(())
    }
}

#[test]
fn detaching_a_has_one_by_key_clears_only_that_file() {
    let mut holder = AvatarHolder {
        files: FilesData::new(),
    };
    holder.attach("avatar", "new.png").unwrap();

    holder.detach("avatar", Some("old.png")).unwrap();
    assert_eq!(
        holder.files.get_one("avatar").map(|file| file.key),
        Some("new.png".to_string())
    );

    holder.detach("avatar", Some("new.png")).unwrap();
    assert!(holder.files.get_one("avatar").is_none());
}

#[test]
fn malformed_attachments_have_fallible_reads() {
    let files =
        FilesData::from_json(&serde_json::json!({"one": {"key": 42}, "many": [{"key": 42}]}));
    assert!(files.try_get_one("one").is_err());
    assert!(files.try_get_many("many").is_err());
    assert!(files.try_get_one("absent").unwrap().is_none());
    assert!(files.try_get_many("absent").unwrap().is_empty());
}

#[test]
fn bulk_removal_preserves_order_and_removes_duplicate_keys() {
    let mut files = FilesData::new();
    for key in ["a", "b", "a", "c", "d"] {
        files.add_many("many", FileAttachment::new(key));
    }
    files.remove_many("many", &["a", "c"]);
    assert_eq!(
        files
            .try_get_many("many")
            .unwrap()
            .iter()
            .map(|f| f.key.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "d"]
    );
}

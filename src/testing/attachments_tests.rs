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
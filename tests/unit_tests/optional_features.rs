// EXTENDED ATTACHMENTS TESTS
// =============================================================================

#[cfg(test)]
#[cfg(feature = "attachments")]
mod attachments_extended_tests {
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
        let attachment = FileAttachment::new("uploads/文档/图片.jpg");
        assert_eq!(attachment.filename, "图片.jpg");
    }

    #[test]
    fn test_file_attachment_special_characters() {
        let attachment = FileAttachment::new("uploads/file with spaces (1).pdf");
        assert_eq!(attachment.filename, "file with spaces (1).pdf");
    }
}

// =============================================================================
// EXTENDED TRANSLATIONS TESTS
// =============================================================================

#[cfg(test)]
#[cfg(feature = "translations")]
mod translations_extended_tests {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use tideorm::translations::{
        ApplyTranslations, HasTranslations, TranslationError, TranslationInput, TranslationsData,
    };

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TestProduct {
        id: i64,
        name: String,
        description: String,
        translations: Option<serde_json::Value>,
    }

    impl HasTranslations for TestProduct {
        fn translatable_fields() -> Vec<&'static str> {
            vec!["name", "description"]
        }

        fn allowed_languages() -> Vec<String> {
            vec![
                "en".to_string(),
                "ar".to_string(),
                "fr".to_string(),
                "es".to_string(),
            ]
        }

        fn fallback_language() -> String {
            "en".to_string()
        }

        fn get_translations_data(&self) -> Result<TranslationsData, TranslationError> {
            match &self.translations {
                Some(json) => Ok(TranslationsData::from_json(json)),
                None => Ok(TranslationsData::new()),
            }
        }

        fn set_translations_data(
            &mut self,
            data: TranslationsData,
        ) -> Result<(), TranslationError> {
            self.translations = Some(data.to_json());
            Ok(())
        }

        fn get_default_value(&self, field: &str) -> Result<serde_json::Value, TranslationError> {
            match field {
                "name" => Ok(serde_json::json!(self.name)),
                "description" => Ok(serde_json::json!(self.description)),
                _ => Err(TranslationError::InvalidField(format!(
                    "Unknown field: {}",
                    field
                ))),
            }
        }
    }

    impl TestProduct {
        fn new(id: i64, name: &str, description: &str) -> Self {
            Self {
                id,
                name: name.to_string(),
                description: description.to_string(),
                translations: None,
            }
        }
    }

    #[test]
    fn test_has_translations_set_single() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "ar", "منتج").unwrap();

        let trans = product.get_translation("name", "ar").unwrap();
        assert_eq!(trans, Some(serde_json::json!("منتج")));
    }

    #[test]
    fn test_has_translations_set_multiple() {
        let mut product = TestProduct::new(1, "Product", "Description");

        let mut translations = HashMap::new();
        translations.insert("en", "Product Name");
        translations.insert("ar", "اسم المنتج");
        translations.insert("fr", "Nom du produit");

        product.set_translations("name", translations).unwrap();

        assert_eq!(
            product.get_translation("name", "en").unwrap(),
            Some(serde_json::json!("Product Name"))
        );
        assert_eq!(
            product.get_translation("name", "ar").unwrap(),
            Some(serde_json::json!("اسم المنتج"))
        );
        assert_eq!(
            product.get_translation("name", "fr").unwrap(),
            Some(serde_json::json!("Nom du produit"))
        );
    }

    #[test]
    fn test_has_translations_get_translated_with_fallback() {
        let mut product = TestProduct::new(1, "Default Product", "Default Description");

        product
            .set_translation("name", "en", "English Name")
            .unwrap();
        product
            .set_translation("name", "ar", "الاسم العربي")
            .unwrap();

        let ar = product.get_translated("name", "ar").unwrap();
        assert_eq!(ar, serde_json::json!("الاسم العربي"));

        let es = product.get_translated("name", "es").unwrap();
        assert_eq!(es, serde_json::json!("English Name"));
    }

    #[test]
    fn test_has_translations_fallback_to_default() {
        let product = TestProduct::new(1, "Default Product", "Default Description");

        let name = product.get_translated("name", "ar").unwrap();
        assert_eq!(name, serde_json::json!("Default Product"));
    }

    #[test]
    fn test_has_translations_get_all() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "en", "English").unwrap();
        product.set_translation("name", "ar", "عربي").unwrap();
        product.set_translation("name", "fr", "Français").unwrap();

        let all = product.get_all_translations("name").unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all.get("en"), Some(&serde_json::json!("English")));
        assert_eq!(all.get("ar"), Some(&serde_json::json!("عربي")));
        assert_eq!(all.get("fr"), Some(&serde_json::json!("Français")));
    }

    #[test]
    fn test_has_translations_get_for_language() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "ar", "منتج").unwrap();
        product
            .set_translation("description", "ar", "الوصف")
            .unwrap();
        product.set_translation("name", "en", "Product").unwrap();

        let ar_trans = product.get_translations_for_language("ar").unwrap();
        assert_eq!(ar_trans.len(), 2);
        assert_eq!(ar_trans.get("name"), Some(&serde_json::json!("منتج")));
        assert_eq!(
            ar_trans.get("description"),
            Some(&serde_json::json!("الوصف"))
        );
    }

    #[test]
    fn test_has_translations_remove_single() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("name", "fr", "Produit").unwrap();

        product.remove_translation("name", "ar").unwrap();

        assert!(!product.has_translation("name", "ar").unwrap());
        assert!(product.has_translation("name", "fr").unwrap());
    }

    #[test]
    fn test_has_translations_remove_field() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("name", "fr", "Produit").unwrap();

        product.remove_field_translations("name").unwrap();

        assert!(!product.has_any_translation("name").unwrap());
    }

    #[test]
    fn test_has_translations_clear_all() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "ar", "منتج").unwrap();
        product
            .set_translation("description", "ar", "الوصف")
            .unwrap();

        product.clear_translations().unwrap();

        assert!(!product.has_any_translation("name").unwrap());
        assert!(!product.has_any_translation("description").unwrap());
    }

    #[test]
    fn test_has_translations_sync() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "ar", "قديم").unwrap();
        product.set_translation("name", "fr", "Ancien").unwrap();

        let mut new_trans = HashMap::new();
        new_trans.insert("en", "New");
        new_trans.insert("es", "Nuevo");

        product.sync_translations("name", new_trans).unwrap();

        assert!(!product.has_translation("name", "ar").unwrap());
        assert!(!product.has_translation("name", "fr").unwrap());
        assert!(product.has_translation("name", "en").unwrap());
        assert!(product.has_translation("name", "es").unwrap());
    }

    #[test]
    fn test_has_translations_available_languages() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "en", "English").unwrap();
        product.set_translation("name", "ar", "عربي").unwrap();
        product.set_translation("name", "fr", "Français").unwrap();

        let langs = product.available_languages("name").unwrap();
        assert_eq!(langs.len(), 3);
        assert!(langs.contains(&"en".to_string()));
        assert!(langs.contains(&"ar".to_string()));
        assert!(langs.contains(&"fr".to_string()));
    }

    #[test]
    fn test_has_translations_invalid_field() {
        let mut product = TestProduct::new(1, "Product", "Description");

        let result = product.set_translation("invalid_field", "en", "value");
        assert!(result.is_err());
    }

    #[test]
    fn test_has_translations_invalid_language() {
        let mut product = TestProduct::new(1, "Product", "Description");

        let result = product.set_translation("name", "invalid_lang", "value");
        assert!(result.is_err());
    }

    #[test]
    fn test_has_translations_to_translated_json() {
        let mut product = TestProduct::new(1, "Default Product", "Default Description");

        product.set_translation("name", "ar", "منتج عربي").unwrap();
        product
            .set_translation("description", "ar", "وصف عربي")
            .unwrap();
        product
            .set_translation("name", "en", "English Product")
            .unwrap();
        product
            .set_translation("description", "en", "English Description")
            .unwrap();

        let mut opts = std::collections::HashMap::new();
        opts.insert("language".to_string(), "ar".to_string());
        let json = product.to_translated_json(Some(opts));

        assert_eq!(json.get("name"), Some(&serde_json::json!("منتج عربي")));
        assert_eq!(
            json.get("description"),
            Some(&serde_json::json!("وصف عربي"))
        );
        assert!(json.get("translations").is_none());
    }

    #[test]
    fn test_has_translations_to_json_with_all() {
        let mut product = TestProduct::new(1, "Default Product", "Default Description");

        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("name", "en", "Product").unwrap();

        let json = product.to_json_with_all_translations();
        assert!(json.get("translations").is_some());
    }

    #[test]
    fn test_apply_translations() {
        let mut product = TestProduct::new(1, "Product", "Description");

        let mut input = TranslationInput::new();
        input.add("name", "ar", "منتج");
        input.add("name", "fr", "Produit");
        input.add("description", "ar", "الوصف");

        product.apply_translations(input).unwrap();

        assert_eq!(
            product.get_translation("name", "ar").unwrap(),
            Some(serde_json::json!("منتج"))
        );
        assert_eq!(
            product.get_translation("name", "fr").unwrap(),
            Some(serde_json::json!("Produit"))
        );
        assert_eq!(
            product.get_translation("description", "ar").unwrap(),
            Some(serde_json::json!("الوصف"))
        );
    }

    #[test]
    fn test_apply_translations_from_json() {
        let mut product = TestProduct::new(1, "Product", "Description");

        let api_data = serde_json::json!({
            "name": {"ar": "منتج", "fr": "Produit"},
            "description": {"ar": "الوصف"}
        });

        let input = TranslationInput::from_json(&api_data).unwrap();
        product.apply_translations(input).unwrap();

        assert_eq!(
            product.get_translation("name", "ar").unwrap(),
            Some(serde_json::json!("منتج"))
        );
        assert_eq!(
            product.get_translation("description", "ar").unwrap(),
            Some(serde_json::json!("الوصف"))
        );
    }

    #[test]
    fn test_translations_json_persistence() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "ar", "منتج").unwrap();
        product
            .set_translation("description", "ar", "الوصف")
            .unwrap();

        let json = serde_json::to_string(&product).unwrap();
        let loaded: TestProduct = serde_json::from_str(&json).unwrap();

        assert_eq!(
            loaded.get_translation("name", "ar").unwrap(),
            Some(serde_json::json!("منتج"))
        );
        assert_eq!(
            loaded.get_translation("description", "ar").unwrap(),
            Some(serde_json::json!("الوصف"))
        );
    }

    #[test]
    fn test_translations_rtl_languages() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product
            .set_translation("name", "ar", "منتج رائع جداً")
            .unwrap();

        let ar = product.get_translated("name", "ar").unwrap();
        assert_eq!(ar, serde_json::json!("منتج رائع جداً"));
    }

    #[test]
    fn test_translations_with_html() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product
            .set_translation(
                "description",
                "en",
                "<p>Product <strong>description</strong></p>",
            )
            .unwrap();

        let desc = product.get_translated("description", "en").unwrap();
        assert_eq!(
            desc,
            serde_json::json!("<p>Product <strong>description</strong></p>")
        );
    }

    #[test]
    fn test_translations_with_emoji() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product
            .set_translation("name", "en", "Product 🎉 Special Edition")
            .unwrap();

        let name = product.get_translated("name", "en").unwrap();
        assert_eq!(name, serde_json::json!("Product 🎉 Special Edition"));
    }

    #[test]
    fn test_translations_empty_string() {
        let mut product = TestProduct::new(1, "Product", "Description");

        product.set_translation("name", "en", "").unwrap();

        let name = product.get_translation("name", "en").unwrap();
        assert_eq!(name, Some(serde_json::json!("")));
    }

    #[test]
    fn test_translations_long_text() {
        let mut product = TestProduct::new(1, "Product", "Description");

        let long_text = "A".repeat(10000);
        product
            .set_translation("description", "en", long_text.clone())
            .unwrap();

        let desc = product.get_translated("description", "en").unwrap();
        assert_eq!(desc, serde_json::json!(long_text));
    }
}

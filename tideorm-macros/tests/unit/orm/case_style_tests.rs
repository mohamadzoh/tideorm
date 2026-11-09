use super::*;

#[test]
fn test_convert_case() {
    assert_eq!("testMe", CaseStyle::CamelCase.apply("test_me"));
    assert_eq!("TestMe", CaseStyle::PascalCase.apply("test_me"));
}

/// Every style splits words as the model derive does: digits are words.
#[test]
fn every_style_splits_words_like_the_model_derive() {
    let cases = [
        (CaseStyle::PascalCase, "S3BucketURL", "S3BucketUrl"),
        (CaseStyle::CamelCase, "S3BucketURL", "s3BucketUrl"),
        (CaseStyle::MixedCase, "S3BucketURL", "s3BucketUrl"),
        (CaseStyle::SnakeCase, "S3BucketURL", "s_3_bucket_url"),
        (CaseStyle::KebabCase, "S3BucketURL", "s-3-bucket-url"),
        (CaseStyle::ShoutySnakeCase, "S3BucketURL", "S_3_BUCKET_URL"),
        (
            CaseStyle::ScreamingKebabCase,
            "S3BucketURL",
            "S-3-BUCKET-URL",
        ),
        (CaseStyle::TitleCase, "S3BucketURL", "S 3 Bucket Url"),
        (CaseStyle::UpperCase, "S3BucketURL", "S3BUCKETURL"),
        (CaseStyle::LowerCase, "S3BucketURL", "s3bucketurl"),
        (CaseStyle::SnakeCase, "Sha256", "sha_256"),
        (CaseStyle::CamelCase, "", ""),
    ];
    for (style, text, expected) in cases {
        assert_eq!(style.apply(text), expected, "{style:?} of {text}");
    }
}

/// The values the error lists are the ones accepted, and nothing else is.
#[test]
fn only_the_listed_styles_are_accepted() {
    for style in VALID_CASE_STYLES {
        assert!(style.parse::<CaseStyle>().is_ok(), "{style}");
    }
    for alias in [
        "camel_case",
        "snek_case",
        "kebab_case",
        "shouty_snake_case",
        "shouty_snek_case",
    ] {
        assert!(alias.parse::<CaseStyle>().is_err(), "{alias}");
    }
}

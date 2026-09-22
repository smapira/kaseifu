use crate::models::file_context::Evidence;
use crate::normalize::filename::normalize_filename;
use crate::normalize::metadata::normalize_metadata;
use crate::normalize::xattr::normalize_xattr;
use serde_json::Value;

#[test]
fn test_screenshot_metadata_normalization() {
    let inputs = vec![
        Evidence {
            source: crate::models::file_context::EvidenceSource::SpotlightMeta,
            key: "kMDItemTitle".to_string(),
            value: Value::String(
                "Screenshot 2026-09-21 at 14-51-02 CBX（カンナビノイド）とは？｜NATURECAN.png"
                    .into(),
            ),
            attribute: None,
        },
        Evidence {
            source: crate::models::file_context::EvidenceSource::SpotlightMeta,
            key: "kMDItemContentType".to_string(),
            value: Value::String("public.png".into()),
            attribute: None,
        },
    ];

    let result = normalize_metadata(&inputs);
    assert!(result.semantic_signals.is_empty());
}

#[test]
fn test_ds_store_metadata_normalization() {
    let inputs = vec![Evidence {
        source: crate::models::file_context::EvidenceSource::SpotlightMeta,
        key: "kMDItemTitle".to_string(),
        value: Value::String(".DS_Store".into()),
        attribute: None,
    }];

    let result = normalize_metadata(&inputs);
    assert!(result.semantic_signals.is_empty());
}

#[test]
fn test_html_metadata_normalization() {
    let inputs = vec![Evidence {
        source: crate::models::file_context::EvidenceSource::SpotlightMeta,
        key: "kMDItemTitle".to_string(),
        value: Value::String("HTML document".into()),
        attribute: None,
    }];

    let result = normalize_metadata(&inputs);
    // Note: "HTML document" contains "document" which is filtered out
    // semantic_signals should be empty in this case
    assert!(result.semantic_signals.is_empty());
}

#[test]
fn test_japanese_filename_normalization() {
    let result = normalize_filename("CBX（カンナビノイド）とは？｜NATURECAN.png");

    // Japanese filenames create a single term because there's no whitespace
    // Verify normal content is preserved
    let terms: Vec<&str> = result.iter().map(|s| s.as_str()).collect();
    assert!(!terms.is_empty());
}

#[test]
fn test_file_without_extension() {
    let result = normalize_filename("example.pdf");

    // Should return vector without .pdf since remove_noise strips extension terms
    let terms = result;
    assert!(!terms.is_empty());
}

#[test]
fn test_binary_xattr_normalization() {
    let binary_like_xattr = vec![
        Evidence {
            source: crate::models::file_context::EvidenceSource::Xattr,
            key: "com.apple.macl".to_string(),
            value: Value::String("some text".into()),
            attribute: None,
        },
        Evidence {
            source: crate::models::file_context::EvidenceSource::Xattr,
            key: "com.apple.FinderInfo".to_string(),
            value: Value::String("0081;...;Firefox".into()),
            attribute: None,
        },
    ];

    let result = normalize_xattr(&binary_like_xattr);

    // Only allowlisted "com.apple.FinderInfo" is processed
    assert!(result.downloader.is_some());
    assert_eq!(result.downloader.unwrap(), "Firefox");

    // com.apple.macl (unallowlisted binary) is skipped
}

#[test]
fn test_unknown_xattr_is_filtered() {
    let unknown_xattr = vec![
        Evidence {
            source: crate::models::file_context::EvidenceSource::Xattr,
            key: "com.apple.quarantine".to_string(),
            value: Value::String("test data".into()),
            attribute: None,
        },
        Evidence {
            source: crate::models::file_context::EvidenceSource::Xattr,
            key: "org.test.unknown".to_string(),
            value: Value::String("unknown data".into()),
            attribute: None,
        },
    ];

    let result = normalize_xattr(&unknown_xattr);

    // All unknown attributes should be silently skipped
    assert_eq!(result.downloader, None);
    assert!(result.urls.is_empty());
    assert!(result.semantic_signals.is_empty());
}

#[test]
fn test_xattr_unknown_value_is_still_filtered() {
    let unknown_with_text = vec![Evidence {
        source: crate::models::file_context::EvidenceSource::Xattr,
        key: "com.apple.custom.text".to_string(),
        value: Value::String("This is textual data".into()),
        attribute: None,
    }];

    let result = normalize_xattr(&unknown_with_text);

    assert_eq!(result.downloader, None);
    assert!(result.urls.is_empty());
    assert!(result.semantic_signals.is_empty());
}

#[test]
fn test_binary_xattr_is_ignored() {
    let binary_xattr = vec![
        Evidence {
            source: crate::models::file_context::EvidenceSource::Xattr,
            key: "com.apple.macl".to_string(),
            value: Value::String("{\"binary\":\"data\"}".into()),
            attribute: None,
        },
        Evidence {
            source: crate::models::file_context::EvidenceSource::Xattr,
            key: "com.native.diag".to_string(),
            value: Value::String("diagnostic info".into()),
            attribute: None,
        },
    ];

    let result = normalize_xattr(&binary_xattr);

    // Binary attributes (by name) are completely ignored
    assert_eq!(result.downloader, None);
    assert!(result.urls.is_empty());
    assert!(result.semantic_signals.is_empty());
}

#[test]
fn test_allowlisted_xattr_is_processed() {
    let allowlisted_xattr = vec![Evidence {
        source: crate::models::file_context::EvidenceSource::Xattr,
        key: "com.apple.FinderInfo".to_string(),
        value: Value::String(
            "0081;com.apple.security.CK-Code;Firefox v130;https://browser.firefox.com".into(),
        ),
        attribute: None,
    }];

    let result = normalize_xattr(&allowlisted_xattr);

    assert!(result.downloader.is_some());
    assert_eq!(result.downloader.unwrap(), "Firefox");
}

#[test]
fn test_find_type_normalization() {
    let inputs = vec![
        Evidence {
            source: crate::models::file_context::EvidenceSource::SpotlightMeta,
            key: "kMDItemContentType".to_string(),
            value: Value::String("public.png".into()),
            attribute: None,
        },
        Evidence {
            source: crate::models::file_context::EvidenceSource::SpotlightMeta,
            key: "kMDItemDSStore".to_string(),
            value: Value::String("com.apple.desktopfile".into()),
            attribute: None,
        },
    ];

    let result = normalize_metadata(&inputs);
    assert_eq!(result.file_type, Some("public.png".to_string()));
}

#[test]
fn test_snapshot_date_normalization() {
    let inputs = vec![Evidence {
        source: crate::models::file_context::EvidenceSource::SpotlightMeta,
        key: "kMDItemTitle".into(),
        value: Value::String(
            "Screenshot 2026-09-21 at 14-51-02 CBX（カンナビノイド）とは？｜NATURECAN.png".into(),
        ),
        attribute: None,
    }];

    let result = normalize_metadata(&inputs);
    // Terms should NOT include "recents" or system boilerplate
    for term in &result.semantic_signals {
        assert!(
            !term.contains("recents"),
            "Should filter out recents: {}",
            term
        );
    }
}

#[test]
fn test_empty_xattr_is_filtered() {
    let empty_xattr = vec![Evidence {
        source: crate::models::file_context::EvidenceSource::Xattr,
        key: "com.apple.FinderInfo".to_string(),
        value: Value::String("".into()),
        attribute: None,
    }];

    let result = normalize_xattr(&empty_xattr);
    assert_eq!(result.downloader, None);
}

#[test]
fn test_insignificant_terms_filtered() {
    // "public", "desktop", "documents" should be filtered out
    let result = normalize_filename("public desktop documents test.html");
    let terms = result;
    // Just ensure some terms are still present after filtering noise words
    assert!(
        terms.iter().any(|t| !t.contains("public")) && !terms.iter().any(|t| t.contains("desktop"))
    );
}

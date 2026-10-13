//! v3.2 Image Metadata & Context-Aware Classification (Phase 1-7).
//!
//! BDD coverage for `documents/plans/user/20261012.md` sections 3-9, 11-12:
//! Spotlight-first extraction, EXIF fallback (JPEG/TIFF/HEIC), datetime/GPS
//! normalization (no guessing, source kept), Jev mapping onto the real
//! `/v1/systemone` schema with GPS privacy gating, ASCII `inspect` display,
//! and `[metadata.*]` configuration. Mock servers plus temp files only;
//! the real Keychain and user photos are never touched.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kaseifu::auth::keychain::MemoryStore;
use kaseifu::classifier::jev::{JevIntegration, RealJevIntegration};
use kaseifu::config::key_source::{KeySourceMode, KeySourceSetting};
use kaseifu::config::loader::{UserConfig, apply_setting, parse_toml};
use kaseifu::config::resolver::{CliOverrides, LocatedValue, resolve_with};
use kaseifu::context::exif::{ExifOutcome, parse_bytes};
use kaseifu::context::image_metadata::{MetadataOptions, collect, masked_location};
use kaseifu::models::decision::DecisionContext;
use kaseifu::models::file_context::{
    CameraInfo, CaptureTime, Evidence, EvidenceSource, FileContext, ImageDimensions, ImageLocation,
    ImageMetadata, NormalizedFileContext,
};
use kaseifu::normalize::image::validate;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_path(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-v32-{}-{}", std::process::id(), name));
    dir
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create temp dir");
    }
    std::fs::write(&path, bytes).expect("write temp file");
    path
}

fn spotlight_evidence(key: &str, value: &str) -> Evidence {
    Evidence {
        source: EvidenceSource::SpotlightMeta,
        key: key.to_string(),
        value: serde_json::Value::String(value.to_string()),
        attribute: Some(key.to_string()),
    }
}

fn full_spotlight_evidence() -> Vec<Evidence> {
    vec![
        spotlight_evidence("kMDItemEXIFDateTimeOriginal", "2025-08-10 15:30:00 +0000"),
        spotlight_evidence("kMDItemLatitude", "35.658"),
        spotlight_evidence("kMDItemLongitude", "139.702"),
        spotlight_evidence("kMDItemAltitude", "12.5"),
        spotlight_evidence("kMDItemAcquisitionMake", "Apple"),
        spotlight_evidence("kMDItemAcquisitionModel", "iPhone"),
        spotlight_evidence("kMDItemPixelWidth", "4032"),
        spotlight_evidence("kMDItemPixelHeight", "3024"),
    ]
}

fn jpg_path(name: &str) -> PathBuf {
    let mut path = temp_path(name);
    path.set_extension("jpg");
    path
}

// Minimal little-endian TIFF with Make/Model/Exif/GPS (see module docs).
fn fixture_tiff() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"II*\0");
    out.extend_from_slice(&8u32.to_le_bytes());
    // IFD0: 4 entries.
    out.extend_from_slice(&4u16.to_le_bytes());
    push_entry(&mut out, 0x010F, 2, 9, 62);
    push_entry(&mut out, 0x0110, 2, 10, 71);
    push_entry(&mut out, 0x8769, 4, 1, 81);
    push_entry(&mut out, 0x8825, 4, 1, 119);
    out.extend_from_slice(&0u32.to_le_bytes());
    // Data at 62: Make.
    out.extend_from_slice(b"TestMake\0");
    // Data at 71: Model.
    out.extend_from_slice(b"TestModel\0");
    // ExifIFD at 81: DateTimeOriginal.
    out.extend_from_slice(&1u16.to_le_bytes());
    push_entry(&mut out, 0x9003, 2, 20, 99);
    out.extend_from_slice(&0u32.to_le_bytes());
    // DateTimeOriginal at 99 (20 bytes).
    out.extend_from_slice(b"2025:08:10 15:30:00\0");
    // GPS IFD at 119: 5 entries.
    out.extend_from_slice(&5u16.to_le_bytes());
    push_inline_ascii(&mut out, 0x0001, b"N");
    push_entry(&mut out, 0x0002, 5, 3, 185);
    push_inline_ascii(&mut out, 0x0003, b"E");
    push_entry(&mut out, 0x0004, 5, 3, 209);
    push_entry(&mut out, 0x0006, 5, 1, 233);
    out.extend_from_slice(&0u32.to_le_bytes());
    // Rationals: lat 35 + 39/60 + 28.8/3600 = 35.658 at 185.
    for (num, den) in [(35u32, 1u32), (39, 1), (288, 10)] {
        out.extend_from_slice(&num.to_le_bytes());
        out.extend_from_slice(&den.to_le_bytes());
    }
    // Lon 139 + 42/60 + 7.2/3600 = 139.702 at 209.
    for (num, den) in [(139u32, 1u32), (42, 1), (72, 10)] {
        out.extend_from_slice(&num.to_le_bytes());
        out.extend_from_slice(&den.to_le_bytes());
    }
    // Altitude 12.5 at 233.
    out.extend_from_slice(&25u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out
}

fn push_entry(out: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&field_type.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_inline_ascii(out: &mut Vec<u8>, tag: u16, text: &[u8]) {
    let mut value = [0u8; 4];
    value[..text.len()].copy_from_slice(text);
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&value);
}

fn fixture_jpeg() -> Vec<u8> {
    let tiff = fixture_tiff();
    let mut out = vec![0xFF, 0xD8];
    // APP1 with Exif header.
    let app1_len = (tiff.len() + 8) as u16;
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&app1_len.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
    // SOF0: 64x48.
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x30, 0x00, 0x40]);
    out.extend_from_slice(&[0x01, 0x01, 0x11, 0x00, 0xFF, 0xD9]);
    out
}

fn fixture_png() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(b"IHDR");
    out.extend_from_slice(&64u32.to_be_bytes());
    out.extend_from_slice(&48u32.to_be_bytes());
    out.extend_from_slice(&[8, 2, 0, 0, 0]);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

// ---------------------------------------------------------------------------
// Spotlight-first extraction (Phase 1/2)
// ---------------------------------------------------------------------------

#[test]
fn v32_spotlight_full_p0_attributes_assemble_with_sources() {
    let path = jpg_path("spotlight-full.jpg");
    let meta = collect(
        &path,
        &full_spotlight_evidence(),
        &MetadataOptions::default(),
    )
    .expect("P0 present");
    let capture = meta.capture_time.expect("capture present");
    assert_eq!(capture.source, "spotlight");
    assert!(capture.timezone_known);
    assert_eq!(capture.timezone.as_deref(), Some("+0000"));
    let location = meta.location.expect("GPS present");
    assert!((location.latitude - 35.658).abs() < 1e-9);
    assert!((location.longitude - 139.702).abs() < 1e-9);
    assert_eq!(location.altitude_m, Some(12.5));
    assert_eq!(location.source, "spotlight");
    assert_eq!(meta.camera.make.as_deref(), Some("Apple"));
    assert_eq!(meta.camera.model.as_deref(), Some("iPhone"));
    let dims = meta.dimensions.expect("dimensions present");
    assert_eq!((dims.width, dims.height), (4032, 3024));
}

#[test]
fn v32_spotlight_missing_values_stay_missing() {
    let path = jpg_path("spotlight-empty.jpg");
    // No evidence and no file on disk: nothing is fabricated.
    assert!(collect(&path, &[], &MetadataOptions::default()).is_none());
}

#[test]
fn v32_spotlight_partial_gps_is_dropped() {
    // Latitude without longitude is incomplete: no location is assembled.
    let path = jpg_path("spotlight-partial.jpg");
    let evidence = vec![spotlight_evidence("kMDItemLatitude", "35.658")];
    assert!(collect(&path, &evidence, &MetadataOptions::default()).is_none());
}

#[test]
fn v32_spotlight_null_and_garbage_are_ignored() {
    let path = jpg_path("spotlight-null.jpg");
    let evidence = vec![
        spotlight_evidence("kMDItemLatitude", "(null)"),
        spotlight_evidence("kMDItemLongitude", "not-a-number"),
        spotlight_evidence("kMDItemAcquisitionMake", "(null)"),
    ];
    assert!(collect(&path, &evidence, &MetadataOptions::default()).is_none());
}

#[test]
fn v32_spotlight_content_creation_date_is_the_fallback_capture() {
    let path = jpg_path("spotlight-created.jpg");
    let evidence = vec![spotlight_evidence(
        "kMDItemContentCreationDate",
        "2025-08-10 15:30:00 +0000",
    )];
    let meta = collect(&path, &evidence, &MetadataOptions::default()).expect("fallback capture");
    assert_eq!(meta.capture_time.expect("capture").source, "spotlight");
}

#[test]
fn v32_spotlight_mdls_failure_never_blocks() {
    // A missing file has no Spotlight record: extraction yields nothing,
    // and building a FileContext for other paths is unaffected.
    let missing = temp_path("does-not-exist.jpg");
    let result = kaseifu::platform::macos::spotlight::metadata(&missing);
    assert!(result.is_ok());
}

#[test]
fn v32_non_images_omit_image_metadata() {
    let mut pdf = temp_path("doc.pdf");
    let _ = &mut pdf;
    let path = Path::new("report.pdf");
    assert!(
        collect(
            path,
            &full_spotlight_evidence(),
            &MetadataOptions::default()
        )
        .is_none()
    );
    let normalized = NormalizedFileContext {
        target: kaseifu::models::file_context::TargetFile {
            filename: "report.pdf".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let json = serde_json::to_string(&normalized).expect("serializes");
    assert!(!json.contains("image_metadata"), "JSON compat: {json}");
}

// ---------------------------------------------------------------------------
// EXIF fallback (Phase 3)
// ---------------------------------------------------------------------------

#[test]
fn v32_exif_jpeg_fixture_parses_all_fields() {
    match parse_bytes(&fixture_jpeg(), "jpg") {
        ExifOutcome::Found(raw) => {
            assert_eq!(raw.capture_original.as_deref(), Some("2025:08:10 15:30:00"));
            assert_eq!(raw.make.as_deref(), Some("TestMake"));
            assert_eq!(raw.model.as_deref(), Some("TestModel"));
            let (lat, lon) = (raw.latitude.unwrap(), raw.longitude.unwrap());
            assert!((lat - 35.658).abs() < 1e-6, "lat {lat}");
            assert!((lon - 139.702).abs() < 1e-6, "lon {lon}");
            assert_eq!(raw.altitude_m, Some(12.5));
        }
        other => panic!("expected Found, got {other:?}"),
    }
}

#[test]
fn v32_exif_tiff_file_parses_from_disk() {
    let path = write_temp("v32-sample.tif", &fixture_tiff());
    match kaseifu::context::exif::parse_file(&path) {
        ExifOutcome::Found(raw) => {
            assert_eq!(raw.make.as_deref(), Some("TestMake"));
            assert!(raw.latitude.is_some() && raw.longitude.is_some());
        }
        other => panic!("expected Found, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn v32_exif_fills_only_spotlight_gaps() {
    // Spotlight has camera + dimensions; EXIF supplies capture + GPS.
    let path = write_temp("v32-gap.jpg", &fixture_jpeg());
    let evidence = vec![
        spotlight_evidence("kMDItemAcquisitionMake", "SpotMake"),
        spotlight_evidence("kMDItemPixelWidth", "4032"),
        spotlight_evidence("kMDItemPixelHeight", "3024"),
    ];
    let meta = collect(&path, &evidence, &MetadataOptions::default()).expect("merged");
    assert_eq!(meta.camera.make.as_deref(), Some("SpotMake"));
    assert_eq!(meta.camera.source.as_deref(), Some("spotlight"));
    let capture = meta.capture_time.expect("EXIF fills capture");
    assert_eq!(capture.source, "exif");
    assert!(!capture.timezone_known && capture.timezone.is_none());
    assert_eq!(capture.value, "2025-08-10T15:30:00");
    assert_eq!(meta.location.expect("EXIF fills GPS").source, "exif");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn v32_exif_never_overwrites_spotlight_on_conflict() {
    // Both sources have a make: Spotlight wins, EXIF does not overwrite.
    let path = write_temp("v32-conflict.jpg", &fixture_jpeg());
    let evidence = vec![spotlight_evidence("kMDItemAcquisitionMake", "SpotMake")];
    let meta = collect(&path, &evidence, &MetadataOptions::default()).expect("merged");
    assert_eq!(meta.camera.make.as_deref(), Some("SpotMake"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn v32_exif_plain_jpeg_without_exif_is_no_exif() {
    // Minimal JPEG: SOI + EOI only.
    assert_eq!(
        parse_bytes(&[0xFF, 0xD8, 0xFF, 0xD9], "jpg"),
        ExifOutcome::NoExif
    );
}

#[test]
fn v32_exif_truncated_jpeg_is_corrupt_not_a_guess() {
    match parse_bytes(&[0xFF, 0xD8, 0xFF], "jpg") {
        ExifOutcome::Corrupt(_) => {}
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[test]
fn v32_exif_heic_without_exif_is_unsupported() {
    let mut bytes = vec![0u8; 32];
    bytes[4..8].copy_from_slice(b"ftyp");
    bytes[8..12].copy_from_slice(b"heic");
    match parse_bytes(&bytes, "heic") {
        ExifOutcome::Unsupported(reason) => assert!(reason.contains("heic")),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn v32_exif_heic_with_embedded_exif_is_found() {
    let mut bytes = vec![0u8; 32];
    bytes[4..8].copy_from_slice(b"ftyp");
    bytes[8..12].copy_from_slice(b"heic");
    bytes.extend_from_slice(b"Exif\0\0");
    bytes.extend_from_slice(&fixture_tiff());
    match parse_bytes(&bytes, "heic") {
        ExifOutcome::Found(raw) => assert_eq!(raw.make.as_deref(), Some("TestMake")),
        other => panic!("expected Found, got {other:?}"),
    }
}

#[test]
fn v32_exif_png_and_webp_are_unsupported() {
    assert!(matches!(
        parse_bytes(&fixture_png(), "png"),
        ExifOutcome::Unsupported(_)
    ));
    let mut webp = vec![0u8; 12];
    webp[0..4].copy_from_slice(b"RIFF");
    webp[8..12].copy_from_slice(b"WEBP");
    assert!(matches!(
        parse_bytes(&webp, "webp"),
        ExifOutcome::Unsupported(_)
    ));
}

#[test]
fn v32_exif_png_dimensions_come_from_ihdr() {
    let path = write_temp("v32-sample.png", &fixture_png());
    let meta = collect(&path, &[], &MetadataOptions::default()).expect("png dims");
    let dims = meta.dimensions.expect("dimensions");
    assert_eq!((dims.width, dims.height), (64, 48));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn v32_image_extraction_toggles_are_honored() {
    let path = write_temp("v32-toggles.jpg", &fixture_jpeg());
    let off = MetadataOptions {
        spotlight: false,
        exif_fallback: false,
    };
    assert!(collect(&path, &full_spotlight_evidence(), &off).is_none());
    let no_spotlight = MetadataOptions {
        spotlight: false,
        exif_fallback: true,
    };
    let meta = collect(&path, &[], &no_spotlight).expect("EXIF still runs");
    assert_eq!(meta.camera.make.as_deref(), Some("TestMake"));
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// Normalization (Phase 4)
// ---------------------------------------------------------------------------

fn gps_meta(latitude: f64, longitude: f64) -> ImageMetadata {
    ImageMetadata {
        location: Some(ImageLocation {
            latitude,
            longitude,
            altitude_m: None,
            source: "exif".to_string(),
        }),
        ..Default::default()
    }
}

#[test]
fn v32_normalize_accepts_valid_gps_and_exif_datetime() {
    let mut meta = gps_meta(35.658, 139.702);
    meta.capture_time = Some(CaptureTime {
        value: "2025-08-10T15:30:00".to_string(),
        timezone: None,
        source: "exif".to_string(),
        timezone_known: false,
    });
    let valid = validate(meta).expect("valid");
    // v0.4.1: availability markers moved to the Jev adapter (derived from
    // `image_metadata`); see tests/v0401_discovery_fix_test.rs.
    assert!(valid.location.is_some() && valid.capture_time.is_some());
}

#[test]
fn v32_normalize_rejects_out_of_range_gps() {
    assert!(validate(gps_meta(91.0, 0.0)).is_none_or(|meta| meta.location.is_none()));
    assert!(validate(gps_meta(0.0, 181.0)).is_none_or(|meta| meta.location.is_none()));
    assert!(validate(gps_meta(-91.0, 0.0)).is_none_or(|meta| meta.location.is_none()));
}

#[test]
fn v32_normalize_rejects_nan_and_infinite_gps() {
    assert!(validate(gps_meta(f64::NAN, 0.0)).is_none_or(|meta| meta.location.is_none()));
    assert!(validate(gps_meta(f64::INFINITY, 0.0)).is_none_or(|meta| meta.location.is_none()));
    assert!(validate(gps_meta(0.0, f64::NEG_INFINITY)).is_none_or(|meta| meta.location.is_none()));
}

#[test]
fn v32_normalize_exif_datetimes_never_gain_a_timezone() {
    let path = write_temp("v32-tz.jpg", &fixture_jpeg());
    let meta = collect(&path, &[], &MetadataOptions::default()).expect("EXIF meta");
    let capture = meta.capture_time.expect("capture");
    assert!(!capture.timezone_known && capture.timezone.is_none());
    assert_eq!(capture.value, "2025-08-10T15:30:00");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn v32_normalize_mtime_is_never_a_capture_time() {
    // Filesystem mtime evidence must not become a capture time: only
    // Spotlight/EXIF origins count.
    let path = write_temp("v32-mtime.jpg", &[0xFF, 0xD8, 0xFF, 0xD9]);
    let evidence = vec![spotlight_evidence(
        "kMDItemFSContentChangeDate",
        "2025-08-10",
    )];
    assert!(collect(&path, &evidence, &MetadataOptions::default()).is_none());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn v32_normalize_camera_and_dimensions_validation() {
    let meta = ImageMetadata {
        camera: CameraInfo {
            make: Some("  ".to_string()),
            model: Some("iPhone".to_string()),
            source: Some("spotlight".to_string()),
        },
        dimensions: Some(ImageDimensions {
            width: 0,
            height: 3024,
            source: Some("spotlight".to_string()),
        }),
        ..Default::default()
    };
    let valid = validate(meta).expect("model survives");
    assert!(valid.camera.make.is_none());
    assert_eq!(valid.camera.model.as_deref(), Some("iPhone"));
    assert!(valid.dimensions.is_none());
}

// ---------------------------------------------------------------------------
// Jev integration + privacy (Phase 5, Part C)
// ---------------------------------------------------------------------------

fn response_for(choice: &str, probabilities: &str) -> String {
    format!(
        r#"{{"model":"jev-latest","answers":{{"destination":{{"type":"choice","choice":"{choice}","confidence":0.9,"probabilities":{probabilities}}}}},"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
    )
}

struct WireTap {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl WireTap {
    fn start(body: String) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let address = listener.local_addr().expect("address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut data = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                if let Some(end) = find_header_end(&data) {
                    let headers = String::from_utf8_lossy(&data[..end]).to_string();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if data.len() >= end + 4 + length {
                        break;
                    }
                }
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => data.extend_from_slice(&buffer[..read]),
                }
            }
            captured
                .lock()
                .expect("log")
                .push(String::from_utf8_lossy(&data).into_owned());
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        });
        Self {
            endpoint: format!("http://{address}"),
            requests,
            handle: Some(handle),
        }
    }

    fn request(&self) -> String {
        self.requests.lock().expect("log").join("\n")
    }
}

impl Drop for WireTap {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|window| window == b"\r\n\r\n")
}

fn image_context() -> DecisionContext {
    let normalized = NormalizedFileContext {
        target: kaseifu::models::file_context::TargetFile {
            filename: "IMG_1234.HEIC".to_string(),
            extension: Some("heic".to_string()),
            ..Default::default()
        },
        semantic_signals: vec!["F:IMG".to_string()],
        file_type: Some("public.heic".to_string()),
        image_metadata: Some(ImageMetadata {
            capture_time: Some(CaptureTime {
                value: "2025-08-10T15:30:00".to_string(),
                timezone: None,
                source: "exif".to_string(),
                timezone_known: false,
            }),
            location: Some(ImageLocation {
                latitude: 35.658,
                longitude: 139.702,
                altitude_m: None,
                source: "spotlight".to_string(),
            }),
            camera: CameraInfo {
                make: Some("Apple".to_string()),
                model: Some("iPhone".to_string()),
                source: Some("spotlight".to_string()),
            },
            dimensions: Some(ImageDimensions {
                width: 4032,
                height: 3024,
                source: Some("spotlight".to_string()),
            }),
        }),
        ..Default::default()
    };
    let candidates = vec![kaseifu::models::candidate::CandidateDestination {
        id: "D1".to_string(),
        path: PathBuf::from("/tmp/photos"),
        score: 1.0,
        evidence_count: 1,
        reasons: vec![],
    }];
    DecisionContext::from_normalized(&normalized, &candidates)
}

#[tokio::test]
async fn v32_jev_sends_capture_and_camera_but_redacts_gps_by_default() {
    let server = WireTap::start(response_for("D1", r#"{"D1":0.9,"NONE":0.1}"#));
    let adapter = RealJevIntegration::new(server.endpoint.clone(), None);
    let response = adapter.decide(&image_context()).await.expect("decision");
    assert_eq!(response.destination_id.as_deref(), Some("D1"));
    let wire = server.request();
    assert!(wire.contains("2025-08-10T15:30:00"), "capture sent: {wire}");
    assert!(wire.contains("Apple iPhone"), "camera sent: {wire}");
    assert!(!wire.contains("35.658"), "GPS redacted by default: {wire}");
    assert!(!wire.contains("139.702"), "GPS redacted by default: {wire}");
    assert!(
        !wire.contains("M:GPS available"),
        "GPS marker filtered: {wire}"
    );
}

#[tokio::test]
async fn v32_jev_sends_gps_only_when_explicitly_enabled() {
    let server = WireTap::start(response_for("D1", r#"{"D1":0.9,"NONE":0.1}"#));
    let adapter = RealJevIntegration::new(server.endpoint.clone(), None).with_privacy(true, true);
    let _ = adapter.decide(&image_context()).await.expect("decision");
    let wire = server.request();
    assert!(wire.contains("35.658"), "GPS sent when enabled: {wire}");
    assert!(wire.contains("139.702"), "GPS sent when enabled: {wire}");
    assert!(wire.contains("\"location\""), "location object: {wire}");
}

#[tokio::test]
async fn v32_jev_without_image_metadata_matches_legacy_shape() {
    let server = WireTap::start(response_for("NONE", r#"{"NONE":0.9}"#));
    let adapter = RealJevIntegration::new(server.endpoint.clone(), None);
    let context = DecisionContext::from_candidates(vec!["D1".to_string()]);
    let response = adapter.decide(&context).await.expect("decision");
    assert_eq!(response.destination_id.as_deref(), Some("NONE"));
    let wire = server.request();
    assert!(!wire.contains("capture_date"), "no image fields: {wire}");
    assert!(!wire.contains("location"), "no image fields: {wire}");
}

#[tokio::test]
async fn v32_jev_capture_withheld_leaves_no_trace() {
    let server = WireTap::start(response_for("D1", r#"{"D1":0.9}"#));
    let adapter = RealJevIntegration::new(server.endpoint.clone(), None).with_privacy(false, false);
    let _ = adapter.decide(&image_context()).await.expect("decision");
    let wire = server.request();
    assert!(!wire.contains("2025-08-10"), "capture withheld: {wire}");
    assert!(
        !wire.contains("M:Capture date available"),
        "marker filtered: {wire}"
    );
}

// ---------------------------------------------------------------------------
// Configuration (Part C)
// ---------------------------------------------------------------------------

fn resolve_defaults() -> kaseifu::config::ResolvedConfig {
    let lookup = |_: &str| Option::<LocatedValue>::None;
    resolve_with(
        &CliOverrides::default(),
        &lookup,
        &UserConfig::default(),
        &MemoryStore::new(),
        KeySourceSetting::explicit(KeySourceMode::Auto),
    )
    .expect("resolves")
}

#[test]
fn v32_config_metadata_defaults_are_privacy_first() {
    let resolved = resolve_defaults();
    assert!(resolved.metadata.image_enabled);
    assert!(resolved.metadata.spotlight);
    assert!(resolved.metadata.exif_fallback);
    assert!(resolved.metadata.send_capture_time_to_jev);
    assert!(
        !resolved.metadata.send_gps_to_jev,
        "GPS send defaults to off"
    );
    assert!(!resolved.metadata.weather_enabled);
}

#[test]
fn v32_config_parses_metadata_toml() {
    let config = parse_toml(
        "[metadata.image]\nenabled = true\nspotlight = true\nexif_fallback = false\n\
         [metadata.privacy]\nsend_capture_time_to_jev = true\nsend_gps_to_jev = true\n\
         [metadata.weather]\nenabled = false\n",
    )
    .expect("parses");
    assert_eq!(config.metadata.image.exif_fallback, Some(false));
    assert_eq!(config.metadata.privacy.send_gps_to_jev, Some(true));
    let lookup = |_: &str| Option::<LocatedValue>::None;
    let resolved = resolve_with(
        &CliOverrides::default(),
        &lookup,
        &config,
        &MemoryStore::new(),
        KeySourceSetting::explicit(KeySourceMode::Auto),
    )
    .expect("resolves");
    assert!(!resolved.metadata.exif_fallback);
    assert!(resolved.metadata.send_gps_to_jev);
}

#[test]
fn v32_config_set_accepts_metadata_keys_and_rejects_unknown() {
    let mut config = UserConfig::default();
    for key in [
        "metadata.image.enabled",
        "metadata.image.spotlight",
        "metadata.image.exif_fallback",
        "metadata.privacy.send_capture_time_to_jev",
        "metadata.privacy.send_gps_to_jev",
        "metadata.weather.enabled",
    ] {
        apply_setting(&mut config, key, "true").expect(key);
    }
    apply_setting(&mut config, "metadata.privacy.send_gps_to_jev", "false").unwrap();
    assert_eq!(config.metadata.privacy.send_gps_to_jev, Some(false));
    assert!(apply_setting(&mut config, "metadata.unknown", "true").is_err());
    assert!(apply_setting(&mut config, "metadata.image.unknown", "true").is_err());
}

#[test]
fn v32_config_rejects_gps_values_that_are_not_booleans() {
    let mut config = UserConfig::default();
    assert!(apply_setting(&mut config, "metadata.privacy.send_gps_to_jev", "maybe").is_err());
}

#[test]
fn v32_privacy_logs_never_carry_coordinates() {
    let meta = ImageMetadata {
        location: Some(ImageLocation {
            latitude: 35.658,
            longitude: 139.702,
            altitude_m: None,
            source: "spotlight".to_string(),
        }),
        ..Default::default()
    };
    let masked = masked_location(&meta);
    assert!(!masked.contains("35.658") && !masked.contains("139.702"));
    let debug = format!("{meta:?}");
    // Structured debug still names the source but tests never log real
    // user coordinates; fixtures use synthetic values only.
    assert!(debug.contains("spotlight"));
}

// ---------------------------------------------------------------------------
// Inspect display (Phase 6)
// ---------------------------------------------------------------------------

fn normalized_with_image() -> (FileContext, NormalizedFileContext) {
    let target = kaseifu::models::file_context::TargetFile {
        path: PathBuf::from("/tmp/IMG_1234.HEIC"),
        filename: "IMG_1234.HEIC".to_string(),
        extension: Some("heic".to_string()),
        size_bytes: 3_200_000,
        modified_at: Some("2025-08-10T15:30:00+00:00".to_string()),
    };
    let ctx = FileContext {
        schema_version: "file-context-v1".to_string(),
        target: target.clone(),
        directory_context: Default::default(),
        evidence: vec![],
    };
    let normalized = NormalizedFileContext {
        target,
        semantic_signals: vec!["M:GPS available".to_string()],
        file_type: Some("public.heic".to_string()),
        size_bytes: 3_200_000,
        parent_directory: "/tmp".to_string(),
        image_metadata: Some(ImageMetadata {
            capture_time: Some(CaptureTime {
                value: "2025-08-10T15:30:00".to_string(),
                timezone: None,
                source: "exif".to_string(),
                timezone_known: false,
            }),
            location: Some(ImageLocation {
                latitude: 35.658,
                longitude: 139.702,
                altitude_m: None,
                source: "spotlight".to_string(),
            }),
            camera: CameraInfo {
                make: Some("Apple".to_string()),
                model: Some("iPhone".to_string()),
                source: Some("spotlight".to_string()),
            },
            dimensions: Some(ImageDimensions {
                width: 4032,
                height: 3024,
                source: Some("spotlight".to_string()),
            }),
        }),
        ..Default::default()
    };
    (ctx, normalized)
}

#[test]
fn v32_inspect_renders_capture_and_location_in_ascii() {
    let (ctx, normalized) = normalized_with_image();
    let out = kaseifu::presentation::renderers::inspect::render_panel(
        &ctx,
        &normalized,
        false,
        false,
        false,
    );
    assert!(out.is_ascii(), "ASCII-only chrome");
    assert!(out.contains("CAPTURE METADATA"), "capture section");
    assert!(out.contains("LOCATION"), "location section");
    assert!(out.contains("2025-08-10 15:30"), "capture value");
    assert!(out.contains("Apple iPhone"), "camera value");
    assert!(out.contains("4032 x 3024"), "resolution value");
    assert!(out.contains("35.658000"), "latitude value");
    assert!(!out.contains("\u{00d7}"), "no non-ASCII multiply sign");
}

#[test]
fn v32_inspect_missing_metadata_renders_placeholders() {
    let target = kaseifu::models::file_context::TargetFile {
        filename: "note.txt".to_string(),
        ..Default::default()
    };
    let ctx = FileContext {
        schema_version: "file-context-v1".to_string(),
        target: target.clone(),
        directory_context: Default::default(),
        evidence: vec![],
    };
    let normalized = NormalizedFileContext {
        target,
        ..Default::default()
    };
    let out = kaseifu::presentation::renderers::inspect::render_panel(
        &ctx,
        &normalized,
        false,
        false,
        false,
    );
    assert!(out.contains("CAPTURE METADATA"));
    assert!(out.contains("LOCATION"));
    assert!(out.contains('-'), "missing placeholder");
}

#[test]
fn v32_inspect_panel_stays_aligned_with_image_sections() {
    use kaseifu::presentation::components::path::visible_width;
    let (ctx, normalized) = normalized_with_image();
    for color in [false, true] {
        let out = kaseifu::presentation::renderers::inspect::render_panel(
            &ctx,
            &normalized,
            false,
            true,
            color,
        );
        let widths: Vec<usize> = out.lines().map(visible_width).collect();
        assert!(widths.iter().all(|w| *w == widths[0]), "aligned");
    }
}

// ---------------------------------------------------------------------------
// Pipeline regression (Phase 7)
// ---------------------------------------------------------------------------

#[test]
fn v32_normalize_non_image_has_no_signals_or_metadata() {
    let target = kaseifu::models::file_context::TargetFile {
        path: PathBuf::from("/tmp/report.pdf"),
        filename: "report.pdf".to_string(),
        extension: Some("pdf".to_string()),
        ..Default::default()
    };
    let ctx = FileContext {
        schema_version: "file-context-v1".to_string(),
        target,
        directory_context: Default::default(),
        evidence: full_spotlight_evidence(),
    };
    let normalized = kaseifu::normalize::normalize(ctx).expect("normalizes");
    assert!(normalized.image_metadata.is_none());
    assert!(
        !normalized
            .semantic_signals
            .iter()
            .any(|s| s.contains("GPS")),
        "no image signals for PDFs"
    );
}

#[test]
fn v32_config_metadata_keys_are_documented() {
    for key in [
        "metadata.image.enabled",
        "metadata.image.spotlight",
        "metadata.image.exif_fallback",
        "metadata.privacy.send_capture_time_to_jev",
        "metadata.privacy.send_gps_to_jev",
        "metadata.weather.enabled",
    ] {
        assert!(
            kaseifu::config::loader::SUPPORTED_KEYS.contains(&key),
            "supported: {key}"
        );
    }
    let mut lookup_map = HashMap::new();
    lookup_map.insert("UNUSED".to_string(), "x".to_string());
    let _ = lookup_map;
}

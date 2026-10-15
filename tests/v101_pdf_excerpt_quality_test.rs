//! v1.0.1 PDF child-excerpt quality tests (M2 `pdf-child-excerpt-quality`).
//!
//! Deterministic and model-independent: synthetic PDFs are written to temp
//! files and read back through the same bounded reader the folder path uses,
//! so the assertions pin what a PDF child's excerpts contribute. The real-model
//! folder titles are recorded in the issue's verification note (the model is
//! non-deterministic, so titles stay out of the committed suite).
//!
//! Only temp directories are touched; the developer's files are never read.

use kaseifu::rename::document::{DocumentKind, MAX_DOCUMENT_BYTES, extract_signals, read_bounded};
use std::path::{Path, PathBuf};

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-v101-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// One minimal single-page PDF with a metadata title and one text-showing
/// content stream, plus the xref section real generators emit.
fn synthetic_pdf(title: &str, body: &str) -> Vec<u8> {
    let stream = format!("BT /F1 12 Tf 72 720 Td ({body}) Tj ET");
    let objects = [
        format!("<< /Type /Catalog /Pages 2 0 R /Title ({title}) >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 4 0 R >> >> \
         /MediaBox [0 0 612 792] /Contents 5 0 R >>"
            .to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (number, content) in objects.iter().enumerate() {
        offsets.push((number + 1, out.len()));
        out.extend_from_slice(format!("{} 0 obj\n{content}\nendobj\n", number + 1).as_bytes());
    }
    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", offsets.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for (_, offset) in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<A1B2C3D4> <00112233>] >>\n\
             startxref\n{xref_at}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn child_signals(dir: &Path, name: &str, bytes: &[u8]) -> String {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("fixture pdf");
    let bounded = read_bounded(&path, MAX_DOCUMENT_BYTES).expect("readable");
    extract_signals(DocumentKind::Pdf, &bounded).join(" ")
}

#[test]
fn v101_pdf_children_contribute_words_not_format_vocabulary() {
    let dir = work_dir("words");
    let joined = child_signals(
        &dir,
        "report.pdf",
        &synthetic_pdf("Quarterly Report Q3", "revenue grew fifteen percent"),
    );
    assert!(joined.contains("Quarterly Report Q3"), "got: {joined}");
    assert!(
        joined.contains("revenue grew fifteen percent"),
        "got: {joined}"
    );
    for noise in [
        "/Type",
        "Catalog",
        "endobj",
        "xref",
        "startxref",
        "%%EOF",
        "FlateDecode",
    ] {
        assert!(!joined.contains(noise), "noise leaked: {noise} in {joined}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v101_three_pdf_children_keep_their_own_words() {
    let dir = work_dir("three");
    let first = child_signals(
        &dir,
        "a.pdf",
        &synthetic_pdf("Lease Agreement", "monthly rent due first"),
    );
    let second = child_signals(
        &dir,
        "b.pdf",
        &synthetic_pdf("Field Trip", "science museum friday"),
    );
    assert!(first.contains("Lease Agreement"), "got: {first}");
    assert!(second.contains("science museum friday"), "got: {second}");
    assert!(
        !first.contains("Field Trip"),
        "children do not bleed: {first}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

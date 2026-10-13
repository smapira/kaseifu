//! v3.2 EXIF fallback (dependency-free, best-effort).
//!
//! Spotlight is the first source; this module only runs when Spotlight
//! lacks a value. Supported: JPEG, TIFF. HEIC/HEIF: best-effort embedded
//! `Exif` block scan (dimensions often unavailable). PNG/WebP and others
//! are reported as unsupported, never guessed.
//!
//! Evaluation note: `kamadak-exif` covers JPEG/TIFF only, adds a new
//! dependency plus license surface, and still needs custom HEIC handling,
//! so the minimal internal parser below is used instead (existing policy:
//! minimal deps, measured format coverage).

use std::path::Path;

/// Raw EXIF fields (unvalidated; normalization validates later).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawExif {
    pub capture_original: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude_m: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Outcome of one EXIF parse attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum ExifOutcome {
    /// Usable fields were found (any subset; missing stays missing).
    Found(RawExif),
    /// Format parsed but no EXIF block exists.
    NoExif,
    /// Format is known-unsupported for EXIF (e.g. PNG, WebP).
    Unsupported(String),
    /// Bytes claim the format but do not parse.
    Corrupt(String),
}

/// Parse one file's embedded EXIF (best-effort, never panics).
///
/// Reads from disk; callers gate this behind the Spotlight-missing check.
pub fn parse_file(path: &Path) -> ExifOutcome {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return ExifOutcome::Corrupt(format!("unreadable: {error}")),
    };
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    parse_bytes(&bytes, ext)
}

/// Parse in-memory bytes with an extension hint.
///
/// Public for tests so fixtures avoid temp files where convenient.
pub fn parse_bytes(bytes: &[u8], extension: &str) -> ExifOutcome {
    if is_jpeg(bytes, extension) {
        parse_jpeg_bytes(bytes)
    } else if is_tiff(bytes) {
        parse_tiff_bytes(bytes)
    } else if is_heic(bytes, extension) {
        parse_heic_bytes(bytes)
    } else if is_png(bytes, extension) {
        ExifOutcome::Unsupported("png has no EXIF; IHDR dimensions only".to_string())
    } else if is_webp(bytes, extension) {
        ExifOutcome::Unsupported("webp EXIF is out of scope".to_string())
    } else {
        ExifOutcome::NoExif
    }
}

/// PNG pixel dimensions from IHDR (no EXIF involved).
///
/// Returns `None` for non-PNG bytes.
pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || bytes[0..8] != *b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    if bytes[12..16] != *b"IHDR" {
        return None;
    }
    Some((read_u32_be(bytes, 16), read_u32_be(bytes, 20)))
}

fn is_jpeg(bytes: &[u8], extension: &str) -> bool {
    let ext = extension.to_ascii_lowercase();
    (ext == "jpg" || ext == "jpeg") || (bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8)
}

fn is_tiff(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && (bytes[0..4] == *b"II*\0" || bytes[0..4] == *b"MM\0*")
}

fn is_heic(bytes: &[u8], extension: &str) -> bool {
    let ext = extension.to_ascii_lowercase();
    if ext == "heic" || ext == "heif" || ext == "hif" {
        return true;
    }
    has_ftyp_brand(bytes, b"heic")
        || has_ftyp_brand(bytes, b"heix")
        || has_ftyp_brand(bytes, b"mif1")
}

fn is_png(bytes: &[u8], extension: &str) -> bool {
    extension.eq_ignore_ascii_case("png")
        || (bytes.len() >= 8 && bytes[0..8] == *b"\x89PNG\r\n\x1a\n")
}

fn is_webp(bytes: &[u8], extension: &str) -> bool {
    extension.eq_ignore_ascii_case("webp")
        || (bytes.len() >= 12 && bytes[0..4] == *b"RIFF" && bytes[8..12] == *b"WEBP")
}

fn has_ftyp_brand(bytes: &[u8], brand: &[u8; 4]) -> bool {
    if bytes.len() < 12 || bytes[4..8] != *b"ftyp" {
        return false;
    }
    bytes[8..12] == *brand
}

fn read_u32_be(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

// ---------------------------------------------------------------------------
// JPEG
// ---------------------------------------------------------------------------

fn parse_jpeg_bytes(bytes: &[u8]) -> ExifOutcome {
    if bytes.len() < 4 {
        return ExifOutcome::Corrupt("truncated jpeg".to_string());
    }
    let mut raw = RawExif::default();
    let mut found_exif = false;
    let mut offset = 2;
    while offset + 4 <= bytes.len() {
        match step_jpeg(bytes, offset, &mut raw, &mut found_exif) {
            JpegStep::Next(next) => offset = next,
            JpegStep::Done => break,
        }
    }
    if found_exif || raw.width.is_some() {
        ExifOutcome::Found(raw)
    } else {
        ExifOutcome::NoExif
    }
}

/// One JPEG marker step: advance, resync, or stop.
enum JpegStep {
    Next(usize),
    Done,
}

fn step_jpeg(bytes: &[u8], offset: usize, raw: &mut RawExif, found_exif: &mut bool) -> JpegStep {
    if bytes[offset] != 0xFF {
        return JpegStep::Next(offset + 1);
    }
    let marker = bytes[offset + 1];
    if marker == 0xD9 {
        return JpegStep::Done;
    }
    if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
        return JpegStep::Next(offset + 2);
    }
    let length = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
    if length < 2 || offset + 2 + length > bytes.len() {
        return JpegStep::Done;
    }
    apply_jpeg_segment(
        bytes,
        marker,
        offset + 4,
        offset + 2 + length,
        raw,
        found_exif,
    );
    JpegStep::Next(offset + 2 + length)
}

fn apply_jpeg_segment(
    bytes: &[u8],
    marker: u8,
    payload_start: usize,
    payload_end: usize,
    raw: &mut RawExif,
    found_exif: &mut bool,
) {
    if marker == 0xE1
        && is_exif_header(&bytes[payload_start..payload_end])
        && let Some(parsed) = parse_exif_payload(&bytes[payload_start + 6..payload_end])
    {
        merge_raw(raw, &parsed);
        *found_exif = true;
    }
    if is_sof_marker(marker)
        && let Some((width, height)) = parse_sof_dimensions(&bytes[payload_start..payload_end])
    {
        raw.width = Some(width);
        raw.height = Some(height);
    }
}

fn is_exif_header(payload: &[u8]) -> bool {
    payload.len() >= 6 && payload[0..6] == *b"Exif\0\0"
}

fn is_sof_marker(marker: u8) -> bool {
    matches!(
        marker,
        0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB
    )
}

fn parse_sof_dimensions(payload: &[u8]) -> Option<(u32, u32)> {
    if payload.len() < 6 {
        return None;
    }
    let height = u16::from_be_bytes([payload[1], payload[2]]) as u32;
    let width = u16::from_be_bytes([payload[3], payload[4]]) as u32;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

fn merge_raw(target: &mut RawExif, source: &RawExif) {
    if target.capture_original.is_none() {
        target.capture_original.clone_from(&source.capture_original);
    }
    if target.make.is_none() {
        target.make.clone_from(&source.make);
    }
    if target.model.is_none() {
        target.model.clone_from(&source.model);
    }
    if target.latitude.is_none() {
        target.latitude = source.latitude;
    }
    if target.longitude.is_none() {
        target.longitude = source.longitude;
    }
    if target.altitude_m.is_none() {
        target.altitude_m = source.altitude_m;
    }
    if target.width.is_none() {
        target.width = source.width;
    }
    if target.height.is_none() {
        target.height = source.height;
    }
}

// ---------------------------------------------------------------------------
// HEIC (best-effort)
// ---------------------------------------------------------------------------

fn parse_heic_bytes(bytes: &[u8]) -> ExifOutcome {
    match find_subslice(bytes, b"Exif\0\0") {
        Some(index) => {
            let start = index + 6;
            match parse_exif_payload(&bytes[start..]) {
                Some(raw) => ExifOutcome::Found(raw),
                None => ExifOutcome::Corrupt("heic Exif block does not parse".to_string()),
            }
        }
        None => ExifOutcome::Unsupported("heic without embedded Exif".to_string()),
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ---------------------------------------------------------------------------
// TIFF
// ---------------------------------------------------------------------------

fn parse_tiff_bytes(bytes: &[u8]) -> ExifOutcome {
    match parse_exif_payload(bytes) {
        Some(raw) => ExifOutcome::Found(raw),
        None => ExifOutcome::NoExif,
    }
}

fn parse_exif_payload(payload: &[u8]) -> Option<RawExif> {
    let tiff = TiffReader::new(payload)?;
    let ifd0_offset = tiff.read_u32(4)? as usize;
    let ifd0 = tiff.entries(ifd0_offset)?;
    let mut raw = RawExif {
        make: tiff.ascii_value(&ifd0, 0x010F),
        model: tiff.ascii_value(&ifd0, 0x0110),
        width: tiff
            .u32_value(&ifd0, 0x0100)
            .or_else(|| tiff.u32_value(&ifd0, 0xA002)),
        height: tiff
            .u32_value(&ifd0, 0x0101)
            .or_else(|| tiff.u32_value(&ifd0, 0xA003)),
        ..Default::default()
    };
    if let Some(exif_offset) = tiff.u32_value(&ifd0, 0x8769) {
        apply_exif_ifd(&tiff, exif_offset, &mut raw);
    }
    if let Some(gps_offset) = tiff.u32_value(&ifd0, 0x8825) {
        apply_gps_ifd(&tiff, gps_offset, &mut raw);
    }
    Some(raw)
}

fn apply_exif_ifd(tiff: &TiffReader<'_>, offset: u32, raw: &mut RawExif) {
    let Some(entries) = tiff.entries(offset as usize) else {
        return;
    };
    if raw.capture_original.is_none() {
        raw.capture_original = tiff.ascii_value(&entries, 0x9003);
    }
    if raw.width.is_none() {
        raw.width = tiff.u32_value(&entries, 0xA002);
    }
    if raw.height.is_none() {
        raw.height = tiff.u32_value(&entries, 0xA003);
    }
}

fn apply_gps_ifd(tiff: &TiffReader<'_>, offset: u32, raw: &mut RawExif) {
    let Some(entries) = tiff.entries(offset as usize) else {
        return;
    };
    let lat_ref = tiff.ascii_value(&entries, 0x0001).unwrap_or_default();
    let lon_ref = tiff.ascii_value(&entries, 0x0003).unwrap_or_default();
    if let Some(triplet) = tiff.rational_triplet(&entries, 0x0002) {
        raw.latitude = dms_to_decimal(triplet, lat_ref.starts_with('S'));
    }
    if let Some(triplet) = tiff.rational_triplet(&entries, 0x0004) {
        raw.longitude = dms_to_decimal(triplet, lon_ref.starts_with('W'));
    }
    raw.altitude_m = tiff.single_rational(&entries, 0x0006);
}

fn dms_to_decimal(triplet: (f64, f64, f64), negative: bool) -> Option<f64> {
    let (degrees, minutes, seconds) = triplet;
    if !degrees.is_finite() || !minutes.is_finite() || !seconds.is_finite() {
        return None;
    }
    let value = degrees.abs() + minutes.abs() / 60.0 + seconds.abs() / 3600.0;
    if negative { Some(-value) } else { Some(value) }
}

#[derive(Debug, Clone, Copy)]
struct IfdEntry {
    tag: u16,
    field_type: u16,
    count: u32,
    value_offset: usize,
}

struct TiffReader<'a> {
    bytes: &'a [u8],
    little_endian: bool,
}

impl<'a> TiffReader<'a> {
    fn new(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        let little_endian = match &bytes[0..4] {
            b"II*\0" => true,
            b"MM\0*" => false,
            _ => return None,
        };
        Some(Self {
            bytes,
            little_endian,
        })
    }

    fn read_u16(&self, offset: usize) -> Option<u16> {
        let slice = self.bytes.get(offset..offset + 2)?;
        let pair = [slice[0], slice[1]];
        Some(if self.little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        })
    }

    fn read_u32(&self, offset: usize) -> Option<u32> {
        let slice = self.bytes.get(offset..offset + 4)?;
        let quad = [slice[0], slice[1], slice[2], slice[3]];
        Some(if self.little_endian {
            u32::from_le_bytes(quad)
        } else {
            u32::from_be_bytes(quad)
        })
    }

    fn entries(&self, ifd_offset: usize) -> Option<Vec<IfdEntry>> {
        let count = self.read_u16(ifd_offset)? as usize;
        let mut entries = Vec::new();
        for index in 0..count {
            let base = ifd_offset.checked_add(2 + index.checked_mul(12)?)?;
            let tag = self.read_u16(base)?;
            let field_type = self.read_u16(base + 2)?;
            let count = self.read_u32(base + 4)?;
            entries.push(IfdEntry {
                tag,
                field_type,
                count,
                value_offset: base + 8,
            });
        }
        Some(entries)
    }

    fn entry(&self, entries: &[IfdEntry], tag: u16) -> Option<IfdEntry> {
        entries.iter().find(|entry| entry.tag == tag).copied()
    }

    fn data_offset(&self, entry: &IfdEntry) -> Option<usize> {
        let unit = type_size(entry.field_type)?;
        let total = (entry.count as usize).checked_mul(unit)?;
        if total <= 4 {
            return Some(entry.value_offset);
        }
        self.read_u32(entry.value_offset).map(|v| v as usize)
    }

    fn ascii_value(&self, entries: &[IfdEntry], tag: u16) -> Option<String> {
        let entry = self.entry(entries, tag)?;
        if entry.field_type != 2 || entry.count == 0 {
            return None;
        }
        let offset = self.data_offset(&entry)?;
        let bytes = self.bytes.get(offset..offset + entry.count as usize)?;
        ascii_to_string(bytes)
    }

    fn u32_value(&self, entries: &[IfdEntry], tag: u16) -> Option<u32> {
        let entry = self.entry(entries, tag)?;
        match entry.field_type {
            3 => self.short_value(&entry),
            4 => self.long_value(&entry),
            _ => None,
        }
    }

    fn short_value(&self, entry: &IfdEntry) -> Option<u32> {
        if entry.count == 0 {
            return None;
        }
        let offset = self.data_offset(entry)?;
        self.read_u16(offset).map(u32::from)
    }

    fn long_value(&self, entry: &IfdEntry) -> Option<u32> {
        if entry.count == 0 {
            return None;
        }
        let offset = self.data_offset(entry)?;
        self.read_u32(offset)
    }

    fn single_rational(&self, entries: &[IfdEntry], tag: u16) -> Option<f64> {
        let entry = self.entry(entries, tag)?;
        if entry.field_type != 5 || entry.count == 0 {
            return None;
        }
        let offset = self.data_offset(&entry)?;
        self.read_rational(offset)
    }

    fn rational_triplet(&self, entries: &[IfdEntry], tag: u16) -> Option<(f64, f64, f64)> {
        let entry = self.entry(entries, tag)?;
        if entry.field_type != 5 || entry.count != 3 {
            return None;
        }
        let offset = self.data_offset(&entry)?;
        Some((
            self.read_rational(offset)?,
            self.read_rational(offset + 8)?,
            self.read_rational(offset + 16)?,
        ))
    }

    fn read_rational(&self, offset: usize) -> Option<f64> {
        let numerator = self.read_u32(offset)? as f64;
        let denominator = self.read_u32(offset + 4)? as f64;
        if denominator == 0.0 || !numerator.is_finite() || !denominator.is_finite() {
            return None;
        }
        Some(numerator / denominator)
    }
}

fn type_size(field_type: u16) -> Option<usize> {
    match field_type {
        1 | 2 | 7 => Some(1),
        3 => Some(2),
        4 | 9 => Some(4),
        5 | 10 => Some(8),
        _ => None,
    }
}

fn ascii_to_string(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let text = std::str::from_utf8(&bytes[..end]).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

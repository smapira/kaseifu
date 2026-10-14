//! Corpus scattering (request sections 15-16).
//!
//! The raw corpus stays untouched: scattered documents are **copies** in the
//! workspace, named from a seeded hash so neither the file name, the directory
//! nor the timestamp can leak the ground truth. Only files this module created
//! (listed in the scattering manifest) are ever removed again.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{BENCHMARK_ID, FIXED_MTIME_UNIX, SplitMix64, Workspace, fnv1a64, sha256_bytes};

/// File name strategy id written into reports.
pub const FILENAME_STRATEGY: &str = "deterministic-opaque";
/// Timestamp strategy id written into reports.
pub const TIMESTAMP_STRATEGY: &str = "fixed";
/// Directory layout patterns (request section 15.5).
pub const STRATEGY_FLAT: &str = "flat";
pub const STRATEGY_UNIFORM: &str = "uniform";

/// Bytes of a document inspected when looking for its TREC doc number.
const DOCNO_SCAN_BYTES: usize = 8 * 1024;

/// One scattered document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScatterFile {
    pub doc_id: String,
    /// Path relative to the scattered root, always with `/` separators.
    pub relative_path: String,
    pub sha256: String,
    pub bytes: u64,
    pub mtime_unix: i64,
}

/// doc id -> real path mapping, plus everything needed to reproduce it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScatterManifest {
    pub benchmark: String,
    pub algorithm: String,
    pub strategy: String,
    pub seed: u64,
    pub directory_count: usize,
    pub filename_strategy: String,
    pub timestamp_strategy: String,
    pub fixed_mtime_unix: i64,
    /// Unix creation time cannot be pinned from user space; recorded, not faked.
    pub birthtime_settable: bool,
    pub birthtime_note: String,
    pub birthtime_observed_unix: Option<i64>,
    pub root: String,
    pub selector: String,
    pub files: Vec<ScatterFile>,
    /// Documents of the qrels that the source tree did not contain.
    pub missing_doc_ids: Vec<String>,
    pub total_bytes: u64,
    pub tree_sha256: String,
    pub manifest_sha256: String,
}

impl ScatterManifest {
    pub fn path_of(&self, doc_id: &str) -> Option<PathBuf> {
        let file = self.files.iter().find(|f| f.doc_id == doc_id)?;
        Some(Path::new(&self.root).join(&file.relative_path))
    }

    pub fn doc_id_of_path(&self, path: &Path) -> Option<&str> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        self.files
            .iter()
            .find(|f| f.relative_path == relative)
            .map(|f| f.doc_id.as_str())
    }

    pub fn paths(&self) -> Vec<(String, PathBuf)> {
        self.files
            .iter()
            .map(|f| {
                (
                    f.doc_id.clone(),
                    Path::new(&self.root).join(&f.relative_path),
                )
            })
            .collect()
    }
}

/// A document found in the raw corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDoc {
    pub doc_id: String,
    pub source: PathBuf,
}

/// Store every deterministic check inside the manifest.
pub fn finalize_scatter_manifest(manifest: &mut ScatterManifest) -> Result<()> {
    manifest.manifest_sha256 = String::new();
    manifest.manifest_sha256 = super::sha256_json(manifest)?;
    Ok(())
}

/// `doc_id` from the document's own `<DOCNO>` tag when present, otherwise
/// from the file stem. TREC text carries the official number in the body.
pub fn doc_id_from_head(head: &str, file_stem: &str) -> String {
    let lowered = head.to_lowercase();
    if let Some(start) = lowered.find("<docno>") {
        let after = &head[start + "<docno>".len()..];
        if let Some(end) = after.to_lowercase().find("</docno>") {
            let value = after[..end].trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    file_stem.to_string()
}

/// Walk the raw corpus and collect one document per file, sorted by doc id.
pub fn discover_documents(source: &Path) -> Result<Vec<DiscoveredDoc>> {
    let mut docs: Vec<DiscoveredDoc> = Vec::new();
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
    for entry in walkdir::WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if is_noise_file(path) {
            continue;
        }
        let head = read_head(path)?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let doc_id = doc_id_from_head(&head, &stem);
        if let Some(previous) = seen.get(&doc_id) {
            anyhow::bail!(
                "duplicate doc id {doc_id}: {} and {}",
                previous.display(),
                path.display()
            );
        }
        seen.insert(doc_id.clone(), path.to_path_buf());
        docs.push(DiscoveredDoc {
            doc_id,
            source: path.to_path_buf(),
        });
    }
    docs.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));
    Ok(docs)
}

fn is_noise_file(path: &Path) -> bool {
    path.file_name()
        .map(|name| {
            let name = name.to_string_lossy();
            name.starts_with('.') || name.ends_with('~')
        })
        .unwrap_or(true)
}

fn read_head(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let head = &bytes[..bytes.len().min(DOCNO_SCAN_BYTES)];
    Ok(String::from_utf8_lossy(head).to_string())
}

/// Opaque, deterministic file name: `<hex>.txt`, derived from doc id and seed.
///
/// A short name can collide by chance; collisions are resolved deterministically
/// (longer name first, then a counter), so the same input always yields the same
/// names.
pub fn opaque_names(docs: &[DiscoveredDoc], seed: u64) -> Vec<String> {
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut names = Vec::with_capacity(docs.len());
    for doc in docs {
        let mut candidate = hex_name(seed, &doc.doc_id, 8);
        if used.contains(&candidate) {
            candidate = hex_name(seed, &doc.doc_id, 16);
        }
        let mut attempt = 2;
        while used.contains(&candidate) {
            candidate = format!("{}-{attempt}", hex_name(seed, &doc.doc_id, 16));
            attempt += 1;
        }
        used.insert(candidate.clone());
        names.push(format!("{candidate}.txt"));
    }
    names
}

fn hex_name(seed: u64, doc_id: &str, width: usize) -> String {
    let mut rng = SplitMix64::new(seed ^ fnv1a64(doc_id));
    let value = rng.next_u64();
    format!("{:016x}", value)[..width].to_string()
}

/// Directory of one document for the chosen strategy.
pub fn scatter_directory(
    strategy: &str,
    seed: u64,
    doc_id: &str,
    directory_count: usize,
) -> Result<String> {
    match strategy {
        STRATEGY_FLAT => Ok(String::new()),
        STRATEGY_UNIFORM => {
            if directory_count == 0 {
                anyhow::bail!("uniform scattering needs at least one directory");
            }
            let mut rng = SplitMix64::new(seed ^ fnv1a64(doc_id));
            let index = rng.next_below(directory_count);
            Ok(format!("dir_{:04}", index + 1))
        }
        other => anyhow::bail!("unknown scatter strategy: {other}"),
    }
}

/// Copy the corpus into the workspace as scattered, renamed, timestamp-pinned
/// documents, and write the mapping manifest.
pub fn scatter(
    workspace: &Workspace,
    source: &Path,
    strategy: &str,
    seed: u64,
    directory_count: usize,
    known_doc_ids: &[String],
) -> Result<ScatterManifest> {
    let docs = discover_documents(source)?;
    if docs.is_empty() {
        anyhow::bail!("no documents found under {}", source.display());
    }
    let root = workspace.scattered_dir();
    std::fs::create_dir_all(&root).with_context(|| format!("cannot create {}", root.display()))?;
    let names = opaque_names(&docs, seed);

    let mut files: Vec<ScatterFile> = Vec::with_capacity(docs.len());
    let mut total_bytes: u64 = 0;
    let mut birthtime_observed = None;
    for (doc, name) in docs.iter().zip(names.iter()) {
        let directory = scatter_directory(strategy, seed, &doc.doc_id, directory_count)?;
        let target = root.join(&directory).join(name);
        super::ensure_inside(&root, &target)?;
        let bytes = std::fs::read(&doc.source)
            .with_context(|| format!("cannot read {}", doc.source.display()))?;
        let digest = sha256_bytes(&bytes);
        write_new_file(&target, &bytes)?;
        set_fixed_times(&target, FIXED_MTIME_UNIX)?;
        if birthtime_observed.is_none() {
            birthtime_observed = birthtime_unix(&target);
        }
        total_bytes += bytes.len() as u64;
        let relative_path = if directory.is_empty() {
            name.clone()
        } else {
            format!("{directory}/{name}")
        };
        files.push(ScatterFile {
            doc_id: doc.doc_id.clone(),
            relative_path,
            sha256: digest,
            bytes: bytes.len() as u64,
            mtime_unix: FIXED_MTIME_UNIX,
        });
    }
    files.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));

    let placed: BTreeSet<&String> = files.iter().map(|f| &f.doc_id).collect();
    let missing_doc_ids: Vec<String> = known_doc_ids
        .iter()
        .filter(|doc_id| !placed.contains(doc_id))
        .cloned()
        .collect();

    let mut manifest = ScatterManifest {
        benchmark: BENCHMARK_ID.to_string(),
        algorithm: "seeded-uniform-random-distribution".to_string(),
        strategy: strategy.to_string(),
        seed,
        directory_count: if strategy == STRATEGY_FLAT {
            1
        } else {
            directory_count
        },
        filename_strategy: FILENAME_STRATEGY.to_string(),
        timestamp_strategy: TIMESTAMP_STRATEGY.to_string(),
        fixed_mtime_unix: FIXED_MTIME_UNIX,
        birthtime_settable: false,
        birthtime_note: "creation time (birthtime) is assigned by the filesystem at copy time and cannot be pinned from user space; modification time is pinned".to_string(),
        birthtime_observed_unix: birthtime_observed,
        root: root.to_string_lossy().to_string(),
        selector: "doc id from the document's <DOCNO> tag, else the file stem".to_string(),
        tree_sha256: tree_checksum(&files),
        files,
        missing_doc_ids,
        total_bytes,
        manifest_sha256: String::new(),
    };
    finalize_scatter_manifest(&mut manifest)?;
    Ok(manifest)
}

/// sha256 over the sorted `doc_id, relative path, digest, size` lines.
pub fn tree_checksum(files: &[ScatterFile]) -> String {
    let mut lines: Vec<String> = files
        .iter()
        .map(|f| {
            format!(
                "{}\t{}\t{}\t{}",
                f.doc_id, f.relative_path, f.sha256, f.bytes
            )
        })
        .collect();
    lines.sort();
    sha256_bytes(lines.join("\n").as_bytes())
}

fn write_new_file(target: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let mut handle = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .with_context(|| {
            format!(
                "refusing to overwrite an existing file: {} (run cleanup first)",
                target.display()
            )
        })?;
    handle
        .write_all(bytes)
        .with_context(|| format!("cannot write {}", target.display()))?;
    Ok(())
}

/// Pin access and modification time. Creation time is out of reach here.
#[cfg(unix)]
pub fn set_fixed_times(path: &Path, unix_seconds: i64) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path_c = CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("path is not usable as a C string: {}", path.display()))?;
    let stamp = libc::timespec {
        tv_sec: unix_seconds as libc::time_t,
        tv_nsec: 0,
    };
    let times = [stamp, stamp];
    let result = unsafe { libc::utimensat(libc::AT_FDCWD, path_c.as_ptr(), times.as_ptr(), 0) };
    if result != 0 {
        anyhow::bail!("utimensat failed for {}", path.display());
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn set_fixed_times(path: &Path, _unix_seconds: i64) -> Result<()> {
    anyhow::bail!(
        "setting timestamps is not implemented on this platform: {}",
        path.display()
    )
}

/// Observed creation time, when the platform exposes it.
///
/// Read through `std::fs::Metadata::created`, which reports the birthtime on
/// macOS. It is recorded, never faked: no stable API lets a user process set
/// it, so the manifest says so instead of pretending otherwise.
pub fn birthtime_unix(path: &Path) -> Option<i64> {
    let created = std::fs::metadata(path).ok()?.created().ok()?;
    created
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

/// Result of `verify-scatter`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScatterVerification {
    pub checked: usize,
    pub missing: Vec<String>,
    pub size_mismatch: Vec<String>,
    pub digest_mismatch: Vec<String>,
    pub mtime_mismatch: Vec<String>,
    pub extra_files: Vec<String>,
}

impl ScatterVerification {
    pub fn ok(&self) -> bool {
        self.missing.is_empty()
            && self.size_mismatch.is_empty()
            && self.digest_mismatch.is_empty()
            && self.mtime_mismatch.is_empty()
            && self.extra_files.is_empty()
    }
}

/// Re-check every scattered file against the manifest, and report files the
/// manifest does not know (which the harness must never have created).
pub fn verify_scatter(manifest: &ScatterManifest) -> Result<ScatterVerification> {
    let root = Path::new(&manifest.root);
    let mut report = ScatterVerification::default();
    for file in &manifest.files {
        report.checked += 1;
        let path = root.join(&file.relative_path);
        let Ok(metadata) = std::fs::metadata(&path) else {
            report.missing.push(file.relative_path.clone());
            continue;
        };
        if metadata.len() != file.bytes {
            report.size_mismatch.push(file.relative_path.clone());
            continue;
        }
        let bytes =
            std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))?;
        if sha256_bytes(&bytes) != file.sha256 {
            report.digest_mismatch.push(file.relative_path.clone());
        }
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        if modified != Some(file.mtime_unix) {
            report.mtime_mismatch.push(file.relative_path.clone());
        }
    }
    let known: BTreeSet<String> = manifest
        .files
        .iter()
        .map(|f| f.relative_path.clone())
        .collect();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if !known.contains(&relative) && !is_noise_file(entry.path()) {
            report.extra_files.push(relative);
        }
    }
    report.extra_files.sort();
    Ok(report)
}

/// Removed-file report of `cleanup`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupReport {
    pub targets: usize,
    pub removed_files: usize,
    pub removed_bytes: u64,
    pub kept_outside_manifest: usize,
}

/// Delete only the files listed in the scattering manifest.
pub fn cleanup(manifest: &ScatterManifest, execute: bool) -> Result<CleanupReport> {
    let root = Path::new(&manifest.root);
    let mut report = CleanupReport {
        targets: manifest.files.len(),
        ..CleanupReport::default()
    };
    for file in &manifest.files {
        let path = root.join(&file.relative_path);
        super::ensure_inside(root, &path)?;
        if !execute {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                report.removed_files += 1;
                report.removed_bytes += file.bytes;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("cannot remove {}", path.display()));
            }
        }
    }
    if execute {
        remove_empty_dirs(root);
    }
    Ok(report)
}

fn remove_empty_dirs(root: &Path) {
    let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_dir() && entry.path() != root)
        .map(|entry| entry.path().to_path_buf())
        .collect();
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    for dir in dirs {
        let _ = std::fs::remove_dir(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> Vec<DiscoveredDoc> {
        (0..5)
            .map(|i| DiscoveredDoc {
                doc_id: format!("DOC-{i}"),
                source: PathBuf::from(format!("/corpus/DOC-{i}")),
            })
            .collect()
    }

    #[test]
    fn doc_id_prefers_the_docno_tag() {
        assert_eq!(
            doc_id_from_head("<DOC>\n<DOCNO>FBIS3-10082</DOCNO>\n", "file1"),
            "FBIS3-10082"
        );
        assert_eq!(
            doc_id_from_head("<DOC>\n<DOCNO> fb-1 </DOCNO>\n", "file1"),
            "fb-1"
        );
        assert_eq!(doc_id_from_head("no tag here", "7f82a91c"), "7f82a91c");
    }

    #[test]
    fn opaque_names_are_deterministic_unique_and_content_free() {
        let first = opaque_names(&docs(), 42);
        let second = opaque_names(&docs(), 42);
        let other = opaque_names(&docs(), 43);
        assert_eq!(first, second);
        assert_ne!(first, other);
        let unique: BTreeSet<&String> = first.iter().collect();
        assert_eq!(unique.len(), first.len());
        for name in &first {
            assert!(name.ends_with(".txt"));
            assert_eq!(name.len(), 12, "8 hex chars plus extension: {name}");
            assert!(!name.contains("DOC"), "the doc id must not leak: {name}");
        }
    }

    #[test]
    fn strategies_place_documents_deterministically() {
        assert_eq!(
            scatter_directory(STRATEGY_FLAT, 42, "DOC-1", 100).unwrap(),
            ""
        );
        let dir = scatter_directory(STRATEGY_UNIFORM, 42, "DOC-1", 100).unwrap();
        assert!(dir.starts_with("dir_"), "{dir}");
        assert_eq!(
            dir,
            scatter_directory(STRATEGY_UNIFORM, 42, "DOC-1", 100).unwrap()
        );
        assert!(scatter_directory("nope", 42, "DOC-1", 100).is_err());
        assert!(scatter_directory(STRATEGY_UNIFORM, 42, "DOC-1", 0).is_err());
    }

    #[test]
    fn uniform_placement_spreads_over_the_directory_budget() {
        let docs: Vec<DiscoveredDoc> = (0..500)
            .map(|i| DiscoveredDoc {
                doc_id: format!("DOC-{i:04}"),
                source: PathBuf::new(),
            })
            .collect();
        let used: BTreeSet<String> = docs
            .iter()
            .map(|doc| scatter_directory(STRATEGY_UNIFORM, 42, &doc.doc_id, 10).unwrap())
            .collect();
        assert_eq!(used.len(), 10, "every directory should be used: {used:?}");
    }

    #[test]
    fn tree_checksum_covers_paths_and_digests() {
        let files = vec![ScatterFile {
            doc_id: "A".to_string(),
            relative_path: "dir_0001/aaaa.txt".to_string(),
            sha256: "deadbeef".to_string(),
            bytes: 3,
            mtime_unix: FIXED_MTIME_UNIX,
        }];
        let mut changed = files.clone();
        changed[0].relative_path = "dir_0002/aaaa.txt".to_string();
        assert_ne!(tree_checksum(&files), tree_checksum(&changed));
        assert_eq!(tree_checksum(&files), tree_checksum(&files.clone()));
    }
}

//! Spotlight readiness gate (request section 7).
//!
//! Spotlight indexing is asynchronous: a corpus copied a moment ago may be
//! invisible to `mdfind` for a while. A score measured against a half-built
//! index is not a result, so `run` asks this module first and refuses to start
//! while the index is incomplete.
//!
//! The check is mechanical and uses the corpus itself (no probe files are
//! written into the corpus), so it cannot pass by accident on a directory the
//! scattered documents do not live in.

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::scatter::{ScatterFile, ScatterManifest};
use super::search::{ScopedSearch, mdutil_status, parse_indexing_enabled};
use super::{MacOsVersion, Workspace, macos_version, normalize_path, now_rfc3339};

/// Bytes read from a document when looking for a content probe token.
const TOKEN_SCAN_BYTES: usize = 4 * 1024;
/// Shortest token used as a content probe.
const TOKEN_MIN_LENGTH: usize = 6;

/// One probed document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SampleProbe {
    pub doc_id: String,
    pub relative_path: String,
    pub found_by_name: bool,
    pub content_token: Option<String>,
    /// `None` when the sample offered no usable token.
    pub found_by_content: Option<bool>,
}

/// Whether the directories the corpus lives in survive kaseifu's own rules.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ExclusionAudit {
    pub root_excluded: bool,
    pub checked_directories: usize,
    pub excluded_directories: Vec<String>,
}

/// Everything recorded about index readiness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexReport {
    pub ready: bool,
    pub reasons: Vec<String>,
    pub root: String,
    pub corpus_present: bool,
    pub documents: usize,
    pub samples: Vec<SampleProbe>,
    pub exclusions: ExclusionAudit,
    pub mdutil_volume_root: String,
    pub mdutil_data_volume: String,
    pub indexing_enabled: Option<bool>,
    pub macos: MacOsVersion,
    pub checked_at: String,
    pub timeout_ms: u64,
}

/// Deterministically spread the samples over the corpus instead of probing the
/// alphabetically first documents only.
pub fn sample_files(files: &[ScatterFile], count: usize) -> Vec<&ScatterFile> {
    let mut sorted: Vec<&ScatterFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    if sorted.is_empty() || count == 0 {
        return Vec::new();
    }
    if sorted.len() <= count {
        return sorted;
    }
    let step = sorted.len() / count;
    (0..count)
        .map(|index| sorted[(index * step).min(sorted.len() - 1)])
        .collect()
}

/// A content word that proves the *body* is indexed: the document's own
/// `<DOCNO>` when it has one, otherwise its longest plain word, skipping
/// anything the file name would already match.
pub fn content_token(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let bytes = std::fs::read(path).ok()?;
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(TOKEN_SCAN_BYTES)]).to_string();
    if let Some(token) = docno_token(&head)
        && !name.contains(&token.to_lowercase())
    {
        return Some(token);
    }
    let mut best: Option<String> = None;
    for word in head.split(|c: char| !c.is_ascii_alphanumeric()) {
        if word.len() < TOKEN_MIN_LENGTH || name.contains(&word.to_lowercase()) {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|current| word.len() > current.len())
        {
            best = Some(word.to_string());
        }
    }
    best
}

fn docno_token(head: &str) -> Option<String> {
    let lowered = head.to_lowercase();
    let start = lowered.find("<docno>")? + "<docno>".len();
    let rest = &head[start..];
    let end = rest.to_lowercase().find("</docno>")?;
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Probe the index for the corpus and decide whether a run may start.
pub fn check_index(
    workspace: &Workspace,
    manifest: &ScatterManifest,
    sample_count: usize,
    timeout_ms: u64,
) -> Result<IndexReport> {
    let root = workspace.scattered_dir();
    let search = ScopedSearch::new(&root, timeout_ms);
    let mut reasons: Vec<String> = Vec::new();

    let corpus_present = root.is_dir() && !manifest.files.is_empty();
    if !corpus_present {
        reasons.push(format!(
            "scattered corpus is missing or empty: {} (run scatter first)",
            root.display()
        ));
    }

    let exclusions = audit_exclusions(&root, manifest);
    if exclusions.root_excluded {
        reasons.push(format!(
            "corpus root is dropped by kaseifu's destination rules: {} (src/discovery/destinations.rs:112)",
            root.display()
        ));
    }
    for directory in &exclusions.excluded_directories {
        reasons.push(format!(
            "scattered directory is dropped by kaseifu's destination rules: {directory}"
        ));
    }

    let mut samples: Vec<SampleProbe> = Vec::new();
    if corpus_present {
        for file in sample_files(&manifest.files, sample_count) {
            samples.push(probe_file(&search, &root, file)?);
        }
    }

    let by_name_failures: Vec<&SampleProbe> = samples
        .iter()
        .filter(|probe| !probe.found_by_name)
        .collect();
    for probe in &by_name_failures {
        reasons.push(format!(
            "not found by mdfind (name query): {} (Spotlight indexing is asynchronous; retry later)",
            probe.relative_path
        ));
    }
    let by_content_failures: Vec<&SampleProbe> = samples
        .iter()
        .filter(|probe| probe.found_by_content == Some(false))
        .collect();
    for probe in &by_content_failures {
        reasons.push(format!(
            "not found by mdfind (content query): {} (body text is not searchable yet)",
            probe.relative_path
        ));
    }

    let mdutil_volume_root = mdutil_status(Path::new("/"));
    let mdutil_data_volume = mdutil_status(Path::new("/System/Volumes/Data"));
    let indexing_enabled = parse_indexing_enabled(&mdutil_volume_root)
        .or_else(|| parse_indexing_enabled(&mdutil_data_volume));
    if indexing_enabled == Some(false) {
        reasons.push("Spotlight indexing is disabled for this volume".to_string());
    }
    if root.to_string_lossy().starts_with("/tmp")
        || root.to_string_lossy().starts_with("/private/tmp")
    {
        reasons.push(
            "the corpus lives under /tmp, which mdfind does not index (measured)".to_string(),
        );
    }

    let files_count = manifest.files.len();
    Ok(IndexReport {
        ready: reasons.is_empty(),
        reasons,
        root: root.to_string_lossy().to_string(),
        corpus_present,
        documents: files_count,
        samples,
        exclusions,
        mdutil_volume_root,
        mdutil_data_volume,
        indexing_enabled,
        macos: macos_version(),
        checked_at: now_rfc3339(),
        timeout_ms,
    })
}

/// `check-index` also has to be usable before a scatter manifest exists: it
/// then reports the layout and the exclusion rules, and says it is not ready.
pub fn check_index_layout_only(workspace: &Workspace, timeout_ms: u64) -> Result<IndexReport> {
    let root = workspace.scattered_dir();
    let reasons: Vec<String> = vec![format!(
        "no scattering manifest found: {}",
        workspace.scatter_manifest_path().display()
    )];
    let mdutil_volume_root = mdutil_status(Path::new("/"));
    let mdutil_data_volume = mdutil_status(Path::new("/System/Volumes/Data"));
    Ok(IndexReport {
        ready: false,
        reasons,
        root: root.to_string_lossy().to_string(),
        corpus_present: false,
        documents: 0,
        samples: Vec::new(),
        exclusions: ExclusionAudit::default(),
        mdutil_volume_root,
        mdutil_data_volume,
        indexing_enabled: parse_indexing_enabled(&mdutil_status(Path::new("/"))),
        macos: macos_version(),
        checked_at: now_rfc3339(),
        timeout_ms,
    })
}

fn probe_file(search: &ScopedSearch, root: &Path, file: &ScatterFile) -> Result<SampleProbe> {
    let path = root.join(&file.relative_path);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let name_query = format!("kMDItemFSName == \"{file_name}\"");
    let by_name = search.search_checked(&name_query, 20)?.unwrap_or_default();
    let found_by_name = by_name
        .iter()
        .any(|hit| normalize_path(Path::new(hit)) == normalize_path(&path));

    let token = content_token(&path);
    let found_by_content = match &token {
        None => None,
        Some(token) => {
            let query = format!("kMDItemTextContent == \"*{token}*\"c");
            let hits = search.search_checked(&query, 20)?.unwrap_or_default();
            Some(
                hits.iter()
                    .any(|hit| normalize_path(Path::new(hit)) == normalize_path(&path)),
            )
        }
    };

    Ok(SampleProbe {
        doc_id: file.doc_id.clone(),
        relative_path: file.relative_path.clone(),
        found_by_name,
        content_token: token,
        found_by_content,
    })
}

/// Count the scattered directories kaseifu would refuse as destinations.
fn audit_exclusions(root: &Path, manifest: &ScatterManifest) -> ExclusionAudit {
    let root_string = root.to_string_lossy().to_string();
    let mut audit = ExclusionAudit {
        root_excluded: crate::discovery::is_excluded_destination(&root_string),
        ..ExclusionAudit::default()
    };
    let mut checked: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for file in &manifest.files {
        let directory = root
            .join(&file.relative_path)
            .parent()
            .map(|path| path.to_string_lossy().to_string());
        if let Some(directory) = directory {
            checked.insert(directory);
        }
    }
    audit.checked_directories = checked.len();
    audit.excluded_directories = checked
        .into_iter()
        .filter(|directory| crate::discovery::is_excluded_destination(directory))
        .take(5)
        .collect();
    audit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::scatter::ScatterFile;

    fn files(count: usize) -> Vec<ScatterFile> {
        (0..count)
            .map(|index| ScatterFile {
                doc_id: format!("DOC-{index:03}"),
                relative_path: format!("dir_{:04}/f{index:03}.txt", index + 1),
                sha256: "x".to_string(),
                bytes: 1,
                mtime_unix: 0,
            })
            .collect()
    }

    #[test]
    fn samples_are_spread_over_the_corpus() {
        let all = files(100);
        let picked = sample_files(&all, 5);
        assert_eq!(picked.len(), 5);
        assert_eq!(picked[0].doc_id, "DOC-000");
        assert_eq!(picked[4].doc_id, "DOC-080");
        assert!(sample_files(&all, 0).is_empty());
        assert_eq!(sample_files(&files(3), 5).len(), 3);
        assert!(sample_files(&[], 5).is_empty());
    }

    #[test]
    fn content_token_prefers_the_docno_and_avoids_the_file_name() {
        let dir = std::env::temp_dir().join(format!("kaseifu-bench-token-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let with_tag = dir.join("7f82a91c.txt");
        std::fs::write(
            &with_tag,
            "<DOC>\n<DOCNO>FBIS3-10082</DOCNO>\nsome body text\n",
        )
        .unwrap();
        assert_eq!(content_token(&with_tag).as_deref(), Some("FBIS3-10082"));

        let name_only = dir.join("documented.txt");
        std::fs::write(&name_only, "documented material appears here\n").unwrap();
        assert_eq!(content_token(&name_only).as_deref(), Some("material"));

        let nothing = dir.join("x.txt");
        std::fs::write(&nothing, "a b c\n").unwrap();
        assert_eq!(content_token(&nothing), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn library_tree_corpus_is_reported_as_excluded() {
        let root = Path::new(
            "/Users/test/Library/Application Support/kaseifu/benchmark/workspace/scattered",
        );
        let manifest = ScatterManifest {
            benchmark: "test".to_string(),
            algorithm: "test".to_string(),
            strategy: "flat".to_string(),
            seed: 42,
            directory_count: 1,
            filename_strategy: "test".to_string(),
            timestamp_strategy: "fixed".to_string(),
            fixed_mtime_unix: 0,
            birthtime_settable: false,
            birthtime_note: String::new(),
            birthtime_observed_unix: None,
            root: root.to_string_lossy().to_string(),
            selector: "test".to_string(),
            files: vec![ScatterFile {
                doc_id: "A".to_string(),
                relative_path: "f.txt".to_string(),
                sha256: "x".to_string(),
                bytes: 1,
                mtime_unix: 0,
            }],
            missing_doc_ids: Vec::new(),
            total_bytes: 1,
            tree_sha256: "x".to_string(),
            manifest_sha256: "x".to_string(),
        };
        let audit = audit_exclusions(root, &manifest);
        assert!(
            audit.root_excluded,
            "a ~/Library corpus root must be flagged"
        );
        assert_eq!(audit.excluded_directories.len(), 1);

        let ok_root = Path::new("/Users/test/Documents/kaseifu-benchmark/workspace/scattered");
        let ok_audit = audit_exclusions(ok_root, &manifest);
        assert!(!ok_audit.root_excluded);
        assert!(ok_audit.excluded_directories.is_empty());
    }
}

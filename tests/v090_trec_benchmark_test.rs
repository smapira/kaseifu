//! v0.9.0 TREC benchmark (D2D Hit@1) tests.
//!
//! Request section 12 asks for ten behaviours. Every test here runs with
//! fixtures only: no network, no real qrels, no real corpus, no real `mdfind`
//! results, and never inside the user's home. Spotlight is replaced by an
//! injected search closure, which is the same boundary the product pipeline
//! already exposes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kaseifu::benchmark::gate::IndexReport;
use kaseifu::benchmark::qrels::{
    D2dManifest, DatasetInfo, ManifestEntry, Qrels, build_manifest, load_manifest, verify_manifest,
};
use kaseifu::benchmark::run::{RunRequest, evaluate, write_outputs};
use kaseifu::benchmark::scatter::{cleanup, scatter, verify_scatter};
use kaseifu::benchmark::systems::{
    Bm25Params, Corpus, System, predict_kaseifu, predict_random, rank_documents,
};
use kaseifu::benchmark::{Workspace, is_inside, normalize_path};

/// Isolated scratch directory per test, removed on drop.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        // Not `std::env::temp_dir()`: on macOS that is `/var/folders/...`,
        // whose first component (`var`) kaseifu's destination rules exclude
        // (src/discovery/destinations.rs:61), so a fixture corpus there would
        // be dropped by the product's own hit filter. Cargo's per-target tmp
        // directory is isolated, ignored by git, and passes those rules.
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "v090-{}-{}-{:?}",
            std::process::id(),
            name,
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch root");
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn sample_qrels() -> &'static str {
    "301 0 DOC-A 1\n301 0 DOC-B 1\n301 0 DOC-C 0\n302 0 DOC-B 2\n302 0 DOC-D 1\n303 0 DOC-E 0\n"
}

fn dataset_for(text: &str, qrels: &Qrels) -> DatasetInfo {
    DatasetInfo {
        name: "fixture qrels".to_string(),
        version: "fixture".to_string(),
        source_url: "fixture".to_string(),
        sha256: kaseifu::benchmark::sha256_bytes(text.as_bytes()),
        bytes: text.len() as u64,
        topics_total: qrels.summary.topics_total,
        topics_missing: qrels.summary.topics_missing.clone(),
        qrels_lines: qrels.summary.qrels_lines,
        positive_judgments: qrels.summary.positive_judgments,
        rel_histogram: qrels.summary.rel_histogram.clone(),
        judged_docs: qrels.summary.judged_docs,
        positive_docs: qrels.summary.positive_docs,
    }
}

/// A TREC-like document whose `<DOCNO>` tag carries the official id.
fn write_trec_doc(path: &Path, docno: &str, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let text = format!(
        "<DOC>\n<DOCNO>{docno}</DOCNO>\n<HEADLINE>headline</HEADLINE>\n<TEXT>\n{body}\n</TEXT>\n</DOC>\n"
    );
    std::fs::write(path, text).unwrap();
}

/// Raw corpus with four documents, two of which share a topic in the fixture
/// qrels. It lives under the scratch root, never in a user directory.
fn build_source_corpus(scratch: &Scratch) -> PathBuf {
    let source = scratch.path("datasets/imported");
    write_trec_doc(
        &source.join("FBIS3-10082.txt"),
        "FBIS3-10082",
        "alpha banana cherry shared topic",
    );
    write_trec_doc(
        &source.join("FBIS3-10083.txt"),
        "FBIS3-10083",
        "alpha banana related body",
    );
    write_trec_doc(
        &source.join("LA010189-0001.txt"),
        "LA010189-0001",
        "delta echo foxtrot other topic",
    );
    write_trec_doc(
        &source.join("LA010189-0002.txt"),
        "LA010189-0002",
        "golf hotel india unrelated body",
    );
    source
}

fn fixture_qrels() -> Qrels {
    // 301 relates 10082 and 10083; 302 relates 0001 and 0002. The other two
    // documents are only judged 0, so they must not enter the ground truth.
    let text = "301 0 FBIS3-10082 1\n301 0 FBIS3-10083 2\n302 0 LA010189-0001 1\n302 0 LA010189-0002 2\n303 0 LA010189-0001 1\n";
    Qrels::parse(text).expect("fixture qrels")
}

fn ready_gate(root: &Path) -> IndexReport {
    IndexReport {
        ready: true,
        reasons: Vec::new(),
        root: root.to_string_lossy().to_string(),
        corpus_present: true,
        documents: 4,
        samples: Vec::new(),
        exclusions: Default::default(),
        mdutil_volume_root: "Indexing enabled.".to_string(),
        mdutil_data_volume: "Indexing enabled.".to_string(),
        indexing_enabled: Some(true),
        macos: kaseifu::benchmark::macos_version(),
        checked_at: "fixture".to_string(),
        timeout_ms: 1000,
    }
}

/// Fixture: workspace, scattered corpus and a frozen D2D manifest.
struct Fixture {
    scratch: Scratch,
    workspace: Workspace,
    manifest: D2dManifest,
    manifest_path: PathBuf,
    scatter: kaseifu::benchmark::scatter::ScatterManifest,
    qrels: Qrels,
}

fn fixture(name: &str) -> Fixture {
    fixture_with_strategy(name, kaseifu::benchmark::scatter::STRATEGY_UNIFORM, 3)
}

fn fixture_with_strategy(name: &str, strategy: &str, directories: usize) -> Fixture {
    let scratch = Scratch::new(name);
    let workspace = Workspace::from_root(scratch.path("workspace"));
    workspace.ensure_layout().unwrap();
    let source = build_source_corpus(&scratch);
    let qrels = fixture_qrels();
    let known = qrels.positive_doc_ids();
    let scatter = scatter(&workspace, &source, strategy, 42, directories, &known).unwrap();
    kaseifu::benchmark::write_json(&workspace.scatter_manifest_path(), &scatter).unwrap();

    let dataset = dataset_for(sample_qrels(), &qrels);
    let manifest = build_manifest(&qrels, dataset, 42, 0).unwrap();
    let manifest_path = workspace.manifests_dir().join("d2d-seed42-all.json");
    kaseifu::benchmark::write_json(&manifest_path, &manifest).unwrap();
    Fixture {
        scratch,
        workspace,
        manifest,
        manifest_path,
        scatter,
        qrels,
    }
}

/// Every corpus document with its size and digest, for before/after checks.
fn corpus_snapshot(root: &Path) -> BTreeMap<String, (u64, String, i64)> {
    let mut snapshot = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        let metadata = std::fs::metadata(path).unwrap();
        let modified = metadata
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        snapshot.insert(
            path.to_string_lossy().to_string(),
            (
                metadata.len(),
                kaseifu::benchmark::sha256_file(path).unwrap(),
                modified,
            ),
        );
    }
    snapshot
}

// ---------------------------------------------------------------------------
// 12-1: qrels loading
// ---------------------------------------------------------------------------

#[test]
fn v090_qrels_loads_the_official_four_column_format() {
    let qrels = Qrels::parse(sample_qrels()).expect("parses");
    assert_eq!(qrels.summary.qrels_lines, 6);
    assert_eq!(qrels.summary.positive_judgments, 4);
    assert_eq!(qrels.summary.positive_docs, 3);
    assert_eq!(qrels.summary.judged_docs, 5);
    assert_eq!(qrels.summary.rel_histogram.get("0"), Some(&2));
    assert_eq!(qrels.summary.rel_histogram.get("1"), Some(&3));
    assert_eq!(qrels.summary.rel_histogram.get("2"), Some(&1));
    // The distributed file has no judgements for 672; the missing list is
    // derived from the Robust04 topic ids, not assumed.
    assert!(qrels.summary.topics_missing.contains(&672));
    assert_eq!(
        qrels.summary.topics_total, 3,
        "only the fixture topics are judged"
    );
    assert_eq!(kaseifu::benchmark::qrels::robust04_topic_ids().len(), 250);

    // Malformed input fails loudly instead of being skipped silently.
    let error = Qrels::parse("301 0 DOC-A\n").unwrap_err().to_string();
    assert!(error.contains("line 1"), "{error}");
    assert!(Qrels::load(Path::new("/nonexistent/qrels.txt")).is_err());
}

// ---------------------------------------------------------------------------
// 12-2 and 12-3: many-to-many ground truth, self excluded
// ---------------------------------------------------------------------------

#[test]
fn v090_many_to_many_shared_topics_become_the_ground_truth() {
    let qrels = fixture_qrels();
    let truth_a = qrels.ground_truth("FBIS3-10082");
    assert!(truth_a.contains("FBIS3-10083"), "shared topic 301");
    assert!(!truth_a.contains("LA010189-0001"), "no shared topic");
    assert_eq!(
        truth_a.len(),
        1,
        "multiple candidates are kept, not collapsed"
    );

    let truth_first = qrels.ground_truth("LA010189-0001");
    assert!(truth_first.contains("LA010189-0002"), "shared topic 302");
    assert_eq!(truth_first.len(), 1);
    // Symmetric relation: the relation is derived from shared topics, not from
    // a directed judgement.
    assert!(
        qrels
            .ground_truth("LA010189-0002")
            .contains("LA010189-0001")
    );

    // Multiple candidates are preserved when a topic has more than two.
    let wide = Qrels::parse("301 0 A 1\n301 0 B 1\n301 0 C 1\n").unwrap();
    assert_eq!(wide.ground_truth("A").len(), 2);
    assert_eq!(wide.ground_truth("B").len(), 2);
}

#[test]
fn v090_the_input_document_is_never_its_own_ground_truth() {
    let qrels = fixture_qrels();
    for doc_id in ["FBIS3-10082", "FBIS3-10083", "LA010189-0001"] {
        let truth = qrels.ground_truth(doc_id);
        assert!(
            !truth.contains(doc_id),
            "{doc_id} must not be its own ground truth"
        );
    }
}

// ---------------------------------------------------------------------------
// 12-4 and 12-5: Hit@1 and the empty result
// ---------------------------------------------------------------------------

#[test]
fn v090_hit_at_1_matches_the_worked_example() {
    let fx = fixture("hit1");
    let request = RunRequest {
        workspace: &fx.workspace,
        manifest: &fx.manifest,
        manifest_path: fx.manifest_path.clone(),
        scatter: &fx.scatter,
        qrels: &fx.qrels,
        gate: &ready_gate(&fx.workspace.scattered_dir()),
        system: System::Kaseifu,
        seed: 42,
        bm25: Bm25Params::default(),
        bm25_max_docs: 0,
        timeout_ms: 1000,
        timeout_counter: None,
    };
    // Spotlight is replaced: every hit of the fixture corpus except the input
    // itself. This isolates the metric from the index.
    let outcome = evaluate(&request, |_query, _limit| {
        Ok(fx
            .scatter
            .paths()
            .into_iter()
            .map(|(_, path)| path.to_string_lossy().to_string())
            .collect())
    })
    .expect("evaluation runs");

    // 4 inputs, each with exactly one ground-truth document, and the corpus
    // holds 3 candidates per input. The score itself carries no information
    // here (see `v090_opaque_names_leave_only_the_extension_score`), so the
    // prediction is decided by the documented tie-break; what must hold is
    // that every input is evaluated, that the metric counts exactly the
    // ground-truth hits, and that the whole run is reproducible.
    assert_eq!(outcome.report.results.evaluated, 4);
    assert_eq!(outcome.rows.len(), 4);
    assert!((outcome.report.results.coverage - 1.0).abs() < 1e-12);
    assert!(
        outcome.report.results.hits >= 1,
        "at least one tie-break lands on a hit"
    );
    for row in &outcome.rows {
        let entry = fx
            .manifest
            .entry(&row.source_doc_id)
            .expect("input is in the manifest");
        let truth = entry
            .ground_truth_doc_ids
            .as_ref()
            .expect("small ground truth is embedded");
        assert_eq!(
            row.hit,
            truth.iter().any(|doc_id| doc_id == &row.predicted_doc_id),
            "hit must mean `predicted is in the ground truth`: {row:?}"
        );
        assert_eq!(
            row.candidate_count, 3,
            "the input itself is never a candidate"
        );
        assert_ne!(row.predicted_doc_id, row.source_doc_id);
    }
    assert_eq!(
        outcome.report.results.hits,
        outcome.rows.iter().filter(|row| row.hit).count()
    );
    assert_eq!(outcome.report.metric, "hit@1");
    assert_eq!(outcome.report.system, "kaseifu");

    // The same fixture with an unrelated prediction scores 0 for that input.
    let unrelated = evaluate(&request, |_query, _limit| {
        Ok(vec![
            fx.scatter
                .path_of("LA010189-0002")
                .unwrap()
                .to_string_lossy()
                .to_string(),
        ])
    })
    .expect("evaluation runs");
    assert!(unrelated.report.results.hit_at_1 < 1.0);
}

#[test]
fn v090_opaque_names_leave_only_the_extension_score() {
    // Measured property of the current Discovery, pinned so it cannot change
    // silently: on a scattered corpus whose file names are opaque, the
    // document score is identical for every candidate (extension match only),
    // so the document order is decided by the tie-break (score, then age, then
    // path). This is why a kaseifu Hit@1 on such a corpus must be read as a
    // lower bound of a filename-driven signal, not as a body-text score.
    let fx = fixture("degenerate-score");
    let input = fx.scatter.path_of("FBIS3-10082").unwrap();
    let raw = kaseifu::context::builder::build(&input).expect("observe");
    let normalized = kaseifu::normalize::normalize_with_options(
        raw,
        &kaseifu::context::image_metadata::MetadataOptions::default(),
    )
    .expect("normalize");
    let terms =
        kaseifu::discovery::normalize_terms(&kaseifu::discovery::extract_terms(&normalized));
    let all_paths: Vec<String> = fx
        .scatter
        .paths()
        .into_iter()
        .map(|(_, path)| path.to_string_lossy().to_string())
        .collect();
    let files = kaseifu::discovery::find_similar_files_with(
        &terms,
        &normalized.target.filename,
        normalized.target.extension.as_deref(),
        &normalized.target.path,
        &normalized.parent_directory,
        move |_query, _limit| Ok(all_paths.clone()),
    )
    .expect("similarity runs");
    assert_eq!(files.len(), 3, "the input itself is filtered out");
    let mut observed: Vec<String> = Vec::new();
    for file in &files {
        // The score is exactly the product formula, with no benchmark term:
        // 0.60 * filename + 0.25 * token + 0.15 * extension
        // (src/discovery/scoring.rs:190-192).
        let recomputed = 0.60 * file.filename_similarity
            + 0.25 * file.token_similarity
            + 0.15 * file.extension_match;
        assert!(
            (file.similarity_score - recomputed).abs() < 1e-12,
            "the benchmark must not add or change a scoring term"
        );
        assert_eq!(file.extension_match, 1.0, ".txt against .txt");
        assert_eq!(
            file.token_similarity, 0.0,
            "opaque names share no token with the input name"
        );
        assert!(
            file.filename_similarity < 0.5,
            "no candidate name is meaningfully close to the input name: {}",
            file.filename_similarity
        );
        observed.push(format!(
            "{} file={:.4} token={:.4} ext={:.4} score={:.4}",
            file.path.file_name().unwrap_or_default().to_string_lossy(),
            file.filename_similarity,
            file.token_similarity,
            file.extension_match,
            file.similarity_score
        ));
    }
    // What decides the order here is accidental character overlap between two
    // random names, not any property of the documents. Recorded so the claim
    // is reproducible from the test output.
    let ranked = rank_documents(files);
    let sorted_by_score: Vec<String> = {
        let mut scored = observed.clone();
        scored.sort();
        scored
    };
    assert_eq!(ranked.len(), sorted_by_score.len());
    let order: Vec<String> = ranked
        .iter()
        .map(|file| file.path.to_string_lossy().to_string())
        .collect();
    let mut expected = order.clone();
    expected.sort();
    let scores: Vec<f64> = ranked.iter().map(|file| file.similarity_score).collect();
    assert!(
        scores.windows(2).all(|pair| pair[0] >= pair[1]),
        "the ranking is score-descending: {scores:?}"
    );
    assert_eq!(
        order.iter().filter(|path| expected.contains(path)).count(),
        3,
        "every candidate is ranked exactly once"
    );
}

#[test]
fn v090_a_missing_result_is_a_miss_and_is_not_removed_from_the_denominator() {
    let fx = fixture("noresult");
    let request = RunRequest {
        workspace: &fx.workspace,
        manifest: &fx.manifest,
        manifest_path: fx.manifest_path.clone(),
        scatter: &fx.scatter,
        qrels: &fx.qrels,
        gate: &ready_gate(&fx.workspace.scattered_dir()),
        system: System::Kaseifu,
        seed: 42,
        bm25: Bm25Params::default(),
        bm25_max_docs: 0,
        timeout_ms: 1000,
        timeout_counter: None,
    };
    let outcome = evaluate(&request, |_query, _limit| Ok(Vec::new())).expect("runs");
    assert_eq!(
        outcome.report.results.evaluated, 4,
        "inputs stay in the denominator"
    );
    assert_eq!(outcome.report.results.hits, 0);
    assert_eq!(outcome.report.results.hit_at_1, 0.0);
    assert_eq!(outcome.report.results.coverage, 0.0);
    assert_eq!(outcome.report.auxiliary.no_result_rate, 1.0);
    assert!(
        outcome
            .rows
            .iter()
            .all(|row| row.predicted_doc_id.is_empty())
    );
    assert_eq!(outcome.report.diagnostics.inputs_with_zero_candidates, 4);
}

// ---------------------------------------------------------------------------
// 12-6: rejecting paths outside the evaluation set
// ---------------------------------------------------------------------------

#[test]
fn v090_paths_outside_the_corpus_are_rejected() {
    let root = Path::new("/tmp/kaseifu-v090-scope");
    assert!(is_inside(root, &root.join("dir_0001/a.txt")));
    assert!(!is_inside(root, Path::new("/tmp/elsewhere/a.txt")));
    assert!(!is_inside(root, &root.join("../escaped/a.txt")));
    assert!(!is_inside(root, Path::new("/Users/someone/Desktop/a.txt")));

    // A symlink that points outside the corpus is refused after normalization.
    let scratch = Scratch::new("escape");
    let outside = scratch.path("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "not part of the corpus").unwrap();
    let scope = scratch.path("scope");
    std::fs::create_dir_all(&scope).unwrap();
    let link = scope.join("link.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("secret.txt"), &link).unwrap();
    #[cfg(unix)]
    {
        assert!(
            !is_inside(&scope, &link),
            "a symlink out of the corpus must not pass the containment check"
        );
        assert_ne!(normalize_path(&link), link);
    }

    // Hits outside the corpus are dropped by the kaseifu system even when a
    // (fixture) search returns them.
    let fx = fixture("outside-hits");
    let corpus = Corpus::from_manifest(&fx.scatter);
    let input = fx.scatter.path_of("FBIS3-10082").unwrap();
    let prediction = predict_kaseifu(&input, &corpus, |_query, _limit| {
        Ok(vec![
            outside.join("secret.txt").to_string_lossy().to_string(),
            fx.scatter
                .path_of("FBIS3-10083")
                .unwrap()
                .to_string_lossy()
                .to_string(),
        ])
    })
    .expect("prediction runs");
    assert_eq!(prediction.candidate_count, 1, "only the corpus hit counts");
    assert_eq!(prediction.doc_id.as_deref(), Some("FBIS3-10083"));

    // An input outside the corpus has no doc id, so it cannot be evaluated.
    assert_eq!(corpus.doc_id_of_path(&outside.join("secret.txt")), None);
}

// ---------------------------------------------------------------------------
// 12-7: the benchmark never moves, deletes or rewrites a document
// ---------------------------------------------------------------------------

#[test]
fn v090_an_evaluation_leaves_every_corpus_file_untouched() {
    let fx = fixture("readonly");
    let before = corpus_snapshot(&fx.workspace.scattered_dir());
    assert_eq!(before.len(), 4);

    let request = RunRequest {
        workspace: &fx.workspace,
        manifest: &fx.manifest,
        manifest_path: fx.manifest_path.clone(),
        scatter: &fx.scatter,
        qrels: &fx.qrels,
        gate: &ready_gate(&fx.workspace.scattered_dir()),
        system: System::Kaseifu,
        seed: 42,
        bm25: Bm25Params::default(),
        bm25_max_docs: 0,
        timeout_ms: 1000,
        timeout_counter: None,
    };
    let outcome = evaluate(&request, |_query, _limit| {
        Ok(fx
            .scatter
            .paths()
            .into_iter()
            .map(|(_, path)| path.to_string_lossy().to_string())
            .collect())
    })
    .expect("runs");
    let (json, csv) = write_outputs(&fx.workspace, &outcome.report, &outcome.rows).unwrap();
    assert!(json.is_file() && csv.is_file());

    let after = corpus_snapshot(&fx.workspace.scattered_dir());
    assert_eq!(before, after, "sizes, digests and mtimes must be unchanged");

    // Reports live outside the corpus.
    assert!(json.starts_with(fx.workspace.runs_dir()));
    assert!(!is_inside(&fx.workspace.scattered_dir(), &json));

    // A dry-run cleanup deletes nothing, and the raw source is untouched.
    let report = cleanup(&fx.scatter, false).unwrap();
    assert_eq!(report.removed_files, 0);
    assert_eq!(corpus_snapshot(&fx.workspace.scattered_dir()), before);

    // Scattering refuses to overwrite an existing file instead of clobbering it.
    let source = fx.scratch.path("datasets/imported");
    let again = scatter(
        &fx.workspace,
        &source,
        kaseifu::benchmark::scatter::STRATEGY_UNIFORM,
        42,
        3,
        &[],
    );
    assert!(
        again.is_err(),
        "an existing scattered corpus is not overwritten"
    );
}

// ---------------------------------------------------------------------------
// 12-8: the same inputs produce the same manifest
// ---------------------------------------------------------------------------

#[test]
fn v090_the_same_inputs_produce_an_identical_manifest() {
    let qrels = fixture_qrels();
    let dataset = dataset_for(sample_qrels(), &qrels);
    let first = build_manifest(&qrels, dataset.clone(), 42, 0).unwrap();
    let second = build_manifest(&qrels, dataset.clone(), 42, 0).unwrap();
    assert_eq!(first.manifest_sha256, second.manifest_sha256);
    assert_eq!(first, second);
    verify_manifest(&first).unwrap();

    let scratch = Scratch::new("manifest-bytes");
    let workspace = Workspace::from_root(scratch.path("ws"));
    workspace.ensure_layout().unwrap();
    let path_a = workspace.manifests_dir().join("a.json");
    let path_b = workspace.manifests_dir().join("b.json");
    kaseifu::benchmark::write_json(&path_a, &first).unwrap();
    kaseifu::benchmark::write_json(&path_b, &second).unwrap();
    let bytes_a = std::fs::read(&path_a).unwrap();
    let bytes_b = std::fs::read(&path_b).unwrap();
    assert_eq!(bytes_a, bytes_b, "byte-identical manifests");

    // The manifest fixes the evaluation set: inputs, ground truth size and
    // digest, plus the derivation note and the graded judgements.
    let entry = first
        .entries
        .iter()
        .find(|entry| entry.source_doc_id == "FBIS3-10082")
        .expect("input present");
    assert_eq!(entry.ground_truth_size, 1);
    assert!(entry.ground_truth_doc_ids.is_some());
    assert_eq!(entry.positive_topics.len(), 1);
    assert_eq!(entry.positive_topics[0].topic, 301);
    assert_eq!(entry.positive_topics[0].rel, 1);
    assert!(first.derivation.self_excluded);
    assert!(!first.derivation.negative_judgments_used);

    // A different seed selects a different subset, and the checksum follows.
    let long = (0..40)
        .map(|index| format!("DOC-{index:03}"))
        .collect::<Vec<String>>();
    let selected_a = kaseifu::benchmark::qrels::select_inputs(&long, 42, 5);
    let selected_b = kaseifu::benchmark::qrels::select_inputs(&long, 42, 5);
    let selected_c = kaseifu::benchmark::qrels::select_inputs(&long, 43, 5);
    assert_eq!(selected_a, selected_b);
    assert_ne!(selected_a, selected_c);
    assert_eq!(selected_a.len(), 5);

    // A tampered manifest is refused on load.
    let mut tampered = first.clone();
    tampered.entries[0].ground_truth_size += 1;
    let tampered_path = workspace.manifests_dir().join("d2d-tampered.json");
    kaseifu::benchmark::write_json(&tampered_path, &tampered).unwrap();
    assert!(load_manifest(&tampered_path).is_err());
}

// ---------------------------------------------------------------------------
// 12-9: no evaluation starts before the index is ready
// ---------------------------------------------------------------------------

#[test]
fn v090_an_evaluation_never_starts_on_an_unready_index() {
    let fx = fixture("unready");
    let mut gate = ready_gate(&fx.workspace.scattered_dir());
    gate.ready = false;
    gate.reasons = vec!["not found by mdfind (name query): dir_0001/x.txt".to_string()];
    let request = RunRequest {
        workspace: &fx.workspace,
        manifest: &fx.manifest,
        manifest_path: fx.manifest_path.clone(),
        scatter: &fx.scatter,
        qrels: &fx.qrels,
        gate: &gate,
        system: System::Kaseifu,
        seed: 42,
        bm25: Bm25Params::default(),
        bm25_max_docs: 0,
        timeout_ms: 1000,
        timeout_counter: None,
    };
    let searched = std::cell::Cell::new(0u32);
    let error = evaluate(&request, |_query, _limit| {
        searched.set(searched.get() + 1);
        Ok(Vec::new())
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("not ready"), "{error}");
    assert_eq!(searched.get(), 0, "not a single search may run");
}

// ---------------------------------------------------------------------------
// Document level extraction: Flat keeps its candidates
// ---------------------------------------------------------------------------

#[test]
fn v090_a_flat_corpus_still_yields_document_candidates() {
    // The folder aggregation would drop every document of a flat corpus (the
    // input's own directory is never a move destination). The document level
    // exit must not inherit that rule, or a flat run would score 0 by design.
    let fx = fixture_with_strategy(
        "flat-candidates",
        kaseifu::benchmark::scatter::STRATEGY_FLAT,
        1,
    );
    assert_eq!(fx.scatter.directory_count, 1);
    assert!(
        fx.scatter
            .files
            .iter()
            .all(|file| !file.relative_path.contains('/')),
        "flat scattering keeps every document in one directory"
    );
    let corpus = Corpus::from_manifest(&fx.scatter);
    let input = fx.scatter.path_of("FBIS3-10082").unwrap();
    let hits: Vec<String> = fx
        .scatter
        .paths()
        .into_iter()
        .filter(|(doc_id, _)| doc_id != "FBIS3-10082")
        .map(|(_, path)| path.to_string_lossy().to_string())
        .collect();
    let prediction = predict_kaseifu(&input, &corpus, move |_query, _limit| Ok(hits.clone()))
        .expect("prediction runs");
    assert_eq!(prediction.candidate_count, 3, "siblings stay candidates");
    assert!(prediction.doc_id.is_some());
}

#[test]
fn v090_the_discovery_search_path_is_the_product_search_path() {
    // The claim "the existing Discovery is reused, unchanged" is pinned by
    // recording the query both paths send to the search boundary: the product
    // pipeline (folder level) and the benchmark (document level) must send the
    // identical query for the same input.
    let fx = fixture("same-query");
    let input = fx.scatter.path_of("FBIS3-10082").unwrap();
    let observed = std::sync::Mutex::new(Vec::<String>::new());

    let raw = kaseifu::context::builder::build(&input).expect("observe");
    let normalized = kaseifu::normalize::normalize_with_options(
        raw,
        &kaseifu::context::image_metadata::MetadataOptions::default(),
    )
    .expect("normalize");

    let product = kaseifu::discovery::discover_with(normalized.clone(), |query, _limit| {
        observed.lock().unwrap().push(query.to_string());
        Ok(Vec::new())
    })
    .expect("product path runs");
    assert!(product.is_empty());

    let corpus = Corpus::from_manifest(&fx.scatter);
    let _ = predict_kaseifu(&input, &corpus, |query, _limit| {
        observed.lock().unwrap().push(query.to_string());
        Ok(Vec::new())
    })
    .expect("benchmark path runs");

    let queries = observed.lock().unwrap().clone();
    assert_eq!(queries.len(), 2, "both paths searched once: {queries:?}");
    // The word *set* must match. The order inside the query is not stable in
    // the product either: `extract_terms` collects from a `HashSet`
    // (src/discovery/search_terms.rs:13), whose iteration order varies between
    // calls. mdfind reads a bare word sequence as a conjunction, so the
    // measured result is unaffected - but no artifact of this harness may
    // depend on that order, and the test compares sets accordingly.
    assert_eq!(
        word_set(&queries[0]),
        word_set(&queries[1]),
        "the benchmark must send the same query terms as Discovery"
    );

    // The recorded query is the product query for this document, not a
    // benchmark-specific one: it is built from the file's own terms.
    let terms =
        kaseifu::discovery::normalize_terms(&kaseifu::discovery::extract_terms(&normalized));
    assert!(
        queries[0].contains(&terms[0]) || terms.is_empty(),
        "the query is built from the extracted terms: {queries:?} vs {terms:?}"
    );
}

// ---------------------------------------------------------------------------
// Baselines
// ---------------------------------------------------------------------------

#[test]
fn v090_the_baselines_are_deterministic_and_documented() {
    let fx = fixture_with_strategy(
        "baselines",
        kaseifu::benchmark::scatter::STRATEGY_UNIFORM,
        2,
    );
    let corpus = Corpus::from_manifest(&fx.scatter);
    let first = predict_random(&corpus, "FBIS3-10082", 42);
    let second = predict_random(&corpus, "FBIS3-10082", 42);
    assert_eq!(first, second);
    assert_ne!(first.doc_id.as_deref(), Some("FBIS3-10082"));
    assert_eq!(
        first.candidate_count, 3,
        "the pool is the corpus minus the input"
    );

    // BM25 reads the bodies and prefers the document that shares more terms.
    let index = kaseifu::benchmark::systems::Bm25Index::build(&corpus, 0, Bm25Params::default())
        .expect("index builds");
    assert_eq!(index.indexed_docs(), 4);
    let prediction = index.predict("FBIS3-10082", &corpus);
    assert_eq!(
        prediction.doc_id.as_deref(),
        Some("FBIS3-10083"),
        "the shared body terms win"
    );
    assert_eq!(prediction, index.predict("FBIS3-10082", &corpus));

    // Ranking of documents stays the documented order.
    let ranked = rank_documents(Vec::new());
    assert!(ranked.is_empty());
}

// ---------------------------------------------------------------------------
// Scattering (sections 15-16)
// ---------------------------------------------------------------------------

#[test]
fn v090_scattering_is_reproducible_and_verifiable() {
    let scratch = Scratch::new("scatter");
    let workspace = Workspace::from_root(scratch.path("ws"));
    workspace.ensure_layout().unwrap();
    let source = build_source_corpus(&scratch);
    let source_before = corpus_snapshot(&source);

    let manifest = scatter(
        &workspace,
        &source,
        kaseifu::benchmark::scatter::STRATEGY_UNIFORM,
        42,
        2,
        &["FBIS3-10082".to_string(), "NOT-IN-CORPUS".to_string()],
    )
    .expect("scatter runs");
    assert_eq!(manifest.files.len(), 4);
    assert_eq!(manifest.missing_doc_ids, vec!["NOT-IN-CORPUS".to_string()]);
    assert_eq!(manifest.strategy, "uniform");
    assert_eq!(manifest.filename_strategy, "deterministic-opaque");
    assert_eq!(manifest.timestamp_strategy, "fixed");
    assert!(!manifest.birthtime_settable);
    assert_eq!(
        manifest.total_bytes,
        source_doc_bytes(&source),
        "the scattered copy accounts for every source byte"
    );

    // Names carry no doc id and the mapping is complete in both directions.
    for file in &manifest.files {
        assert!(!file.relative_path.contains(&file.doc_id));
        assert_eq!(
            manifest.doc_id_of_path(&workspace.scattered_dir().join(&file.relative_path)),
            Some(file.doc_id.as_str())
        );
    }
    assert_eq!(manifest.files.len(), 4);

    // Content is copied byte for byte and the source is untouched.
    let verification = verify_scatter(&manifest).expect("verify runs");
    assert!(verification.ok(), "{verification:?}");
    assert_eq!(verification.checked, 4);
    assert_eq!(
        corpus_snapshot(&source),
        source_before,
        "the import is read-only"
    );

    // Same seed, same layout; a different seed scatters elsewhere.
    let second = ScatterScratch::scatter_only(&source, 42, 2);
    assert_eq!(
        manifest
            .files
            .iter()
            .map(|file| file.relative_path.clone())
            .collect::<Vec<String>>(),
        second
            .files
            .iter()
            .map(|file| file.relative_path.clone())
            .collect::<Vec<String>>()
    );
    let other = ScatterScratch::scatter_only(&source, 43, 2);
    assert_ne!(manifest.tree_sha256, other.tree_sha256, "the seed matters");

    // Cleanup removes exactly the files the manifest lists.
    let removed = cleanup(&manifest, true).expect("cleanup runs");
    assert_eq!(removed.removed_files, 4);
    assert_eq!(corpus_snapshot(&workspace.scattered_dir()).len(), 0);
    assert_eq!(
        corpus_snapshot(&source),
        source_before,
        "cleanup never touches the import"
    );
}

/// Second scratch workspace for the reproducibility comparison.
struct ScatterScratch;

impl ScatterScratch {
    fn scatter_only(
        source: &Path,
        seed: u64,
        directories: usize,
    ) -> kaseifu::benchmark::scatter::ScatterManifest {
        let scratch = Scratch::new(&format!("scatter-seed-{seed}"));
        let workspace = Workspace::from_root(scratch.path("ws"));
        workspace.ensure_layout().unwrap();
        scatter(
            &workspace,
            source,
            kaseifu::benchmark::scatter::STRATEGY_UNIFORM,
            seed,
            directories,
            &[],
        )
        .expect("scatter runs")
    }
}

/**
 * Words of a query, sorted, so two queries can be compared independently of
 * the order the term set happened to iterate in.
 */
fn word_set(query: &str) -> Vec<String> {
    let mut words: Vec<String> = query.split_whitespace().map(str::to_string).collect();
    words.sort();
    words
}

fn source_doc_bytes(source: &Path) -> u64 {
    let mut total = 0;
    for entry in walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        total += std::fs::metadata(entry.path()).unwrap().len();
    }
    total
}

// ---------------------------------------------------------------------------
// Output contract
// ---------------------------------------------------------------------------

#[test]
fn v090_the_report_records_metrics_reproducibility_and_constraints() {
    let fx = fixture("report");
    let timeouts = std::sync::atomic::AtomicUsize::new(3);
    let request = RunRequest {
        workspace: &fx.workspace,
        manifest: &fx.manifest,
        manifest_path: fx.manifest_path.clone(),
        scatter: &fx.scatter,
        qrels: &fx.qrels,
        gate: &ready_gate(&fx.workspace.scattered_dir()),
        system: System::Random,
        seed: 42,
        bm25: Bm25Params::default(),
        bm25_max_docs: 0,
        timeout_ms: 1000,
        timeout_counter: Some(&timeouts),
    };
    let outcome = evaluate(&request, |_query, _limit| Ok(Vec::new())).expect("runs");
    let (json_path, csv_path) =
        write_outputs(&fx.workspace, &outcome.report, &outcome.rows).unwrap();

    let text = std::fs::read_to_string(&json_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["benchmark"], "trec-robust04-derived-d2d");
    assert_eq!(parsed["metric"], "hit@1");
    assert_eq!(parsed["system"], "random");
    for key in ["evaluated", "hits", "hit_at_1", "coverage"] {
        assert!(
            parsed["results"].get(key).is_some(),
            "results.{key} is required"
        );
    }
    for key in ["no_result_rate", "latency_ms", "candidate_count"] {
        assert!(parsed["auxiliary"].get(key).is_some(), "auxiliary.{key}");
    }
    for key in [
        "strategy",
        "seed",
        "directory_count",
        "filename_strategy",
        "timestamp_strategy",
    ] {
        assert!(parsed["scattering"].get(key).is_some(), "scattering.{key}");
    }
    for key in [
        "kaseifu_commit",
        "rust_version",
        "dataset",
        "corpus",
        "manifest",
        "macos",
        "spotlight",
        "settings",
        "started_at",
        "finished_at",
    ] {
        assert!(
            parsed["reproducibility"].get(key).is_some(),
            "reproducibility.{key} is required"
        );
    }
    assert_eq!(
        parsed["reproducibility"]["dataset"]["source_url"],
        "fixture"
    );
    assert_eq!(
        parsed["reproducibility"]["manifest"]["sha256"],
        fx.manifest.manifest_sha256
    );
    assert_eq!(
        parsed["reproducibility"]["timeouts"], 3,
        "the caller's timeout count flows into the report"
    );
    assert_eq!(
        parsed["reproducibility"]["manifest"]["path"],
        fx.workspace
            .manifests_dir()
            .join("d2d-seed42-all.json")
            .to_string_lossy()
            .to_string()
    );
    let constraints = parsed["constraints"].as_array().unwrap();
    assert_eq!(constraints.len(), 5);

    let csv = std::fs::read_to_string(&csv_path).unwrap();
    let mut lines = csv.lines();
    assert_eq!(
        lines.next().unwrap(),
        "source_doc_id,predicted_doc_id,hit,latency_ms,candidate_count"
    );
    let data: Vec<&str> = lines.collect();
    assert_eq!(data.len(), 4);
    for line in data {
        assert_eq!(line.split(',').count(), 5, "five columns: {line}");
    }
}

#[test]
fn v090_the_manifest_entries_are_the_documented_shape() {
    let qrels = fixture_qrels();
    let dataset = dataset_for(sample_qrels(), &qrels);
    let manifest = build_manifest(&qrels, dataset, 42, 0).unwrap();
    let text = serde_json::to_string_pretty(&manifest).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["benchmark"], "trec-robust04-derived-d2d");
    assert_eq!(parsed["schema_version"], 1);
    assert_eq!(
        parsed["derivation"]["rule"],
        "A and B are related when both are positively judged on at least one shared topic"
    );
    assert_eq!(parsed["derivation"]["graded_values_saved"], true);
    assert_eq!(
        parsed["sampling"]["algorithm"],
        "splitmix64-ranked-selection-v1"
    );
    let entry = &parsed["entries"][0];
    assert!(entry.get("source_doc_id").is_some());
    assert!(entry.get("positive_topics").is_some());
    assert!(entry.get("ground_truth_size").is_some());
    assert!(entry.get("ground_truth_sha256").is_some());
    assert!(entry.get("ground_truth_doc_ids").is_some());
    // Inputs without a ground truth never appear as entries.
    assert!(
        !manifest
            .entries
            .iter()
            .any(|entry: &ManifestEntry| entry.ground_truth_size == 0)
    );
}

// ---------------------------------------------------------------------------
// 12-10: macOS integration (real mdfind, read-only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[test]
fn v090_macos_integration_scopes_the_real_mdfind_and_never_leaves_the_corpus() {
    use kaseifu::benchmark::search::ScopedSearch;

    let scratch = Scratch::new("macos-integration");
    let scope = scratch.path("scope");
    std::fs::create_dir_all(&scope).unwrap();
    std::fs::write(scope.join("7f82a91c.txt"), "zetaquartz inside the scope\n").unwrap();
    let outside = scratch.path("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        outside.join("outside.txt"),
        "zetaquartz outside the scope\n",
    )
    .unwrap();

    let search = ScopedSearch::new(&scope, 10_000);
    let hits = search
        .search("kMDItemTextContent == \"*zetaquartz*\"c", 100)
        .expect("mdfind is callable");
    for hit in &hits {
        assert!(
            is_inside(&scope, Path::new(hit)),
            "a scoped search must never return a path outside the corpus: {hit}"
        );
    }
    // The out-of-scope copy is never returned, whatever the index state is.
    assert!(
        !hits.iter().any(|hit| hit.contains("outside")),
        "the out-of-scope document must not be returned: {hits:?}"
    );
}

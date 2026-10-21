//! v112 `--keywords`: user keywords as Jev decision material.
//!
//! The flag hands extra words to Jev and to nothing else. These tests pin the
//! contract end to end without any live service:
//!
//! - the wire: a std-only mock HTTP server captures the real
//!   `/v1/systemone` body and checks `user: <KEY>` entries in
//!   `state.semantic_signals`, plus the absence of any new request field;
//! - the boundary: `MockJevIntegration` (through a recording decorator) sees
//!   the keywords on the `DecisionContext` while content signals stay intact;
//! - Discovery isolation: an injected search boundary proves no mdfind query
//!   ever contains a keyword;
//! - priority: a repository rule wins and the report says the keywords were
//!   not used; `--to`/`--select` are rejected at parse time;
//! - the report: text/panel gain a `Keywords` section and `--format json`
//!   gains exactly one additive `keywords` object when (and only when) the
//!   flag is given;
//! - validation and help: empty values are refused, help stays ASCII and
//!   free of the words the whole-help scan forbids.
//!
//! Every fixture is synthetic: candidate directories live under the crate's
//! own `target/` tree, HOME is isolated per run, `mdfind` is faked through
//! `PATH` for CLI runs (or injected as a closure in-process), the Jev server
//! is a local loopback listener, and no test touches a real user repository,
//! the Keychain, the journal of a real home, or the on-device model.

use std::future::Future;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kaseifu::classifier::jev::{JevError, JevIntegration, JevResponse, RealJevIntegration};
use kaseifu::classifier::keywords::{KeywordJevIntegration, USER_KEYWORD_PREFIX};
use kaseifu::classifier::mock_jev::MockJevIntegration;
use kaseifu::classifier::pipeline;
use kaseifu::context::image_metadata::MetadataOptions;
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::DecisionContext;
use kaseifu::models::file_context::{NormalizedFileContext, TargetFile};
use kaseifu::operations::organize::organize_file_with_search;
use kaseifu::policy::decision::{self, Thresholds};
use kaseifu::presentation::renderers::classify as classify_view;
use kaseifu::presentation::renderers::organize as organize_view;
use kaseifu::repo_policy;
use kaseifu::storage::history::OperationHistory;

const DUMMY_KEY: &str = "test-key-not-a-secret-0123456789";
const TARGET_NAME: &str = "keyword-note-2026.txt";

// ---------------------------------------------------------------------------
// Minimal HTTP mock server (same std-only pattern as the adapter tests)
// ---------------------------------------------------------------------------

/// A loopback server that answers each accepted connection with a `choice`
/// envelope selecting D1, and records every raw request it received.
struct MockJevServer {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
    _handle: JoinHandle<()>,
}

impl MockJevServer {
    /// Serve exactly `replies` requests (one per CLI run or adapter call).
    fn start(replies: usize) -> Self {
        let body = r#"{"model":"jev-latest","answers":{"destination":{"type":"choice","choice":"D1","probabilities":{"D1":0.90,"D2":0.05,"NONE":0.05}}}}"#.to_string();
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
        let address = listener.local_addr().expect("mock address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            for _ in 0..replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let request = read_http_request(&mut stream);
                captured.lock().expect("request log").push(request);
                write_http_response(&mut stream, 200, &body);
            }
        });
        Self {
            endpoint: format!("http://{address}"),
            requests,
            _handle: handle,
        }
    }

    /// The raw requests received so far (headers plus body).
    fn captured(&self) -> Vec<String> {
        self.requests.lock().expect("request log").clone()
    }
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        if let Some(header_end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..header_end]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if data.len() >= header_end + 4 + content_length {
                break;
            }
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => data.extend_from_slice(&buffer[..read]),
        }
    }
    String::from_utf8_lossy(&data).into_owned()
}

fn write_http_response(stream: &mut TcpStream, code: u16, body: &str) {
    let response = format!(
        "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// The JSON body of one captured raw HTTP request.
fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn request_json(raw: &str) -> serde_json::Value {
    serde_json::from_str(request_body(raw)).expect("request body is one JSON document")
}

fn semantic_signals(value: &serde_json::Value) -> Vec<String> {
    value["state"]["semantic_signals"]
        .as_array()
        .expect("state.semantic_signals is an array")
        .iter()
        .map(|entry| entry.as_str().expect("signal is a string").to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// In-process fixtures
// ---------------------------------------------------------------------------

/// One synthetic tree under the crate's `target/` directory (never `$HOME`,
/// never a real user file), removed when the test ends.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("v112-keywords-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("sandbox root");
        Self { root }
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(&path).expect("sandbox directory");
        path
    }

    fn file(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sandbox directory");
        }
        std::fs::write(&path, b"fixture").expect("sandbox file");
        path
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A context shaped like production: filename words ride as content signals.
fn normalized_with_signal(filename: &str, extension: &str, signal: &str) -> NormalizedFileContext {
    NormalizedFileContext {
        target: TargetFile {
            filename: filename.to_string(),
            extension: Some(extension.to_string()),
            ..TargetFile::default()
        },
        semantic_signals: vec![signal.to_string()],
        ..NormalizedFileContext::default()
    }
}

fn one_candidate() -> Vec<CandidateDestination> {
    vec![CandidateDestination {
        id: "D1".to_string(),
        path: PathBuf::from("/tmp/v112-candidate-dir"),
        score: 12.0,
        evidence_count: 2,
        reasons: vec!["found 2 items".to_string()],
    }]
}

fn keywords(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// A Jev mock that records the context it was handed, then delegates to the
/// production `MockJevIntegration` (no business logic of its own).
struct RecordingJev {
    inner: MockJevIntegration,
    seen: Mutex<Vec<DecisionContext>>,
}

impl RecordingJev {
    fn new(inner: MockJevIntegration) -> Self {
        Self {
            inner,
            seen: Mutex::new(Vec::new()),
        }
    }

    fn recorded(&self) -> Vec<DecisionContext> {
        self.seen.lock().expect("recorder").clone()
    }
}

impl JevIntegration for RecordingJev {
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>> {
        self.seen.lock().expect("recorder").push(context.clone());
        self.inner.decide(context)
    }
}

/// D1 with a decisive margin (auto-move under the default thresholds).
fn jev_selecting_d1() -> MockJevIntegration {
    let mut probabilities = std::collections::HashMap::new();
    probabilities.insert("D1".to_string(), 0.95);
    probabilities.insert("D2".to_string(), 0.03);
    MockJevIntegration::configured_decision(JevResponse {
        destination_id: Some("D1".to_string()),
        probabilities,
        confidence: 0.0,
        margin: 0.0,
    })
}

// ---------------------------------------------------------------------------
// B1-B2: the wire contract (mock HTTP server, no live Jev)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b1_keywords_reach_the_wire_as_user_prefixed_semantic_signals() {
    let server = MockJevServer::start(1);
    let normalized = normalized_with_signal("report-2026.pdf", "pdf", "X:report");
    let context = DecisionContext::from_normalized(&normalized, &one_candidate())
        .with_user_keywords(&keywords(&["invoice", "receipt"]));

    let _ = RealJevIntegration::new(server.endpoint.clone(), None)
        .decide(&context)
        .await
        .expect("the mock server answers");

    let captured = server.captured();
    assert_eq!(captured.len(), 1, "one request per decision");
    let value = request_json(&captured[0]);

    // The keywords ride the existing channel with the reserved prefix, and
    // the content signal rides along untouched.
    let signals = semantic_signals(&value);
    assert!(
        signals.contains(&"user: invoice".to_string()),
        "got {signals:?}"
    );
    assert!(
        signals.contains(&"user: receipt".to_string()),
        "got {signals:?}"
    );
    assert!(signals.contains(&"X:report".to_string()), "got {signals:?}");

    // No new top-level field: the verified contract is state/model/questions.
    let object = value.as_object().expect("request is an object");
    let mut names: Vec<&str> = object.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["model", "questions", "state"],
        "no new request field"
    );
}

#[tokio::test]
async fn b2_a_run_without_keywords_sends_no_user_entries_and_no_new_field() {
    let server = MockJevServer::start(1);
    let normalized = normalized_with_signal("report-2026.pdf", "pdf", "X:report");
    let context = DecisionContext::from_normalized(&normalized, &one_candidate());

    let _ = RealJevIntegration::new(server.endpoint.clone(), None)
        .decide(&context)
        .await
        .expect("the mock server answers");

    let captured = server.captured();
    assert_eq!(captured.len(), 1);
    let value = request_json(&captured[0]);
    let signals = semantic_signals(&value);
    assert_eq!(signals, vec!["X:report".to_string()], "payload unchanged");
    assert!(
        !signals
            .iter()
            .any(|signal| signal.starts_with(USER_KEYWORD_PREFIX)),
        "no user entries without the flag: {signals:?}"
    );
    assert!(
        value.get("keywords").is_none(),
        "no keywords field on the wire"
    );
}

// ---------------------------------------------------------------------------
// B3: the boundary decorator (MockJevIntegration observes the context)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b3_the_boundary_decorator_attaches_keywords_and_keeps_content_signals() {
    let recorder = RecordingJev::new(jev_selecting_d1());
    let supplied = keywords(&["invoice"]);
    let decorated = KeywordJevIntegration::new(&recorder, &supplied);
    let normalized = normalized_with_signal("report-2026.pdf", "pdf", "X:report");
    let context = DecisionContext::from_normalized(&normalized, &one_candidate());

    let response = decorated.decide(&context).await.expect("mock responds");

    // The mock answers verbatim: the decorator changes the input only.
    assert_eq!(response.destination_id.as_deref(), Some("D1"));

    let recorded = recorder.recorded();
    assert_eq!(recorded.len(), 1, "Jev was consulted exactly once");
    assert_eq!(recorded[0].user_keywords(), supplied.as_slice());
    assert_eq!(
        recorded[0].semantic_signals(),
        normalized.semantic_signals.as_slice(),
        "content signals stay exactly as derived"
    );
    assert!(
        context.user_keywords().is_empty(),
        "the pipeline's context is not mutated"
    );

    let delivery = decorated.delivery();
    assert_eq!(delivery.provided, supplied);
    assert_eq!(delivery.sent, vec!["user: invoice"]);
}

// ---------------------------------------------------------------------------
// B4: Jev gets the keywords; Discovery never does
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b4_keywords_reach_jev_but_never_the_discovery_query() {
    let sandbox = Sandbox::new("discovery-isolation");
    let target = sandbox.file("inbox/keyword-note-2026.txt");
    let alpha = sandbox.file("alpha/similar-note.txt");
    let beta = sandbox.file("beta/similar-draft.txt");
    let hits = vec![
        alpha.to_string_lossy().into_owned(),
        beta.to_string_lossy().into_owned(),
    ];
    let queries = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded = Arc::clone(&queries);
    let search = move |query: &str, _limit: usize| {
        recorded.lock().expect("query log").push(query.to_string());
        Ok(hits.clone())
    };

    let supplied = keywords(&["invoice"]);
    let mock = jev_selecting_d1();
    let jev = KeywordJevIntegration::new(&mock, &supplied);
    let history = OperationHistory::open_in_memory().expect("journal");

    let mut report = organize_file_with_search(
        &target,
        &jev,
        &history,
        false,
        Thresholds::default(),
        &MetadataOptions::default(),
        search,
    )
    .await
    .expect("organize runs");
    // Same wiring as the CLI: the report learns what the decorator sent.
    report.keywords = jev.delivery();

    // The search really ran, and no query ever saw a keyword.
    let queries = queries.lock().expect("query log").clone();
    assert!(
        !queries.is_empty(),
        "discovery must run for this test to mean anything"
    );
    for query in &queries {
        assert!(
            !query.contains("invoice"),
            "keyword leaked into query: {query}"
        );
        assert!(
            !query.contains(USER_KEYWORD_PREFIX),
            "prefix leaked: {query}"
        );
    }

    // Jev was consulted and the report knows exactly what it received.
    assert_eq!(report.keywords.provided, supplied);
    assert_eq!(report.keywords.sent, vec!["user: invoice"]);

    let plain =
        organize_view::render_text_with(&report, &target, false, Thresholds::default(), false);
    assert!(plain.contains("== Keywords =="), "got:\n{plain}");
    assert!(plain.contains("Provided"), "got:\n{plain}");
    assert!(plain.contains("user: invoice"), "got:\n{plain}");
    assert!(plain.is_ascii(), "output stays ASCII");

    let panel = organize_view::render_panel(&report, &target, false, false);
    assert!(panel.contains("KEYWORDS"), "got:\n{panel}");

    let json = organize_view::json_report(&report, &target, false, None);
    assert_eq!(json["keywords"]["provided"], serde_json::json!(["invoice"]));
    assert_eq!(
        json["keywords"]["sent_to_jev"],
        serde_json::json!(["user: invoice"])
    );
    assert!(target.exists(), "a dry run never moves the file");
}

// ---------------------------------------------------------------------------
// B5: the repository rule wins; the report says the keywords were not used
// ---------------------------------------------------------------------------

fn rule_sandbox(tag: &str) -> Sandbox {
    let sandbox = Sandbox::new(tag);
    sandbox.dir("repo/sorted");
    let policy = sandbox
        .root
        .join("repo")
        .join(repo_policy::POLICY_FILE_NAME);
    std::fs::write(
        &policy,
        "[[rule]]\nname = \"keyword-override\"\nmatch_glob = \"**\"\ndestination = \"sorted\"\n",
    )
    .expect("policy file");
    sandbox
}

/// One CLI run in an isolated HOME with no key and no network endpoint.
fn run_cli(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .current_dir(cwd)
        .env("HOME", home)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env("NO_COLOR", "1")
        .env_remove("JEV_API_KEY")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR")
        .stdin(Stdio::null())
        .args(args);
    command.output().expect("kaseifu runs")
}

#[test]
fn b5_a_rule_hit_wins_and_the_report_marks_the_keywords_unused() {
    let sandbox = rule_sandbox("rule-wins");
    let file = sandbox.file("repo/content/note.md");
    let home = sandbox.dir("home");
    let cwd = sandbox.root.join("repo");
    let file_arg = file.to_string_lossy().into_owned();

    // Repeated-flag form plus edge-trimmed values; no API key anywhere, so a
    // successful run also proves Jev was never consulted.
    let json_run = run_cli(
        &home,
        &cwd,
        &[
            "organize",
            &file_arg,
            "--format",
            "json",
            "--keywords",
            "  invoice  ",
            "--keywords",
            "receipt",
        ],
    );
    let stderr = String::from_utf8_lossy(&json_run.stderr).into_owned();
    assert!(json_run.status.success(), "got:\n{stderr}");
    let document: serde_json::Value =
        serde_json::from_slice(&json_run.stdout).expect("one JSON document");
    assert_eq!(document["rule"]["name"], "keyword-override");
    assert_eq!(
        document["keywords"]["provided"],
        serde_json::json!(["invoice", "receipt"]),
        "values are trimmed and accumulated in order"
    );
    assert_eq!(
        document["keywords"]["sent_to_jev"],
        serde_json::json!([]),
        "the rule decided; nothing reached Jev"
    );
    assert_eq!(document["outcome"]["type"], "would_move");
    assert_eq!(document["mode"], "dry_run");
    assert!(file.exists(), "the default is a dry run");

    let text_run = run_cli(
        &home,
        &cwd,
        &["organize", &file_arg, "--keywords", "invoice"],
    );
    let stdout = String::from_utf8_lossy(&text_run.stdout).into_owned();
    assert!(text_run.status.success(), "got:\n{stdout}");
    assert!(stdout.contains("keyword-override"), "got:\n{stdout}");
    assert!(stdout.contains("== Keywords =="), "got:\n{stdout}");
    assert!(stdout.contains("Provided"), "got:\n{stdout}");
    assert!(
        stdout.contains("not used (Jev was not consulted)"),
        "got:\n{stdout}"
    );
    assert!(stdout.is_ascii(), "output stays ASCII");
}

// ---------------------------------------------------------------------------
// B6-B7: the CLI classifier path with and without the flag
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-v112-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

/// Fixture root under the worktree target dir: candidate folders must not be
/// system-root paths (the same rule as the v0.7.0 CLI fixture).
fn fixture_root(name: &str) -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    dir.push(format!("v112-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture root");
    dir
}

struct Fixture {
    home: PathBuf,
    inbox: PathBuf,
    target: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
    fake_bin: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = fixture_root(name);
        let home = temp_dir(&format!("home-{name}"));
        let inbox = dir.join("inbox");
        let alpha = dir.join("dest-alpha");
        let beta = dir.join("dest-beta");
        let fake_bin = temp_dir(&format!("bin-{name}"));
        for folder in [&inbox, &alpha, &beta] {
            std::fs::create_dir_all(folder).expect("fixture folder");
        }
        let target = inbox.join(TARGET_NAME);
        std::fs::write(&target, "hello from the v112 keywords test").expect("target file");
        std::fs::write(alpha.join("keyword-note-copy.txt"), "a").expect("alpha evidence");
        std::fs::write(beta.join("keyword-note-draft.txt"), "b").expect("beta evidence");
        let fixture = Self {
            home,
            inbox,
            target,
            alpha,
            beta,
            fake_bin,
        };
        fixture.write_fake_mdfind(&[
            fixture
                .alpha
                .join("keyword-note-copy.txt")
                .display()
                .to_string(),
            fixture
                .beta
                .join("keyword-note-draft.txt")
                .display()
                .to_string(),
        ]);
        fixture
    }

    /// One executable `mdfind` stand-in that prints the given hits.
    fn write_fake_mdfind(&self, hits: &[String]) {
        let mut script = String::from("#!/bin/sh\n");
        for hit in hits {
            script.push_str(&format!(
                "printf '%s\\n' '{}'\n",
                hit.replace('\'', "'\\''")
            ));
        }
        let path = self.fake_bin.join("mdfind");
        std::fs::write(&path, script).expect("fake mdfind");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fake mdfind executable");
    }
}

/// Run the real binary against the fixture and the loopback Jev server.
fn run_with_server(
    fixture: &Fixture,
    server: &MockJevServer,
    args: &[&str],
    key: Option<&str>,
) -> Output {
    let path = format!(
        "{}:{}",
        fixture.fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .args(args)
        .current_dir(&fixture.inbox)
        .env("HOME", &fixture.home)
        .env("PATH", path)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env("NO_COLOR", "1")
        .env("JEV_API_URL", &server.endpoint)
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR");
    match key {
        Some(key) => command.env("JEV_API_KEY", key),
        None => command.env_remove("JEV_API_KEY"),
    };

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("kaseifu spawns");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("collect output");
                panic!(
                    "kaseifu exceeded 30s: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    child.wait_with_output().expect("collect output")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json_of(output: &Output) -> serde_json::Value {
    serde_json::from_str(stdout_of(output).trim()).unwrap_or_else(|error| {
        panic!(
            "stdout must be one JSON document ({error}): {}\nstderr: {}",
            stdout_of(output),
            stderr_of(output)
        )
    })
}

#[test]
fn b6_cli_organize_sends_keywords_to_jev_and_reports_them() {
    let fixture = Fixture::new("organize-with-keywords");
    let server = MockJevServer::start(2);
    let target = fixture.target.to_string_lossy().into_owned();

    // Multi-value form of the flag (the requested `--keywords KEY KEY`).
    let json_run = run_with_server(
        &fixture,
        &server,
        &[
            "organize",
            &target,
            "--format",
            "json",
            "--keywords",
            "invoice",
            "receipt",
        ],
        Some(DUMMY_KEY),
    );
    assert!(
        json_run.status.success(),
        "stderr: {}",
        stderr_of(&json_run)
    );
    let json = json_of(&json_run);
    assert_eq!(
        json["keywords"]["provided"],
        serde_json::json!(["invoice", "receipt"])
    );
    assert_eq!(
        json["keywords"]["sent_to_jev"],
        serde_json::json!(["user: invoice", "user: receipt"])
    );
    // Add-only: every historical key keeps its presence and type.
    for key in [
        "source",
        "mode",
        "decision",
        "policy",
        "candidate_path",
        "target_indexed",
        "outcome",
    ] {
        assert!(json.get(key).is_some(), "existing key {key} stays present");
    }
    assert_eq!(json["decision"]["destination_id"], "D1");
    assert_eq!(json["mode"], "dry_run");
    assert!(fixture.target.exists(), "a dry run never moves the file");

    // The text report says the same thing in words.
    let text_run = run_with_server(
        &fixture,
        &server,
        &["organize", &target, "--keywords", "invoice", "receipt"],
        Some(DUMMY_KEY),
    );
    assert!(
        text_run.status.success(),
        "stderr: {}",
        stderr_of(&text_run)
    );
    let stdout = stdout_of(&text_run);
    assert!(stdout.contains("== Keywords =="), "got:\n{stdout}");
    assert!(
        stdout.contains("user: invoice, user: receipt"),
        "got:\n{stdout}"
    );
    assert!(stdout.is_ascii(), "output stays ASCII");

    // And the server saw the entries on the wire, twice.
    let captured = server.captured();
    assert_eq!(captured.len(), 2, "both runs reached Jev");
    for raw in &captured {
        let signals = semantic_signals(&request_json(raw));
        assert!(
            signals.contains(&"user: invoice".to_string()),
            "got {signals:?}"
        );
        assert!(
            signals.contains(&"user: receipt".to_string()),
            "got {signals:?}"
        );
    }
}

#[test]
fn b7_cli_organize_without_keywords_is_unchanged_in_shape_and_payload() {
    let fixture = Fixture::new("organize-no-keywords");
    let server = MockJevServer::start(2);
    let target = fixture.target.to_string_lossy().into_owned();

    let json_run = run_with_server(
        &fixture,
        &server,
        &["organize", &target, "--format", "json"],
        Some(DUMMY_KEY),
    );
    assert!(
        json_run.status.success(),
        "stderr: {}",
        stderr_of(&json_run)
    );
    let json = json_of(&json_run);
    assert!(
        json.get("keywords").is_none(),
        "no keywords key when the flag was not given: {json}"
    );
    // Exact historical key set: nothing was added, nothing renamed.
    let mut names: Vec<&str> = json
        .as_object()
        .expect("one object")
        .keys()
        .map(|k| k.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "candidate_path",
            "decision",
            "mode",
            "outcome",
            "policy",
            "source",
            "target_indexed"
        ]
    );

    let text_run = run_with_server(&fixture, &server, &["organize", &target], Some(DUMMY_KEY));
    assert!(
        text_run.status.success(),
        "stderr: {}",
        stderr_of(&text_run)
    );
    let stdout = stdout_of(&text_run);
    assert!(!stdout.contains("== Keywords =="), "got:\n{stdout}");
    assert!(!stdout.contains("user: "), "got:\n{stdout}");

    let captured = server.captured();
    assert_eq!(captured.len(), 2);
    for raw in &captured {
        let signals = semantic_signals(&request_json(raw));
        assert!(
            !signals
                .iter()
                .any(|signal| signal.starts_with(USER_KEYWORD_PREFIX)),
            "no user entries without the flag: {signals:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// B8-B10: parse-time behavior (conflicts, validation, help)

// ---------------------------------------------------------------------------

/// A run with an isolated HOME for the parse-time tests (no fixture, no
/// server: clap rejects these before anything is read or contacted).
fn run_raw(args: &[&str]) -> Output {
    let home = temp_dir(&format!("raw-home-{}", args.join("-").replace('/', "_")));
    run_cli(&home, &home, args)
}

#[test]
fn b8_to_and_select_conflict_with_keywords() {
    // `--to` takes a destination; `--select` is a bare flag.
    for extra in [vec!["--to", "/tmp/v112-destination"], vec!["--select"]] {
        let mut args = vec!["organize", "any-file.txt"];
        args.extend(extra.iter().copied());
        args.push("--keywords");
        args.push("invoice");
        let output = run_raw(&args);
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(2), "got:\n{stderr}");
        assert!(stderr.contains("cannot be used with"), "got:\n{stderr}");
        assert!(stderr.contains("--keywords"), "got:\n{stderr}");
        assert!(stderr.contains(extra[0]), "got:\n{stderr}");
    }
}

#[test]
fn b9_empty_and_whitespace_keywords_are_rejected() {
    for value in ["", "   "] {
        let output = run_raw(&["organize", "any-file.txt", "--keywords", value]);
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(2), "got:\n{stderr}");
        assert!(
            stderr.contains("keyword must not be empty"),
            "got:\n{stderr}"
        );
    }

    // A value without a following keyword is also a parse error.
    let output = run_raw(&["organize", "any-file.txt", "--keywords"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        stderr_of(&output)
    );
}

#[test]
fn b10_help_documents_keywords_and_stays_clean() {
    let forbidden = [
        "Phase",
        "FileContext",
        "NormalizedFileContext",
        "JevDecision",
        "DecisionContext",
        "CandidateDestination",
        "OrganizeReport",
        "MovePlan",
        "PlanError",
        "Observe ->",
        "decision -> policy",
        "policy -> plan",
        "DETAILS layer",
    ];

    let organize_help = stdout_of(&run_raw(&["organize", "--help"]));
    assert!(
        organize_help.contains("--keywords"),
        "got:\n{organize_help}"
    );
    assert!(
        organize_help.contains("not search terms"),
        "the Discovery decision is stated:\n{organize_help}"
    );
    assert!(
        organize_help.contains("cannot be combined with --select or --to"),
        "the conflicts are stated:\n{organize_help}"
    );
    assert!(organize_help.is_ascii(), "help stays ASCII");

    let classify_help = stdout_of(&run_raw(&["classify", "--help"]));
    assert!(
        classify_help.contains("--keywords"),
        "got:\n{classify_help}"
    );
    assert!(
        classify_help.contains("not search terms"),
        "got:\n{classify_help}"
    );
    assert!(classify_help.is_ascii(), "help stays ASCII");

    for text in [&organize_help, &classify_help] {
        for word in forbidden {
            assert!(!text.contains(word), "help leaks {word}:\n{text}");
        }
    }
}

// ---------------------------------------------------------------------------
// B11: `classify --keywords` on the same code path
// ---------------------------------------------------------------------------

#[test]
fn b11_classify_sends_keywords_and_gains_only_an_additive_json_key() {
    let fixture = Fixture::new("classify-keywords");
    let server = MockJevServer::start(2);
    let target = fixture.target.to_string_lossy().into_owned();

    let verbose_run = run_with_server(
        &fixture,
        &server,
        &[
            "classify",
            &target,
            "--keywords",
            "invoice",
            "--verbose",
            "--format",
            "json",
        ],
        Some(DUMMY_KEY),
    );
    assert!(
        verbose_run.status.success(),
        "stderr: {}",
        stderr_of(&verbose_run)
    );
    let json = json_of(&verbose_run);
    assert_eq!(json["destination_id"], "D1", "existing keys stay");
    assert_eq!(json["confidence"], 0.90);
    assert_eq!(json["keywords"]["provided"], serde_json::json!(["invoice"]));
    assert_eq!(
        json["keywords"]["sent_to_jev"],
        serde_json::json!(["user: invoice"])
    );
    // The verbose view (stderr) names the user input next to the signals.
    let stderr = stderr_of(&verbose_run);
    assert!(stderr.contains("== User keywords =="), "got:\n{stderr}");
    assert!(stderr.contains("  invoice"), "got:\n{stderr}");

    let plain_run = run_with_server(
        &fixture,
        &server,
        &["classify", &target, "--format", "json"],
        Some(DUMMY_KEY),
    );
    assert!(
        plain_run.status.success(),
        "stderr: {}",
        stderr_of(&plain_run)
    );
    let json = json_of(&plain_run);
    assert!(
        json.get("keywords").is_none(),
        "no keywords key when the flag was not given: {json}"
    );
    assert_eq!(json["destination_id"], "D1");

    let captured = server.captured();
    assert_eq!(captured.len(), 2);
    let with_keywords = semantic_signals(&request_json(&captured[0]));
    assert!(
        with_keywords.contains(&"user: invoice".to_string()),
        "got {with_keywords:?}"
    );
    let without = semantic_signals(&request_json(&captured[1]));
    assert!(
        !without
            .iter()
            .any(|signal| signal.starts_with(USER_KEYWORD_PREFIX)),
        "got {without:?}"
    );
}

// ---------------------------------------------------------------------------
// B12: the classify panel carries the section too
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b12_the_classify_panel_shows_the_keywords_section() {
    let sandbox = Sandbox::new("classify-panel");
    let target = sandbox.file("inbox/keyword-note-2026.txt");
    let alpha = sandbox.file("alpha/similar-note.txt");
    let hits = vec![alpha.to_string_lossy().into_owned()];
    let search = move |_query: &str, _limit: usize| Ok(hits.clone());

    let supplied = keywords(&["invoice", "receipt"]);
    let mock = jev_selecting_d1();
    let jev = KeywordJevIntegration::new(&mock, &supplied);

    let mut report = pipeline::classify_report_with_search(&target, &jev, search)
        .await
        .expect("classify runs");
    report.keywords = jev.delivery();

    let panel = classify_view::render_panel(
        &report,
        &decision::evaluate(&report.decision),
        false,
        false,
        false,
    );
    assert!(panel.contains("KEYWORDS"), "got:\n{panel}");
    assert!(panel.contains("Provided"), "got:\n{panel}");
    assert!(
        panel.contains("user: invoice, user: receipt"),
        "got:\n{panel}"
    );
    assert!(panel.is_ascii(), "panel stays ASCII");
}

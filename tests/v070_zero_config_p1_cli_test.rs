//! v0.7.0 P1 ZC-5 / ZC-7 tests: real CLI output for near/`--explain` and
//! `organize --format json`.
//!
//! Everything is deterministic and local: the Jev decision comes from a
//! std-only HTTP mock server on 127.0.0.1, and Discovery's `mdfind` is
//! injected through PATH. Candidate directories are created under
//! `CARGO_TARGET_TMPDIR` because a temp-dir path (`/var/folders/...`) is a
//! blocked destination by design; HOME and the working directory are
//! isolated so the developer's journal, config and Keychain are never
//! touched. The isolated HOME keeps the journal under the fixture root.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const DUMMY_KEY: &str = "test-key-not-a-secret-0123456789";
const TARGET_NAME: &str = "vacation-note-2026.txt";

// ---------------------------------------------------------------------------
// Fixture workspace (candidate directories must not be system-root paths)
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-p1-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

/// Fixture root under the worktree target dir: `is_excluded_destination`
/// rejects `/var/folders/...`, so candidate folders cannot live in
/// `std::env::temp_dir()`.
fn fixture_root(name: &str) -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    dir.push(format!("p1-cli-{}-{name}", std::process::id()));
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
        std::fs::write(&target, "hello from the p1 cli test").expect("target file");
        // Similar files that make the two folders candidates.
        std::fs::write(alpha.join("vacation-note-copy.txt"), "a").expect("alpha evidence");
        std::fs::write(beta.join("vacation-note-draft.txt"), "b").expect("beta evidence");
        let fixture = Self {
            home,
            inbox,
            target,
            alpha,
            beta,
            fake_bin,
        };
        fixture.write_fake_mdfind(&[
            // An excluded hit first: its parent (`~/Library/...`) must be
            // dropped before the filesystem is ever touched, so the path
            // deliberately does not exist.
            "/Users/nobody/Library/Application Support/SeedStore/vacation-note.txt".to_string(),
            fixture
                .alpha
                .join("vacation-note-copy.txt")
                .display()
                .to_string(),
            fixture
                .beta
                .join("vacation-note-draft.txt")
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

    fn journal_path(&self) -> PathBuf {
        self.home
            .join("Library/Application Support/kaseifu/kaseifu.db")
    }
}

// ---------------------------------------------------------------------------
// Minimal HTTP mock server (same std-only pattern as the adapter tests)
// ---------------------------------------------------------------------------

struct MockJevServer {
    endpoint: String,
    _handle: JoinHandle<()>,
}

impl MockJevServer {
    /// Answer every accepted connection with a `choice` envelope that selects
    /// `choice` with the given probability distribution.
    fn start(choice: &str, probabilities: &str) -> Self {
        let body = format!(
            r#"{{"model":"jev-latest","answers":{{"destination":{{"type":"choice","choice":"{choice}","probabilities":{probabilities}}}}}}}"#
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
        let address = listener.local_addr().expect("mock address");
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let _ = read_http_request(&mut stream);
                write_http_response(&mut stream, 200, &body);
            }
        });
        Self {
            endpoint: format!("http://{address}"),
            _handle: handle,
        }
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

// ---------------------------------------------------------------------------
// Running the real binary
// ---------------------------------------------------------------------------

fn run(
    fixture: &Fixture,
    server: &MockJevServer,
    args: &[&str],
    key: Option<&str>,
    timeout: Duration,
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
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("collect output");
                panic!(
                    "kaseifu exceeded {timeout:?}: {} {}",
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

// ---------------------------------------------------------------------------
// ZC-7: organize --format json
// ---------------------------------------------------------------------------

#[test]
fn organize_json_reports_decision_policy_and_would_move_outcome() {
    let fixture = Fixture::new("json-dry-run");
    let server = MockJevServer::start("D1", r#"{"D1":0.90,"D2":0.05,"NONE":0.05}"#);
    let output = run(
        &fixture,
        &server,
        &[
            "organize",
            fixture.target.to_str().expect("utf-8"),
            "--format",
            "json",
        ],
        Some(DUMMY_KEY),
        Duration::from_secs(30),
    );

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(json["mode"], "dry_run");
    assert_eq!(json["source"], fixture.target.to_string_lossy().as_ref());

    // decision: the existing JevDecision shape.
    assert_eq!(json["decision"]["destination_id"], "D1");
    let confidence = json["decision"]["confidence"].as_f64().expect("confidence");
    let margin = json["decision"]["margin"].as_f64().expect("margin");
    assert!((confidence - 0.90).abs() < 1e-9, "{confidence}");
    assert!((margin - 0.85).abs() < 1e-9, "{margin}");
    assert_eq!(json["decision"]["action"]["move_to"], "D1");

    // policy: the Rust verdict, machine-readable.
    assert_eq!(json["policy"]["auto_move"], "D1");

    // outcome: dry run, destination resolved from the discovered candidate.
    let candidate_path = json["candidate_path"].as_str().expect("candidate_path");
    assert!(
        candidate_path == fixture.alpha.to_string_lossy()
            || candidate_path == fixture.beta.to_string_lossy(),
        "unexpected candidate: {candidate_path}"
    );
    assert_eq!(json["outcome"]["type"], "would_move");
    let destination = json["outcome"]["destination"]
        .as_str()
        .expect("destination");
    assert_eq!(
        Path::new(destination),
        Path::new(candidate_path).join(TARGET_NAME).as_path()
    );
    assert!(fixture.target.exists(), "a dry run never moves the file");
    assert!(
        !Path::new(destination).exists(),
        "nothing is created at the destination"
    );
}

#[test]
fn organize_json_execute_reports_a_moved_outcome_and_journals_it() {
    let fixture = Fixture::new("json-execute");
    let server = MockJevServer::start("D1", r#"{"D1":0.90,"D2":0.05,"NONE":0.05}"#);
    let output = run(
        &fixture,
        &server,
        &[
            "organize",
            fixture.target.to_str().expect("utf-8"),
            "--execute",
            "--format",
            "json",
        ],
        Some(DUMMY_KEY),
        Duration::from_secs(30),
    );

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(json["mode"], "execute");
    assert_eq!(json["outcome"]["type"], "moved");
    let destination = json["outcome"]["destination"]
        .as_str()
        .expect("destination");
    assert!(Path::new(destination).exists(), "the file moved");
    assert!(!fixture.target.exists(), "the source is gone");
    assert!(
        json["outcome"]["operation_id"].is_string(),
        "the journal id is reported: {json}"
    );

    let inspection = kaseifu::storage::history::inspect_journal(&fixture.journal_path())
        .expect("read journal")
        .expect("journal exists");
    assert_eq!(inspection.total_operations, 1);
    assert!(inspection.pending.is_empty());
}

#[test]
fn organize_json_refusal_keeps_the_file_and_says_why() {
    let fixture = Fixture::new("json-review");
    // 0.70 confidence: REVIEW, no move allowed.
    let server = MockJevServer::start("D1", r#"{"D1":0.70,"D2":0.10,"NONE":0.20}"#);
    let output = run(
        &fixture,
        &server,
        &[
            "organize",
            fixture.target.to_str().expect("utf-8"),
            "--format",
            "json",
        ],
        Some(DUMMY_KEY),
        Duration::from_secs(30),
    );

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(json["policy"]["review"], "D1");
    assert_eq!(json["outcome"]["type"], "unplanable");
    assert_eq!(json["outcome"]["kind"], "not_auto_move");
    assert_eq!(
        json["outcome"]["reason"],
        "policy refused: automatic move not allowed"
    );
    assert!(fixture.target.exists(), "a refusal keeps the file");
}

// ---------------------------------------------------------------------------
// ZC-5: near-threshold display and --explain through the real CLI
// ---------------------------------------------------------------------------

#[test]
fn organize_plain_text_shows_the_near_miss_line_and_explanation() {
    let fixture = Fixture::new("explain-near");
    // 0.840 with margin 0.500: REVIEW, 0.010 below the confidence threshold.
    let server = MockJevServer::start("D1", r#"{"D1":0.84,"D2":0.34,"NONE":0.01}"#);
    let output = run(
        &fixture,
        &server,
        &[
            "organize",
            fixture.target.to_str().expect("utf-8"),
            "--explain",
        ],
        Some(DUMMY_KEY),
        Duration::from_secs(30),
    );

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(
        text.contains("near auto move: 0.010 more confidence would reach AUTO MOVE"),
        "got:\n{text}"
    );
    assert!(text.contains("== Explanation =="), "got:\n{text}");
    assert!(
        text.contains("Thresholds   auto move: confidence >= 0.850 and margin >= 0.400; review: confidence >= 0.600"),
        "got:\n{text}"
    );
    assert!(text.contains("Verdict      REVIEW"), "got:\n{text}");
}

#[test]
fn organize_json_explain_adds_the_explanation_object() {
    let fixture = Fixture::new("explain-json");
    let server = MockJevServer::start("D1", r#"{"D1":0.84,"D2":0.34,"NONE":0.01}"#);
    let output = run(
        &fixture,
        &server,
        &[
            "organize",
            fixture.target.to_str().expect("utf-8"),
            "--format",
            "json",
            "--explain",
        ],
        Some(DUMMY_KEY),
        Duration::from_secs(30),
    );

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(json["policy"]["review"], "D1");
    let explanation = &json["explanation"];
    assert_eq!(explanation["policy"], "review");
    assert_eq!(explanation["near_auto_move"], true);
    assert_eq!(explanation["thresholds"]["auto_move_confidence"], 0.85);
    assert_eq!(explanation["destination_id"], "D1");
}

#[test]
fn classify_json_explain_is_additive_only() {
    let fixture = Fixture::new("classify-json");
    let server = MockJevServer::start("D1", r#"{"D1":0.84,"D2":0.34,"NONE":0.01}"#);
    let plain = run(
        &fixture,
        &server,
        &["classify", fixture.target.to_str().expect("utf-8")],
        Some(DUMMY_KEY),
        Duration::from_secs(30),
    );
    assert!(plain.status.success(), "stderr: {}", stderr_of(&plain));
    let plain_json = json_of(&plain);
    assert!(
        plain_json.get("explanation").is_none(),
        "the default JSON stays as it was: {plain_json}"
    );
    for key in ["destination_id", "confidence", "margin", "action"] {
        assert!(plain_json.get(key).is_some(), "{key} missing: {plain_json}");
    }

    let server = MockJevServer::start("D1", r#"{"D1":0.84,"D2":0.34,"NONE":0.01}"#);
    let explained = run(
        &fixture,
        &server,
        &[
            "classify",
            fixture.target.to_str().expect("utf-8"),
            "--explain",
        ],
        Some(DUMMY_KEY),
        Duration::from_secs(30),
    );
    assert!(
        explained.status.success(),
        "stderr: {}",
        stderr_of(&explained)
    );
    let explained_json = json_of(&explained);
    for key in ["destination_id", "confidence", "margin", "action"] {
        assert!(
            explained_json.get(key).is_some(),
            "{key} must stay: {explained_json}"
        );
    }
    assert_eq!(explained_json["explanation"]["policy"], "review");
    assert_eq!(explained_json["explanation"]["near_auto_move"], true);
    assert_eq!(
        explained_json["explanation"]["thresholds"]["review_confidence"],
        0.60
    );
    assert!(
        !stdout_of(&explained).contains(DUMMY_KEY),
        "the key never reaches stdout"
    );
}

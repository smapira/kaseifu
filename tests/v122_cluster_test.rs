//! v0.13.2 issue #26: `kaseifu cluster`.
//!
//! The command groups a directory's flat files without a model, names the groups
//! with deterministic candidates (the model only picks among them), and files
//! them through the #25 journal batch, so `undo --batch` puts the directory back
//! exactly as it was.
//!
//! Sandboxes live under `target/`, never under the system temp directory:
//! `is_excluded_destination` refuses anything under `/var`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

fn sandbox(tag: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-sandboxes")
        .join(format!("v122-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("sandbox");
    root
}

fn home_of(root: &Path) -> PathBuf {
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    home
}

fn run_with_env(home: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env("NO_COLOR", "1")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_ENDPOINT")
        .env_remove("JEV_API_KEY")
        .env_remove("KASEIFU_DEBUG");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run kaseifu")
}

fn run(home: &Path, args: &[&str]) -> Output {
    run_with_env(home, args, &[])
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// The 37-file fixture from the issue (37 files, 2 existing directories).
fn fixture(root: &Path) -> PathBuf {
    let dir = root.join("misc");
    std::fs::create_dir_all(dir.join("sub1")).expect("sub1");
    std::fs::create_dir_all(dir.join("sub2")).expect("sub2");
    std::fs::write(dir.join("sub1/keep.txt"), b"keep").expect("keep");
    for index in 1..=11 {
        std::fs::write(dir.join(format!("cbd-chart-{index:02}.png")), b"").expect("png");
    }
    for index in 1..=10 {
        std::fs::write(
            dir.join(format!("cannabis-hearing-note-{index:02}.md")),
            b"note",
        )
        .expect("md");
    }
    for index in 1..=5 {
        std::fs::write(
            dir.join(format!("hearing-page-{index}.html")),
            b"<html></html>",
        )
        .expect("html");
    }
    for index in 1..=4 {
        std::fs::write(dir.join(format!("archive-bundle-{index}.zip")), b"zip").expect("zip");
    }
    std::fs::write(dir.join("hearing-brief.pdf"), b"pdf").expect("pdf");
    std::fs::write(dir.join("cbd-summary.pdf"), b"pdf").expect("pdf");
    std::fs::write(dir.join("thc-photo-a.jpg"), b"jpg").expect("jpg");
    std::fs::write(dir.join("thc-photo-b.jpg"), b"jpg").expect("jpg");
    std::fs::write(dir.join("budget.numbers"), b"n").expect("numbers");
    std::fs::write(dir.join("README"), b"plain").expect("readme");
    dir
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn v122_the_dry_run_is_deterministic_and_touches_nothing() {
    let root = sandbox("deterministic");
    let home = home_of(&root);
    let dir = fixture(&root);
    let before = entries(&dir);

    let first = run(&home, &["cluster", &dir.to_string_lossy()]);
    let second = run(&home, &["cluster", &dir.to_string_lossy()]);

    assert!(first.status.success(), "stderr: {}", stderr(&first));
    assert_eq!(stdout(&first), stdout(&second), "same input, same plan");
    assert_eq!(entries(&dir), before, "a dry run changes nothing");
    let text = stdout(&first);
    assert!(text.contains("cluster: "), "{text}");
    assert!(text.contains("ungrouped: 4 files"), "{text}");
    assert!(text.contains("directories left alone: 2"), "{text}");
    assert!(text.contains("mode: dry-run"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v122_execute_files_only_the_files_and_undo_batch_restores_the_flat_layout() {
    let root = sandbox("execute");
    let home = home_of(&root);
    let dir = fixture(&root);

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--execute",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    let batch = json["cluster"]["batch_id"]
        .as_str()
        .expect("batch id")
        .to_string();
    assert_eq!(json["cluster"]["executed"], true);

    // Files moved into the groups, the existing directories untouched.
    assert!(dir.join("sub1/keep.txt").is_file(), "a folder was touched");
    assert!(!dir.join("cbd-chart-01.png").exists());
    let groups = json["cluster"]["groups"].as_array().expect("groups");
    assert!(!groups.is_empty());
    for group in groups {
        let directory = PathBuf::from(group["directory"].as_str().expect("directory"));
        assert!(
            directory.is_dir(),
            "{} was not created",
            directory.display()
        );
        for assignment in group["assignments"].as_array().expect("assignments") {
            let destination = PathBuf::from(assignment["destination"].as_str().expect("dest"));
            assert!(
                destination.is_file(),
                "{} is missing",
                destination.display()
            );
        }
    }

    let undo = run(&home, &["undo", "--batch", &batch]);
    assert!(undo.status.success(), "stderr: {}", stderr(&undo));
    let text = stdout(&undo);
    assert!(text.contains("removed created directory:"), "{text}");
    assert!(
        dir.join("cbd-chart-01.png").is_file(),
        "not restored: {text}"
    );
    assert!(dir.join("cannabis-hearing-note-01.md").is_file());
    assert!(dir.join("sub1/keep.txt").is_file());
    for group in groups {
        let directory = PathBuf::from(group["directory"].as_str().expect("directory"));
        assert!(!directory.exists(), "{} survived undo", directory.display());
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v122_a_pre_existing_directory_takes_a_sequence_suffix() {
    let root = sandbox("collision");
    let home = home_of(&root);
    let dir = fixture(&root);
    std::fs::create_dir_all(dir.join("images")).expect("images");
    std::fs::write(dir.join("images/own.png"), b"own").expect("own");

    let output = run(
        &home,
        &["cluster", &dir.to_string_lossy(), "--format", "json"],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    let names: Vec<String> = json["cluster"]["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .map(|group| group["name"].as_str().expect("name").to_string())
        .collect();
    assert!(names.contains(&"images (2)".to_string()), "{names:?}");
    let left: Vec<String> = json["cluster"]["directories_left_alone"]
        .as_array()
        .expect("left alone")
        .iter()
        .map(|path| path.as_str().expect("path").to_string())
        .collect();
    assert!(left.iter().any(|path| path.ends_with("images")), "{left:?}");
    assert!(
        dir.join("images/own.png").is_file(),
        "the existing folder is untouched"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v122_without_a_key_every_group_uses_the_type_based_name() {
    let root = sandbox("fallback");
    let home = home_of(&root);
    let dir = fixture(&root);

    let output = run(
        &home,
        &["cluster", &dir.to_string_lossy(), "--format", "json"],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert_eq!(json["cluster"]["naming"]["jev_groups"], 0);
    let groups = json["cluster"]["groups"].as_array().expect("groups");
    assert!(!groups.is_empty());
    assert!(
        groups
            .iter()
            .all(|group| group["name_source"] == "fallback"),
        "{groups:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v122_a_jev_answer_chooses_the_group_name() {
    let root = sandbox("jev");
    let home = home_of(&root);
    let dir = fixture(&root);
    let server = JeNamerStub::start();

    let output = run_with_env(
        &home,
        &["cluster", &dir.to_string_lossy(), "--format", "json"],
        &[
            ("JEV_API_URL", &server.endpoint),
            ("JEV_API_KEY", "test-key"),
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    let jev_groups = json["cluster"]["naming"]["jev_groups"]
        .as_u64()
        .expect("count");
    assert!(jev_groups > 0, "no group was named by the model");
    let chosen = json["cluster"]["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .find(|group| group["name_source"] == "jev")
        .expect("a jev group");
    let candidates: Vec<String> = chosen["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .map(|name| name.as_str().expect("name").to_string())
        .collect();
    assert!(candidates.len() >= 2, "{candidates:?}");
    let name = chosen["name"].as_str().expect("name").to_string();
    assert!(
        candidates.contains(&name),
        "{name} is not an offered candidate"
    );
    assert_ne!(
        name, candidates[0],
        "the model's choice replaced the fallback"
    );

    // The request carries the groups and only file names (never paths).
    let request = server.request();
    assert!(request.contains("group_1"), "{request}");
    assert!(request.contains("choice"), "{request}");
    assert!(
        !request.contains(&root.to_string_lossy().to_string()),
        "paths leaked: {request}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v122_include_folders_moves_the_direct_child_folders_too() {
    let root = sandbox("folders");
    let home = home_of(&root);
    let dir = root.join("misc");
    std::fs::create_dir_all(dir.join("cbd-charts-alpha")).expect("alpha");
    std::fs::create_dir_all(dir.join("cbd-charts-beta")).expect("beta");
    std::fs::write(dir.join("cbd-charts-alpha/a.txt"), b"a").expect("a");
    std::fs::write(dir.join("cbd-charts-beta/b.txt"), b"b").expect("b");

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--include-folders",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert_eq!(
        json["cluster"]["directories_left_alone"]
            .as_array()
            .expect("left alone")
            .len(),
        0
    );
    let destinations: Vec<String> = json["cluster"]["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .flat_map(|group| group["assignments"].as_array().expect("assignments"))
        .map(|assignment| {
            assignment["destination"]
                .as_str()
                .expect("dest")
                .to_string()
        })
        .collect();
    assert!(
        destinations
            .iter()
            .any(|path| path.ends_with("cbd-charts-alpha")),
        "{destinations:?}"
    );
    assert!(
        destinations
            .iter()
            .any(|path| path.ends_with("cbd-charts-beta")),
        "{destinations:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v122_recursive_collects_from_subdirectories_and_leaves_them() {
    let root = sandbox("recursive");
    let home = home_of(&root);
    let dir = root.join("misc");
    std::fs::create_dir_all(dir.join("inner")).expect("inner");
    for index in 1..=3 {
        std::fs::write(dir.join(format!("inner/cbd-note-{index}.md")), b"n").expect("note");
    }

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--recursive",
            "--execute",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert!(dir.join("inner").is_dir(), "the folder itself stays");
    let batch = json["cluster"]["batch_id"]
        .as_str()
        .expect("batch")
        .to_string();
    assert!(
        json["cluster"]["groups"][0]["assignments"]
            .as_array()
            .expect("assignments")
            .iter()
            .all(|assignment| assignment["source"].as_str().unwrap().contains("inner"))
    );
    let undo = run(&home, &["undo", "--batch", &batch]);
    assert!(undo.status.success(), "stderr: {}", stderr(&undo));
    assert!(dir.join("inner/cbd-note-1.md").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v122_min_decides_what_is_worth_a_group() {
    let root = sandbox("min");
    let home = home_of(&root);
    let dir = root.join("misc");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("cbd-a.md"), b"a").expect("a");
    std::fs::write(dir.join("cbd-b.md"), b"b").expect("b");

    let output = run(
        &home,
        &["cluster", &dir.to_string_lossy(), "--min", "5", "--execute"],
    );
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("no group reached --min"),
        "{}",
        stderr(&output)
    );
    assert!(dir.join("cbd-a.md").is_file(), "nothing was moved");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v122_recursive_and_include_folders_are_refused_together() {
    let root = sandbox("conflict");
    let home = home_of(&root);
    let dir = root.join("misc");
    std::fs::create_dir_all(&dir).expect("dir");

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--recursive",
            "--include-folders",
        ],
    );
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("cannot also treat them as members"),
        "{}",
        stderr(&output)
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// A one-request stub for the naming call
// ---------------------------------------------------------------------------

struct JeNamerStub {
    endpoint: String,
    request: Arc<Mutex<String>>,
}

impl JeNamerStub {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind stub");
        let address = listener.local_addr().expect("address");
        let request = Arc::new(Mutex::new(String::new()));
        let captured = Arc::clone(&request);
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let body = read_http_request(&mut stream);
            captured.lock().expect("request log").push_str(&body);
            write_http_response(&mut stream, 200, &stub_answer(&body));
        });
        Self {
            endpoint: format!("http://{address}"),
            request,
        }
    }

    fn request(&self) -> String {
        // The request is read before the response is written; give the thread
        // a moment to hand it over.
        for _ in 0..50 {
            let request = self.request.lock().expect("request log").clone();
            if !request.is_empty() {
                return request;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.request.lock().expect("request log").clone()
    }
}

/// Answer every `group_N` question with the second offered candidate, so the
/// name visibly differs from the deterministic fallback.
fn stub_answer(request: &str) -> String {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let mut answers = serde_json::Map::new();
    if let Some(questions) = parsed["questions"].as_object() {
        for (name, question) in questions {
            let choice = question["criteria"]
                .as_object()
                .and_then(|criteria| criteria.keys().nth(1).cloned())
                .or_else(|| {
                    question["criteria"]
                        .as_object()
                        .and_then(|criteria| criteria.keys().next().cloned())
                })
                .unwrap_or_default();
            answers.insert(
                name.clone(),
                serde_json::json!({"type": "choice", "choice": choice}),
            );
        }
    }
    serde_json::json!({"model": "jev-latest", "answers": answers}).to_string()
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        if let Some(header_end) = find_subslice(&data, b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..header_end]).to_string();
            let content_length = headers
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

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

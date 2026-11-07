//! v0.13.2 issue #25: `kaseifu move --create-dir` and several paths in one run.
//!
//! The command already moved a file or a folder with no decision (#22). This
//! round adds the two things real use needed: creating the destination (and any
//! missing parent) first, and moving several paths as one batch.
//!
//! Sandboxes live under `target/`, never under the system temp directory:
//! `is_excluded_destination` refuses anything under `/var` (where macOS puts its
//! temp directories), which would make every move here a refusal.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(tag: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-sandboxes")
        .join(format!("v121-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("sandbox");
    root
}

fn home_of(root: &Path) -> PathBuf {
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    home
}

fn run(home: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env("NO_COLOR", "1")
        .env_remove("JEV_API_KEY")
        .env_remove("KASEIFU_DEBUG");
    command.output().expect("run kaseifu")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn v121_create_dir_makes_the_destination_and_undo_removes_it() {
    let root = sandbox("create-undo");
    let home = home_of(&root);
    let source = root.join("brief.md");
    std::fs::write(&source, b"brief").expect("source");
    // Two missing levels: the run has to create both, and undo has to take both
    // away again (deepest first, only while empty).
    let destination = root.join("archive").join("2026");

    let output = run(
        &home,
        &[
            "move",
            &source.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--create-dir",
            "--execute",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(destination.join("brief.md").is_file());
    assert!(!source.exists());

    let undo = run(&home, &["undo"]);
    assert!(undo.status.success(), "stderr: {}", stderr(&undo));
    assert!(source.is_file(), "restored: {}", stdout(&undo));
    assert!(!destination.exists(), "created directory removed");
    assert!(!root.join("archive").exists(), "created parent removed");
    assert!(
        stdout(&undo).contains("removed created directory:"),
        "{}",
        stdout(&undo)
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v121_create_dir_dry_run_shows_the_directory_and_each_path() {
    let root = sandbox("dry-run");
    let home = home_of(&root);
    let first = root.join("a.txt");
    let second = root.join("b.txt");
    std::fs::write(&first, b"a").expect("a");
    std::fs::write(&second, b"b").expect("b");
    let destination = root.join("filed");

    let output = run(
        &home,
        &[
            "move",
            &first.to_string_lossy(),
            &second.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--create-dir",
        ],
    );
    let text = stdout(&output);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(
        text.contains(&format!("create directory: {}", destination.display())),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "1. {} -> {}",
            first.display(),
            destination.join("a.txt").display()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "2. {} -> {}",
            second.display(),
            destination.join("b.txt").display()
        )),
        "{text}"
    );
    assert!(first.is_file() && second.is_file(), "nothing moved: {text}");
    assert!(!destination.exists(), "a dry run creates nothing: {text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v121_a_file_in_the_destination_position_is_refused() {
    let root = sandbox("file-in-the-way");
    let home = home_of(&root);
    let source = root.join("a.txt");
    std::fs::write(&source, b"a").expect("source");
    let blocked = root.join("blocked");
    std::fs::write(&blocked, b"not a directory").expect("blocked");

    let output = run(
        &home,
        &[
            "move",
            &source.to_string_lossy(),
            "--to",
            &blocked.to_string_lossy(),
            "--create-dir",
            "--execute",
        ],
    );

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("destination is not a directory"),
        "{}",
        stderr(&output)
    );
    assert!(source.is_file(), "the source is untouched");
    assert_eq!(std::fs::read(&blocked).expect("file"), b"not a directory");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v121_an_excluded_destination_is_never_created() {
    let root = sandbox("excluded-create");
    let home = home_of(&root);
    let source = root.join("a.txt");
    std::fs::write(&source, b"a").expect("source");
    // A cache tree anywhere in the path is never a destination, so it cannot be
    // created either. The rule is checked on every level the run would make.
    let destination = root.join(".cache").join("nested").join("archive");

    let output = run(
        &home,
        &[
            "move",
            &source.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--create-dir",
            "--execute",
        ],
    );

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("destination is not a usable folder"),
        "{}",
        stderr(&output)
    );
    assert!(!root.join(".cache").exists(), "nothing was created");
    assert!(source.is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v121_several_paths_move_in_one_run_and_undo_batch_reverts_them() {
    let root = sandbox("several");
    let home = home_of(&root);
    let first = root.join("one.txt");
    let second = root.join("two.txt");
    std::fs::write(&first, b"one").expect("one");
    std::fs::write(&second, b"two").expect("two");
    let destination = root.join("filed");

    let output = run(
        &home,
        &[
            "move",
            &first.to_string_lossy(),
            &second.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--create-dir",
            "--execute",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    let batch = json["move"]["batch_id"]
        .as_str()
        .expect("a batch id")
        .to_string();
    assert_eq!(json["move"]["items"].as_array().expect("items").len(), 2);
    assert!(destination.join("one.txt").is_file());
    assert!(destination.join("two.txt").is_file());

    let undo = run(&home, &["undo", "--batch", &batch]);
    assert!(undo.status.success(), "stderr: {}", stderr(&undo));
    assert!(first.is_file() && second.is_file(), "{}", stdout(&undo));
    assert!(!destination.exists(), "the created directory is gone");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v121_one_failed_path_does_not_roll_back_the_others() {
    let root = sandbox("partial");
    let home = home_of(&root);
    let good = root.join("good.txt");
    std::fs::write(&good, b"good").expect("good");
    let missing = root.join("missing.txt");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");

    let output = run(
        &home,
        &[
            "move",
            &good.to_string_lossy(),
            &missing.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--execute",
        ],
    );
    let text = stdout(&output);

    assert!(!output.status.success(), "a partial failure is not success");
    assert!(destination.join("good.txt").is_file(), "{text}");
    assert!(text.contains("1. ") && text.contains("error:"), "{text}");
    assert!(
        stderr(&output).contains("1 of 2 paths could not be moved"),
        "{}",
        stderr(&output)
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v121_two_paths_with_one_name_collide() {
    let root = sandbox("same-name");
    let home = home_of(&root);
    let left = root.join("left").join("same.txt");
    let right = root.join("right").join("same.txt");
    std::fs::create_dir_all(left.parent().expect("left")).expect("left");
    std::fs::create_dir_all(right.parent().expect("right")).expect("right");
    std::fs::write(&left, b"left").expect("left file");
    std::fs::write(&right, b"right").expect("right file");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");

    let output = run(
        &home,
        &[
            "move",
            &left.to_string_lossy(),
            &right.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--execute",
        ],
    );
    let text = stdout(&output);

    assert!(!output.status.success());
    assert!(destination.join("same.txt").is_file(), "{text}");
    assert!(right.is_file(), "the loser stays where it was: {text}");
    assert!(text.contains("refusing to overwrite"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v121_a_batch_that_moves_nothing_leaves_no_created_directory() {
    let root = sandbox("nothing-moved");
    let home = home_of(&root);
    let first = root.join("missing-one.txt");
    let second = root.join("missing-two.txt");
    let destination = root.join("filed");

    let output = run(
        &home,
        &[
            "move",
            &first.to_string_lossy(),
            &second.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--create-dir",
            "--execute",
        ],
    );

    assert!(!output.status.success());
    assert!(
        !destination.exists(),
        "an empty created directory is removed"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v121_single_path_json_keeps_its_keys() {
    let root = sandbox("json");
    let home = home_of(&root);
    let source = root.join("a.txt");
    std::fs::write(&source, b"a").expect("source");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");

    let output = run(
        &home,
        &[
            "move",
            &source.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    let document = &json["move"];

    // The keys issue #22 promised, unchanged.
    assert_eq!(document["kind"], "file");
    assert_eq!(document["executed"], false);
    assert_eq!(document["operation_id"], serde_json::Value::Null);
    assert!(document["destination_path"].is_string());
    // The additions issue #25 makes.
    assert!(document["create_directories"].is_array());
    assert_eq!(document["items"].as_array().expect("items").len(), 1);
    assert!(document["items"][0]["source"].is_string());
    let _ = std::fs::remove_dir_all(&root);
}

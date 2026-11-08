//! v0.16.0 issue #27: `cluster` files each kind into the OS standard folder.
//!
//! #26 created the group folders inside the scanned directory, which left a
//! Desktop folder as a tree of folders on the Desktop. The folders now go to
//! the OS folder for the kind (`~/Pictures`, `~/Documents`, `~/Downloads`,
//! `~/Movies`, `~/Music`), `~/Documents` for a kind the table does not name,
//! `--to` files everything under one directory instead, and `--in-place` keeps
//! the old layout for a folder that is being reorganised in place.
//!
//! Sandboxes live under `target/`, never under the system temp directory:
//! `is_excluded_destination` refuses anything under `/var`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(tag: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-sandboxes")
        .join(format!("v123-{tag}-{}", std::process::id()));
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
    Command::new(env!("CARGO_BIN_EXE_kaseifu"))
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env("NO_COLOR", "1")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_ENDPOINT")
        .env_remove("JEV_API_KEY")
        .env_remove("KASEIFU_DEBUG")
        .output()
        .expect("run kaseifu")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn json_of(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout_of(output)).expect("one json document")
}

/// Two files of every kind the default table answers for, plus one kind it does
/// not (csv) and one it groups as code, so one run covers every destination.
fn fixture(root: &Path) -> PathBuf {
    let dir = root.join("misc");
    std::fs::create_dir_all(&dir).expect("misc");
    for name in [
        "cbd-chart-1.png",
        "cbd-chart-2.png",
        "cannabis-note-1.md",
        "cannabis-note-2.md",
        "archive-bundle-1.zip",
        "archive-bundle-2.zip",
        "clip-1.mp4",
        "clip-2.mp4",
        "rows-1.csv",
        "rows-2.csv",
        "main.rs",
        "lib.rs",
    ] {
        std::fs::write(dir.join(name), b"x").expect("file");
    }
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

/// The directory one group would be created in, as the report states it.
fn group_directory(json: &serde_json::Value, name: &str) -> PathBuf {
    let groups = json["cluster"]["groups"].as_array().expect("groups");
    let group = groups
        .iter()
        .find(|group| group["name"] == name)
        .unwrap_or_else(|| panic!("no group named {name}"));
    PathBuf::from(group["directory"].as_str().expect("directory"))
}

#[test]
fn v123_the_default_files_each_kind_into_its_standard_folder() {
    let root = sandbox("defaults");
    let home = home_of(&root);
    let dir = fixture(&root);

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--by-type",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(json["cluster"]["destination_mode"], "default_dirs");
    assert!(json["cluster"]["destination_base"].is_null());
    assert_eq!(
        group_directory(&json, "images"),
        home.join("Pictures/images")
    );
    assert_eq!(
        group_directory(&json, "documents"),
        home.join("Documents/documents")
    );
    assert_eq!(
        group_directory(&json, "archives"),
        home.join("Downloads/archives")
    );
    assert_eq!(group_directory(&json, "videos"), home.join("Movies/videos"));
    // A kind the platform table does not name is a document, not a leftover.
    assert_eq!(group_directory(&json, "data"), home.join("Documents/data"));
    assert_eq!(group_directory(&json, "code"), home.join("Documents/code"));
    // A dry run creates nothing, and the scanned directory keeps its files.
    assert!(!home.join("Pictures").exists());
    assert_eq!(entries(&dir).len(), 12);

    let text = stdout_of(&run(
        &home,
        &["cluster", &dir.to_string_lossy(), "--by-type"],
    ));
    assert!(
        text.contains(&format!(
            "  create directory: {}",
            home.join("Pictures/images").display()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "1. {} -> {}",
            dir.join("cbd-chart-1.png").display(),
            home.join("Pictures/images/cbd-chart-1.png").display()
        )),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_the_default_execute_and_undo_put_the_files_back() {
    let root = sandbox("defaults-execute");
    let home = home_of(&root);
    let dir = fixture(&root);

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--by-type",
            "--execute",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    let batch = json["cluster"]["batch_id"]
        .as_str()
        .expect("batch id")
        .to_string();

    // The files left the scanned directory, and the scanned directory itself
    // did not become a tree.
    assert_eq!(entries(&dir), Vec::<String>::new());
    assert!(home.join("Pictures/images/cbd-chart-1.png").is_file());
    assert!(
        home.join("Documents/documents/cannabis-note-1.md")
            .is_file()
    );
    assert!(
        home.join("Downloads/archives/archive-bundle-1.zip")
            .is_file()
    );
    assert!(home.join("Movies/videos/clip-1.mp4").is_file());

    let undo = run(&home, &["undo", "--batch", &batch]);
    assert!(undo.status.success(), "stderr: {}", stderr_of(&undo));
    assert_eq!(entries(&dir).len(), 12, "every file came back");
    // The standard folders this run created are gone again; the ones that hold
    // nothing of this run stay.
    assert!(!home.join("Pictures").exists(), "Pictures survived undo");
    assert!(!home.join("Movies").exists(), "Movies survived undo");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_in_place_keeps_the_folders_inside_the_scanned_directory() {
    let root = sandbox("in-place");
    let home = home_of(&root);
    let dir = fixture(&root);

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--by-type",
            "--in-place",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(json["cluster"]["destination_mode"], "in_place");
    assert!(json["cluster"]["destination_base"].is_null());
    assert_eq!(group_directory(&json, "images"), dir.join("images"));

    // The text keeps the #26 shape: the group name is the destination prefix and
    // no directory is announced, because it is the folder the reader is in.
    let text = stdout_of(&run(
        &home,
        &["cluster", &dir.to_string_lossy(), "--by-type", "--in-place"],
    ));
    assert!(!text.contains("create directory:"), "{text}");
    assert!(
        text.contains(&format!(
            "1. {} -> images/cbd-chart-1.png",
            dir.join("cbd-chart-1.png").display()
        )),
        "{text}"
    );

    let executed = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--by-type",
            "--in-place",
            "--execute",
            "--format",
            "json",
        ],
    );
    assert!(
        executed.status.success(),
        "stderr: {}",
        stderr_of(&executed)
    );
    assert!(dir.join("images/cbd-chart-1.png").is_file());
    assert!(!home.join("Pictures").exists(), "the default leaked in");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_a_file_already_under_the_base_is_left_alone() {
    let root = sandbox("already-placed");
    let home = home_of(&root);
    let documents = home.join("Documents");
    std::fs::create_dir_all(&documents).expect("Documents");
    std::fs::write(documents.join("cannabis-note-1.md"), b"a").expect("a");
    std::fs::write(documents.join("cannabis-note-2.md"), b"b").expect("b");

    let output = run(
        &home,
        &["cluster", &documents.to_string_lossy(), "--format", "json"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    let placed = json["cluster"]["already_in_destination"]
        .as_array()
        .expect("already_in_destination");
    assert_eq!(placed.len(), 2, "{json}");
    assert!(
        json["cluster"]["groups"]
            .as_array()
            .expect("groups")
            .is_empty(),
        "a folder beside files that are already in place is not a plan: {json}"
    );

    let executed = run(
        &home,
        &[
            "cluster",
            &documents.to_string_lossy(),
            "--execute",
            "--format",
            "json",
        ],
    );
    assert_eq!(executed.status.code(), Some(1), "a no-op is not success");
    assert!(stderr_of(&executed).contains("already in the destination"));
    assert_eq!(entries(&documents).len(), 2, "nothing was filed");
    assert!(!documents.join("documents").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_to_files_every_group_under_one_directory_and_undo_removes_it() {
    let root = sandbox("to");
    let home = home_of(&root);
    let dir = fixture(&root);
    let base = root.join("out/deep");

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--by-type",
            "--to",
            &base.to_string_lossy(),
            "--execute",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(json["cluster"]["destination_mode"], "to");
    assert_eq!(
        json["cluster"]["destination_base"],
        serde_json::json!(base.to_string_lossy()),
        "--to is reported as the base"
    );
    let batch = json["cluster"]["batch_id"]
        .as_str()
        .expect("batch id")
        .to_string();
    // Every kind lands under the one base, including the ones the OS would
    // have taken.
    assert!(base.join("images/cbd-chart-1.png").is_file());
    assert!(base.join("documents/cannabis-note-1.md").is_file());
    assert!(!home.join("Pictures").exists());
    assert_eq!(entries(&dir), Vec::<String>::new());

    let undo = run(&home, &["undo", "--batch", &batch]);
    assert!(undo.status.success(), "stderr: {}", stderr_of(&undo));
    assert_eq!(entries(&dir).len(), 12);
    assert!(!base.exists(), "the created base survived undo");
    assert!(!root.join("out").exists(), "the parents survived undo");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_an_excluded_base_is_refused_before_anything_moves() {
    let root = sandbox("excluded");
    let home = home_of(&root);
    let dir = fixture(&root);
    let before = entries(&dir);

    // A cache tree is refused at any depth; the same rule covers a real
    // `~/Library/**`, which needs the real home to be recognised as one.
    for base in [
        home.join(".cache/cluster"),
        PathBuf::from("/Library/Cluster"),
    ] {
        let output = run(
            &home,
            &[
                "cluster",
                &dir.to_string_lossy(),
                "--to",
                &base.to_string_lossy(),
            ],
        );
        assert_eq!(
            output.status.code(),
            Some(1),
            "{} was accepted",
            base.display()
        );
        assert!(
            stderr_of(&output).contains("--to is not a usable destination"),
            "stderr: {}",
            stderr_of(&output)
        );
        assert!(!base.exists(), "{} was created", base.display());
    }
    assert_eq!(entries(&dir), before, "a refusal changes nothing");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_to_and_in_place_are_mutually_exclusive() {
    let root = sandbox("exclusive");
    let home = home_of(&root);
    let dir = fixture(&root);

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--to",
            &root.join("out").to_string_lossy(),
            "--in-place",
        ],
    );
    assert_eq!(output.status.code(), Some(2), "clap reports the conflict");
    assert!(
        stderr_of(&output).contains("cannot be used with"),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(!root.join("out").exists());
    let _ = std::fs::remove_dir_all(&root);
}

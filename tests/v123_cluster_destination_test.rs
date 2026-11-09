//! v0.16.0 issues #27, #28: where `cluster` puts the files, and whether it
//! creates anything.
//!
//! #27 filed every group into the OS standard folder for its kind, but created
//! a folder for each group there (`~/Pictures/images`) - a name that repeats
//! the parent and a `documents (2)` queue on every run. #28 settled the rules:
//! a run creates nothing unless `--create-dir` is given.
//!
//! - Default, the type folder exists: file into it (`~/Pictures/images`).
//! - Default, it does not: file into the kind root (`~/Pictures`), and refuse
//!   the run when the kind root itself is missing.
//! - `--create-dir`: create `<kind root>/<type>/<name>`, and land flat in
//!   `<kind root>/<type>` when the name is the type name.
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

/// The groups of one report, keyed by the type word.
/// The group whose folder chain is `label` (`images/png`, `documents/html`, or
/// `documents` when its files have no extension).
fn group_of<'a>(json: &'a serde_json::Value, label: &str) -> &'a serde_json::Value {
    let (class_folder, extension) = match label.split_once('/') {
        Some((class_folder, extension)) => (class_folder, Some(extension)),
        None => (label, None),
    };
    json["cluster"]["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .find(|group| {
            group["class_folder"] == class_folder && group["extension"].as_str() == extension
        })
        .unwrap_or_else(|| panic!("no group filed under {label}"))
}

fn directory_of(group: &serde_json::Value) -> PathBuf {
    PathBuf::from(group["directory"].as_str().expect("directory"))
}

/// Every directory below `home`, so a test can prove that a run created none.
fn directories_under(home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![home.to_path_buf()];
    while let Some(next) = queue.pop() {
        for entry in std::fs::read_dir(&next).expect("read dir") {
            let path = entry.expect("entry").path();
            if !path.is_dir() || path.file_name().is_some_and(|name| name == "Library") {
                continue;
            }
            found.push(path.clone());
            queue.push(path);
        }
    }
    found.sort();
    found
}

/// The home a default run needs: the OS folders for the fixture's kinds, empty.
fn standard_home(root: &Path) -> PathBuf {
    let home = home_of(root);
    for folder in ["Pictures", "Documents", "Downloads", "Movies"] {
        std::fs::create_dir_all(home.join(folder)).expect("standard folder");
    }
    home
}

#[test]
fn v123_the_default_creates_nothing_and_files_into_the_standard_folder() {
    let root = sandbox("defaults");
    let home = standard_home(&root);
    let dir = fixture(&root);
    let before = directories_under(&home);

    let output = run(
        &home,
        &["cluster", &dir.to_string_lossy(), "--format", "json"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(json["cluster"]["creates_directory"], false);
    assert_eq!(json["cluster"]["destination_mode"], "default_dirs");
    // No type folder exists yet, so every kind goes into its own standard folder.
    assert_eq!(
        directory_of(group_of(&json, "images/png")),
        home.join("Pictures")
    );
    assert_eq!(
        directory_of(group_of(&json, "documents/md")),
        home.join("Documents")
    );
    assert_eq!(
        directory_of(group_of(&json, "archives/zip")),
        home.join("Downloads")
    );
    assert_eq!(
        directory_of(group_of(&json, "videos/mp4")),
        home.join("Movies")
    );
    // Kinds the platform table does not name are documents, and their extension
    // is the type that says what they are (issue #28).
    assert_eq!(
        directory_of(group_of(&json, "documents/csv")),
        home.join("Documents")
    );
    assert_eq!(
        directory_of(group_of(&json, "documents/rs")),
        home.join("Documents")
    );
    for group in json["cluster"]["groups"].as_array().expect("groups") {
        assert!(group["name"].is_null(), "nothing is named: {group}");
        assert_eq!(
            group["create_directories"]
                .as_array()
                .expect("levels")
                .len(),
            0,
            "nothing is created: {group}"
        );
    }
    assert_eq!(
        directories_under(&home),
        before,
        "a dry run created a folder"
    );
    assert_eq!(entries(&dir).len(), 12);

    let text = stdout_of(&run(&home, &["cluster", &dir.to_string_lossy()]));
    assert!(
        text.starts_with("destination: OS standard folders (no directory is created)"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "images/png -> {} (2 files",
            home.join("Pictures").display()
        )),
        "{text}"
    );
    assert!(!text.contains("create directory:"), "{text}");
    assert!(
        text.contains(&format!(
            "1. {} -> {}",
            dir.join("cbd-chart-1.png").display(),
            home.join("Pictures/cbd-chart-1.png").display()
        )),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_the_type_folder_is_used_when_it_already_exists() {
    let root = sandbox("type-folder");
    let home = standard_home(&root);
    let dir = fixture(&root);
    std::fs::create_dir_all(home.join("Pictures/images/png")).expect("Pictures/images/png");
    std::fs::create_dir_all(home.join("Documents/documents/md")).expect("Documents/documents/md");

    let output = run(
        &home,
        &["cluster", &dir.to_string_lossy(), "--format", "json"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(
        directory_of(group_of(&json, "images/png")),
        home.join("Pictures/images/png")
    );
    assert_eq!(
        directory_of(group_of(&json, "documents/md")),
        home.join("Documents/documents/md")
    );
    // The folders that do not exist keep the kind root, which costs nothing.
    assert_eq!(
        directory_of(group_of(&json, "archives/zip")),
        home.join("Downloads")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_the_default_execute_and_undo_put_the_files_back() {
    let root = sandbox("defaults-execute");
    let home = standard_home(&root);
    let dir = fixture(&root);
    let before = directories_under(&home);

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
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let batch = json_of(&output)["cluster"]["batch_id"]
        .as_str()
        .expect("batch id")
        .to_string();

    assert_eq!(entries(&dir), Vec::<String>::new());
    assert!(home.join("Pictures/cbd-chart-1.png").is_file());
    assert!(home.join("Documents/cannabis-note-1.md").is_file());
    assert!(home.join("Downloads/archive-bundle-1.zip").is_file());
    assert!(home.join("Movies/clip-1.mp4").is_file());
    assert_eq!(directories_under(&home), before, "a directory was created");

    let undo = run(&home, &["undo", "--batch", &batch]);
    assert!(undo.status.success(), "stderr: {}", stderr_of(&undo));
    assert_eq!(entries(&dir).len(), 12, "every file came back");
    assert!(
        !stdout_of(&undo).contains("removed created directory"),
        "this run created nothing to remove"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_a_missing_standard_folder_names_the_flag_that_creates_it() {
    let root = sandbox("missing-root");
    let home = home_of(&root);
    std::fs::create_dir_all(home.join("Pictures")).expect("Pictures");
    std::fs::create_dir_all(home.join("Documents")).expect("Documents");
    let dir = fixture(&root);

    let output = run(&home, &["cluster", &dir.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr_of(&output);
    assert!(text.contains("no standard folder for"), "{text}");
    assert!(
        text.contains("Downloads"),
        "the missing folder is named: {text}"
    );
    assert!(text.contains("--create-dir"), "{text}");
    assert_eq!(entries(&dir).len(), 12, "nothing moved");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_create_dir_builds_the_type_folder_and_undo_removes_it() {
    let root = sandbox("create-dir");
    let home = standard_home(&root);
    let dir = fixture(&root);

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--create-dir",
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
    assert_eq!(json["cluster"]["creates_directory"], true);
    // The default namer is the type name, so rule 5 applies: flat in the type
    // folder, no folder called `png` inside `~/Pictures/images`.
    let images = group_of(&json, "images/png");
    assert!(images["name"].is_null(), "{images}");
    assert_eq!(directory_of(images), home.join("Pictures/images/png"));
    assert!(home.join("Pictures/images/png/cbd-chart-1.png").is_file());
    assert!(
        home.join("Documents/documents/md/cannabis-note-1.md")
            .is_file()
    );

    let undo = run(&home, &["undo", "--batch", &batch]);
    assert!(undo.status.success(), "stderr: {}", stderr_of(&undo));
    assert_eq!(entries(&dir).len(), 12);
    for created in [
        "Pictures/images/png",
        "Documents/documents/md",
        "Downloads/archives/zip",
    ] {
        assert!(!home.join(created).exists(), "{created} survived undo");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_a_collision_leaves_that_file_and_files_the_rest() {
    let root = sandbox("collision");
    let home = standard_home(&root);
    let dir = fixture(&root);
    std::fs::write(home.join("Documents/cannabis-note-1.md"), b"mine").expect("existing");

    let dry = run(
        &home,
        &["cluster", &dir.to_string_lossy(), "--format", "json"],
    );
    assert!(dry.status.success(), "stderr: {}", stderr_of(&dry));
    let json = json_of(&dry);
    let assignments = group_of(&json, "documents/md")["assignments"]
        .as_array()
        .expect("assignments")
        .clone();
    let refused: Vec<&serde_json::Value> = assignments
        .iter()
        .filter(|assignment| !assignment["error"].is_null())
        .collect();
    assert_eq!(refused.len(), 1, "{assignments:?}");
    assert!(
        refused[0]["error"]
            .as_str()
            .expect("error")
            .contains("refusing to overwrite"),
        "{refused:?}"
    );

    let output = run(&home, &["cluster", &dir.to_string_lossy(), "--execute"]);
    assert_eq!(output.status.code(), Some(1), "a leftover is not success");
    assert!(
        stderr_of(&output).contains("1 file(s) could not be filed"),
        "stderr: {}",
        stderr_of(&output)
    );
    assert_eq!(
        std::fs::read(home.join("Documents/cannabis-note-1.md")).expect("read"),
        b"mine",
        "the existing file was overwritten"
    );
    assert!(
        home.join("Documents/cannabis-note-2.md").is_file(),
        "the rest moved"
    );
    assert_eq!(entries(&dir), vec!["cannabis-note-1.md".to_string()]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_a_file_already_under_the_kind_root_is_left_alone() {
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
    assert_eq!(
        json["cluster"]["already_in_destination"]
            .as_array()
            .expect("already_in_destination")
            .len(),
        2
    );
    assert!(
        json["cluster"]["groups"]
            .as_array()
            .expect("groups")
            .is_empty(),
        "a file in the folder it belongs in is not a plan: {json}"
    );

    let executed = run(
        &home,
        &["cluster", &documents.to_string_lossy(), "--execute"],
    );
    assert_eq!(executed.status.code(), Some(1), "a no-op is not success");
    assert!(stderr_of(&executed).contains("already in the destination"));
    assert_eq!(entries(&documents).len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_to_files_every_group_under_one_directory() {
    let root = sandbox("to");
    let home = standard_home(&root);
    let dir = fixture(&root);
    let base = root.join("out");
    std::fs::create_dir_all(base.join("documents")).expect("out/documents");

    let dry = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--to",
            &base.to_string_lossy(),
            "--format",
            "json",
        ],
    );
    assert!(dry.status.success(), "stderr: {}", stderr_of(&dry));
    let json = json_of(&dry);
    assert_eq!(json["cluster"]["destination_mode"], "to");
    assert_eq!(
        json["cluster"]["destination_base"],
        serde_json::json!(base.to_string_lossy())
    );
    // Rule 2 applies to a named base too: its type folder wins when it exists.
    assert_eq!(
        directory_of(group_of(&json, "documents/md")),
        base.join("documents")
    );
    assert_eq!(directory_of(group_of(&json, "images/png")), base);

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--to",
            &base.to_string_lossy(),
            "--execute",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let batch = json_of(&output)["cluster"]["batch_id"]
        .as_str()
        .expect("batch id")
        .to_string();
    assert!(base.join("cbd-chart-1.png").is_file());
    assert!(base.join("documents/cannabis-note-1.md").is_file());
    assert!(
        !home
            .join("Pictures")
            .read_dir()
            .expect("Pictures")
            .any(|e| {
                e.expect("entry")
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "png")
            })
    );

    let undo = run(&home, &["undo", "--batch", &batch]);
    assert!(undo.status.success(), "stderr: {}", stderr_of(&undo));
    assert_eq!(entries(&dir).len(), 12);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_to_with_create_dir_uses_the_type_folder() {
    let root = sandbox("to-create");
    let home = standard_home(&root);
    let dir = fixture(&root);
    let base = root.join("Archive");

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--to",
            &base.to_string_lossy(),
            "--create-dir",
            "--execute",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let batch = json_of(&output)["cluster"]["batch_id"]
        .as_str()
        .expect("batch id")
        .to_string();
    assert!(base.join("images/png/cbd-chart-1.png").is_file());
    assert!(base.join("documents/md/cannabis-note-1.md").is_file());

    let undo = run(&home, &["undo", "--batch", &batch]);
    assert!(undo.status.success(), "stderr: {}", stderr_of(&undo));
    assert!(!base.exists(), "the created base survived undo");
    assert_eq!(entries(&dir).len(), 12);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_in_place_needs_create_dir_and_keeps_the_folders_inside_the_directory() {
    let root = sandbox("in-place");
    let home = home_of(&root);
    let dir = fixture(&root);

    let refused = run(&home, &["cluster", &dir.to_string_lossy(), "--in-place"]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(
        stderr_of(&refused).contains("required arguments were not provided"),
        "stderr: {}",
        stderr_of(&refused)
    );

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--in-place",
            "--create-dir",
            "--execute",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(json["cluster"]["destination_mode"], "in_place");
    // In place there is no type root to explain the kind, so the folder is made.
    assert_eq!(directory_of(group_of(&json, "images/png")), dir.join("png"));
    assert!(dir.join("png/cbd-chart-1.png").is_file());
    assert!(!home.join("Pictures").exists(), "the default leaked in");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_the_creating_flags_require_create_dir() {
    let root = sandbox("requires");
    let home = standard_home(&root);
    let dir = fixture(&root);
    let path = dir.to_string_lossy().to_string();

    for flags in [
        vec!["--min", "3"],
        vec!["--max", "3"],
        vec!["--by-type"],
        vec!["--name-source", "auto"],
        vec!["--in-place"],
    ] {
        let mut args = vec!["cluster", path.as_str()];
        args.extend(flags.iter().copied());
        let output = run(&home, &args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{flags:?} was accepted without --create-dir"
        );
        assert!(
            stderr_of(&output).contains("--create-dir"),
            "{flags:?}: {}",
            stderr_of(&output)
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_an_excluded_base_is_refused_before_anything_moves() {
    let root = sandbox("excluded");
    let home = standard_home(&root);
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
                "--create-dir",
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
fn v123_a_quoted_tilde_in_a_path_is_the_home_directory() {
    let root = sandbox("tilde");
    let home = standard_home(&root);
    // The scanned folder has to sit under the home the path is expanded into.
    let dir = home.join("misc");
    std::fs::create_dir_all(&dir).expect("misc");
    for name in ["cbd-chart-1.png", "cbd-chart-2.png"] {
        std::fs::write(dir.join(name), b"x").expect("file");
    }

    // The shell expands an unquoted `~`; a script that quotes it is the case
    // worth pinning, because the path reaches the process as `~/misc`.
    let output = run(
        &home,
        &[
            "cluster",
            "~/misc",
            "--to",
            "~/Archive",
            "--create-dir",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json = json_of(&output);
    assert_eq!(
        json["cluster"]["directory"],
        serde_json::json!(home.join("misc").to_string_lossy())
    );
    assert_eq!(
        directory_of(group_of(&json, "images/png")),
        home.join("Archive/images/png")
    );
    assert!(!home.join("~").exists(), "a folder named `~` was created");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v123_to_and_in_place_are_mutually_exclusive() {
    let root = sandbox("exclusive");
    let home = standard_home(&root);
    let dir = fixture(&root);

    let output = run(
        &home,
        &[
            "cluster",
            &dir.to_string_lossy(),
            "--create-dir",
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

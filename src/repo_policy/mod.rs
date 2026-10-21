//! Repository policy file (`.kaseifu.toml`): `pattern -> destination` rules.
//!
//! kaseifu answers "where does this file belong?" from discovered evidence and
//! one classifier call. A repository can now state a placement rule itself, in
//! a file committed next to the code it governs:
//!
//! ```toml
//! [[rule]]
//! name = "site-owned-content"
//! match_glob = "content/{project}/sites/{site}/**"
//! destination = "sites/<site>/content/"
//! reason = "content filed under a site folder belongs to that site"
//! ```
//!
//! A matching rule is stronger evidence than the classifier, so a hit decides
//! the file on its own: Discovery and Jev are not consulted, the proposal states
//! the rule name and its reason, and everything else stays the same (dry run by
//! default, `--execute` only, journal plus `undo`, never-a-destination rules,
//! collision refusal). A repository without the file behaves exactly as before.
//!
//! # Matching
//!
//! `match_glob` must match the whole path relative to the workspace root, so
//! `content/{project}/**` covers everything under `content/<project>/` and
//! nothing outside it. `match_contains` (optional; every entry must appear) is a
//! substring test on the same relative path: directory names and the file name,
//! never the file body. A site the path does not name cannot be matched from the
//! path at all; a project that belongs to one site pins that destination in its
//! own rule instead.
//!
//! # Workspace
//!
//! The workspace of a file is the nearest directory at or above the file's own
//! directory that holds a `.kaseifu.toml`. The search stops at the user's home
//! directory, so a policy file above the home cannot govern a person's files (a
//! file outside the home is searched up to the filesystem root, so a project on
//! another volume can carry its own policy). The workspace root is also the
//! boundary for a rule's destination: a destination written as an absolute path,
//! or with a `.` or `..` element, is rejected when the file is read (an error
//! that names the file and the rule), and a destination that resolves outside
//! the root - through a symbolic link - is refused (REVIEW). The policy file is
//! read-only input that kaseifu never writes.
//!
//! This module owns parsing, matching and destination resolution. Whether a
//! resolved destination may actually be used (existing directory, not a
//! never-a-destination shape, no collision, inside the workspace) is decided by
//! `operations::plan::build_rule_move_plan`, next to the same checks the other
//! plan builders use.

mod glob;
mod toml;

use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};

pub use glob::{Captures, Glob, Template};
pub use toml::POLICY_FILE_NAME;

/// Prefix the journal selection of a rule-driven move carries.
///
/// The journal column is `candidate_id`; a rule move records `rule:<label>`
/// there and leaves the classifier numbers NULL, the same way `rename` records
/// its naming rule. `undo` needs only the two paths, so no schema change is
/// involved.
pub const RULE_ID_PREFIX: &str = "rule:";

/// One validated rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// 1-based position in the file (labels a rule without a name).
    pub index: usize,
    /// Optional author-supplied label.
    pub name: Option<String>,
    /// Pattern matched against the path relative to the workspace root.
    pub match_glob: Glob,
    /// Substrings the relative path must contain (all of them, case-sensitive).
    pub match_contains: Vec<String>,
    /// Destination directory template, relative to the workspace root.
    pub destination: Template,
    /// Why the rule exists.
    pub reason: Option<String>,
}

impl Rule {
    /// The label shown to the user (`<name>`, or `rule #<index>`).
    pub fn label(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("rule #{}", self.index))
    }

    /// Compile and validate one `[[rule]]` table.
    fn compile(index: usize, entry: toml::RuleEntry) -> Result<Self> {
        let match_glob = Glob::compile(entry.match_glob.trim())
            .map_err(|message| anyhow::anyhow!("{message}"))?;
        let destination = Template::parse(entry.destination.trim())
            .map_err(|message| anyhow::anyhow!("{message}"))?;
        validate_destination_path(&destination)?;
        validate_placeholders(&match_glob, &destination)?;
        let match_contains = entry
            .match_contains
            .map(toml::OneOrMany::into_vec)
            .unwrap_or_default();
        validate_contains(&match_contains)?;
        Ok(Self {
            index,
            name: entry
                .name
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty()),
            match_glob,
            match_contains,
            destination,
            reason: entry.reason.filter(|reason| !reason.trim().is_empty()),
        })
    }

    /// True when this rule matches one relative path.
    fn matches(&self, relative: &str) -> Option<Captures> {
        if !self
            .match_contains
            .iter()
            .all(|needle| relative.contains(needle.as_str()))
        {
            return None;
        }
        self.match_glob.matches(relative)
    }
}

/// A policy file loaded from one workspace root.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoPolicy {
    /// The directory that holds `.kaseifu.toml` (the workspace root).
    pub root: PathBuf,
    /// Rules in file order; the first match wins.
    pub rules: Vec<Rule>,
}

impl RepoPolicy {
    /// The first rule that matches `relative`, with its captures.
    pub fn matching_rule(&self, relative: &str) -> Option<(&Rule, Captures)> {
        self.rules
            .iter()
            .find_map(|rule| rule.matches(relative).map(|captures| (rule, captures)))
    }
}

/// A rule that matched one file, with its destination already resolved.
///
/// The destination is absolute and normalized; whether it may be used is
/// decided by the plan layer.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleMatch {
    /// The rule label (`<name>` or `rule #N`).
    pub rule: String,
    /// The glob as written (shown next to the label).
    pub match_glob: String,
    /// The author's reason, when stated.
    pub reason: Option<String>,
    /// The resolved destination directory.
    pub destination: PathBuf,
    /// The workspace root the destination is confined to.
    pub workspace: PathBuf,
}

impl RuleMatch {
    /// The selection the journal records for a move this rule chose.
    pub fn journal_id(&self) -> String {
        format!("{RULE_ID_PREFIX}{}", self.rule)
    }
}

/// Load the policy of one workspace root.
///
/// A missing file is not an error here (callers look the root up first); a
/// file that exists but cannot be read, parsed or validated is an error, and
/// nothing is moved: a broken policy must not silently fall back to the
/// classifier.
pub fn load(root: &Path) -> Result<RepoPolicy> {
    let path = root.join(POLICY_FILE_NAME);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read repository policy {}", path.display()))?;
    let file =
        toml::parse(&text).with_context(|| format!("repository policy {}", path.display()))?;
    let mut rules: Vec<Rule> = Vec::with_capacity(file.rule.len());
    for (offset, entry) in file.rule.into_iter().enumerate() {
        let index = offset + 1;
        let rule = Rule::compile(index, entry)
            .with_context(|| format!("repository policy {} (rule #{index})", path.display()))?;
        if let Some(name) = rule.name.as_deref()
            && rules
                .iter()
                .any(|existing| existing.name.as_deref() == Some(name))
        {
            bail!(
                "repository policy {} (rule #{index}): duplicate rule name `{name}`",
                path.display()
            );
        }
        rules.push(rule);
    }
    Ok(RepoPolicy {
        root: root.to_path_buf(),
        rules,
    })
}

/// The nearest workspace root at or above `start`.
///
/// `home` bounds the search: the walk never ascends above it, so a policy file
/// outside the user's home cannot govern their files. `None` means "walk to the
/// filesystem root".
pub fn locate_root(start: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join(POLICY_FILE_NAME).is_file() {
            return Some(dir.to_path_buf());
        }
        if home.is_some_and(|home| home == dir) {
            return None;
        }
        current = dir.parent();
    }
    None
}

/// Consult the policy that governs `file`.
///
/// - `Ok(None)`: no policy file, or no rule matched (the classifier decides,
///   exactly as before),
/// - `Ok(Some(hit))`: a rule matched; the caller plans the move,
/// - `Err`: the policy exists but is broken (nothing must move).
///
/// `home` bounds the workspace lookup (see [`locate_root`]); production passes
/// the user's home directory, tests pass an isolated sandbox.
pub fn evaluate(file: &Path, home: Option<&Path>) -> Result<Option<RuleMatch>> {
    let source = absolute(file);
    let Some(parent) = source.parent() else {
        return Ok(None);
    };
    let Some(root) = locate_root(parent, home) else {
        return Ok(None);
    };
    let policy = load(&root)?;
    let Ok(relative) = source.strip_prefix(&root) else {
        // A path that does not sit under the root (only reachable through a
        // `..` spelling) is not governed by this policy.
        return Ok(None);
    };
    let relative = relative.to_string_lossy().into_owned();
    let Some((rule, captures)) = policy.matching_rule(&relative) else {
        return Ok(None);
    };
    let resolved = root.join(rule.destination.expand(&captures));
    Ok(Some(RuleMatch {
        rule: rule.label(),
        match_glob: rule.match_glob.source().to_string(),
        reason: rule.reason.clone(),
        destination: crate::benchmark::normalize_path(&resolved),
        workspace: crate::benchmark::normalize_path(&root),
    }))
}

/// Absolute spelling of a file the user named (`.`/`..` resolved textually,
/// symbolic links kept: a file's policy is the one where the entry lives).
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Reject a destination that is absolute, empty, or escapes its root.
fn validate_destination_path(destination: &Template) -> Result<()> {
    let text = destination.source().trim();
    if text.is_empty() {
        bail!("destination must not be empty");
    }
    if text.starts_with('~') {
        bail!("destination `{text}` must be relative to the repository root, not a home path");
    }
    let path = Path::new(text);
    if path.is_absolute() {
        bail!("destination `{text}` must be relative to the repository root");
    }
    // A capture is written `<name>` in a destination; `{name}` is the pattern
    // spelling and would become a literal directory name, so a rule copied from
    // a `match_glob` fails here instead of filing files under `{site}`.
    if text.contains('{') || text.contains('}') {
        bail!("destination `{text}` must read a capture as `<name>`, not `{{name}}`");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                bail!("destination `{text}` must stay inside the repository root (no `.` or `..`)");
            }
        }
    }
    Ok(())
}

/// Every placeholder a destination reads must be a capture of its glob.
fn validate_placeholders(match_glob: &Glob, destination: &Template) -> Result<()> {
    for name in destination.placeholders() {
        if !match_glob
            .capture_names()
            .iter()
            .any(|capture| capture == name)
        {
            bail!(
                "destination placeholder `<{name}>` has no capture: match_glob `{}` captures {}",
                match_glob.source(),
                describe_captures(match_glob)
            );
        }
    }
    Ok(())
}

/// Human list of the captures a glob declares (for one clear message).
fn describe_captures(match_glob: &Glob) -> String {
    match match_glob.capture_names() {
        [] => "none (use `{name}` to capture a path segment)".to_string(),
        names => names
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// `match_contains` entries are non-empty substrings.
fn validate_contains(match_contains: &[String]) -> Result<()> {
    if let Some(empty) = match_contains
        .iter()
        .find(|needle| needle.trim().is_empty())
    {
        bail!("match_contains has an empty entry `{empty}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile a policy from text (no filesystem involved).
    fn compile(text: &str) -> Result<RepoPolicy> {
        let file = toml::parse(text)?;
        let mut rules = Vec::new();
        for (offset, entry) in file.rule.into_iter().enumerate() {
            rules.push(Rule::compile(offset + 1, entry)?);
        }
        Ok(RepoPolicy {
            root: PathBuf::from("/repo"),
            rules,
        })
    }

    const SITE_RULE: &str = r#"
        [[rule]]
        name = "site-owned-content"
        match_glob = "content/{project}/**"
        match_contains = "sites/thch-vape"
        destination = "sites/<project>/content/"
        reason = "site-owned content"
    "#;

    #[test]
    fn a_matching_rule_resolves_captures_into_the_destination() {
        let policy = compile(SITE_RULE).expect("policy compiles");
        let (rule, captures) = policy
            .matching_rule("content/hearing/sites/thch-vape/brief.md")
            .expect("rule matches");
        assert_eq!(rule.label(), "site-owned-content");
        assert_eq!(rule.destination.expand(&captures), "sites/hearing/content/");
    }

    #[test]
    fn a_rule_without_a_name_is_labelled_by_position() {
        let policy = compile(
            r#"
            [[rule]]
            match_glob = "**"
            destination = "archive"
        "#,
        )
        .expect("policy compiles");
        assert_eq!(policy.rules[0].label(), "rule #1");
    }

    #[test]
    fn every_contains_string_must_appear() {
        let policy = compile(SITE_RULE).expect("policy compiles");
        assert!(
            policy
                .matching_rule("content/hearing/sites/thch-vape/brief.md")
                .is_some()
        );
        assert!(policy.matching_rule("content/hearing/brief.md").is_none());
    }

    #[test]
    fn broken_rules_are_rejected_with_one_sentence() {
        for text in [
            r#"[[rule]]
               match_glob = "content/*"
               destination = "../outside""#,
            r#"[[rule]]
               match_glob = "content/*"
               destination = "/abs""#,
            r#"[[rule]]
               match_glob = "content/*"
               destination = "sites/<missing>""#,
            r#"[[rule]]
               match_glob = "content/{site}/**"
               destination = "sites/{site}/content/""#,
            r#"[[rule]]
               match_glob = "content/*"
               match_contains = [""]
               destination = "sites""#,
            r#"[[rule]]
               match_glob = "content/*"
               destination = """#,
        ] {
            assert!(compile(text).is_err(), "expected a refusal for: {text}");
        }
    }

    #[test]
    fn unknown_keys_and_duplicate_names_are_rejected() {
        assert!(
            compile(
                r#"
                [[rule]]
                match_glob = "a/*"
                destination = "b"
                dest = "typo"
            "#
            )
            .is_err()
        );
        assert!(
            compile(
                r#"
                [[rule]]
                match_glob = "a/*"
                destination = "b"
                [[rule]]
                match_glob = "c/*"
                destination = "d"
                extra = true
            "#
            )
            .is_err()
        );
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let policy = compile(
            r#"
            [[rule]]
            name = "first"
            match_glob = "content/**"
            destination = "one"
            [[rule]]
            name = "second"
            match_glob = "content/**"
            destination = "two"
        "#,
        )
        .expect("policy compiles");
        let (rule, _) = policy.matching_rule("content/a.md").expect("matches");
        assert_eq!(rule.label(), "first");
    }
}

//! `.kaseifu.toml` schema and parsing.
//!
//! The file is a committed, read-only input: kaseifu parses it and never
//! writes or generates it. Unknown keys are rejected so a typo fails loudly
//! instead of silently changing where files go, the same convention the user
//! configuration file follows.
//!
//! ```toml
//! [[rule]]
//! name = "site-owned-content"
//! match_glob = "content/{project}/**"
//! match_contains = "sites/thch-vape"
//! destination = "sites/thch-vape/content/"
//! reason = "content that names thch-vape belongs to that site"
//! ```
//!
//! A destination can also read a `{name}` capture of its own pattern
//! (`match_glob = "content/{project}/sites/{site}/**"` with
//! `destination = "sites/<site>/content/"`; the capture is written with angle
//! brackets there - `{site}` would be a literal directory name and is refused);
//! `match_contains` is the optional filter for a site the path does not name.

use anyhow::{Context, Result};
use serde::Deserialize;

/// File name of the repository policy at the root of a workspace.
pub const POLICY_FILE_NAME: &str = ".kaseifu.toml";

/// Top-level policy document (`[[rule]]` array).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PolicyFile {
    /// Rules in file order; the first matching rule wins.
    #[serde(default)]
    pub rule: Vec<RuleEntry>,
}

/// One `[[rule]]` table, exactly as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuleEntry {
    /// Optional label shown in the proposal (`rule #N` when absent).
    #[serde(default)]
    pub name: Option<String>,
    /// Pattern matched against the path relative to the policy root.
    pub match_glob: String,
    /// Substrings the same relative path must contain (all of them).
    #[serde(default)]
    pub match_contains: Option<OneOrMany>,
    /// Destination directory relative to the policy root.
    pub destination: String,
    /// Why the rule exists (shown in the proposal).
    #[serde(default)]
    pub reason: Option<String>,
}

/// `match_contains` accepts one string or a list of strings.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    /// The strings as a list.
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

/// Parse the policy text (structure only; rule semantics are checked when the
/// policy is built, which has the file path for one clear message).
pub(super) fn parse(text: &str) -> Result<PolicyFile> {
    toml::from_str(text).context("parse repository policy TOML")
}

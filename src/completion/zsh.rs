//! zsh-specific post-processing for generated completion scripts.
//!
//! `clap_complete` renders subcommands marked `hide = true` in every shell it
//! supports, and `clap` offers no way to detach a subcommand from a `Command`
//! tree once the derive has added it (upstream clap-rs/clap#1335). The zsh
//! script is therefore generated from the real tree and the artifacts of
//! hidden commands (and of anything below them, which is unreachable) are
//! removed line-wise. The rules are pinned by tests: if upstream ever learns
//! to skip hidden subcommands this filter quietly becomes a no-op and can be
//! deleted.

use clap::Command;

/// Insert the version marker right below the `#compdef` line.
///
/// compinit only registers a file whose first line is `#compdef` (measured
/// with zsh 5.9), so the marker must never take that place.
pub fn with_marker(script: String, marker: &str) -> String {
    match script.split_once('\n') {
        Some((first, rest)) => format!("{first}\n{marker}\n{rest}"),
        None => format!("{script}\n{marker}\n"),
    }
}

/// Case labels (command names and visible aliases) that a zsh script must not
/// offer: every hidden subcommand plus everything below it.
pub fn hidden_labels(cmd: &Command) -> Vec<String> {
    let mut labels = Vec::new();
    collect_labels(cmd, false, &mut labels);
    labels
}

fn collect_labels(cmd: &Command, unreachable: bool, labels: &mut Vec<String>) {
    for subcommand in cmd.get_subcommands() {
        let hidden = unreachable || subcommand.is_hide_set();
        if hidden {
            labels.push(subcommand.get_name().to_string());
            labels.extend(subcommand.get_visible_aliases().map(str::to_string));
        }
        collect_labels(subcommand, hidden, labels);
    }
}

/// Remove every artifact of `labels` from a generated zsh script.
pub fn strip_hidden(script: &str, labels: &[String]) -> String {
    if labels.is_empty() {
        return script.to_string();
    }
    let lines: Vec<&str> = script.split('\n').collect();
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    let mut index = 0;
    while index < lines.len() {
        if let Some(next) = skip_case_arm(&lines, index, labels) {
            index = next;
            continue;
        }
        if let Some(next) = skip_function(&lines, index, labels) {
            index = next;
            continue;
        }
        if is_command_entry(lines[index], labels) {
            index += 1;
            continue;
        }
        kept.push(lines[index]);
        index += 1;
    }
    kept.join("\n")
}

/// `(name)` case arm: skip the label and its body up to the matching `;;`.
///
/// Nested `case`/`esac` blocks inside the arm are tracked so that an arm of a
/// hidden command with children is removed as a whole.
fn skip_case_arm(lines: &[&str], start: usize, labels: &[String]) -> Option<usize> {
    let label = case_label(lines[start])?;
    if !labels.iter().any(|known| known == label) {
        return None;
    }
    let mut depth: usize = 0;
    let mut index = start + 1;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed.starts_with("case ") && trimmed.ends_with(" in") {
            depth += 1;
        } else if trimmed == "esac" {
            depth = depth.saturating_sub(1);
        } else if trimmed == ";;" && depth == 0 {
            return Some(index + 1);
        }
        index += 1;
    }
    Some(index)
}

/// Case label of a single line, when the line is a plain `(name)` arm.
fn case_label(line: &str) -> Option<&str> {
    let inner = line.trim().strip_prefix('(')?.strip_suffix(')')?;
    let plain = !inner.is_empty()
        && inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    plain.then_some(inner)
}

/// `(( $+functions[_kaseifu__subcmd__<path>_commands] )) ||`: skip the guard
/// line and the function definition that follows it.
fn skip_function(lines: &[&str], start: usize, labels: &[String]) -> Option<usize> {
    let trimmed = lines[start].trim();
    if !trimmed.starts_with("(( $+functions[") {
        return None;
    }
    if !labels.iter().any(|label| guard_mentions(trimmed, label)) {
        return None;
    }
    let mut index = start + 1;
    while index < lines.len() {
        let last = lines[index].trim() == "}";
        index += 1;
        if last {
            break;
        }
    }
    Some(index)
}

fn guard_mentions(guard: &str, label: &str) -> bool {
    guard.contains(&format!("__subcmd__{label}_commands]"))
        || guard.contains(&format!("__subcmd__{label}__subcmd__"))
}

/// `'name:help' \` command list entry.
fn is_command_entry(line: &str, labels: &[String]) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix('\'') else {
        return false;
    };
    let Some(colon) = rest.find(':') else {
        return false;
    };
    labels.iter().any(|label| label == &rest[..colon])
        && (trimmed.ends_with('\'') || trimmed.ends_with("' \\"))
}

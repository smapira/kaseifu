//! ZC-5 explanation view: how a decision met (or missed) the thresholds.
//!
//! Read-only: the verdict is produced by `policy::decision::evaluate_with`
//! and passed in; nothing here re-scores, re-selects or changes a decision.
//! The **effective** thresholds (user TOML overrides included) are carried
//! so both the human views and the additive `--explain` JSON show the same
//! numbers the policy actually used.

use crate::models::decision::{DecisionAction, JevDecision};
use crate::policy::decision::Thresholds;
use crate::presentation::components::path::{paint_path, path_budget, relative_display};
use crate::presentation::renderers::generic;
use crate::presentation::theme;
use std::path::Path;

/// How close to the auto-move thresholds still counts as "near".
///
/// The plan's example window (confidence 0.80-0.85 under the built-in
/// thresholds) is 0.05 wide.
pub const NEAR_MOVE_WINDOW: f64 = 0.05;

/// Key column width for the `--explain` rows (matches the decision rows).
const KEY_WIDTH: usize = 13;

/// Why the policy reached its verdict, with the numbers it used.
#[derive(Debug, Clone)]
pub struct Explanation {
    pub confidence: f64,
    pub margin: f64,
    /// The thresholds the policy evaluated with (effective, not default).
    pub thresholds: Thresholds,
    /// The verdict the policy produced (AutoMove / Review / Leave).
    pub policy: DecisionAction,
    pub destination_id: Option<String>,
    pub destination_path: Option<String>,
    /// How far confidence still is from the auto-move threshold (0 = met).
    pub confidence_gap: f64,
    /// How far margin still is from the auto-move threshold (0 = met).
    pub margin_gap: f64,
}

/// Build the explanation for one validated decision.
pub fn explain(
    decision: &JevDecision,
    policy: &DecisionAction,
    thresholds: Thresholds,
    destination_path: Option<&Path>,
) -> Explanation {
    Explanation {
        confidence: decision.confidence,
        margin: decision.margin,
        thresholds,
        policy: policy.clone(),
        destination_id: decision.destination_id.clone(),
        destination_path: destination_path.map(|path| path.to_string_lossy().into_owned()),
        confidence_gap: rounded_gap(thresholds.auto_move_confidence - decision.confidence),
        margin_gap: rounded_gap(thresholds.auto_move_margin - decision.margin),
    }
}

/// Distance to a threshold, floored at 0 and rounded to 6 decimals.
///
/// `0.85 - 0.84` is `0.010000000000000009` in binary floating point; the
/// display rounds to 3 decimals anyway, and a machine consumer should not
/// have to know about that noise.
fn rounded_gap(difference: f64) -> f64 {
    let gap = difference.max(0.0);
    (gap * 1_000_000.0).round() / 1_000_000.0
}

impl Explanation {
    /// The human word for the verdict (shared with the panel headings).
    pub fn verdict(&self) -> &'static str {
        generic::policy_view(&self.policy).0
    }

    /// The machine word for the verdict (stable JSON value).
    pub fn policy_word(&self) -> &'static str {
        match self.policy {
            DecisionAction::AutoMove(_) => "auto_move",
            DecisionAction::Review(_) => "review",
            _ => "leave",
        }
    }

    /// True when a non-auto verdict is within [`NEAR_MOVE_WINDOW`] of the
    /// auto-move thresholds: every unmet threshold must be close.
    pub fn near_auto_move(&self) -> bool {
        if matches!(self.policy, DecisionAction::AutoMove(_)) {
            return false;
        }
        if self.destination_id.as_deref().is_none_or(|id| id == "NONE") {
            return false;
        }
        let gaps = [self.confidence_gap, self.margin_gap];
        let unmet: Vec<f64> = gaps.iter().copied().filter(|gap| *gap > 0.0).collect();
        !unmet.is_empty() && unmet.iter().all(|gap| *gap <= NEAR_MOVE_WINDOW)
    }

    /// The near-miss line for the POLICY section, when one applies.
    pub fn near_line(&self) -> Option<String> {
        if !self.near_auto_move() {
            return None;
        }
        let mut missing = Vec::new();
        if self.confidence_gap > 0.0 {
            missing.push(format!("{:.3} more confidence", self.confidence_gap));
        }
        if self.margin_gap > 0.0 {
            missing.push(format!("{:.3} more margin", self.margin_gap));
        }
        Some(format!(
            "near auto move: {} would reach AUTO MOVE",
            missing.join(" and ")
        ))
    }

    /// The `--explain` rows: confidence / margin / thresholds / verdict /
    /// resolved destination in one screen.
    pub fn lines(&self, destination_display: &str) -> Vec<String> {
        vec![
            row(
                "Confidence",
                &format!(
                    "{:.3} (auto move needs >= {:.3})",
                    self.confidence, self.thresholds.auto_move_confidence
                ),
            ),
            row(
                "Margin",
                &format!(
                    "{:.3} (auto move needs >= {:.3})",
                    self.margin, self.thresholds.auto_move_margin
                ),
            ),
            row(
                "Thresholds",
                &format!(
                    "auto move: confidence >= {:.3} and margin >= {:.3}; review: confidence >= {:.3}",
                    self.thresholds.auto_move_confidence,
                    self.thresholds.auto_move_margin,
                    self.thresholds.review_confidence
                ),
            ),
            row("Verdict", self.verdict()),
            row("Destination", destination_display),
        ]
    }

    /// The additive `explanation` object for `--format json` (ZC-5).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "confidence": self.confidence,
            "margin": self.margin,
            "thresholds": {
                "auto_move_confidence": self.thresholds.auto_move_confidence,
                "auto_move_margin": self.thresholds.auto_move_margin,
                "review_confidence": self.thresholds.review_confidence,
            },
            "policy": self.policy_word(),
            "destination_id": self.destination_id,
            "destination_path": self.destination_path,
            "near_auto_move": self.near_auto_move(),
            "confidence_gap": self.confidence_gap,
            "margin_gap": self.margin_gap,
        })
    }
}

fn row(key: &str, value: &str) -> String {
    format!("{key:<KEY_WIDTH$}{value}")
}

/// `D1 - <directory>` for the resolved selection, mirroring the DECISION row:
/// `no destination` for `NONE`/no selection, `unresolved candidate` when the
/// selected ID has no discovered directory.
pub fn destination_display(
    id: Option<&str>,
    path: Option<&Path>,
    rich: bool,
    color: bool,
) -> String {
    let painted = |id: &str| theme::paint(id, theme::ANSI_VIOLET, color);
    match id {
        Some("NONE") => format!("{} - no destination", painted("NONE")),
        None => "no destination".to_string(),
        Some(id) => match path {
            Some(path) => {
                let text = path.to_string_lossy();
                let shown = if rich {
                    paint_path(&text, path_budget(), color)
                } else {
                    relative_display(&text)
                };
                format!("{} - {shown}", painted(id))
            }
            None => format!("{} - unresolved candidate", painted(id)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::decision::JevDecision;

    fn decision(destination: Option<&str>, confidence: f64, margin: f64) -> JevDecision {
        JevDecision {
            destination_id: destination.map(str::to_string),
            confidence,
            margin,
            ..JevDecision::default()
        }
    }

    fn explanation(confidence: f64, margin: f64) -> Explanation {
        let decision = decision(Some("D1"), confidence, margin);
        let policy = crate::policy::decision::evaluate_with(&decision, Thresholds::default());
        explain(&decision, &policy, Thresholds::default(), None)
    }

    #[test]
    fn near_line_names_only_the_unmet_thresholds() {
        assert_eq!(
            explanation(0.840, 0.500).near_line().as_deref(),
            Some("near auto move: 0.010 more confidence would reach AUTO MOVE")
        );
        assert_eq!(
            explanation(0.840, 0.380).near_line().as_deref(),
            Some(
                "near auto move: 0.010 more confidence and 0.020 more margin would reach AUTO MOVE"
            )
        );
    }

    #[test]
    fn near_line_is_absent_outside_the_window_or_when_decided() {
        assert!(explanation(0.780, 0.500).near_line().is_none());
        assert!(explanation(0.840, 0.200).near_line().is_none());
        assert!(explanation(0.900, 0.500).near_line().is_none());
        let none = explanation_for_none();
        assert!(none.near_line().is_none());
    }

    fn explanation_for_none() -> Explanation {
        let decision = decision(Some("NONE"), 0.840, 0.500);
        let policy = crate::policy::decision::evaluate_with(&decision, Thresholds::default());
        explain(&decision, &policy, Thresholds::default(), None)
    }

    #[test]
    fn effective_thresholds_are_used_not_the_defaults() {
        let thresholds = Thresholds {
            auto_move_confidence: 0.70,
            auto_move_margin: 0.10,
            review_confidence: 0.40,
        };
        let decision = decision(Some("D1"), 0.680, 0.090);
        let policy = crate::policy::decision::evaluate_with(&decision, thresholds);
        let explanation = explain(&decision, &policy, thresholds, None);
        assert_eq!(explanation.verdict(), "REVIEW");
        assert_eq!(
            explanation.near_line().as_deref(),
            Some(
                "near auto move: 0.020 more confidence and 0.010 more margin would reach AUTO MOVE"
            )
        );
        let lines = explanation.lines("D1 - /data/sinks");
        assert!(lines.iter().any(|line| line.contains("needs >= 0.700")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("review: confidence >= 0.400"))
        );
    }

    #[test]
    fn json_object_carries_every_number_and_the_resolved_destination() {
        let decision = decision(Some("D2"), 0.840, 0.380);
        let policy = crate::policy::decision::evaluate_with(&decision, Thresholds::default());
        let explanation = explain(
            &decision,
            &policy,
            Thresholds::default(),
            Some(Path::new("/data/sinks/State Of Mind Labs")),
        );
        let json = explanation.to_json();
        assert_eq!(json["confidence"], 0.840);
        assert_eq!(json["margin"], 0.380);
        assert_eq!(json["thresholds"]["auto_move_confidence"], 0.85);
        assert_eq!(json["thresholds"]["auto_move_margin"], 0.40);
        assert_eq!(json["thresholds"]["review_confidence"], 0.60);
        assert_eq!(json["policy"], "review");
        assert_eq!(json["destination_id"], "D2");
        assert_eq!(json["destination_path"], "/data/sinks/State Of Mind Labs");
        assert_eq!(json["near_auto_move"], true);
        assert_eq!(json["confidence_gap"], 0.01);
        assert_eq!(json["margin_gap"], 0.02);
    }

    #[test]
    fn destination_display_mirrors_the_decision_row() {
        assert_eq!(
            destination_display(Some("D1"), None, false, false),
            "D1 - unresolved candidate"
        );
        assert_eq!(
            destination_display(Some("NONE"), None, false, false),
            "NONE - no destination"
        );
        assert_eq!(
            destination_display(None, None, false, false),
            "no destination"
        );
        assert_eq!(
            destination_display(Some("D1"), Some(Path::new("/data/sinks")), false, false),
            "D1 - /data/sinks"
        );
    }
}

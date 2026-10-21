//! `--keywords`: extra decision material the user hands to Jev.
//!
//! Two small pieces live here:
//!
//! - [`KeywordDelivery`] is what a report needs to say about the flag: what
//!   the user provided, and what actually reached the Jev request (empty when
//!   Jev was never consulted - a repository rule hit, or no candidates).
//! - [`KeywordJevIntegration`] is the boundary decorator that attaches the
//!   keywords to the `DecisionContext` handed to the inner implementation and
//!   records whether that happened. It follows the `ObservingJevIntegration`
//!   pattern: the `JevIntegration` trait and every pipeline signature stay
//!   unchanged, so `organize` and `classify` share one code path.
//!
//! Keywords are decision material only: they are never merged into the
//! normalized file context, so Discovery's mdfind query cannot see them.
//! The wire encoding (`user: <KEY>` entries appended to the existing
//! `state.semantic_signals` list) belongs to the adapter in
//! [`crate::classifier::jev`]; this module deliberately holds no HTTP and no
//! serde of its own beyond the additive report JSON.

use super::jev::{JevError, JevIntegration, JevResponse};
use crate::models::decision::DecisionContext;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};

/// Prefix that marks a `state.semantic_signals` entry as user-provided.
///
/// Existing signals use `M:`/`K:`/`F:`/`X:`/`FB:` or bare words; `user: ` is
/// reserved so a keyword can never be mistaken for a derived signal.
pub const USER_KEYWORD_PREFIX: &str = "user: ";

/// What the user asked Jev to consider, and what actually went over the wire.
///
/// `sent` holds the literal `user: <KEY>` entries the adapter writes into
/// `state.semantic_signals`, so a report answer "what did Jev receive?" with
/// the exact request strings. It is empty whenever `decide` was never called,
/// even when `provided` is not.
#[derive(Debug, Clone, Default)]
pub struct KeywordDelivery {
    /// The keywords as given on the command line (already validated).
    pub provided: Vec<String>,
    /// The `user: <KEY>` entries actually sent; `[]` when Jev was not asked.
    pub sent: Vec<String>,
}

impl KeywordDelivery {
    /// Keywords that were provided but never reached Jev (rule hit path).
    pub fn unused(provided: &[String]) -> Self {
        Self {
            provided: provided.to_vec(),
            sent: Vec::new(),
        }
    }

    /// True when the flag was not given; reports stay silent then so the
    /// no-keywords output keeps its exact historical shape.
    pub fn is_empty(&self) -> bool {
        self.provided.is_empty()
    }

    /// The wire entries for `sent`, derived from the same prefix rule the
    /// adapter applies.
    pub fn wire_entries(keywords: &[String]) -> Vec<String> {
        keywords
            .iter()
            .map(|keyword| format!("{USER_KEYWORD_PREFIX}{keyword}"))
            .collect()
    }

    /// The additive `--format json` object: `{provided, sent_to_jev}`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "provided": self.provided,
            "sent_to_jev": self.sent,
        })
    }

    /// The two `key value` rows shared by the organize and classify views
    /// (same 13-column key width as every other section row).
    pub fn rows(&self) -> Vec<String> {
        let sent = if self.sent.is_empty() {
            "not used (Jev was not consulted)".to_string()
        } else {
            self.sent.join(", ")
        };
        vec![
            format!("{:<13}{}", "Provided", self.provided.join(", ")),
            format!("{:<13}{}", "Sent to Jev", sent),
        ]
    }
}

/// Boundary decorator that hands `--keywords` to the inner Jev implementation.
///
/// On `decide` it clones the context, attaches the keywords as user input and
/// records that the consultation happened; the response passes through
/// verbatim. With no keywords the context is forwarded untouched (no clone,
/// no record) - an undecorated run behaves exactly as before.
pub struct KeywordJevIntegration<'a> {
    inner: &'a dyn JevIntegration,
    keywords: &'a [String],
    consulted: AtomicBool,
}

impl<'a> KeywordJevIntegration<'a> {
    /// Wrap `inner`; `keywords` are attached to every decision context.
    pub fn new(inner: &'a dyn JevIntegration, keywords: &'a [String]) -> Self {
        Self {
            inner,
            keywords,
            consulted: AtomicBool::new(false),
        }
    }

    /// The report data after a run: what was provided, plus the wire entries
    /// when (and only when) `decide` was actually called.
    pub fn delivery(&self) -> KeywordDelivery {
        let provided = self.keywords.to_vec();
        let sent = if self.consulted.load(Ordering::SeqCst) {
            KeywordDelivery::wire_entries(&provided)
        } else {
            Vec::new()
        };
        KeywordDelivery { provided, sent }
    }
}

impl JevIntegration for KeywordJevIntegration<'_> {
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>> {
        if self.keywords.is_empty() {
            return self.inner.decide(context);
        }
        self.consulted.store(true, Ordering::SeqCst);
        let context = context.clone().with_user_keywords(self.keywords);
        let inner = self.inner;
        Box::pin(async move { inner.decide(&context).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::mock_jev::MockJevIntegration;

    #[test]
    fn unused_delivery_keeps_provided_and_sends_nothing() {
        let provided = vec!["invoice".to_string()];
        let delivery = KeywordDelivery::unused(&provided);
        assert_eq!(delivery.provided, provided);
        assert!(delivery.sent.is_empty());
        assert!(!delivery.is_empty(), "provided keywords are reported");
        let json = delivery.to_json();
        assert_eq!(json["provided"], serde_json::json!(["invoice"]));
        assert_eq!(json["sent_to_jev"], serde_json::json!([]));
    }

    #[test]
    fn default_delivery_is_silent() {
        let delivery = KeywordDelivery::default();
        assert!(delivery.is_empty());
    }

    #[test]
    fn rows_name_provided_and_sent_entries() {
        let delivery = KeywordDelivery {
            provided: vec!["invoice".to_string(), "receipt".to_string()],
            sent: KeywordDelivery::wire_entries(&["invoice".to_string(), "receipt".to_string()]),
        };
        let rows = delivery.rows();
        assert!(rows[0].contains("invoice, receipt"), "got {rows:?}");
        assert!(
            rows[1].contains("user: invoice, user: receipt"),
            "got {rows:?}"
        );
    }

    #[test]
    fn rows_say_when_jev_was_not_consulted() {
        let delivery = KeywordDelivery::unused(&["invoice".to_string()]);
        let rows = delivery.rows();
        assert!(
            rows[1].contains("not used (Jev was not consulted)"),
            "got {rows:?}"
        );
    }

    #[tokio::test]
    async fn decorator_passes_keywords_into_the_context_and_stays_transparent() {
        let mock = MockJevIntegration::select_d1(0.9);
        let keywords = vec!["invoice".to_string()];
        let decorated = KeywordJevIntegration::new(&mock, &keywords);
        let context = DecisionContext::from_candidates(vec!["D1".to_string()]);

        let response = decorated.decide(&context).await.expect("mock responds");

        // The mock ignores the context, so transparency is checked against the
        // recorded delivery and the untouched original context.
        let delivery = decorated.delivery();
        assert_eq!(delivery.provided, keywords);
        assert_eq!(delivery.sent, vec!["user: invoice"]);
        assert!(
            context.user_keywords().is_empty(),
            "the input context is not mutated"
        );
        assert_eq!(response.destination_id.as_deref(), Some("D1"));
    }

    #[tokio::test]
    async fn decorator_without_keywords_never_records_a_consultation() {
        let mock = MockJevIntegration::select_none(0.7);
        let decorated = KeywordJevIntegration::new(&mock, &[]);
        let context = DecisionContext::from_candidates(vec!["D1".to_string()]);

        let _ = decorated.decide(&context).await.expect("mock responds");

        let delivery = decorated.delivery();
        assert!(delivery.is_empty());
        assert!(delivery.sent.is_empty());
    }
}

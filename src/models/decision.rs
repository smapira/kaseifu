use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevDecision {
    pub destination_id: Option<String>,
    pub confidence: f64,
    pub margin: f64,
    pub action: DecisionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAction {
    AutoMove,
    Review,
    Leave,
}

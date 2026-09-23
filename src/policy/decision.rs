use crate::models::decision::DecisionAction;

pub fn decide(confidence: f64, margin: f64) -> DecisionAction {
    if confidence >= 0.85 && margin >= 0.40 {
        DecisionAction::AutoMove("best_match".into())
    } else if confidence >= 0.60 {
        DecisionAction::Review(String::new())
    } else {
        DecisionAction::Leave {
            what: "none".into(),
        }
    }
}

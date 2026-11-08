pub mod domain;
pub mod normalize;

use domain::DecisionProposal;

pub fn proposal_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(DecisionProposal))
        .expect("DecisionProposal schema must serialize")
}

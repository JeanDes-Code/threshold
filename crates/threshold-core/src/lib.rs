pub mod domain;
pub mod model;
pub mod normalize;

pub use model::{Gm, GmError, Prompt, RecordedGm};

use domain::DecisionProposal;

pub fn proposal_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(DecisionProposal))
        .expect("DecisionProposal schema must serialize")
}

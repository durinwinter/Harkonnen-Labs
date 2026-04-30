use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BriefingScope {
    CoobiePreflight,
    ScoutPreflight,
    MasonPreflight,
    PiperPreflight,
    SablePreflight,
    CoobieConsolidation,
    OperatorQuery,
}

impl std::fmt::Display for BriefingScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            BriefingScope::CoobiePreflight => "coobie_preflight",
            BriefingScope::ScoutPreflight => "scout_preflight",
            BriefingScope::MasonPreflight => "mason_preflight",
            BriefingScope::PiperPreflight => "piper_preflight",
            BriefingScope::SablePreflight => "sable_preflight",
            BriefingScope::CoobieConsolidation => "coobie_consolidation",
            BriefingScope::OperatorQuery => "operator_query",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextSection {
    ProjectInterview,
    OperatorModel,
    SoulIdentity,
}

impl std::fmt::Display for ContextSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            ContextSection::ProjectInterview => "project_interview",
            ContextSection::OperatorModel => "operator_model",
            ContextSection::SoulIdentity => "soul_identity",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextTarget {
    pub scope: BriefingScope,
    pub task_description: String,
    pub token_budget: u32,
    pub min_hits: u32,
    #[serde(default)]
    pub required_sections: Vec<ContextSection>,
}

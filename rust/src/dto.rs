use serde::{self, Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct InsrtRequest {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ScoreValue {
    pub score: u32,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ScoreRange {
    pub min_score: u32,
    pub max_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct UpdateCluster {
    pub hosts: Vec<String>,
    pub index: u32,
}

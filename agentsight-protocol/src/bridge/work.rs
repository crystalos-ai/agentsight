//! Closed metadata about the session's first user request.
//!
//! This is a source classification, not verified intent or a task outcome. No
//! field can hold prompt text, a title, a path or a free-form summary. It never
//! selects policy. Unknown or malformed optional work is withheld while the
//! surrounding host-session observation remains usable.

use serde::{Deserialize, Deserializer, Serialize};

/// A metadata-only classification of one session's first user request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "WorkWire")]
pub struct SessionWork {
    /// Version of the closed work vocabulary.
    pub schema_version: WorkSchema,
    /// The source the producer classified.
    pub source: WorkSource,
    /// Which part of that source the classification describes.
    pub basis: WorkBasis,
    /// How the source derived these labels; not a semantic-model claim.
    pub method: WorkMethod,
    /// Overlapping work labels, in source order, without duplicates.
    pub categories: Vec<WorkCategory>,
    /// Overlapping coarse subjects, in source order, without duplicates.
    pub subjects: Vec<WorkSubject>,
}

/// Supported work schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkSchema {
    /// The initial closed vocabulary.
    #[serde(rename = "session-work/1")]
    V1,
}

/// Source of the classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkSource {
    /// Source-native user request text, kept on the host.
    UserPrompt,
}

/// The part of the source described by the work labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkBasis {
    /// The initial user request, not subsequent work or a completion claim.
    FirstUserRequest,
}

/// Classification algorithm reported by the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkMethod {
    /// Versioned lexical suggestions from request text, not tool traffic.
    #[serde(rename = "lexical/v1")]
    LexicalV1,
}

/// A work label from the closed session-work/1 vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkCategory {
    /// Security.
    Security,
    /// Bug.
    Bug,
    /// Performance.
    Performance,
    /// Edge case.
    EdgeCase,
    /// Quality.
    Quality,
    /// Fix.
    Fix,
    /// Feature.
    Feature,
    /// Refactor.
    Refactor,
    /// Test.
    Test,
    /// Ci.
    Ci,
    /// Docs.
    Docs,
    /// Chore.
    Chore,
    /// Build.
    Build,
    /// Revert.
    Revert,
}

/// A coarse subject; never a repository, file or customer name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkSubject {
    /// Authentication.
    Authentication,
    /// Authorization.
    Authorization,
    /// Api.
    Api,
    /// Ui.
    Ui,
    /// Database.
    Database,
    /// Networking.
    Networking,
    /// Files.
    Files,
    /// Observability.
    Observability,
    /// Dependencies.
    Dependencies,
    /// Build pipeline.
    BuildPipeline,
    /// Deployment.
    Deployment,
    /// Testing.
    Testing,
    /// Documentation.
    Documentation,
    /// Configuration.
    Configuration,
    /// Agent tools.
    AgentTools,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkWire {
    schema_version: WorkSchema,
    source: WorkSource,
    basis: WorkBasis,
    method: WorkMethod,
    categories: Vec<WorkCategory>,
    subjects: Vec<WorkSubject>,
}

impl TryFrom<WorkWire> for SessionWork {
    type Error = &'static str;

    fn try_from(value: WorkWire) -> Result<Self, Self::Error> {
        if value.categories.len() > 14 || value.subjects.len() > 15 {
            return Err("work exceeds the closed vocabulary bounds");
        }
        Ok(Self {
            schema_version: value.schema_version,
            source: value.source,
            basis: value.basis,
            method: value.method,
            categories: distinct(value.categories),
            subjects: distinct(value.subjects),
        })
    }
}

fn distinct<T: PartialEq>(values: Vec<T>) -> Vec<T> {
    let mut kept = Vec::with_capacity(values.len());
    for value in values {
        if !kept.contains(&value) {
            kept.push(value);
        }
    }
    kept
}

/// Parse the optional extension without discarding the basic session row.
pub(crate) fn deserialize_optional<'de, D>(deserializer: D) -> Result<Option<SessionWork>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = ciborium::Value::deserialize(deserializer)?;
    Ok(value.deserialized().ok())
}

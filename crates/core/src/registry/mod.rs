//! The registry model (DESIGN §5): every variable part of the system is
//! a registry of small types behind a trait. A flow is **data** that
//! references registry entries by key; behavior lives here.
//!
//! Adding a capability = one type implementing the trait + a line in
//! `v1()`. No frontend change, no migration: the new entry appears in
//! the served schema, defaults to "any"/identity, and is ignored by
//! existing flows (which simply lack its key).

pub mod condition;
pub mod operation;

use std::path::Path;

use serde_json::Value;

use crate::error::Result;
use crate::facts::FileFacts;
use crate::plan::FfmpegPlan;

/// A condition field: one constraint axis of a step's condition
/// (DESIGN §5, §11 — v1: container, video codec, resolution, pixel
/// format, HDR, audio codec set, file size).
///
/// Implementations are **pure**: matching runs against cached
/// [`FileFacts`] only — never I/O.
pub trait ConditionField: Send + Sync {
    /// Stable key used in flow JSON and the UI schema.
    fn key(&self) -> &'static str;

    /// Human-readable description, shown in the editor and schema.
    fn description(&self) -> &'static str;

    /// Match this field's constraint (the raw JSON from a flow) against
    /// file facts. An "any" constraint (`{}` or empty form) matches
    /// everything. Errors on malformed constraint data — a broken
    /// constraint must surface, not silently match (or not) everything.
    fn match_facts(&self, constraint: &Value, facts: &FileFacts) -> Result<bool>;

    /// UI picker description: enough for the schema-driven editor to
    /// render this field (kind, value list, units, hints). No hardcoded
    /// field lists ever live in the frontend (DESIGN §5).
    fn ui_schema(&self) -> Value;
}

/// An operation section: one media domain of a step's operation
/// (DESIGN §5, §11 — v1: video, audio, subtitles).
///
/// Implementations are **pure**: planning runs against cached facts.
pub trait OperationSection: Send + Sync {
    /// Stable key used in flow JSON and the UI schema.
    fn key(&self) -> &'static str;

    /// Human-readable description, shown in the editor and schema.
    fn description(&self) -> &'static str;

    /// Plan this domain for a matched file. Returns
    /// [`SectionPlan::Identity`] when the parameters would change nothing
    /// for this file (that is what makes it compliant).
    fn plan(&self, params: &Value, facts: &FileFacts) -> Result<SectionPlan>;

    /// UI schema for this section's parameters (DESIGN §5).
    fn ui_schema(&self) -> Value;
}

/// A fact extractor: turns a file into [`FileFacts`] (DESIGN §5).
///
/// The v1 implementation (ffprobe child process) lives in
/// `transcodarr-server` — this trait only fixes the contract; new
/// sources/formats add entries here.
pub trait FactExtractor: Send + Sync {
    /// Stable key (v1: `"ffprobe"`).
    fn key(&self) -> &'static str;

    /// Probe the file at `path`. I/O happens in the implementor.
    fn probe(&self, path: &Path) -> std::io::Result<FileFacts>;
}

/// A verification check: validates a finished output **before** the
/// original is touched (DESIGN §3.3, §5).
///
/// The default check compares output facts against the plan (codec,
/// duration window, stream inventory); the optional full-decode check is
/// a second entry. Implementations live in `transcodarr-server` (they
/// probe the output file); the pure comparison logic is in
/// [`crate::verify`].
pub trait VerificationCheck: Send + Sync {
    /// Stable key (v1: `"metadata"`, `"decode"`).
    fn key(&self) -> &'static str;

    /// Human-readable description.
    fn description(&self) -> &'static str;

    /// Check `output` (facts probed from the job's output file) against
    /// the plan and the source facts. I/O happens in the implementor.
    fn verify(
        &self,
        plan: &FfmpegPlan,
        input: &FileFacts,
        output: &FileFacts,
    ) -> std::io::Result<bool>;
}

/// The plan a matched operation produces for one media domain.
#[derive(Debug, Clone, PartialEq)]
pub enum SectionPlan {
    /// The section would change nothing for this file.
    Identity,
    /// The video domain plan (re-encode or downscale/hdr conversion).
    Video(crate::plan::VideoPlan),
    /// The audio domain plan (per-track decisions).
    Audio(crate::plan::AudioPlan),
    /// The subtitle domain plan.
    Subtitles(crate::plan::SubtitlePlan),
}

/// The set of registry entries available in this build.
///
/// Pure entries (condition fields, operation sections) are registered by
/// [`Registry::v1`]; I/O entries (fact extractors, verification checks,
/// device-backed capabilities) are added by the server at startup.
pub struct Registry {
    condition_fields: Vec<Box<dyn ConditionField>>,
    operation_sections: Vec<Box<dyn OperationSection>>,
    fact_extractors: Vec<Box<dyn FactExtractor>>,
    verification_checks: Vec<Box<dyn VerificationCheck>>,
}

impl Registry {
    /// The v1 registry: all v1 condition fields and operation sections
    /// (DESIGN §5–§6). I/O-bound entries are added by the server.
    #[must_use]
    pub fn v1() -> Self {
        Self {
            condition_fields: condition::v1_condition_fields(),
            operation_sections: operation::v1_operation_sections(),
            fact_extractors: Vec::new(),
            verification_checks: Vec::new(),
        }
    }

    /// Register an I/O-bound fact extractor (server-side).
    pub fn with_fact_extractor(&mut self, e: impl FactExtractor + 'static) -> &mut Self {
        self.fact_extractors.push(Box::new(e));
        self
    }

    /// Register an I/O-bound verification check (server-side).
    pub fn with_verification_check(&mut self, c: impl VerificationCheck + 'static) -> &mut Self {
        self.verification_checks.push(Box::new(c));
        self
    }

    /// Look up a condition field by its flow-JSON key.
    #[must_use]
    pub fn condition_field(&self, key: &str) -> Option<&(dyn ConditionField + 'static)> {
        self.condition_fields
            .iter()
            .find(|f| f.key() == key)
            .map(|b| b.as_ref())
    }

    /// Look up an operation section by its flow-JSON key.
    #[must_use]
    pub fn operation_section(&self, key: &str) -> Option<&(dyn OperationSection + 'static)> {
        self.operation_sections
            .iter()
            .find(|s| s.key() == key)
            .map(|b| b.as_ref())
    }

    /// All condition fields (schema generation, validation).
    pub fn condition_fields(&self) -> impl Iterator<Item = &(dyn ConditionField + 'static)> + '_ {
        self.condition_fields.iter().map(|b| b.as_ref())
    }

    /// All operation sections (schema generation, validation).
    pub fn operation_sections(
        &self,
    ) -> impl Iterator<Item = &(dyn OperationSection + 'static)> + '_ {
        self.operation_sections.iter().map(|b| b.as_ref())
    }

    /// All registered fact extractors.
    pub fn fact_extractors(&self) -> impl Iterator<Item = &(dyn FactExtractor + 'static)> + '_ {
        self.fact_extractors.iter().map(|b| b.as_ref())
    }

    /// All registered verification checks.
    pub fn verification_checks(
        &self,
    ) -> impl Iterator<Item = &(dyn VerificationCheck + 'static)> + '_ {
        self.verification_checks.iter().map(|b| b.as_ref())
    }
}

//! v1 operation sections (DESIGN §5–§6): video, audio, container,
//! subtitles. Adding an operation section = one file here implementing
//! [`super::OperationSection`] + one line in [`v1_operation_sections`].

pub mod audio;
pub mod container;
pub mod subtitles;
pub mod video;

use super::OperationSection;

/// All v1 operation sections, in display order.
#[must_use]
pub fn v1_operation_sections() -> Vec<Box<dyn OperationSection>> {
    vec![
        Box::new(video::Video),
        Box::new(audio::Audio),
        Box::new(container::Container),
        Box::new(subtitles::Subtitles),
    ]
}

/// Parse a section's raw parameter JSON into its typed form, with the
/// section's key in the error on failure.
pub(super) fn parse<T: serde::de::DeserializeOwned>(
    key: &str,
    value: &serde_json::Value,
) -> crate::error::Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|e| crate::error::CoreError::Flow(format!("{key}: {e}")))
}

//! v1 condition fields (DESIGN §5, §11).
//!
//! Adding a condition field = one file here implementing
//! [`super::ConditionField`] + one line in [`v1_condition_fields`].

mod audio_codec;
mod container;
mod file_size;
mod hdr;
mod pixel_format;
mod resolution;
mod video_codec;

use serde_json::Value;

use super::ConditionField;

/// Parse a field's raw constraint JSON into its typed form, with the
/// field's key in the error on failure.
pub(super) fn parse<T: serde::de::DeserializeOwned>(
    key: &str,
    value: &Value,
) -> crate::error::Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|e| crate::error::CoreError::Flow(format!("{key}: {e}")))
}

/// All v1 condition fields, in display order.
#[must_use]
pub fn v1_condition_fields() -> Vec<Box<dyn ConditionField>> {
    vec![
        Box::new(container::Container),
        Box::new(video_codec::VideoCodec),
        Box::new(resolution::Resolution),
        Box::new(pixel_format::PixelFormat),
        Box::new(hdr::HdrField),
        Box::new(audio_codec::AudioCodec),
        Box::new(file_size::FileSize),
    ]
}

//! The served flow schema (DESIGN §5): everything the frontend editor
//! needs to render flows, generated from the registry at runtime.
//!
//! The frontend contains **no hardcoded field lists**: this endpoint's
//! output is the single source of truth for what a flow can say.
//! New registry entries appear here automatically — that is what makes
//! "add a capability = one type + one line" true end to end.

use serde_json::{Map, Value, json};

use crate::device::Device;
use crate::evaluate::FLOW_VERSION;
use crate::registry::Registry;

/// The `/api/schema/flow` payload (DESIGN §5).
///
/// `devices` are the detected transcoding devices (startup encoder
/// probe); the `device` field of a `video` section is rendered as a
/// picker over them, with unavailable encoders disabled.
#[must_use]
pub fn flow_schema(registry: &Registry, devices: &[Device]) -> Value {
    let mut condition_fields = Map::new();
    for field in registry.condition_fields() {
        condition_fields.insert(
            field.key().to_string(),
            json!({
                "description": field.description(),
                "schema": field.ui_schema(),
            }),
        );
    }

    let mut operation_sections = Map::new();
    for section in registry.operation_sections() {
        operation_sections.insert(
            section.key().to_string(),
            json!({
                "description": section.description(),
                "schema": section.ui_schema(),
            }),
        );
    }

    let device_list: Vec<Value> = devices
        .iter()
        .map(|d| {
            json!({
                "id": d.id,
                "kind": d.kind,
                "name": d.name,
                "max_concurrent": d.max_concurrent,
                "encoders": d.encoders.iter().map(|e| json!({
                    "name": e.name,
                    "target": e.target,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    json!({
        "flow_version": FLOW_VERSION,
        "condition_fields": condition_fields,
        "operation_sections": operation_sections,
        "devices": device_list,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceKind;

    #[test]
    fn schema_contains_all_v1_entries() {
        let r = Registry::v1();
        let s = flow_schema(&r, &[]);
        let cf = s.get("condition_fields").unwrap();
        for key in [
            "container",
            "video_codec",
            "resolution",
            "pixel_format",
            "hdr",
            "audio_codec",
            "file_size",
        ] {
            assert!(cf.get(key).is_some(), "missing {key}");
        }
        let os = s.get("operation_sections").unwrap();
        for key in ["video", "audio", "container", "subtitles"] {
            assert!(os.get(key).is_some(), "missing {key}");
        }
        assert_eq!(s.get("flow_version").unwrap(), &Value::from(FLOW_VERSION));
    }

    #[test]
    fn schema_lists_devices() {
        let r = Registry::v1();
        let cpu = Device::cpu(2);
        let mut gpu = Device {
            id: "gpu-0".into(),
            kind: DeviceKind::Gpu,
            name: "NVIDIA".into(),
            ..Default::default()
        };
        gpu.encoders.push(crate::device::Encoder {
            name: "h264_nvenc".into(),
            target: crate::plan::VideoTargetCodec::H264,
        });
        let s = flow_schema(&r, &[cpu, gpu]);
        let devs = s.get("devices").unwrap();
        assert_eq!(devs.as_array().unwrap().len(), 2);
    }
}

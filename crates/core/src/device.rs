use serde::{Deserialize, Serialize};

use crate::plan::VideoTargetCodec;

/// A transcoding device (DESIGN §5, §8): the CPU pseudo-device, or one
/// detected hardware encoder.
///
/// Devices are detected at startup by the server (encoder probe) and
/// persisted in the `devices` table. The `encoders` list is the set of
/// **video** encoders this device can run — it drives the flow editor's
/// device picker and dispatch selection.
///
/// The ffmpeg hardware-acceleration flag is derived from the encoder
/// name's suffix at argv time (`_nvenc` → cuda, `_vaapi` → vaapi,
/// `_qsv` → qsv), so no vendor field is needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Stable id (e.g. `"cpu"`, `"gpu-nvidia-0"`); referenced by
    /// `jobs.device_id` and by the flow's `device` picker.
    pub id: String,
    #[serde(default = "default_kind")]
    pub kind: DeviceKind,
    /// Human-readable name shown in the UI.
    pub name: String,
    /// The video encoders this device can run.
    #[serde(default)]
    pub encoders: Vec<Encoder>,
    /// Maximum simultaneous jobs on this device.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
}

fn default_kind() -> DeviceKind {
    DeviceKind::Cpu
}

fn default_max_concurrent() -> u32 {
    1
}

/// Device class. The CPU pseudo-device (`id: "cpu"`) is always present
/// and always works; GPU devices only appear when detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    #[default]
    Cpu,
    Gpu,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            id: "cpu".into(),
            kind: DeviceKind::Cpu,
            name: "CPU (libx264/libx265)".into(),
            encoders: vec![],
            max_concurrent: 1,
        }
    }
}

impl Device {
    /// The canonical CPU pseudo-device (DESIGN §8: always available,
    /// software encoders, never fails on missing hardware).
    #[must_use]
    pub fn cpu(max_concurrent: u32) -> Self {
        let max_concurrent = max_concurrent.max(1);
        Self {
            id: "cpu".into(),
            kind: DeviceKind::Cpu,
            name: "CPU (libx264/libx265)".into(),
            encoders: vec![
                Encoder {
                    name: "libx264".into(),
                    target: VideoTargetCodec::H264,
                },
                Encoder {
                    name: "libx265".into(),
                    target: VideoTargetCodec::Hevc,
                },
            ],
            max_concurrent,
        }
    }

    /// Whether this device can run `encoder` (by name).
    #[must_use]
    pub fn can_run(&self, encoder: &str) -> bool {
        self.encoders
            .iter()
            .any(|e| e.name == encoder || e.name.starts_with(encoder))
    }
}

/// One video encoder a device can run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encoder {
    /// ffmpeg encoder name (`"libx264"`, `"h264_nvenc"`, …).
    pub name: String,
    /// The video codec it produces.
    pub target: VideoTargetCodec,
}

/// The hardware-acceleration flag implied by an encoder name's suffix.
///
/// `h264_nvenc` → `("h264", "cuda")`; `h264_vaapi` → `("h264", "vaapi")`;
/// `hevc_qsv` → `("hevc", "qsv")`; CPU encoders → `("…", None)`.
#[must_use]
pub fn hwaccel_for(encoder: &str) -> (String, Option<&'static str>) {
    let base = encoder
        .strip_suffix("_nvenc")
        .or_else(|| encoder.strip_suffix("_vaapi"))
        .or_else(|| encoder.strip_suffix("_qsv"));
    match base {
        Some(b) => {
            let accel = if encoder.ends_with("_nvenc") {
                "cuda"
            } else if encoder.ends_with("_vaapi") {
                "vaapi"
            } else {
                "qsv"
            };
            (b.to_string(), Some(accel))
        }
        None => (encoder.to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_pseudo_device_always_present() {
        let cpu = Device::cpu(4);
        assert_eq!(cpu.id, "cpu");
        assert_eq!(cpu.kind, DeviceKind::Cpu);
        assert!(cpu.can_run("libx264"));
        assert!(cpu.can_run("libx265"));
        assert_eq!(cpu.max_concurrent, 4);
    }

    #[test]
    fn hwaccel_suffix_mapping() {
        assert_eq!(hwaccel_for("h264_nvenc"), ("h264".into(), Some("cuda")));
        assert_eq!(hwaccel_for("hevc_nvenc"), ("hevc".into(), Some("cuda")));
        assert_eq!(hwaccel_for("h264_vaapi"), ("h264".into(), Some("vaapi")));
        assert_eq!(hwaccel_for("hevc_qsv"), ("hevc".into(), Some("qsv")));
        assert_eq!(hwaccel_for("libx264"), ("libx264".into(), None));
    }
}

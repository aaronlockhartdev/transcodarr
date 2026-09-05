//! Startup device detection (DESIGN §11): probe the real ffmpeg
//! binary for hardware encoders with a 4-frame micro-encode.
//!
//! A device is reported only if at least one of its encoders
//! test-encodes. The CPU pseudo-device is always present — it is
//! the canonical fallback.

use transcodarr_core::device::{Device, DeviceKind, Encoder};

/// Detect the devices the given ffmpeg binary can use.
///
/// Each GPU stack (NVENC, VAAPI, QSV) is probed by test-encoding a
/// 4-frame 32×32 lavfi source with each of its encoders to
/// `/dev/null`. A stack whose encoders all fail is omitted. This is
/// expensive (a few seconds per stack) and runs once at startup —
/// never on the hot path.
pub fn detect(ffmpeg: &str) -> Vec<Device> {
    let mut out = Vec::new();
    out.push(Device::cpu(1));

    let stacks: [(&str, [(&str, &str); 2]); 3] = [
        ("nvenc", [("h264_nvenc", "h264"), ("hevc_nvenc", "hevc")]),
        ("vaapi", [("h264_vaapi", "h264"), ("hevc_vaapi", "hevc")]),
        ("qsv", [("h264_qsv", "h264"), ("hevc_qsv", "hevc")]),
    ];
    for (stack, encs) in stacks {
        let mut encoders = Vec::new();
        for (enc, codec) in encs {
            if test_encode(ffmpeg, enc) {
                let target = match codec {
                    "h264" => transcodarr_core::plan::VideoTargetCodec::H264,
                    _ => transcodarr_core::plan::VideoTargetCodec::Hevc,
                };
                encoders.push(Encoder {
                    name: enc.to_string(),
                    target,
                });
            }
        }
        if !encoders.is_empty() {
            out.push(Device {
                id: stack.to_string(),
                kind: DeviceKind::Gpu,
                name: stack.to_string(),
                encoders,
                max_concurrent: 1,
            });
        }
    }
    out
}

/// Test-encode 4 frames of 32×32 lavfi noise with `encoder` to
/// null. True iff ffmpeg exits 0.
fn test_encode(ffmpeg: &str, encoder: &str) -> bool {
    let args = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=32x32:rate=25:duration=0.16",
        "-c:v",
        encoder,
        "-an",
        "-f",
        "null",
        "-",
    ];
    std::process::Command::new(ffmpeg)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_is_always_present() {
        // A nonexistent ffmpeg yields no GPU stacks, but the CPU
        // pseudo-device must survive.
        let devices = detect("/nonexistent/ffmpeg");
        assert!(devices.iter().any(|d| d.kind == DeviceKind::Cpu));
    }
}

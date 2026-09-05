//! Server-side verification checks (DESIGN §13.6): the
//! I/O-bound implementations of core's `VerificationCheck`
//! boundary.
//!
//! `MetadataCheck` is the pure post-encode gate (container,
//! duration window, stream inventory) — facts are probed by the
//! runner and handed in, so the check itself does no I/O.
//! `DecodeCheck` is a full-decode integrity check; in v1 it runs as
//! a standalone helper (`decode_file`) because the pure `verify()`
//! signature only sees `FileFacts` (which carry no path).

use transcodarr_core::facts::FileFacts;
use transcodarr_core::plan::FfmpegPlan;
use transcodarr_core::registry::{FactExtractor, VerificationCheck};

use crate::probe::FfprobeFactExtractor;

/// Metadata comparison against the plan (the primary gate).
#[derive(Clone)]
pub struct MetadataCheck;

impl VerificationCheck for MetadataCheck {
    fn key(&self) -> &'static str {
        "metadata"
    }

    fn description(&self) -> &'static str {
        "output container/duration/stream-count/codec match the plan"
    }

    fn verify(
        &self,
        plan: &FfmpegPlan,
        input: &FileFacts,
        output: &FileFacts,
    ) -> std::io::Result<bool> {
        transcodarr_core::verify::verify_output(plan, input, output)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

/// Full-decode integrity check (runs out-of-band; see `decode_file`).
#[derive(Clone)]
pub struct DecodeCheck;

impl VerificationCheck for DecodeCheck {
    fn key(&self) -> &'static str {
        "decode"
    }

    fn description(&self) -> &'static str {
        "ffmpeg can decode the output to null without errors"
    }

    fn verify(
        &self,
        _plan: &FfmpegPlan,
        _input: &FileFacts,
        _output: &FileFacts,
    ) -> std::io::Result<bool> {
        // The pure signature has no path; the job runner calls
        // `decode_file` directly for this check.
        Ok(true)
    }
}

/// Probe a path with the given ffprobe into facts (shared by the
/// runner and the decode check).
pub fn probe_to_facts(
    probe: &FfprobeFactExtractor,
    path: &std::path::Path,
) -> std::io::Result<FileFacts> {
    probe.probe(path)
}

/// Full-decode-to-null integrity check on `path`.
///
/// `ffmpeg -v error -i path -f null -` — any decode error is a
/// failure. Runs on a blocking thread (it can take a while on a
/// long file).
pub fn decode_file(ffmpeg: &str, path: &std::path::Path) -> std::io::Result<bool> {
    let out = std::process::Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .wait_with_output()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(out.status.success() && out.stderr.is_empty())
}

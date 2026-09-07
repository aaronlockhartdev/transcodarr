//! Pure core of Transcodarr: flows, file facts, registries, evaluation,
//! and ffmpeg planning.
//!
//! This crate has **no I/O, no async, and no ffmpeg dependency** —
//! everything here is unit-testable without a filesystem or a media
//! binary (DESIGN §5). The I/O-bound pieces (the ffprobe-based fact
//! extractor, ffprobe-based verification checks, device detection) live
//! in `transcodarr-server` and plug into the registry traits defined
//! here.

pub mod device;
pub mod error;
pub mod evaluate;
pub mod facts;
pub mod flow;
pub mod hash;
pub mod plan;
pub mod registry;
pub mod schema;
pub mod verify;

pub use error::{CoreError, Result};

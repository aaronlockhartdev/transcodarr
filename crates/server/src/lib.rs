//! transcodarr-server: the I/O-bound half of transcodarr —
//! ffprobe, device detection, the job runner, SQLite persistence,
//! and the HTTP API with the embedded frontend.

pub mod api;
pub mod db;
pub mod dbhandle;
pub mod devices;
pub mod events;
pub mod jobs;
pub mod probe;
pub mod verify;

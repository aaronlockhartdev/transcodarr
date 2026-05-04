# transcodarr

Self-hosted media processing and transcoding application. Designed as a single binary with an embedded static frontend, SQLite, and bundled `ffmpeg`.

## Stack & Architecture
- **Backend**: Rust (Axum). Handles REST API, WebSocket server, static file serving, and the Flow engine.
- **Frontend**: SvelteKit (static build). Compiled to a `dist/` directory and embedded directly into the Axum binary.
- **Database**: SQLite. Stores library state, flow definitions, job status, and compressed `ffmpeg` logs as BLOBs.
- **Media**: Uses `ffmpeg` and `ffprobe` binaries.
- **Watching**: Hybrid detection via `notify` (OS events) + periodic `stat` traversal to catch missed events (e.g., NFS/SMB).

## Development & Commands
- **Frontend Dev**: `cd frontend && npm install && npm run dev` (Runs standard SvelteKit dev server)
- **Backend Dev**: `cargo run` (Requires frontend to be built via `npm run build` in `frontend/` to serve the SPA, or run dev API in parallel).
- **Build Backend**: `cargo build --release` (Embeds the static frontend from `frontend/dist`)
- **Docker Build**: `docker build -t transcodarr .` (Multi-stage build: builds ffmpeg, Rust backend, and frontend, combining into a `debian-slim` image)

## Key Conventions
- **Idempotency**: Files are tracked by `(relative_path, inode, size, mtime)`. For actual content identity, compute `xxhash3_128` of the first and last 1MB. Use this hash + `flow_id` as a unique key to prevent duplicate processing.
- **Flow Execution**: Linear flows with conditional branching.
- **Transcoding**: Transcode to a configurable "scratch" directory first. Verify acceptance criteria (bitrate, codec, duration). Only then atomically swap with the source file (Verify-then-replace).
- **Configuration**: User-facing config (watched folders, flows, auth) is entirely UI-driven. Only infrastructure config (DB path, port) is via environment variables.
- **Concurrency**: User-defined maximum concurrent jobs in the UI.

## Operational Notes
- **Recovery**: On container restart, check `ffmpeg` PIDs against the OS. If dead, mark job as failed.
- **Logs**: Stream `ffmpeg` output to the UI via WebSocket. Compress and store as `gzip` BLOB in SQLite upon completion.

## Agent Guidelines
- **.gitignore Maintenance**: Keep `.gitignore` up-to-date. Before generating new artifacts, logs, or build outputs, check `.gitignore` and add new patterns to ensure the git diff remains clean.
- **Package Versions**: Always use the latest stable versions of all packages and dependencies. Verify the latest versions by querying via SearXNG before adding or updating dependencies.
- **Coding Best Practices**:
  - Prioritize safety and zero-cost abstractions; avoid `unsafe` blocks unless absolutely necessary.
  - Keep Axum backend and SvelteKit frontend logic cleanly decoupled.
  - Ensure all file I/O and `ffmpeg` subprocess spawning are resilient to interruptions and handle edge cases (e.g., missing files, permission errors).
- **Testing Best Practices**:
  - Write unit tests for pure logic (flows, hashing, idempotency).
  - Use integration tests that mock `ffmpeg` or run against dummy media files to verify flow execution and acceptance criteria.
  - Test the file watcher, job queue, and PID tracking extensively.

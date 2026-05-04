# Transcodarr Architecture

## Project Overview
**Transcodarr** is a self-hosted media management and transcoding application. It monitors defined media directories, analyzes files using `ffprobe`, and processes them through user-defined "flows"—configurable linear pipelines with conditional branching, pre/post-processing steps, and strict acceptance criteria.

The application is designed as a **single Docker container** containing a Rust binary (Axum) with an embedded SvelteKit frontend, a SQLite database, and a bundled `ffmpeg` binary.

---

## Architecture & Tech Stack

| Layer | Technology | Responsibility |
| :--- | :--- | :--- |
| **Frontend** | SvelteKit (Static SPA) | UI for flows, job queue, library management, user config. |
| **Backend** | Rust (Axum) | REST API, WebSocket server, HTTP static server, Flow engine, Job scheduler. |
| **Database** | SQLite | Persistent storage for library, flows, jobs, users, and compressed logs. |
| **Media** | `ffmpeg` / `ffprobe` | Media analysis, transcoding, and format manipulation. |
| **Deployment** | Docker (debian-slim) | Single binary deployment, bundled `ffmpeg`, multi-stage build. |

---

## Core Components

### 1. Library Watcher
*   **Strategy**: Hybrid detection for maximum reliability.
*   **Real-time**: Uses `notify` (inotify/FSEvents) for instant detection of new/changed files.
*   **Periodic Re-scan**: Regularly runs a full `stat`-only traversal of watched directories to catch any events the OS missed (common on SMB/NFS mounts).
*   **Tracking**: Stores `(relative_path, inode, size, mtime)` in the `files` table. Changes to `inode` or `size` trigger a content hash check.

### 2. Content Identity (Idempotency)
To guarantee files are never reprocessed and no new files are missed:
*   **Fast Check**: `stat`-based comparison `(inode + size + mtime)` during rescans. If identical to DB record, content is assumed unchanged.
*   **Content Hash**: When a file is new or its `inode`/`size` changes, compute `xxhash3_128` of the **first and last 1 Megabyte**.
*   **Flow Guard**: Flows are guaranteed to run at most once per unique content hash. The `completed_jobs` table is keyed on `(flow_id, content_hash)`.

### 3. Flow Engine
A "Flow" is a linear processing chain defined by the user (via UI or JSON).
*   **Linear Pipeline**: Sequence of steps where each step produces the input for the next.
*   **Steps**:
    *   **Trigger / Criteria**: Input filters (resolution, size, codec, format).
    *   **Pre-processing**: Arbitrary commands or built-in actions.
    *   **Transcode**: `ffmpeg` processing with specific encoding parameters.
    *   **Branching**: Conditional `if/else` branching based on file properties (e.g., "If output size < 50%, delete source, else keep both").
    *   **Acceptance Criteria**: Post-transcode validation (bitrate targets, codec verification, duration checks).
    *   **Post-processing**: Cleanup, notifications, or external triggers.
*   **Overwrite Behavior**: **Verify-then-replace**. The transcoded file is written to a configurable scratch directory. Once the acceptance criteria pass and final integrity checks succeed, the original is atomically swapped/replaced and the scratch file is consumed.

### 4. Job Engine & Persistence
*   **Concurrency**: User-defined maximum number of concurrent transcode jobs (typically mapped to CPU cores).
*   **State Persistence**: Job state is stored in SQLite.
*   **Recovery**: Tracks `ffmpeg` PIDs. On container restart, the engine checks PIDs against the OS:
    *   *Alive*: Resume tracking progress.
    *   *Dead*: Mark job as failed; user can manually retry.
*   **Logs**: `ffmpeg` stdout/stderr is captured to a temporary file on disk.
    *   Streamed to the UI in real-time via WebSocket.
    *   On job completion, the full log is compressed via `gzip` and stored as a `BLOB` in SQLite for historical review.

### 5. Configuration & Auth
*   **Authentication**: Configurable via UI. Can be set to `Username + Password` (Argon2id hashed in SQLite) or `No Auth` (for local-only containers).
*   **Settings**: Managed entirely through the UI. Only infrastructure-level settings (like database path or port) are exposed via Docker environment variables.
*   **Output Placement**: Configurable per library. The user defines if outputs should overwrite in-place (via scratch dir) or be placed in a dedicated output folder.

---

## Project Structure

```text
/
├── Cargo.toml
├── Cargo.lock
├── Dockerfile
├── frontend/              # SvelteKit application
│   ├── src/
│   └── ...
└── src/                   # Rust application
    ├── main.rs            # Axum server, WS router, static file server
    ├── lib.rs
    ├── db/                # SQLite connection pool, migrations
    ├── models/            # Database entities (File, Flow, Job, User)
    ├── engine/            # Flow execution logic, job scheduler
    ├── watcher/           # Hybrid file watcher (notify + periodic stat)
    ├── api/               # REST endpoints
    └── media/             # ffprobe/ffmpeg wrappers
```

## Development & Deployment
*   **Build**: Multi-stage Docker build.
    1.  Build `ffmpeg` (or pull a minimal static/alpine base for it).
    2.  Build `frontend` (SvelteKit static output).
    3.  Build Rust binary (`cargo build --release`).
    4.  Combine binary, `ffmpeg`, and static assets in a `debian-slim` final image.
*   **Database**: Single SQLite file (`transcodarr.db`) mounted to a persistent volume.
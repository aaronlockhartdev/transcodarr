# AGENTS.md

Guidance for AI coding agents working in this repository. Humans: see `docs/DESIGN.md` (the full design) — there is no README yet.

## Project overview

Transcodarr is a self-hosted, single-node tool that makes media libraries *format-compliant*: users point it at their libraries (Plex/Jellyfin/Radarr/Sonarr-world video collections), define a **flow** (rules mapping file metadata → transcode operations), and it re-encodes non-conforming files with ffmpeg (CPU or GPU: NVENC/AMF/QSV), keeps the library compliant as files arrive, and never touches files that already conform. Everything is free (no paywalled tiers — that is the positioning vs. Tdarr).

- **Stack**: Rust, single static binary; Svelte + TypeScript SPA embedded into the binary; SQLite (WAL); ffmpeg/ffprobe as **child processes** (no libav bindings).
- **License**: AGPL-3.0 (copyleft — see Security & licensing).
- **State**: greenfield. The design is settled; the code is not scaffolded yet.

## The design doc (read before implementing)

**`docs/DESIGN.md` is the source of truth** for every design decision.

- Before implementing a feature, read its section there: §2 concepts · §3 file lifecycle · §4 discovery & triggers · §5 flow model & extension architecture · §6 operations & ffmpeg planning · §7 scheduling & multi-node seam · §8 stack & deployment · §9 UI · §10 data model · §11–12 v1 scope & parked items · §13 judgment-call defaults.
- If a change would alter the agreed design, **propose it as an edit to `docs/DESIGN.md` first** and get the owner's sign-off before coding.
- Items in §12 are *parked*: designed for, intentionally not built. Do not implement them without an explicit request.

## Repository layout (planned — maintain it once scaffolded)

```
AGENTS.md
docs/DESIGN.md          # source of truth for design
crates/
  core/                 # flows, file facts, registries, evaluate(), ffmpeg argv planner — PURE: no I/O, no ffmpeg, no async
  server/               # tokio HTTP API, job scheduler, file watching, SQLite store — produces the binary
frontend/               # Svelte + TypeScript SPA; built assets embedded into the binary via include_bytes!
Dockerfile              # canonical artifact: pinned ffmpeg (NVENC+AMF+QSV), x86-64 + aarch64
```

Nested `AGENTS.md` files may be added per crate or the frontend; the nearest file to the edited file takes precedence.

## Build & test commands

The repository currently contains **no code yet**. The following are the standing conventions that apply as soon as the workspace is scaffolded — do not introduce other build systems or toolchains.

- `cargo fmt --check` (fix with `cargo fmt`)
- `cargo clippy --all-targets -- -D warnings`
- `cargo test` — the whole workspace must be green before committing
- Frontend: `pnpm --dir frontend build` (produces the embedded assets) · `pnpm --dir frontend check` (svelte-check + tsc)
- Docker image builds are CI-only; do not build images locally unless explicitly asked.

## Code style

**Rust**
- rustfmt defaults; fix clippy warnings instead of suppressing them (no `#![allow]` blanket waivers).
- `thiserror` for error types in `core`; `anyhow` at the `server`/application boundary.
- No `unwrap()`/`expect()` outside tests. No `unsafe` without a `// SAFETY:` comment justifying it.
- Doc comments on all public items in `core` — it is the API other crates consume.
- Keep `core` pure and synchronous: `evaluate()` and the planner must be unit-testable without ffmpeg or a filesystem (design doc §5).

**Frontend**
- TypeScript strict mode; Prettier defaults; Svelte 5 + Tailwind CSS + shadcn-svelte (copy-in-source; Melt UI handles accessibility). UI components live in the repo and follow shadcn-svelte conventions — do not add a second component library.
- The flow editor is **schema-driven**: render every condition/operation picker from `GET /api/schema/flow`. Never hardcode condition or operation field lists into components (design doc §5 — this is what keeps extension a Rust-only change).

## Testing

- Unit tests in `core`: flow schema round-trips, each registry entry, `evaluate()` verdicts (Identity / Plan / NoMatch), and the ffmpeg **argv** planner. Planner tests assert on the argument vector — never on rendered shell strings.
- A stub `ffmpeg`/`ffprobe` script on `PATH` powers integration tests; the suite must pass on machines without a real ffmpeg. Real-ffmpeg checks belong in `#[ignore]` tests or CI jobs that install ffmpeg.
- Add or update tests for the code you change, even if nobody asked.
- Once `.github/workflows/` exists, that is the CI definition of done — check it before claiming a task complete.

## Security & licensing

- **AGPL-3.0 is copyleft**: new dependencies must be AGPL-compatible. If a candidate isn't, stop and flag it to the owner instead of adding it.
- **Never build shell command strings.** All ffmpeg/ffprobe invocation is an argv vector; file names, paths, and flow data are untrusted input (file names may contain arbitrary bytes).
- All filesystem mutation is confined to: the library root, the configured output-tree root, and `.transcodarr/`. Validate before move/rename/delete — no `..` traversal, no following symlinks out of a root.
- **The original file is never modified before output verification passes** (design doc §3.3). This invariant is load-bearing; preserve it in all lifecycle code.
- No built-in authentication by design in v1 (LAN binding + reverse-proxy auth, design doc §8). Do not add a credential store or bind to 0.0.0.0 without an owner decision.
- SQL is always parameterized (rusqlite binds); no concatenated query strings.

## Commits & PRs

- Conventional Commits: `feat:` `fix:` `docs:` `refactor:` `test:` `chore:` — one logical change per commit.
- Before committing: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test` (plus `pnpm --dir frontend check` if the frontend changed).
- Design-affecting changes ship with the matching `docs/DESIGN.md` edit in the same commit/PR, and the diff is called out to the owner.

## Gotchas

- The multi-node claim endpoint exists **behind a config flag and must stay disabled** in v1 (design doc §7). Do not enable or depend on it.
- File identity is path-based; a 3-point sample hash (first/middle/last 1 MB) plus mtime/size decides when to re-probe (design doc §4). Don't invent whole-file hashing — it doesn't scale to 4K libraries.
- In-place mode **adopts the true container extension** (MKV→MP4 renames), which deliberately orphans \*arr DB records until re-scan (design doc §3.1; re-import automation is parked in §12). Don't "fix" this.
- New condition fields, operation sections, or verification checks are **registry entries in `core`** (design doc §5) — one type implementing the trait + registration. Never special-case them in the planner, the server, or the frontend.
- Transcoding is usually **disk-bound, not CPU-bound**: any scheduling change must respect the global concurrency ceiling (design doc §7).
- Network filesystems (NFS/CIFS) cannot be watched; scan is their primary trigger, not a backstop (design doc §4).

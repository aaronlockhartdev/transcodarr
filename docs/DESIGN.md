# Transcodarr — Design Document

**Status**: v0.1 — settled through the grilling session. This is the agreed design; propose changes here before coding.
**License**: AGPL-3.0 (see `LICENSE`).
**One-liner**: A self-hosted, single-node tool that makes media libraries *format-compliant*. Point it at your libraries, describe the formats you want as a **flow**, and it transcodes whatever doesn't fit (ffmpeg, CPU or GPU) and keeps it that way — every feature free, no paywall.

---

## 1. Problem & positioning

Users run large home media libraries (Plex, Jellyfin, Radarr, Sonarr ecosystems) with inconsistent source formats: 4K HEVC next to 1080p H.264, DTS/TrueHD audio in one movie and EAC3 in the next, MKV/MP4/M2TS mixed. They want **rules** that bring the whole library to a target format and keep it there — without a second copy of the library, and without touching files that already conform.

Positioning vs. the field:

- **Tdarr** solves the same problem but gates features behind a paid tier and centers on a distributed node mesh. Transcodarr: **everything free (AGPL-3.0)**, single node first, with a multi-node seam designed in.
- Name note: two unrelated repos already use the name *transcodarr* (an \*arr-orchestrator; a Jellyfin live-transcode cluster). Ours is distinct in kind — a single-box library-format manager — and the positioning statement above is the disambiguation.

Non-goals for v1 (see §11–12): distributed workers, \*arr API integration, audio-only/image libraries, subtitle burn-in, live/on-the-fly transcoding.

---

## 2. Core concepts

| Term | Meaning |
|---|---|
| **Library** | A directory tree the user adds. References a **flow** (a first-class shared object — see below), and carries: lifecycle mode (in-place or output tree), auto-queue toggle, scan schedule. |
| **File facts** | Cached probe result for one file: container, video codec/profile/level/pixel format/resolution/frame rate, HDR format, audio tracks (codec, language, channel layout, Atmos flag), subtitle tracks (type, language, forced), size, duration, bitrate. |
| **Flow** | A format policy: an ordered list of **steps**. A step = **condition** → **operation** and may carry a user-assigned **display name** (unnamed steps show as "Step N"). **First match wins.** Flows are **first-class, library-independent objects**: any number of libraries can reference the same flow, and editing one re-evaluates all of them. |
| **Compliant** | The flow evaluates to **identity** for the file's facts: the matched operation (or no match at all) would change nothing — every stream copied, nothing dropped, container already correct. |
| **Job** | One execution of an evaluated plan against one file: transcode → verify → swap → backup → retain. |

**Evaluation semantics.** For a file with facts `F` and flow `S1 → S2 → … → Sn`:

- First `Si` whose condition matches `F` determines the outcome; later steps are ignored.
- The matched operation plans a transformation:
  - **Identity** (plans to change nothing) → file is **compliant**.
  - **A real plan** → file is **needs-work**; enqueuing creates a **job**.
- **No step matches** → file is **unmatched**: left untouched, status visible in the UI, optionally escalated to a warning (setting) — a no-match is the classic flow-authoring bug and must never be silent.

Example: step 1 condition `{resolution ≥ 3840×2160 AND video codec = hevc}` → operation `{downscale to 1080p, h264, source-capped, …}`. A 4K HEVC file matches step 1 → job. A 1080p HEVC file falls through to step 2 (say `{video codec = hevc} → {h264 …}`) → job. A 1080p H.264 file with EAC3 audio matches a step whose operation is identity for it → compliant.

---

## 3. File lifecycle

### 3.1 Lifecycle modes (per library, user-chosen)

**In-place (default).** Transcode to a temporary sibling file in the *same directory* (same filesystem ⇒ atomic rename), verify (§3.3), then:

1. Move the original to the library's hidden backup area: `.transcodarr/backups/<relative path>.bak`.
2. Rename the verified output to the canonical file name.
3. **The rename adopts the true container extension** (an MKV that is now MP4 becomes `.mp4`). Consequence: Radarr/Sonarr DB records point at the old path and need a re-scan/re-import; automating that via their APIs is a parked future item (§12). Consumers that probe content (Plex, Jellyfin) cope with the extension mismatch either way.

**Output tree.** Transcoded copies are written to a user-chosen root, mirroring the library's relative paths; originals are never modified. Repointing consumers at the new tree is the user's job (documented); \*arr re-import is a parked future item.

### 3.2 Backups, retention, deletion

- Backups live in the hidden `.transcodarr/` folder under the library root (same filesystem, clean directory listings, trivially findable).
- **Retention N days** (per-library setting, default 7) is the test window: the original stays restorable while the user confirms the new file plays.
- **Auto-delete of backups** is a separate per-library toggle, **default OFF** — deletion is opt-in, always.

### 3.3 Failure & verification

- The original is **never touched before verification passes** (temp-file-first), so any failure leaves the library intact.
- **Verification (default, before swap)**: ffprobe the output and check against the plan — codec/profile/level as planned, duration within ±max(2 s, 0.5%), stream inventory matches the plan (what was kept/dropped/re-encoded).
- **Full decode check**: optional per library (default off). CPU-only decode of the whole output (hw decode would not validate the encoded stream). A 2-hour 4K file costs roughly 10–20 min on a modern 8-core; the UI shows the estimate before the user commits.
- **Retry policy**: exactly one automatic retry, and only when the ffmpeg *process* dies abnormally (OOM/kill — non-ffmpeg-exit). Decode/encode errors are deterministic (corrupt source) → **failed** state, no auto-retry, manual retry available.
- **Quarantine**: failed outputs are kept in `.transcodarr/quarantine/` for inspection (a failure is a diagnosis, not garbage), referenced from the job record.
- **File changes mid-job** (size/mtime/sample-hash change, §4): abort the job, clean up the temp file, re-queue after the file stabilizes.
- If the job's chosen **hardware encoder fails at job start** (driver hiccup, missing codec on that chip), retry once on software.

---

## 4. Discovery & triggers

| Trigger | Behavior |
|---|---|
| **Manual** | User queues files/folders from the library detail screen. |
| **Watch** | inotify (Linux) / FSEvents (macOS) on library roots. Qualifying events: **close-write and rename** (the \*arr's final move-into-place is the "arrived" signal). A file is "complete" when its **size is steady for ~60 s** — this is what catches hardlinked downloads still being written through the shared inode. |
| **Scan** | Scheduled (default daily, per-library configurable) + on-demand button. |

Notes:

- \*arr stacks move completed downloads into the library in one shot (copy or hardlink); the stability gate is the robust completion detector, so no temp-name blocklists are needed.
- **Network filesystems (NFS/CIFS) cannot be watched.** At library-add time we test watchability; unwatchable libraries are marked "scan-driven" in the UI, and the scheduled scan is their *primary* mechanism, not a backstop.
- A newly detected file flows through: probe → facts cached → flow evaluated → **auto-queued if needs-work**. Auto-queue is a per-library toggle, **default ON** — unattended compliance is the product promise. The first scan of a big library creates a large backlog; the scheduler's caps (§7) pace it.
- **Flow edits re-evaluate instantly**: facts are cached, so editing a flow re-runs `evaluate()` over cached facts (milliseconds, no re-probe) and updates every file's status — this powers the impact preview (§9.3).

**Change detection / probe cache.** A file's facts are re-probed only when any of (path, mtime, size, sample hash) changes. The sample hash is **SHA-256 over first 1 MB + middle 1 MB + last 1 MB** of the file — catches mid-file edits and tail appends; 3 MB of sequential reads is well under a second even on slow storage, so it can run on every scan pass. A changed hash clears prior verdicts; an in-flight job is aborted and re-queued (§3.3).

---

## 5. Flow model & extension architecture

This section is the structural core of the "extremely easy to extend" requirement.

**Principle: a flow is data; behavior lives in registries.**

A flow is versioned JSON (`"flow_version": 1`) stored as its own row in the `flows` table; libraries reference a flow by id (a library with no flow leaves all its files unmatched). Every variable part of the system is a registry of small Rust types behind a trait. v1 registries:

| Registry | Entries (v1) | What an entry provides |
|---|---|---|
| **Condition fields** | container, video codec, resolution (min/max), pixel format, HDR, audio codec set, file size (min/max) | `key`, `match(&Facts) -> bool`, JSON (de)serializer, **UI schema** (picker description) |
| **Operation sections** | video, audio, container, subtitles | parameter types + defaults, `plan(&Facts) -> FfmpegPlan contribution`, JSON (de)serializer, **UI schema** |
| **Fact extractors** | the ffprobe-based probe | `probe(path) -> FileFacts` (new sources/formats add here) |
| **Verification checks** | metadata check, (optional) decode check | `verify(plan, output) -> Result` (new checks add here) |
| **Devices** | CPU, each detected GPU | capabilities, encoder list, `max_concurrent` |
| **Triggers** | manual, watch, scan | `emit(candidate files)` (webhooks etc. add here) |

**Schema-driven UI.** The backend serves `GET /api/schema/flow` describing every condition field and operation parameter (type, values, defaults, units, hints). The SPA editor renders pickers **from that schema** — it contains no hardcoded field lists.

**The payoff, stated precisely:**

- Adding a condition field (e.g. frame rate, duration) = one Rust file implementing the trait + registration. No frontend change, no migration — the field simply appears in the editor, defaults to "any", and is absent from (and thus ignored in) all existing flows.
- Adding an operation section (e.g. "cover art", "chapter cleanup") = same pattern.
- Evaluation is a **pure function** `evaluate(&Flow, &Facts) -> Identity | Plan | NoMatch`: unit-testable without ffmpeg, and the impact preview is a loop over cached facts.

**Condition semantics (v1).** A step's condition is an **AND of per-field constraints, each defaulting to "any"**. OR is expressed by duplicating a step with the same operation. (OR-groups are a parked extension — the registry design absorbs them without changing existing flows.)

---

## 6. Operations & ffmpeg planning

A step's operation is the **complete** plan for a matched file. Each section is either **absent (= identity for that domain: everything copied/kept)** or **present with an explicit policy** — including its default rule for items it doesn't name. Nothing outside the flow is ever touched.

### 6.1 Video section (absent ⇒ stream copy)

| Parameter | Values / default |
|---|---|
| Container | `mp4` / `mkv`; **smart default**: MP4 if every planned stream is MP4-safe, else MKV |
| Codec | `h264` / `hevc` (v1); AV1 parked |
| Profile | sensible default per codec + bit depth (e.g. HEVC Main10), overridable |
| Level | sensible default per codec + resolution (e.g. 4K HEVC → 5.1), overridable |
| Bitrate mode | **source-capped (default)**: ≤ source bitrate, ceiling scaled by target resolution · fixed bps · quality (CRF) |
| Device | `software` or a specific detected GPU (populated from startup encoder probe; unavailable encoders shown disabled with a hint) |
| Downscale to | optional target resolution (e.g. 1920×1080). **Never upscale.** |
| HDR → SDR | explicit on/off flag (default off; destructive to look, never implicit) |
| Filter graph | optional ffmpeg `-vf` expression the user types in (e.g. `crop=…`, `denoise`); empty = none — §6.5 |

One ffmpeg invocation per job (decode → filter → encode → mux). The exact filter chain (e.g. the zscale/tonemap sequence for HDR→SDR) is an implementation detail. Two-pass encoding is parked.

**Worked example.** Step: `{≥ 4K AND hevc}` →

```
video:     mp4, h264 (h264_nvenc on "RTX 4090"; libx264 on CPU), high, 4.2,
           source-capped (ceiling 12 Mb/s @1080p), downscale-to 1920x1080,
           hdr→sdr on (no-op when source is SDR)
audio:     default: copy; rule: codec in {dts, dts_ma, truehd} → eac3 5.1 48 kHz;
           atmos (eac3-joc): copy, never auto-downmixed
subtitles: keep all (copy)
```

```
ffmpeg -hwaccel cuda -i "In.Movie.2024.2160p.HEVC.mkv" \
  -map 0:v:0 -c:v h264_nvenc -profile:v high -level 4.2 \
       -b:v 12M -maxrate 14M -bufsize 24M \
       -vf "scale=1920:1080:flags=lanczos,<hdr10→bt.1886 chain>,format=yuv420p" \
  -map 0:a -c:a:0 copy -c:a:1 eac3 -b:a:1 640k ... \
  -map 0:s -c:s copy \
  -f mp4 "In.Movie.2024.1080p.mp4.tmp" -progress pipe:1
```

(schematic — exact chain assembled by the planner from the operation).

### 6.2 Audio section (absent ⇒ copy all tracks)

- **Default policy** for unnamed tracks: `copy` (default) / `re-encode` (codec; optional sample rate and channel count — **auto keeps the source values**) / `drop`.
- Optional **per-track rules** matched by codec and/or language (e.g. "all DTS/TrueHD → EAC3 5.1").
- **Atmos (EAC3-JOC) is copied unless an explicit rule re-encodes it** — never auto-downmixed.
- Re-encoding, when invoked, applies to **all** matching tracks (deterministic target state, not "primary only").
- **Filter graph**: optional ffmpeg `-af` expression (e.g. `loudnorm=…`) applied to **re-encoded** tracks only (a copied bitstream cannot be filtered; with no re-encode it is inert); empty = none — §6.5.

### 6.3 Subtitle section (absent ⇒ keep all, copy)

`keep all` (default) / `keep forced-only` / `drop`. Copy only; **no burn-in in v1** (parked).

### 6.4 Container section (`smart` is a resolution input; an explicit choice is an action)

`smart` (default) / `mp4` / `mkv` / `webm` / `mov`. `smart` resolves per §13.7 (MP4 if every planned stream is MP4-safe, else MKV). An explicit choice that differs from the source container turns an otherwise-stream-identical step into a **pure remux** (all streams copied, container changed) — this is how "remux all MKV to MP4" is expressed without enumerating source codecs. A stream-identical step with `smart` **never** remuxes on its own (otherwise enabling any video section would silently remux every MKV in the library). The editor exposes **this section only** as the container control; the `container` field inside the video section remains a parseable wire form for older flows (hidden in the editor, the top-level choice wins in resolution).

### 6.5 Filter graphs (user expressions; one-shot semantics)

A filter graph is an ffmpeg `-vf`/`-af` expression the user types into the video or audio section — e.g. `crop=1920:800:0:0`, `denoise`, `loudnorm=I=-16:TP=-1.5`, `volume=2`. Empty means no filter. The planner places the expression at a fixed position (the video graph is the **last** element of the generated chain, after the downscale/HDR stages; the audio graph applies per re-encoded track) and **validates it twice**: at flow-save (section-parameter parse check, via the registry) and at job start (executed against the installed ffmpeg — a failing job with a clear message beats a silent no-op).

Filter graphs are **one-shot transformations**: the output's new facts do not record that a filter was applied (a cropped H.264 is still H.264). So the post-job idempotency gate (§3.3) re-evaluates the flow *with the filter graphs cleared*, and each file records which graphs it has already had applied (`files.applied_filters`). A rescan re-queues a filter plan only when the flow's graphs differ from the recorded ones, and a changed sample hash (new bytes) clears the ledger; the ledger is written immediately after the file swap (before the gate) so a crash in between cannot cause a double application. The ledger's identity check is only as strong as the sample hash (FNV-1a over a 16KB head+tail sample). A graph must not undo the flow's other constraints for that step (e.g. a `scale` beyond the downscale target): the cleared flow stays non-identity, the job fails `non_idempotent`, and every scan re-queues the file.

---

## 7. Scheduling, concurrency & the multi-node seam

**Queue.** SQLite (WAL mode, single writer). The jobs table carries lease columns (`claimed_by`, `lease_expires`) and a claim endpoint behind a config flag (disabled in v1) — **the multi-node seam**: a future remote worker is a small binary that polls the API and claims tasks; no schema or architecture change. If multi-node ever ships, topology is **one coordinator + N stateless workers** (the Tdarr shape), not a peer mesh.

**Devices & caps (global settings).**

| Device | Default `max_concurrent` |
|---|---|
| CPU (pseudo-device; per-job thread limit set so jobs share cores) | `max(1, cores / 4)` |
| Each detected GPU | 2 |
| **System-wide total ceiling** (protects NAS disk bandwidth — transcoding is often disk-bound, not CPU-bound) | sum of device caps |

**Scheduler.** FIFO global queue; a job's device comes from its plan; dispatch respects per-device caps *and* the system ceiling. Controls: **pause/resume** the whole queue; **kill** a running job (SIGTERM, temp file cleaned, original untouched). A crashed job (abnormal process exit) auto-retries once; encoder-unavailable jobs fall back to software once (§3.3).

**Progress.** ffmpeg `-progress pipe:1` → percentage/ETA on the jobs screen (WebSocket/SSE).

---

## 8. Stack & deployment

- **Rust**, single static binary. `tokio` for async I/O; `rusqlite` for SQLite; **ffmpeg as a child process** — no libav bindings, so a crashed/hung encoder can never take the server down, and the binary stays cgo/FFI-free.
- **Docker-first (canonical artifact).** Image bundles a **pinned ffmpeg build** — one Linux build with **NVENC + VAAPI + QSV** compiled in; x86-64 **and aarch64** (ARM NAS boxes). GPU userspace comes from the host via the vendor toolkits (nvidia-container-toolkit / ROCm / Intel oneAPI runtime). The image is the version control for ffmpeg.
- **Bare binary** (macOS dev, non-Docker Linux): **detects a system ffmpeg** on PATH at startup, validates the version against a supported range, and probes available encoders (this probe also feeds the flow editor's device pickers). Missing/out-of-range → clear startup report in the UI, software path still works.
- **Web**: HTTP API + embedded SPA (**Svelte 5 + Vite + Tailwind CSS + shadcn-svelte** — copy-in-source, Melt UI primitives for accessibility; embedded via `include_bytes!`) served from the same binary; the SPA uses layerchart for the dashboard charts. **Live updates ride one SSE stream per client** (`GET /api/events`) — deliberately **not** WebSocket: the event flow is strictly one-way (every client action is an ordinary POST), plain HTTP sits cleanly behind the reverse-proxy auth, and `EventSource` auto-reconnects with `Last-Event-ID` resume, which WebSocket would force us to rebuild. Job logs stream over the same stream (§9.0).
- **Auth stance (v1)**: no built-in authentication. Bind to localhost/LAN as configured; exposure via a reverse proxy with auth is the documented pattern (the \*arr norm). Single-user tool.

---

## 9. UI

Navigation: **Dashboard** (home) · **Libraries** → (click a library) → **Library detail** · **Flows** · **Jobs** · **Settings**.

### 9.0 Live data
Every value the UI displays is event-driven and current. Each browser client holds one **SSE** stream (`GET /api/events`); the server pushes the moment state changes — job transitions (a nudge carrying the job id; the client refetches the list), **batched log output** (~200 ms batches, 64 KiB chunks split at line boundaries), file verdicts from scan/watch, library and flow changes — plus a 30 s `tick` event and a 15 s transport-level ping for liveness. Reconnects resume via `Last-Event-ID`: the server replays its in-memory ring (1024 events) before joining the live feed, and if a client has fallen behind the channel capacity a `resync` frame triggers a full refetch instead of guessing. Initial page load is still a JSON fetch; SSE only *updates* after that. The 5 s poll is retained purely as a disconnect fallback — and re-arms if a connected stream goes quiet for more than 60 s. Pages render an explicit loading state until their first fetch completes — never a misleading default or an "empty" list while data is in flight.

### 9.1 Dashboard

- Stat cards: total files · compliance % · library storage used · **space saved by transcoding** (cumulative Σ input−output from job history — the number users care about) · failed/quarantined count.
- Charts: files + GB completed per day (last 30 days) · per-device active jobs vs. cap.
- Lists: recent failures (with jump-to-job) · oldest waiting.
All derived from existing tables; no new subsystem.

### 9.2 Libraries / Library detail

- **Libraries**: list with per-library compliance summary (n files / n compliant / n queued / n running / n failed / n unmatched), assigned flow, watchability badge ("watched" vs. "scan-driven").
- **Library detail**: compliance summary strip; **file table** (name, resolution, video/audio codecs, status, last checked, size, size delta after transcode); row actions: re-check · queue now · **history drawer** (that file's full job history) · view quarantined output. Library settings: lifecycle mode, flow **reference** (a picker over the shared flows — flows themselves are edited on the Flows page, not here), auto-queue, retention N, auto-delete, scan schedule.

### 9.3 Flow editor

- Ordered, **collapsible step cards**, each with two zones: **Filters** — an addable/removable list of rows (field picker + value control), all AND-ed; an unset row means "any", and no-op rows are pruned on save — and **Transcode** — one bordered box per operation section (video/audio/container/subtitles, §6), each independently on/off. Field and option names are rendered from schema labels (capitalized, friendly — e.g. "H.264", "Dolby Vision"); resolution bounds are typed as integer width × height pixel inputs (no presets), each bound an unbreakable group that may wrap onto its own line. Steps are renamable (display name only — array order defines precedence), and a filter kind appears at most once per step: picking an already-present kind is rejected without changing the step.
- **Impact preview (the killer feature)**: on every edit, `evaluate()` re-runs over the using libraries' cached facts and shows, before saving: *"this edit changes the fate of N files: 12 will transcode, 3 will lose audio tracks, 4,180 untouched; 2 files become unmatched."*
- **No raw JSON escape hatch in v1** — every field is a schema-driven picker (§5); the schema is the only surface, which keeps every save structurally valid for a shared object.
- The NoMatch behavior (unmatched status / warning escalation) is visible and configurable from this screen.

### 9.4 Jobs

Running (live progress, device, kill; **the log streams live** — the viewer appends `job_log` batches over SSE with auto-scroll, then takes the archived tail in one fetch when the job ends) · completed · failed (full log from storage, quarantine link, retry) · quarantined. Queue-level pause/resume; per-device cap gauges. Job, file, and library tables are sortable on their useful columns and filterable (search box; the library page's status badges double as filters).

### 9.5 Settings

Devices & per-device caps · system ceiling · default scan schedule · retention/auto-delete defaults · backup folder · **ffmpeg report** (detected version, encoder matrix per device) · multi-node claim flag (off, for the future).

---

## 10. Data model (SQLite)

| Table | Key columns |
|---|---|
| `flows` | id, name (unique), flow_json (versioned), created_at, updated_at — **first-class and library-independent; any number of libraries can use one** |
| `libraries` | id, name, path, lifecycle_mode, flow_id (→ flows, nullable — NULL = no flow), auto_queue, retention_days, auto_delete, scan_schedule, watchable |
| `files` | id, library_id, path, dev, inode, size, mtime, sample_hash, facts_json, status (`compliant / needs_work / unmatched / queued / running / failed / quarantined`), last_probed, last_evaluated, input_size, output_size, **applied_filters** (JSON `{hash, graphs}` — the sample hash at the time the graphs were applied, plus the graphs themselves — §6.5) |
| `jobs` | id, file_id, library_id, flow_version, plan_json, state, device_id, claimed_by, lease_expires, started, ended, exit_kind, log_path (the on-disk ffmpeg stderr file, retained for `tail -f`), **log_zstd** (the canonical copy — zstd-compressed on job end, served decompressed by the log endpoint; pre-migration rows fall back to the file), quarantine_path — **retained: this table *is* the per-file job history** |
| `devices` | id, kind (`cpu / gpu`), name, encoders_json, max_concurrent |
| `settings` | key, value |

Indexes: `files(library_id, status)`, `files(path)`, `jobs(state, started)`. File identity is path-based (like the \*arrs); a path that disappears and reappears with new facts is treated as a new file (verdicts cleared, prior job rows preserved as history).

---

## 11. v1 scope

**In:** everything in §2–§10. Condition fields of §5; operations of §6 (h264/hevc targets, MP4/MKV, GPU = NVENC + VAAPI + QSV); in-place + output-tree lifecycle; watch + scan + manual triggers; the five UI screens; the multi-node seam (dormant).

**Out (v1):** see §12.

---

## 12. Parked (designed for, not built)

- **Multi-node**: remote worker binary over the claim endpoint; one coordinator + N workers.
- **\*arr integration**: Radarr/Sonarr re-import/re-scan automation after in-place extension renames; webhooks as a trigger source.
- **AV1** target (newer-GPU-only encoder) as a preset-grade option; **two-pass** encoding; **subtitle burn-in**.
- **Audio-only and image libraries** (different rule shapes).
- Additional condition fields (profile, level, frame rate, duration, subtitle presence) and OR-condition groups — additive via the registry, no migrations.
- HDR handling beyond the on/off HDR→SDR flag (tonemapping presets).

---

## 13. Judgment calls (defaults the owner can veto)

Made during the design session; each is a deliberate default, not a constraint:

1. Backup retention default **7 days**; auto-delete **OFF**.
2. Scan default **daily**; stability gate **60 s**.
3. CPU concurrency default `max(1, cores/4)`; GPU default **2** each.
4. **Auto-queue ON** for newly detected files (the unattended-compliance promise); per-library kill switch.
5. No built-in auth in v1 (reverse-proxy pattern).
6. Single-pass encoding in v1.
7. Target container smart-default MP4-if-safe-else-MKV, overridable per operation.
8. Docker image = canonical artifact; bare binary detects system ffmpeg.
9. In-place renames adopt the true extension (orphaning \*arr records until re-scan; automation parked).
10. **Svelte 5 + Tailwind CSS + shadcn-svelte** for the SPA (copy-in-source; Melt UI underneath). The styling layer is swappable — the schema-driven editor (§5) does not depend on it.
11. **Legacy wire forms are accepted at parse time and upgraded in place** (no flow-migration endpoint in v1): a device object without `kind` (`{"id": "…"}`) ⇒ GPU; `profile`/`level: null` ⇒ auto; the audio policy string `"re-encode"` ⇒ re-encode to the default target, source rate/channels kept.
12. **Live updates ride one SSE stream per client, not WebSockets** (§9.0) — the event flow is strictly one-way (client actions are ordinary POSTs); SSE is proxy-friendly, auto-reconnects, and resumes logs via `Last-Event-ID`.
13. **Flows are first-class, library-independent objects** (a `flows` table; `libraries.flow_id` is nullable). Editing a flow re-evaluates every library that references it; deleting a flow unassigns its libraries (files settle to unmatched) rather than cascading.
14. **Filter graphs are one-shot, not target-state** (§6.5): they run once per file and are recorded per file (`files.applied_filters`), so a file is re-queued only when the flow's graphs change or the file's bytes change. Editing a filter's parameters is a new graph → one more pass.

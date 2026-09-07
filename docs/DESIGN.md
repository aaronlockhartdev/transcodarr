# Transcodarr — Design Document

**Status**: v0.3 — v0.1 was settled through the grilling session; v0.2 adds the processed-marker / applied-operations design (§6.6) and the all-xxh3-128 hashing decision (§13.15); v0.3 adds operation composition across matched steps (the *effective operation*, §5), per-track subtitle rules (§6.3), non-idempotent segment tracking with the `once` flag and marker t2 (§6.5–6.6), and compact rule-row editing with exclusive-field preview diagnostics (§9.3). This is the agreed design; propose changes here before coding.
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
| **File facts** | Cached probe result for one file: container, video codec/profile/level/pixel format/resolution/frame rate, HDR format, audio tracks (codec, language, sample rate, channel layout, title, default flag, Atmos flag), subtitle tracks (type, language, forced), size, duration, bitrate. |
| **Flow** | A format policy: an ordered list of **steps**. A step = **condition** → **operation** and may carry a user-assigned **display name** (unnamed steps show as "Step N"). **Matching is per-domain** (§5): the first matching step decides the exclusive decisions (video targets, container, subtitle policy, audio default); per-track rule tables (audio, subtitles) merge across *all* matching steps; user filter graphs from all matching steps chain together. Flows are **first-class, library-independent objects**: any number of libraries can reference the same flow, and editing one re-evaluates all of them. |
| **Compliant** | Either the flow evaluates to **identity** for the file's facts (the effective operation would change nothing — every stream copied, nothing dropped, container already correct), or the file carries a **processed marker** matching the operation this flow would produce (§6.6) — i.e. this flow already made these bytes and the file is unchanged since. |
| **Job** | One execution of an evaluated plan against one file: transcode → verify → swap → backup → retain. |

**Evaluation semantics.** For a file with facts `F` and flow `S1 → S2 → … → Sn`:

- Every `Si` whose condition matches `F` contributes to the outcome, composed into one **effective operation** (§5 Operation composition): exclusive decisions (video targets, container, subtitle policy, the audio/subtitle default) come from the **first** matching step; per-track rules (audio, subtitles) **merge across all** matching steps in step order (per track, the first matching rule wins); user filter graphs from all matching steps **chain** in reversed step order. A file matching exactly one step behaves exactly as before: its effective operation *is* that step's operation.
- The **effective operation** (§5) plans a transformation:
  - **Identity** (plans to change nothing) → file is **compliant**.
  - **A real plan** → file is **needs-work**; enqueuing creates a **job** — unless the file's processed marker matches this plan's fingerprint and the file is unchanged since tagging (§6.6), in which case the verdict is **compliant**: the plan is one this flow has already produced.
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

**Change detection / probe cache.** A file's facts are re-probed only when any of (path, dev, inode, mtime, size) changes. The stored **sample hash** — **xxh3-128** over three 256 KB windows of the file (head, middle, tail) (recomputed only when the row changes, so a changed hash clears the applied-operations record, §6.6) — is a secondary change signal for byte-level edits; it is not the file's identity (that is path + dev/inode + size + mtime). The windows are 768 KB out of a file that may be 50 GB, so the hash is a *signal*, not an integrity check: any realistic in-place edit changes size or mtime and is caught by the four-tuple anyway, while the hash is the tie-breaker for exotic same-size, same-mtime rewrites (container metadata at the file's ends, or a small content patch caught by the middle window). It is computed only on changed files, in three sequential reads, so even large libraries stay cheap (§13.15). The operation **fingerprint** of §6.6 (xxh3-128) is a different hash with a different job: it names *which operation* produced the file's bytes and lives in the file itself, so it survives a lost database.

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

**Operation composition (the effective operation).** A file's outcome is not "one step's operation" but the **effective operation** composed from *all* steps whose condition matches, in step order. Each operation section declares how it composes:

- **Exclusive** — video targets (codec, profile, level, bitrate, device, downscale, HDR→SDR/tonemap), the container choice, and the subtitle policy: the **first** matching step's value wins; a later step's differing value is ignored (and surfaced by the impact preview, §9.3).
- **Per-item precedence** — audio and subtitle **per-track rules**: the rules of all matching steps concatenate in step order; for each track the **first matching rule** wins; the default policy for tracks no rule names comes from the first matching step. (E.g. step 1 "drop all English audio" + step 2 "drop all Japanese audio" → both drops apply, each to its own tracks.)
- **Additive** — user filter graphs: the graphs of all matching steps chain into one expression in **reversed step order** (lowest-priority step's graph first, first matching step's graph last), so the highest-priority graph sits at the end of the ffmpeg chain and wins setter-style conflicts (`scale`, `format`, …); identical graphs (canonical form) are deduped. The chain follows the structured segments (downscale/HDR), which precede all user graphs.

The fingerprint and marker (§6.6) are computed over the **effective** operation — still a pure function of the flow (the file's facts decide which steps match), so markers stay recomputable at scan time.
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
| Tonemap | settings for the HDR→SDR conversion: method `bt2390` (default) / `narkowe` / `hable` + desaturation `0.0`–`1.0` (default none); applied only when the flag above is on and the source is HDR |
| Filter graph | optional ffmpeg `-vf` expression the user types in (e.g. `crop=…`, `denoise`); empty = none. Graphs from several matching steps chain in **reversed step order** — lowest-priority first, first matching last, so the highest priority wins setter-style conflicts — after the structured downscale/HDR segments (§5) — §6.5 |

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
- Optional **per-track rules** matched on any axis — codec (Atmos is matched here, as the `eac3_joc` pseudo-codec — there is no separate Atmos field), language (both sides normalize to ISO 639-1 using the ISO 639 database shipped by the `rust-iso639` crate, plus a small documented override list for spellings the crate misclassifies; `und`/`mis`/`zzz` → unknown, so "unknown" is matchable), channel count, sample rate, title substring (`tags.title`), or the file's default-track flag (any / default / non-default) (e.g. "all DTS/TrueHD → EAC3 5.1").
- **A plan that would drop every audio track of a file that has audio fails at plan time** (a silent file is data loss, not a target state); files without audio are unaffected (their drop plans are identities).
- **Cross-step**: the rules of **all** matching steps apply — concatenated in step order, first matching rule per track wins, and the default policy for tracks no rule names comes from the first matching step (§5 Operation composition). The zero-audio guardrail applies to the **composed** plan.
- **Atmos (EAC3-JOC) is copied unless an explicit rule re-encodes it** — never auto-downmixed.
- Re-encoding, when invoked, applies to **all** matching tracks (deterministic target state, not "primary only").
- **Filter graph**: optional ffmpeg `-af` expression (e.g. `loudnorm=…`) applied to **re-encoded** tracks only (a copied bitstream cannot be filtered; with no re-encode it is inert); empty = none — §6.5.

### 6.3 Subtitle section (absent ⇒ keep all, copy)

Two layers, mirroring the audio section. **Per-track rules** match on: language (normalized to ISO 639-1, the same normalization as audio), format (the subtitle codec — `srt`, `ass`, `webvtt`, `mov_text`, …), and the forced flag (any / forced / not-forced). A rule's action is **keep** or **drop** (v1 is copy-only — **no burn-in, no format conversion** — parked). Tracks no rule names fall through to the section's **policy**: `keep all` (default) / `keep first` (the first subtitle track in file order; a no-op for files without subtitles) / `keep forced-only` / `drop`. As with audio, rules from all matching steps merge in step order — first matching rule per track wins, the policy (default for unnamed tracks) comes from the first matching step (§5). Kept tracks are mapped explicitly (`-map 0:s:N`, file order) and pass the §6.4 container-fit check (the MP4 family takes only text-based subtitle codecs).

### 6.4 Container section (an explicit choice, with an optional MKV fallback)

The control is a **choice** (`mp4` / `mkv` / `webm` / `mov`) plus a **fall back to MKV** flag (on by default). Resolution happens per operation and judges the *planned* streams (an encoded video counts by its **target** codec; a re-encoded audio track by its target codec; dropped tracks are skipped):

- `mp4` / `mov` (the MP4 family — the one with a known stream matrix: video h264/hevc, audio eac3/ac3/aac, text-based subtitles): if every planned stream fits, the chosen container is used; otherwise **MKV when the fallback is on, a plan-time failure when it is off** (the file fails with a clear reason before anything is encoded).
- `mkv`: verbatim — the superset container.
- `webm`: verbatim — there is no plan-time matrix for it, so an incompatible stream fails at encode time with ffmpeg's own error (visible in the job log).

A choice that resolves to a container different from the source's turns an otherwise-stream-identical step into a **pure remux** (all streams copied, container changed) — this is how "remux all MKV to MP4" is expressed without enumerating source codecs. The **default** (MP4 with the fallback on — the old `smart`) **never** remuxes on its own: a stream-identical file keeps its container, whatever it is (otherwise enabling any video section would silently remux every MKV in the library). The editor exposes **this section only** as the container control; the `container` field inside the video section remains a parseable wire form for older flows (hidden in the editor, the top-level choice wins in resolution). Wire form: the top-level `container` field is `{ "choice": …, "fallback": … }`; legacy string forms (including `"smart"`) upgrade in place at parse time (§13.11).

### 6.5 Filter graphs (user expressions; one-shot semantics)

A filter graph is an ffmpeg `-vf`/`-af` expression the user types into the video or audio section — e.g. `crop=1920:800:0:0`, `denoise`, `loudnorm=I=-16:TP=-1.5`, `volume=2`. Empty means no filter. The planner places the expression at a fixed position (the video graph is the **last** element of the generated chain, after the downscale/HDR stages; graphs from several matching steps chain in reversed step order, §5; the audio graph applies per re-encoded track) and **validates it twice**: at flow-save (section-parameter parse check, via the registry) and at job start (executed against the installed ffmpeg — a failing job with a clear message beats a silent no-op). Each graph carries an optional **`once` flag** (a checkbox in the editor): an `once` graph is applied **at most once per file lineage** — it is never re-applied to a file that already records it, not even when the graph is later edited; changing it means restoring the original source. Unflagged graphs keep the re-run behavior below.

Filter graphs are **one-shot transformations**: the output's new facts do not record that a filter was applied (a cropped H.264 is still H.264), so "already done" is decided from what the file **records about itself** rather than from re-evaluation: the per-file **applied-operations record** plus, in-place, the **in-file marker** (§6.6). Both memories enumerate the **non-idempotent segments** actually run on the file's bytes — the user's filter graphs, and (in the future) any structured segment the registry marks non-idempotent. v1's own structured segments need no tracking: the ones we generate are convergent and fact-gated — `scale` is emitted only when the source size still differs from the target, and the HDR→SDR chain only when the source is still HDR — so re-running one on processed pixels is either a no-op or impossible. Re-run rules are per segment and per trigger: **a flow edit with the file unchanged** (a new effective-operation fingerprint) skips recorded segments that are still *unchanged*; an **`once`-flagged** graph is skipped unconditionally — re-applying an irreversible transformation to pixels it has already transformed would be wrong; an *edited* unflagged graph re-runs (a parameter edit is a new operation → one more pass, §13.14). **A change to the file's bytes** (the new sample hash clears the record) re-applies **everything** — a new source is a fresh canvas. **Neither** → the file is compliant. The record is written immediately after the file swap (before the gate) so a crash in between cannot cause a double application; the byte check is only as strong as the sample hash (xxh3-128 over the three 256 KB windows, §4). A graph must not undo the flow's other constraints for that step (e.g. a `scale` beyond the downscale target): the post-job gate then sees a non-compliant output, the job fails `non_idempotent`, and every scan re-queues the file.
### 6.6 Processed marker & applied-operations record

A file's bytes say nothing about which transformation produced them — a cropped H.264 is still H.264, a denoised HEVC is still HEVC — so a flow with **non-idempotent operations** (filter graphs today; burn-in and the like if ever un-parked) cannot be recognized as "already done" by re-evaluation alone. Two complementary memories solve this:

**The in-file marker.** Every job writes a marker into the **output file during the same ffmpeg invocation** (metadata rides the pass — zero extra work), and verification (§3.3) asserts the marker's presence in the output before the swap. The value is `transcodarr:t2:<op-fp>:<applied-fp>` — two 32-hex xxh3-128 digests: **`op-fp`** over the canonical JSON bytes of the **effective** operation (§5 — all matched sections, filter graphs included), and **`applied-fp`** over the canonical JSON array of the **non-idempotent segments actually run** to produce these bytes (empty array → the digest of `[]`). Written the same in every container. `op-fp` is a pure function of the *authored* flow (the file's facts decide which steps match), never of per-file resolution (device, resolved bitrate): an unrelated flow edit or a newly detected GPU never invalidates it, while any change to *this* file's operation does. `applied-fp` is **history, not a function of the flow** — the file's own account of what is baked into its pixels — verified only as a subset of the effective operation's non-idempotent segments. **No random component is part of it**: a scanner with a lost database recomputes `op-fp` from the flow and reads `applied-fp` from the file — that is what makes a file self-describing (per-event audit identity already lives in the `jobs` table). Markers written by earlier versions (`t1`, a single op-fp) remain readable: they carry no applied list, so a later flow edit re-runs the graphs once and re-stamps the file with a `t2` marker — the one-time upgrade cost, paid at most once per file.

| Output container | Storage location | Read back as |
|---|---|---|
| MKV, WebM | a custom global tag `transcodarr` (players ignore unknown tags) | format tag `TRANSCODARR` — the MKV muxer uppercases it, so lookups are case-insensitive |
| MP4, MOV | the `comment` field, **only if the source carries no comment of its own**; a used slot means the marker stays DB-only — user data is never clobbered | format tag `comment` carrying the value |

Verified on ffmpeg 9.0.1 (the image's pinned major version), with the scanner's own probe shape (`ffprobe -show_format`): custom keys survive in MKV/WebM global tags; the MP4 family accepts only the standard atom set, of which `comment` is the least visible slot (`encoder` is overwritten by the muxer at finalize). The reader accepts a tag whose value matches `transcodarr:t<ver>:<32-hex>` (t1) or `transcodarr:t2:<32-hex>:<32-hex>` (t2) whether it arrives as the custom key (MKV/WebM) or inside `comment` (MP4/MOV).

**The per-file record (database).** `files.applied_ops` (generalizing the former `applied_filters` ledger): `{marker, ops_json, applied: [seg-fp, …], file: {dev, inode, size, mtime, sample_hash}, job_id, applied_at}` — the full applied (effective) operation, the fingerprints of the **non-idempotent segments actually run** (`applied` — the marker's `applied-fp` expanded, so the database can name them and the marker can hash them), and the file fingerprint **at the moment of tagging**. In **in-place** mode the marker and the record are redundant memories: the marker survives a lost database, the record survives a metadata-stripping tool. In **output-tree** mode the original is never touched, so the record is the only memory, as before. Pre-t2 records (no `applied` field) backfill it from `ops_json` on first re-evaluation: a non-empty graph always forced the encode that produced the record, so every graph in the recorded operation was necessarily applied — the upgrade never double-applies, not even once.

**Verdicts.** Compliant = identity, *or* a plan whose **effective-operation fingerprint equals the marker's `op-fp`** and whose file fingerprint matches the record (with a lost database, marker-only suffices: tag present, matches the plan, file unchanged). The marker is ignored for unmatched files. A flow edit that changes the applied operation → new `op-fp` → mismatch → reprocess: the §6.5 skip rules decide which segments actually run (recorded non-idempotent segments are skipped, the rest re-run), and the job rewrites the marker (new `op-fp`, updated `applied-fp`). The post-job gate (§6.5) becomes: verification passes **and** the output carries the correct marker; a missing or wrong marker fails the job as `non_idempotent` (a safety net, not the normal path). **Default on**, with a global setting to disable it (off = DB-only: the record still tracks, the marker is simply not written).

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

- **Rust**, single static binary. `tokio` for async I/O; `rusqlite` for SQLite; **ffmpeg as a child process** — no libav bindings, so a crashed/hung encoder can never take the server down, and the binary stays cgo/FFI-free. All non-cryptographic hashing is **xxh3-128** via the MIT-licensed `twox-hash` crate (sample hash and in-file fingerprint both — §4, §6.6); the better-known `xxhash-rust` crate is BSL-1.0 and excluded by the AGPL rule.
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

- Ordered, **collapsible step cards**, each with two zones: **Filters** — an addable/removable list of rows (field picker + value control), all AND-ed; an unset row means "any", and no-op rows are pruned on save — and **Transcode** — one bordered box per operation section (video/audio/container/subtitles, §6), each independently on/off. Field and option names are rendered from schema labels (capitalized, friendly — e.g. "H.264", "Dolby Vision"); resolution bounds are typed as integer width × height pixel inputs (no presets), each bound an unbreakable group that may wrap onto its own line. **Audio and subtitle per-track rules render as compact rule rows in the same pattern as the step-level filter rows**: one row per rule — each match axis as a compact control (chip multi-selects for codecs / channel counts / sample rates / formats, text for language lists and title substrings, selects for the default / forced flags) that **collapses to nothing while unset**, the action select at the row's end (copy / re-encode / drop for audio; keep / drop for subtitles), added and removed exactly like filter rows; re-encode's sub-fields (codec, rate, channels) sit inline after the action. Unset axes are stripped on save, so logically identical rules serialize identically (fingerprint stability, §6.6); the filter graph's **once** checkbox (§6.5) sits with its field. Steps are renamable (display name only — array order defines precedence), and a filter kind appears at most once per step: picking an already-present kind is rejected without changing the step.
- **Impact preview (the killer feature)**: on every edit, `evaluate()` re-runs over the using libraries' cached facts and shows, before saving: *"this edit changes the fate of N files: 12 will transcode, 3 will lose audio tracks, 4,180 untouched; 2 files become unmatched."* — plus the §6.5 split of any re-encodes (*"of the 12, 4 re-run only the re-runnable parts; 2 already-applied graphs are skipped"*), and **exclusive-field precedence** between matched steps (*"3 files: step 1's \"drop\" wins; step 2's \"keep forced only\" is ignored"* — a later step's differing value never applies, §5).
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
| `files` | id, library_id, path, dev, inode, size, mtime, sample_hash, facts_json, status (`compliant / needs_work / unmatched / queued / running / failed / quarantined`), last_probed, last_evaluated, input_size, output_size, **applied_ops** (JSON `{marker, ops, applied, file-fingerprint-at-tagging, job_id, applied_at}` — the applied-operations record: the in-file marker value (t2: op-fp + applied-fp), the full applied (effective) operation, the fingerprints of the non-idempotent segments actually run, and dev/inode/size/mtime/sample hash at tagging time — §6.6; rename/generalization of the former `applied_filters`) |
| `jobs` | id, file_id, library_id, flow_version, plan_json, state, device_id, claimed_by, lease_expires, started, ended, exit_kind, log_path (the on-disk ffmpeg stderr file, retained for `tail -f`), **log_zstd** (the canonical copy — zstd-compressed on job end, served decompressed by the log endpoint; pre-migration rows fall back to the file), quarantine_path — **retained: this table *is* the per-file job history** |
| `devices` | id, kind (`cpu / gpu`), name, encoders_json, max_concurrent |
| `settings` | key, value |

Indexes: `files(library_id, status)`, `files(path)`, `jobs(state, started)`. File identity is path-based (like the \*arrs); a path that disappears and reappears with new facts is treated as a new file (verdicts cleared, prior job rows preserved as history).

---

## 11. v1 scope

**In:** everything in §2–§10. Condition fields of §5; **operation composition across matched steps** (the effective operation, §5); operations of §6 (h264/hevc targets, MP4/MKV, GPU = NVENC + VAAPI + QSV, **per-track subtitle rules**, **non-idempotent segment tracking** with the `once` flag and marker t2, §6.5–6.6); in-place + output-tree lifecycle; watch + scan + manual triggers; the five UI screens; the multi-node seam (dormant).

**Out (v1):** see §12.

---

## 12. Parked (designed for, not built)

- **Multi-node**: remote worker binary over the claim endpoint; one coordinator + N workers.
- **\*arr integration**: Radarr/Sonarr re-import/re-scan automation after in-place extension renames; webhooks as a trigger source.
- **AV1** target (newer-GPU-only encoder) as a preset-grade option; **two-pass** encoding; **subtitle burn-in**.
- **Audio-only and image libraries** (different rule shapes).
- Additional condition fields (profile, level, frame rate, duration, subtitle presence) and OR-condition groups — additive via the registry, no migrations.
- HDR handling beyond the tonemap method + desat selection (§6.1): per-HDR-format conversion chains, tonemapping presets/bundles.

---

## 13. Judgment calls (defaults the owner can veto)

Made during the design session; each is a deliberate default, not a constraint:

1. Backup retention default **7 days**; auto-delete **OFF**.
2. Scan default **daily**; stability gate **60 s**.
3. CPU concurrency default `max(1, cores/4)`; GPU default **2** each.
4. **Auto-queue ON** for newly detected files (the unattended-compliance promise); per-library kill switch.
5. No built-in auth in v1 (reverse-proxy pattern).
6. Single-pass encoding in v1.
7. Target container defaults to **MP4 with the MKV fallback on** (the old smart: MP4 if safe, else MKV). Any explicit choice **without** the fallback fails at plan time if a stream doesn't fit (§6.4).
8. Docker image = canonical artifact; bare binary detects system ffmpeg.
9. In-place renames adopt the true extension (orphaning \*arr records until re-scan; automation parked).
10. **Svelte 5 + Tailwind CSS + shadcn-svelte** for the SPA (copy-in-source; Melt UI underneath). The styling layer is swappable — the schema-driven editor (§5) does not depend on it.
11. **Legacy wire forms are accepted at parse time and upgraded in place** (no flow-migration endpoint in v1): a device object without `kind` (`{"id": "…"}`) ⇒ GPU; `profile`/`level: null` ⇒ auto; the audio policy string `"re-encode"` ⇒ re-encode to the default target, source rate/channels kept; the container string `"smart"` ⇒ MP4 with the fallback on, and `"mp4"`/`"mkv"`/`"webm"`/`"mov"` ⇒ that container **without** the fallback.
12. **Live updates ride one SSE stream per client, not WebSockets** (§9.0) — the event flow is strictly one-way (client actions are ordinary POSTs); SSE is proxy-friendly, auto-reconnects, and resumes logs via `Last-Event-ID`.
13. **Flows are first-class, library-independent objects** (a `flows` table; `libraries.flow_id` is nullable). Editing a flow re-evaluates every library that references it; deleting a flow unassigns its libraries (files settle to unmatched) rather than cascading.
14. **Filter graphs are one-shot, not target-state** (§6.5): every user graph is recorded per file as a non-idempotent segment (`files.applied_ops`, plus the in-file marker — §6.6). A **flow edit with the file unchanged** skips recorded graphs that are still unchanged; an **`once`-flagged** graph is skipped unconditionally — never re-applied to a file it has already transformed, even if edited (changing it means restoring the source); an edited unflagged graph re-runs (a parameter edit is a new operation → one more pass). A **change to the file's bytes** re-applies everything (a new source is a fresh canvas). The structured segments we generate are convergent and fact-gated — they never need this. The marker (t2) carries the applied list, so a file stays self-describing with a lost database.
15. **All non-cryptographic hashes are xxh3-128** (§4, §6.6): the sample hash (three 256 KB windows — head, middle, tail — up from FNV-1a over 16 KB; pre-v1.0, a clean break, with the one-time migration re-stamping stored values from current bytes so an upgrade never reprocesses a file) and the fingerprint embedded in files. Both digests are 128-bit: xxh3 is the generation upstream recommends (the classic xxh64/xxh32 are kept only for compatibility) and it is the faster one at these sizes (SIMD paths; classic xxh64 has none), and a uniform width removes a branching decision — these are change signals, not security boundaries, so 128 bits is already generous. The sample is three contiguous windows, not scattered offsets: head/tail catch container metadata (MP4 `moov`, MKV cues live at the file's ends) and the middle covers content; contiguous reads cost far less on NAS than scattered ones, and the never-whole-file rule stands. The fingerprint hashes the canonical JSON bytes directly, never a compressed form: at sub-KB input, compression adds frame overhead, and zstd's output depends on encoder version — exactly the kind of cross-release drift that would silently invalidate a library of markers. Stability is enforced in CI in two directions: the hash digests are pinned to published known-answer vectors, and the *fingerprint* is pinned end-to-end (a fixture operation JSON → an exact marker string), so no release can change canonicalization and silently invalidate a library on upgrade; and a marker write→probe round-trip is asserted for every output container, so a change in the pinned ffmpeg's metadata behavior fails the build rather than quietly losing markers.
16. **Processed files are marked in the file, on by default** (§6.6): the marker rides the encoding/remux pass; MP4/MOV use the `comment` slot only when the source has none (user data is never clobbered). A global setting disables the marker (DB-only mode).
17. **Non-idempotency is declared, not inferred** (§6.5): an arbitrary ffmpeg expression cannot be analyzed for idempotency, so *every* user graph is recorded in the file's applied list; the optional **`once` flag (off by default)** is the user's declaration that a graph must not be re-applied — skipping then extends from *unchanged* graphs to *also-edited* ones. Structured segments we generate are convergent (they gate themselves off by facts) and carry no flag.

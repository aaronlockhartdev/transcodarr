# ADR-0003: Typed contract between the Rust API and the Svelte frontend

Status: proposed. Human owner: Aaron; stamped `accepted (date)` at the merge that lands it.

## Context

One Rust binary serves the app: SvelteKit with `ssr = false`, `@sveltejs/adapter-static` with
`fallback: 'index.html'`, embedded by `rust-embed`. There is no JS runtime in production, so every
byte of data reaches the UI through the JSON API, and no `+page.server.ts` load function runs at
runtime. Dev is two processes (`vite dev` proxying `/api` to the Rust binary); release is one. That
asymmetry, plus an agent's stale training data on Svelte 5 and Kit 3, makes the generated contract
more important here than in a typical repo.

## Decision

### The pipeline

utoipa 6 annotates the axum handlers and writes `contracts/openapi.json`, which is
committed. `openapi-typescript` generates `apps/web/src/lib/api/schema.ts` from that file, and the
app calls the API through `openapi-fetch`, so the types come from the spec.

### Generated files are never hand-edited

`apps/web/src/lib/api/**` and `contracts/openapi.json`
are generated; the drift gate regenerates them in CI and fails on `git diff --exit-code` of those
paths. The failure message names the fix: edit the Rust handler, then run `just gen`. Do not
hand-merge them. Resolve with `just gen && git add`.

### Pinned resolution, verified as a set
`typescript 6.0.3` with `overrides: { "openapi-typescript": { "typescript": "$typescript" } }`,
`@sveltejs/kit 3.4.0`, `@sveltejs/adapter-static 4.0.1`, `@sveltejs/vite-plugin-svelte 7.3.2`,
`svelte 5.59.1`, `vite 8.3.8`, `svelte-check 4.7.8`, `openapi-typescript 7.13.0`,
`openapi-fetch 0.17.0`, on `node 22.23.5` via mise (`mise.toml`; `just setup` asserts the effective toolset).

### OpenAPI 3.1

utoipa 6's 3.2 support is opt-in (`Version32`) while `openapi-typescript` supports
3.0 and 3.1, so 3.2 stays off. With it on, the drift gate would generate from a spec the generator
cannot parse.

### Frontend conventions Kit 3 changed

(each cost a build attempt):
no `svelte.config.js`; plugin is `import { sveltekit } from '@sveltejs/kit/vite'`; `tsconfig.json`
extends `"$app/tsconfig"`; `#lib` replaces `$lib` and is declared in `package.json` `imports` as
`{"#lib/*": "./src/lib/*"}`; **specifiers must carry the extension** (`#lib/api/client.ts`) or
TypeScript cannot resolve them; `config.alias` is deprecated and unused.

## Rejected alternatives

|alternative|why rejected|
|---|---|
|hand-written TypeScript types|the exact drift this gate exists to catch; they are also the first thing an agent edits when the generator blocks|
|`specta` / `ts-rs`|no OpenAPI artifact; we want a contract that formalises the boundary and can be diffed, in addition to types|
|`aide`|viable, materially less active|
|`typescript 7.0.2` (npm `latest`)|Kit 3.4.0's own peer is `^6.0.0`, so 7.x conflicts with the framework and the generator|
|dropping `openapi-typescript` to satisfy its `^5.x` peer|the peer is advisory for a type-only generator; one `overrides` entry keeps both tools|
|SSR-capable SvelteKit behind the same binary|needs a JS runtime in the binary, contradicting single-process; SPA plus static embed is the design (ADR-0001 context)|
|`sppl` crate to embed and serve|0.0.5, single maintainer, and the job is ~30 lines of `rust-embed` plus a fallback handler|

## Evidence

Verified on this machine 2026-11-24 with the pin set above. The control: `npm install`
without the `overrides` entry fails `ERESOLVE … peer typescript@"^5.x" from
openapi-typescript@7.13.0`; with it the install is clean. Green run: `svelte-kit sync`, then
`svelte-check --threshold error` (0 errors, 0 warnings), then `vite build` produces
`build/index.html`, typechecking runes, path params and nullable narrowing.

Two claims here are measured. Regenerating the generated types from the same spec is
byte-identical, which is what makes `git diff --exit-code` a valid drift gate. And
extension-less `#lib` specifiers are refused by `tsc --traceResolution` after the mapping
resolves, which is where the extension rule comes from.

## Binding summary for AGENTS.md

1. Do not hand-edit or hand-merge `contracts/openapi.json` or `apps/web/src/lib/api/**`. The drift
   gate refuses and prints `just gen`. Until that gate exists: regenerate, do not patch.
2. Do not add web-workspace dependencies. The pin set resolves as a whole (TS 6.0.3 plus
   `overrides`) and `just check` typechecks the result.

Machine-enforced, so out of `AGENTS.md`: the `#lib/…ts` extension rule (`tsc` refuses an
extension-less specifier; a `tsconfig.json` comment states the fix) and toolchain resolution
(mise prints the override unprompted, and `mise.toml` is the only pin site).

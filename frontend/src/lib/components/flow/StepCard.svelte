<script lang="ts">
  /**
   * One flow step: a collapsible card with two zones — Filters (the file
   * must match all of them; add/remove freely) and Transcode (the
   * operation sections to apply). Everything is rendered from the flow
   * schema (core §5); absent section = identity.
   */
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import SyncSwitch from "./SyncSwitch.svelte";
  import ChevronUpIcon from "@lucide/svelte/icons/chevron-up";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import TrashIcon from "@lucide/svelte/icons/trash-2";
  import { toast } from "svelte-sonner";
  import FlowField from "./FlowField.svelte";
  import type {
    Device,
    FlowStep,
    FlowSchema,
    SchemaOption,
    UiSchema,
  } from "$lib/types.js";

  let {
    step,
    index,
    total,
    schema,
    devices,
    onRemove,
    onMoveUp,
    onMoveDown,
  }: {
    step: FlowStep;
    index: number;
    total: number;
    schema: FlowSchema;
    devices: Device[];
    onRemove: () => void;
    onMoveUp: () => void;
    onMoveDown: () => void;
  } = $props();

  let open = $state(true);

  // Display label for a schema field/section: the schema's own label when
  // present, else a title-cased key ("file_size" → "File size").
  function label(key: string, s?: { label?: string } | null): string {
    if (s?.label) return s.label;
    return key.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  }

  // ---- filters (condition = an ordered list of field constraints) ------
  // The value reads below keep this derived subscribed to constraint
  // changes; key add/remove re-runs it as well (Svelte 5's $state object
  // proxy notifies on both).
  const filterKeys = $derived.by(() => {
    const c = step.condition;
    const keys = Object.keys(c);
    for (const k of keys) void (c as Record<string, unknown>)[k];
    return keys;
  });

  // The "any" constraint for a freshly added filter row: an empty list
  // (in/any_of) or {} both mean "any" on the server, and keeping the key
  // keeps the row visible while the user fills it in. save() prunes
  // no-op constraints before sending.
  function anyConstraint(field: string): Record<string, unknown> {
    const kind = (
      schema.condition_fields[field]?.schema as { kind?: string } | undefined
    )?.kind;
    return field === "audio_codec"
      ? { any_of: [] }
      : kind === "multi_select"
        ? { in: [] }
        : {};
  }

  function addFilter() {
    const c = { ...step.condition };
    for (const k of Object.keys(schema.condition_fields)) {
      if (c[k] === undefined) {
        c[k] = anyConstraint(k);
        step.condition = c;
        return;
      }
    }
    toast("Every filter type is already on this step");
  }

  function setFilterField(
    oldKey: string,
    newKey: string,
    sel: HTMLSelectElement,
  ) {
    if (oldKey === newKey) return;
    // A filter of this kind already on the step: reject the swap (no
    // state change, so nothing is marked unsaved).
    if ((step.condition as Record<string, unknown>)[newKey] !== undefined) {
      toast(
        `A ${label(newKey, schema.condition_fields[newKey]?.schema as { label?: string })} filter is already on this step`,
      );
      // Svelte only rewrites value= when the bound value changes:
      // put the picker back where it was.
      sel.value = oldKey;
      return;
    }
    const c: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(step.condition)) {
      if (k !== oldKey) c[k] = v;
    }
    c[newKey] = anyConstraint(newKey); // the swapped-in row moves last
    step.condition = c;
  }

  // A rejected swap (target kind used by another row) reverts the
  // select's displayed value to the row's current field.
  function resetFilterField(el: HTMLSelectElement, current: string) {
    el.value = current;
  }

  function removeFilter(field: string) {
    const c = { ...step.condition };
    delete c[field];
    step.condition = c;
  }

  // multi_select constraints: audio_codec matches ANY track (any_of), all
  // other fields are exact-membership (in).
  function constraintKey(field: string): "in" | "any_of" {
    return field === "audio_codec" ? "any_of" : "in";
  }
  function listFor(field: string): string[] {
    const c = step.condition[field] as Record<string, unknown> | undefined;
    if (!c) return [];
    const key = constraintKey(field);
    return Array.isArray(c[key]) ? (c[key] as string[]) : [];
  }
  function setList(field: string, list: string[]) {
    step.condition = {
      ...step.condition,
      [field]: { [constraintKey(field)]: list },
    };
  }
  function toggleChip(field: string, v: string) {
    const list = [...listFor(field)];
    const i = list.indexOf(v);
    if (i === -1) list.push(v);
    else list.splice(i, 1);
    setList(field, list);
  }

  // resolution_range: {min?: [w,h], max?: [w,h]} — each bound is edited as
  // two integer pixel inputs (width and height; compared as w*h server-side).
  type ResRange = { min?: [number, number]; max?: [number, number] };
  function resFor(): ResRange {
    return (step.condition["resolution"] as ResRange) ?? {};
  }
  function setRes(which: "min" | "max", wh?: [number, number]) {
    const r: ResRange = { ...resFor() };
    if (wh) r[which] = wh;
    else delete r[which];
    step.condition = { ...step.condition, resolution: r };
  }
  let resMinW = $state("");
  let resMinH = $state("");
  let resMaxW = $state("");
  let resMaxH = $state("");
  $effect(() => {
    // Resync the edit buffers from the constraint (a commit normalizes
    // whatever was typed back into these fields).
    const [mw, mh] = resFor().min ?? [null, null];
    const [xw, xh] = resFor().max ?? [null, null];
    resMinW = mw == null ? "" : String(mw);
    resMinH = mh == null ? "" : String(mh);
    resMaxW = xw == null ? "" : String(xw);
    resMaxH = xh == null ? "" : String(xh);
  });
  function resPair(w: string, h: string): [number, number] | null {
    const wi = /^\d+$/.test(w.trim()) ? Number(w.trim()) : -1;
    const hi = /^\d+$/.test(h.trim()) ? Number(h.trim()) : -1;
    return wi > 0 && hi > 0 ? [wi, hi] : null;
  }
  function commitRes(which: "min" | "max") {
    const w = which === "min" ? resMinW : resMaxW;
    const h = which === "min" ? resMinH : resMaxH;
    if (w.trim() === "" && h.trim() === "") {
      setRes(which); // cleared → unbounded
      return;
    }
    // An incomplete or invalid pair keeps the previous bound.
    setRes(which, resPair(w, h) ?? resFor()[which]);
  }

  // byte_range: {min?, max?} in bytes, edited in megabytes.
  type ByteRange = { min?: number; max?: number };
  function sizeFor(): ByteRange {
    return (step.condition["file_size"] as ByteRange) ?? {};
  }
  function setSize(which: "min" | "max", mb: string) {
    const s: ByteRange = { ...sizeFor() };
    const bytes = Math.round(Number(mb) * 1_000_000);
    if (mb && bytes > 0) s[which] = bytes;
    else delete s[which];
    step.condition = { ...step.condition, file_size: s };
  }

  // ---- operation sections ----------------------------------------------
  // NOTE: `name in step.operation` is NOT a tracked read on the $state
  // proxy (only property gets subscribe), so use explicit gets here.
  function sectionEnabled(name: string): boolean {
    return (step.operation as Record<string, unknown>)[name] !== undefined;
  }
  function setSection(name: string, enabled: boolean) {
    const next = { ...step.operation };
    if (enabled) {
      if (next[name] === undefined) {
        // seedSection wraps the schema field defaults in wire shape.
        next[name] = seedSection(name);
      }
    } else {
      delete next[name];
    }
    step.operation = next;
  }
  // Binding-target map into the live (reactive) operation object.
  const op = $derived.by(() => {
    const o = step.operation as unknown as Record<string, any>;
    return o;
  });
  function sectionValue(name: string): Record<string, unknown> {
    const v = step.operation[name];
    return (v as Record<string, unknown>) ?? {};
  }
  // Enable-time defaults, from the schema (never hardcoded): each field's
  // default wrapped in its wire shape, so an enabled section is complete
  // and parseable (video, for instance, requires codec).
  function seedSection(name: string): Record<string, unknown> {
    const f = schema.operation_sections[name]?.schema as
      | {
          default?: unknown;
          fields?: Record<string, { kind?: string; default?: unknown }>;
        }
      | undefined;
    if (!f) return {};
    // A section with no per-field schema (e.g. `container`) is a
    // single-select: its seed is the schema default itself (a string
    // like "smart"), never an object — the top-level field has to
    // match what Operation's serde expects.
    if (!f.fields) return f.default as unknown as Record<string, unknown>;
    const seeded: Record<string, unknown> = {};
    for (const [k, fld] of Object.entries(f.fields)) {
      if (fld.default === undefined) continue;
      seeded[k] =
        fld.kind === "bitrate_mode" ? { mode: fld.default } : fld.default;
    }
    return seeded;
  }
  function setSectionField(name: string, field: string, value: unknown) {
    const sec = { ...sectionValue(name) };
    if (value === undefined) delete sec[field];
    else sec[field] = value;
    step.operation = { ...step.operation, [name]: sec };
  }

  // The Container section is the single container control. The video
  // section also carries a container field in its schema (legacy wire
  // form — old flows may only have the value there), so it is hidden
  // from the UI. This shows the effective target — top-level field,
  // else the video-section value, both with legacy string forms
  // upgraded in place — and writes the top-level field, which wins in
  // resolution.
  function upgradeContainer(v: unknown): { choice: string; fallback: boolean } {
    if (typeof v === "string") {
      if (v === "smart") return { choice: "mp4", fallback: true };
      if (v === "mp4" || v === "mkv" || v === "webm" || v === "mov")
        return { choice: v, fallback: false };
      return { choice: "mp4", fallback: true };
    }
    if (v !== null && typeof v === "object") {
      const o = v as { choice?: unknown; fallback?: unknown };
      const choice =
        typeof o.choice === "string"
          ? upgradeContainer(o.choice).choice
          : "mp4";
      return {
        choice,
        fallback: o.fallback === undefined ? true : !!o.fallback,
      };
    }
    return { choice: "mp4", fallback: true };
  }
  const containerSpec = $derived.by(() => {
    const o = step.operation as unknown as Record<string, any>;
    return upgradeContainer(
      o["container"] ??
        (o["video"] as { container?: unknown } | undefined)?.container,
    );
  });
  function setContainerSpec(spec: { choice: string; fallback: boolean }) {
    step.operation = {
      ...(step.operation as Record<string, unknown>),
      container: spec,
    };
  }

  // audio rules
  type Rule = {
    match?: {
      codecs?: string[];
      languages?: string[];
      channels?: number[];
      sample_rate?: number[];
      title_contains?: string;
      default?: string;
    };
    action: unknown;
  };
  function rulesFor(): Rule[] {
    const a = step.operation["audio"] as { rules?: Rule[] } | undefined;
    return a?.rules ?? [];
  }
  function setRules(rules: Rule[]) {
    const a = {
      ...((step.operation["audio"] as Record<string, unknown>) ?? {}),
    };
    a["rules"] = rules;
    step.operation = { ...step.operation, audio: a };
  }
  function addRule() {
    setRules([...rulesFor(), { match: {}, action: "copy" }]);
  }
  function removeRule(i: number) {
    setRules(rulesFor().filter((_, j) => j !== i));
  }
  function ruleMatch(i: number): Record<string, unknown> {
    const m = rulesFor()[i]?.match;
    return m !== null && typeof m === "object"
      ? { ...(m as Record<string, unknown>) }
      : {};
  }
  function writeRuleMatch(i: number, m: Record<string, unknown>) {
    // Unset axes are dropped from the wire so the flow JSON — and
    // therefore the marker's fingerprint — stays minimal ("any"/
    // empty mean absent, same as an older flow).
    for (const [k, v] of Object.entries(m)) {
      if (Array.isArray(v) && v.length === 0) delete m[k];
      if (k === "title_contains" && (v == null || v === "")) delete m[k];
      if (k === "default" && (v == null || v === "any")) delete m[k];
    }
    setRules(rulesFor().map((r, j) => (j === i ? { ...r, match: m } : r)));
  }
  function matchList(
    rule: Rule,
    field: "codecs" | "channels" | "sample_rate",
  ): string[] {
    const v = rule.match?.[field];
    return Array.isArray(v) ? (v as (string | number)[]).map(String) : [];
  }
  function toggleRuleList(
    i: number,
    field: "codecs" | "channels" | "sample_rate",
    value: string,
  ) {
    const m = { ...ruleMatch(i) };
    const list = (m[field] as string[] | number[] | undefined) ?? [];
    const num = field !== "codecs";
    const norm = (x: string | number) => (num ? Number(x) : x);
    const next = list.map(norm);
    const v = norm(value);
    const at = next.indexOf(v);
    if (at === -1) next.push(v);
    else next.splice(at, 1);
    m[field] = next;
    writeRuleMatch(i, m);
  }
  function setRuleLanguages(i: number, text: string) {
    const m = { ...ruleMatch(i) };
    const list = text
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    if (list.length > 0) m.languages = list;
    else delete m.languages;
    writeRuleMatch(i, m);
  }
  function setRuleTitle(i: number, text: string) {
    const m = { ...ruleMatch(i) };
    const t = text.trim();
    if (t) m.title_contains = t;
    else delete m.title_contains;
    writeRuleMatch(i, m);
  }
  function setRuleDefault(i: number, v: string) {
    const m = { ...ruleMatch(i) };
    if (v === "any") delete m.default;
    else m.default = v;
    writeRuleMatch(i, m);
  }
  function setRuleAction(i: number, action: unknown) {
    setRules(rulesFor().map((r, j) => (j === i ? { ...r, action } : r)));
  }
  // The audio section's `default` field carries the policy schema (kind:
  // audio_policy) shared by the rule-action select.
  const policyOpts = $derived.by(() => {
    const f = (
      schema.operation_sections["audio"]?.schema as {
        fields?: Record<string, { values?: SchemaOption[] }>;
      }
    )?.fields?.["default"];
    return f?.values ?? [];
  });
  function policySelectValue(action: unknown): string {
    // A re-encode object must display under the "re_encode" option, not
    // the first item in the list.
    if (action !== null && typeof action === "object") return "re_encode";
    return typeof action === "string" ? action : "copy";
  }
  function reencodeActionDefault(): Record<string, unknown> {
    const f = (
      schema.operation_sections["audio"]?.schema as {
        fields?: Record<
          string,
          { reencode?: { codec?: { values?: { value: string }[] } } }
        >;
      }
    )?.fields?.["default"];
    return { codec: f?.reencode?.codec?.values?.[0]?.value ?? "eac3" };
  }
  function ruleActionObj(i: number): Record<string, unknown> {
    const a = rulesFor()[i]?.action;
    return a !== null && typeof a === "object"
      ? { ...(a as Record<string, unknown>) }
      : {};
  }
  function writeRuleAction(i: number, a: Record<string, unknown>) {
    // Empty rate/channels mean "keep the source" — drop the keys so they
    // are omitted from the JSON entirely.
    for (const k of ["sample_rate", "channels"]) {
      const v = a[k];
      if (v === undefined || v === null || v === "") delete a[k];
    }
    setRuleAction(i, a);
  }
  function setRuleReencodeRate(i: number, text: string) {
    const a = ruleActionObj(i);
    const n = Number.parseInt(text, 10);
    a.sample_rate = Number.isInteger(n) && n >= 1 ? n : undefined;
    writeRuleAction(i, a);
  }
  function setRuleReencodeChannels(i: number, text: string) {
    const a = ruleActionObj(i);
    const n = Number.parseInt(text, 10);
    a.channels = Number.isInteger(n) && n >= 1 ? n : undefined;
    writeRuleAction(i, a);
  }

  // The audio section's policy fragment supplies the re-encode codec list.
  const reencodeCodecOpts = $derived.by(() => {
    const f = (
      schema.operation_sections["audio"]?.schema as {
        fields?: Record<
          string,
          { reencode?: { codec?: { values?: SchemaOption[] } } }
        >;
      }
    )?.fields?.["default"];
    return f?.reencode?.codec?.values ?? [];
  });
  // The rule-match field schemas: vocabularies and labels come from
  // the audio section's item schema, never hardcoded.
  type MatchFields = {
    codecs?: {
      kind?: string;
      label?: string;
      hint?: string;
      values?: SchemaOption[];
    };
    languages?: { kind?: string; label?: string; hint?: string };
    channels?: { kind?: string; label?: string; values?: SchemaOption[] };
    sample_rate?: { kind?: string; label?: string; values?: SchemaOption[] };
    title_contains?: { kind?: string; label?: string; hint?: string };
    default?: { kind?: string; label?: string; values?: SchemaOption[] };
  };
  function ruleMatchFields(): MatchFields {
    const f = (
      schema.operation_sections["audio"]?.schema as {
        fields?: Record<string, { item?: { match?: MatchFields } }>;
      }
    )?.fields?.["rules"] as { item?: { match?: MatchFields } } | undefined;
    return f?.item?.match ?? {};
  }
  const ruleMatchCodecOpts = $derived.by(
    () => ruleMatchFields().codecs?.values ?? [],
  );
  const ruleMatchChannelOpts = $derived.by(
    () => ruleMatchFields().channels?.values ?? [],
  );
  const ruleMatchRateOpts = $derived.by(
    () => ruleMatchFields().sample_rate?.values ?? [],
  );
  const ruleMatchDefaultOpts = $derived.by(
    () => ruleMatchFields().default?.values ?? [{ value: "any", label: "Any" }],
  );
  const ruleMatchLangField = $derived.by(() => ruleMatchFields().languages);
  const ruleMatchTitleField = $derived.by(
    () => ruleMatchFields().title_contains,
  );

  // ---- header summary (visible collapsed) ------------------------------
  const enabledSectionNames = $derived.by(() => {
    const o = step.operation as Record<string, unknown>;
    const names = Object.keys(o);
    for (const n of names) void o[n];
    return names.filter((n) => o[n] !== undefined);
  });
  const summary = $derived.by(() => {
    const filters = filterKeys.length;
    const left =
      filters === 0
        ? "matches every file"
        : filters === 1
          ? "1 filter"
          : `${filters} filters`;
    const right =
      enabledSectionNames.length === 0
        ? "no changes"
        : enabledSectionNames
            .map((n) =>
              label(
                n,
                (schema.operation_sections[n]?.schema as
                  { label?: string } | undefined) ?? null,
              ),
            )
            .join(", ");
    return `${left} · ${right}`;
  });
</script>

<div class="rounded-lg border">
  <div class="flex items-center gap-2 p-3">
    <Button
      variant="ghost"
      size="sm"
      onclick={() => (open = !open)}
      title={open ? "Collapse step" : "Expand step"}
    >
      {#if open}<ChevronDownIcon class="size-4" />{:else}<ChevronRightIcon
          class="size-4"
        />{/if}
    </Button>
    <input
      class="h-7 w-44 min-w-0 rounded-md border border-transparent bg-transparent px-1.5 text-sm font-medium outline-none hover:border-input focus-visible:border-input focus-visible:ring-3 focus-visible:ring-ring/50"
      placeholder={`Step ${index + 1}`}
      value={step.name ?? ""}
      title="Rename this step"
      oninput={(e) => (step.name = (e.target as HTMLInputElement).value)}
      onkeydown={(e) => e.key === "Enter" && (e.target as HTMLElement).blur()}
    />
    <span class="hidden truncate text-xs text-muted-foreground sm:inline"
      >{summary}</span
    >
    <div class="ml-auto flex gap-1">
      <Button
        variant="ghost"
        size="sm"
        onclick={onMoveUp}
        disabled={index === 0}
        title="Move up"
      >
        <ChevronUpIcon class="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="sm"
        onclick={onMoveDown}
        disabled={index === total - 1}
        title="Move down"
      >
        <ChevronDownIcon class="size-4" />
      </Button>
      <Button variant="ghost" size="sm" onclick={onRemove} title="Delete step">
        <TrashIcon class="size-4" />
      </Button>
    </div>
  </div>

  {#if open}
    <div class="flex flex-col gap-4 border-t p-3">
      <!-- Filters: the file must match all of them -->
      <section class="rounded-md border bg-muted/40 p-3">
        <div class="flex flex-col gap-0.5">
          <h3 class="text-sm font-medium">Filters</h3>
          <p class="text-xs text-muted-foreground">
            The file must match all of these
          </p>
        </div>
        <div class="mt-3 flex flex-col gap-2">
          {#each filterKeys as field (field)}
            {@const f = schema.condition_fields[field]}
            <div class="flex flex-wrap items-center gap-2">
              <select
                class="h-8 w-36 rounded-lg border border-input bg-transparent px-2 text-sm"
                value={field}
                onchange={(e) => {
                  const v = (e.target as HTMLSelectElement).value;
                  setFilterField(field, v, e.target as HTMLSelectElement);
                  if (v !== field)
                    resetFilterField(e.target as HTMLSelectElement, field);
                }}
              >
                {#each Object.entries(schema.condition_fields) as [k, cf] (k)}
                  <!-- a kind another row already uses can't be picked twice -->
                  <option
                    value={k}
                    disabled={k !== field &&
                      (step.condition as Record<string, unknown>)[k] !==
                        undefined}
                    >{label(k, cf.schema as { label?: string })}</option
                  >
                {/each}
              </select>
              <div class="min-w-0 flex-1">
                {#if f.schema.kind === "multi_select"}
                  <div class="flex flex-wrap gap-1">
                    {#each f.schema.values as v (v.value)}
                      <Button
                        type="button"
                        variant={listFor(field).includes(v.value)
                          ? "default"
                          : "outline"}
                        size="sm"
                        class="h-auto px-2 py-0.5 text-xs"
                        onclick={() => toggleChip(field, v.value)}
                      >
                        {v.label}
                      </Button>
                    {/each}
                  </div>
                {:else if f.schema.kind === "resolution_range"}
                  <div
                    class="flex flex-wrap items-center gap-x-4 gap-y-2 text-xs"
                  >
                    <div class="flex flex-nowrap items-center gap-2">
                      <span class="text-muted-foreground">At least</span>
                      <Input
                        type="number"
                        min="1"
                        class="h-8 w-24"
                        value={resMinW}
                        oninput={(e) =>
                          (resMinW = (e.target as HTMLInputElement).value)}
                        onblur={() => commitRes("min")}
                        onkeydown={(e) =>
                          e.key === "Enter" && (e.target as HTMLElement).blur()}
                        placeholder="width"
                      />
                      <span class="text-muted-foreground">×</span>
                      <Input
                        type="number"
                        min="1"
                        class="h-8 w-24"
                        value={resMinH}
                        oninput={(e) =>
                          (resMinH = (e.target as HTMLInputElement).value)}
                        onblur={() => commitRes("min")}
                        onkeydown={(e) =>
                          e.key === "Enter" && (e.target as HTMLElement).blur()}
                        placeholder="height"
                      />
                    </div>
                    <div class="flex flex-nowrap items-center gap-2">
                      <span class="text-muted-foreground">At most</span>
                      <Input
                        type="number"
                        min="1"
                        class="h-8 w-24"
                        value={resMaxW}
                        oninput={(e) =>
                          (resMaxW = (e.target as HTMLInputElement).value)}
                        onblur={() => commitRes("max")}
                        onkeydown={(e) =>
                          e.key === "Enter" && (e.target as HTMLElement).blur()}
                        placeholder="width"
                      />
                      <span class="text-muted-foreground">×</span>
                      <Input
                        type="number"
                        min="1"
                        class="h-8 w-24"
                        value={resMaxH}
                        oninput={(e) =>
                          (resMaxH = (e.target as HTMLInputElement).value)}
                        onblur={() => commitRes("max")}
                        onkeydown={(e) =>
                          e.key === "Enter" && (e.target as HTMLElement).blur()}
                        placeholder="height"
                      />
                    </div>
                  </div>
                {:else if f.schema.kind === "byte_range"}
                  {@const s = sizeFor()}
                  <div class="flex items-center gap-2">
                    <Input
                      type="number"
                      min="0"
                      class="h-8 w-24"
                      value={s.min ? String(Math.round(s.min / 1_000_000)) : ""}
                      oninput={(e) =>
                        setSize("min", (e.target as HTMLInputElement).value)}
                      placeholder="min"
                    />
                    <span class="text-muted-foreground">→</span>
                    <Input
                      type="number"
                      min="0"
                      class="h-8 w-24"
                      value={s.max ? String(Math.round(s.max / 1_000_000)) : ""}
                      oninput={(e) =>
                        setSize("max", (e.target as HTMLInputElement).value)}
                      placeholder="max"
                    />
                    <span class="text-xs text-muted-foreground">MB</span>
                  </div>
                {:else}
                  <FlowField
                    schema={f.schema}
                    bind:value={step.condition[field]}
                    {devices}
                  />
                {/if}
              </div>
              <Button
                variant="ghost"
                size="sm"
                title="Remove filter"
                onclick={() => removeFilter(field)}
              >
                <TrashIcon class="size-3.5" />
              </Button>
            </div>
          {:else}
            <p class="text-xs text-muted-foreground">
              No filters — this step matches every file.
            </p>
          {/each}
          <Button variant="outline" size="sm" onclick={addFilter}>
            <PlusIcon class="size-3.5" />
            Add filter
          </Button>
        </div>
      </section>

      <!-- Transcode: operation sections; off = pass through -->
      <section class="rounded-md border p-3">
        <div class="flex flex-col gap-0.5">
          <h3 class="text-sm font-medium">Transcode</h3>
          <p class="text-xs text-muted-foreground">
            Sections you leave off leave the file unchanged
          </p>
        </div>
        <div class="mt-3 flex flex-col gap-3">
          {#each Object.entries(schema.operation_sections).sort((a, b) => (a[1].order ?? 99) - (b[1].order ?? 99)) as [name, sec] (name)}
            {@const enabled = sectionEnabled(name)}
            <div class="rounded-md border p-2.5">
              <div class="flex flex-wrap items-center gap-2">
                <SyncSwitch {enabled} onSet={(on) => setSection(name, on)} />
                <span class="text-sm font-medium"
                  >{label(name, sec.schema)}</span
                >
                {#if sec.schema.hint}
                  <span class="text-xs text-muted-foreground"
                    >{sec.schema.hint}</span
                  >
                {/if}
              </div>
              {#if enabled}
                {#if sec.schema.kind === "object"}
                  <div class="mt-2 grid gap-3 md:grid-cols-2">
                    {#each Object.entries(sec.schema.fields).sort((a, b) => ((a[1] as { order?: number }).order ?? 99) - ((b[1] as { order?: number }).order ?? 99)) as [field, f] (field)}
                      {#if name === "video" && field === "container"}
                        <!-- hidden: the standalone Container section is the one control -->
                      {:else if name === "audio" && field === "rules"}
                        <div class="md:col-span-2">
                          <p class="mb-1 text-xs font-medium">
                            {label(field, f as { label?: string })}
                            {#if f.hint}<span
                                class="font-normal text-muted-foreground"
                              >
                                — {f.hint}</span
                              >{/if}
                          </p>
                          {#each rulesFor() as rule, i (i)}
                            <div class="mb-2 rounded border p-2">
                              <!-- Match: every axis is optional; all unset = every track.
									     Vocabularies render from the section's item schema. -->
                              <div
                                class="flex flex-wrap items-center gap-x-3 gap-y-2"
                              >
                                <span
                                  class="text-xs font-medium text-muted-foreground"
                                  >Match</span
                                >
                                <div class="flex flex-wrap gap-1">
                                  {#each ruleMatchCodecOpts as c (c.value)}
                                    <Button
                                      type="button"
                                      variant={matchList(
                                        rule,
                                        "codecs",
                                      ).includes(c.value)
                                        ? "default"
                                        : "outline"}
                                      size="sm"
                                      class="h-auto px-2 py-0.5 text-xs"
                                      onclick={() =>
                                        toggleRuleList(i, "codecs", c.value)}
                                    >
                                      {c.label}
                                    </Button>
                                  {/each}
                                </div>
                                <Input
                                  class="h-7 w-28"
                                  placeholder={(ruleMatchLangField?.label ??
                                    "Languages") + " (en, fr)"}
                                  title={ruleMatchLangField?.hint}
                                  value={(rule.match?.languages ?? []).join(
                                    ", ",
                                  )}
                                  oninput={(e) =>
                                    setRuleLanguages(
                                      i,
                                      (e.target as HTMLInputElement).value,
                                    )}
                                />
                                <div class="flex flex-wrap gap-1">
                                  {#each ruleMatchChannelOpts as c (c.value)}
                                    <Button
                                      type="button"
                                      variant={matchList(
                                        rule,
                                        "channels",
                                      ).includes(c.value)
                                        ? "default"
                                        : "outline"}
                                      size="sm"
                                      class="h-auto px-2 py-0.5 text-xs"
                                      onclick={() =>
                                        toggleRuleList(i, "channels", c.value)}
                                    >
                                      {c.label}
                                    </Button>
                                  {/each}
                                </div>
                                <div class="flex flex-wrap gap-1">
                                  {#each ruleMatchRateOpts as c (c.value)}
                                    <Button
                                      type="button"
                                      variant={matchList(
                                        rule,
                                        "sample_rate",
                                      ).includes(c.value)
                                        ? "default"
                                        : "outline"}
                                      size="sm"
                                      class="h-auto px-2 py-0.5 text-xs"
                                      onclick={() =>
                                        toggleRuleList(
                                          i,
                                          "sample_rate",
                                          c.value,
                                        )}
                                    >
                                      {c.label}
                                    </Button>
                                  {/each}
                                </div>
                                <Input
                                  class="h-7 w-28"
                                  placeholder={ruleMatchTitleField?.label ??
                                    "Title contains"}
                                  title={ruleMatchTitleField?.hint}
                                  value={(rule.match?.title_contains as
                                    string | undefined) ?? ""}
                                  oninput={(e) =>
                                    setRuleTitle(
                                      i,
                                      (e.target as HTMLInputElement).value,
                                    )}
                                />
                                <select
                                  class="h-7 w-28 rounded-lg border border-input bg-transparent px-2 text-xs"
                                  value={(rule.match?.default as
                                    string | undefined) ?? "any"}
                                  onchange={(e) =>
                                    setRuleDefault(
                                      i,
                                      (e.target as HTMLSelectElement).value,
                                    )}
                                >
                                  {#each ruleMatchDefaultOpts as v (v.value)}
                                    <option value={v.value}>{v.label}</option>
                                  {/each}
                                </select>
                              </div>
                              <div
                                class="mt-2 flex flex-wrap items-center gap-2 border-t pt-2"
                              >
                                <span
                                  class="text-xs font-medium text-muted-foreground"
                                  >Action</span
                                >
                                <select
                                  class="h-7 w-32 rounded-lg border border-input bg-transparent px-2 text-xs"
                                  value={typeof rule.action === "string"
                                    ? policySelectValue(rule.action)
                                    : ((rule.action as { codec?: string })
                                        .codec ??
                                      reencodeCodecOpts[0]?.value ??
                                      "")}
                                  onchange={(e) => {
                                    const v = (e.target as HTMLSelectElement)
                                      .value;
                                    if (policyOpts.some((o) => o.value === v))
                                      setRuleAction(
                                        i,
                                        v === "re_encode"
                                          ? reencodeActionDefault()
                                          : v,
                                      );
                                    else
                                      setRuleAction(i, {
                                        ...ruleActionObj(i),
                                        codec: v,
                                      });
                                  }}
                                >
                                  {#each policyOpts as v (v.value)}
                                    <option value={v.value}>{v.label}</option>
                                  {/each}
                                  {#if typeof rule.action !== "string"}
                                    {#each reencodeCodecOpts as v (v.value)}
                                      <option value={v.value}>{v.label}</option>
                                    {/each}
                                  {/if}
                                </select>
                                {#if typeof rule.action !== "string"}
                                  <div
                                    class="flex flex-wrap items-center gap-1"
                                  >
                                    <Input
                                      class="h-7 w-16"
                                      type="number"
                                      min="1"
                                      placeholder="auto"
                                      title="Sample rate (Hz); auto keeps the source rate"
                                      value={(
                                        rule.action as { sample_rate?: number }
                                      ).sample_rate ?? ""}
                                      oninput={(e) =>
                                        setRuleReencodeRate(
                                          i,
                                          (e.target as HTMLInputElement).value,
                                        )}
                                    />
                                    <span class="text-xs text-muted-foreground"
                                      >Hz</span
                                    >
                                    <Input
                                      class="h-7 w-16"
                                      placeholder="auto"
                                      type="number"
                                      min="1"
                                      title="Channel count; auto keeps the source count"
                                      value={(
                                        rule.action as { channels?: number }
                                      ).channels ?? ""}
                                      oninput={(e) =>
                                        setRuleReencodeChannels(
                                          i,
                                          (e.target as HTMLInputElement).value,
                                        )}
                                    />
                                    <span class="text-xs text-muted-foreground"
                                      >ch</span
                                    >
                                  </div>
                                {/if}
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  title="Remove rule"
                                  onclick={() => removeRule(i)}
                                >
                                  <TrashIcon class="size-3.5" />
                                </Button>
                              </div>
                            </div>
                          {/each}
                          <Button variant="outline" size="sm" onclick={addRule}>
                            <PlusIcon class="size-3.5" />
                            Add rule
                          </Button>
                        </div>
                      {:else}
                        <div>
                          <p class="mb-1 text-xs font-medium">
                            {label(field, f as { label?: string })}
                            {#if f.hint}<span
                                class="font-normal text-muted-foreground"
                              >
                                — {f.hint}</span
                              >{/if}
                          </p>
                          <FlowField
                            schema={f as UiSchema}
                            bind:value={op[name][field]}
                            {devices}
                          />
                        </div>
                      {/if}
                    {/each}
                  </div>
                {:else if name === "container"}
                  <div class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-2">
                    <select
                      class="h-8 w-40 rounded-lg border border-input bg-transparent px-2 text-sm"
                      value={containerSpec.choice}
                      onchange={(e) =>
                        setContainerSpec({
                          ...containerSpec,
                          choice: (e.target as HTMLSelectElement).value,
                        })}
                    >
                      {#each (sec.schema as { fields?: { choice?: { values?: { value: string; label: string }[] } } }).fields?.choice?.values ?? [] as v (v.value)}
                        <option value={v.value}>{v.label}</option>
                      {/each}
                    </select>
                    <label
                      class="flex cursor-pointer items-center gap-1.5 text-sm"
                    >
                      <input
                        type="checkbox"
                        class="size-4 accent-primary"
                        checked={containerSpec.fallback}
                        onchange={(e) =>
                          setContainerSpec({
                            ...containerSpec,
                            fallback: (e.target as HTMLInputElement).checked,
                          })}
                      />
                      Fall back to MKV
                    </label>
                  </div>
                {:else}
                  <FlowField
                    schema={sec.schema}
                    bind:value={op[name]}
                    {devices}
                    class="mt-2 w-64"
                  />
                {/if}
              {/if}
            </div>
          {/each}
        </div>
      </section>
    </div>
  {/if}
</div>

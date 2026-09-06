<script lang="ts">
	/**
	 * A sortable table header: label (default slot, via the legacy
	 * `children` prop) + direction indicator. Click cycles asc → desc.
	 * Legacy component so slot children work from rune-mode callers.
	 */
	import * as Table from "$lib/components/ui/table";
	import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
	import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";

	import type { Snippet } from "svelte";

	export let active: boolean = false;
	export let dir: "asc" | "desc" = "asc";
	export let onToggle: () => void = () => {};
	export let children: Snippet<[]> | undefined = undefined;
</script>

<Table.Head {...$$restProps}>
	<button type="button" class="inline-flex items-center gap-1 hover:opacity-75" onclick={onToggle}>
		{@render children?.()}
		{#if active}
			{#if dir === "asc"}
				<ArrowUpIcon class="size-3" />
			{:else}
				<ArrowDownIcon class="size-3" />
			{/if}
		{/if}
	</button>
</Table.Head>

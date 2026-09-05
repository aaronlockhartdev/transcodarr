<script lang="ts">
	import { Badge } from "$lib/components/ui/badge";
	import { cn } from "$lib/utils.js";

	let { status, class: className }: { status: string; class?: string } = $props();

	// File + job lifecycle states → badge treatment. Semantic tokens only
	// (no raw color values); destructive is reserved for real failure.
	const kind = $derived.by(() => {
		switch (status) {
			case "compliant":
			case "completed":
				return { variant: "secondary" as const, text: status };
			case "queued":
			case "unmatched":
				return { variant: "outline" as const, text: status };
			case "running":
			case "verifying":
			case "scanning":
				return { variant: "default" as const, text: status };
			case "failed":
			case "quarantined":
			case "canceled":
				return { variant: "destructive" as const, text: status };
			case "unscanned":
				return { variant: "secondary" as const, text: status };
			default:
				return { variant: "outline" as const, text: status };
		}
	});

	const pulsing = $derived(
		kind.text === "running" || kind.text === "verifying" || kind.text === "scanning",
	);
</script>

<Badge
	variant={kind.variant}
	class={cn(pulsing && "gap-1", className)}
>
	{#if pulsing}
		<span class="size-1.5 animate-pulse rounded-full bg-primary-foreground/80" aria-hidden="true"></span>
	{/if}
	{kind.text}
</Badge>

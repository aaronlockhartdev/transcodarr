<script lang="ts">
	/**
	 * A bits-ui Switch kept in sync with an external boolean state, fully
	 * controlled (no local mirror).
	 *
	 * bits-ui's Switch button renders its `checked`/`data-state`/aria
	 * attributes from the `checked` prop it is given, and its click/keyboard
	 * toggle invokes `onCheckedChange`. Mirroring the value in local state
	 * races with the parent update (the mirror snaps back before the prop
	 * arrives), so instead the prop is the single source of truth:
	 *  - click  -> onCheckedChange -> onSet -> parent state -> prop -> visual
	 *  - parent -> prop -> visual
	 */
	import { Switch } from "$lib/components/ui/switch";

	let {
		enabled = $bindable(false),
		onSet,
		class: className,
	}: {
		enabled: boolean;
		onSet: (v: boolean) => void;
		class?: string;
	} = $props();
</script>

<Switch checked={enabled} onCheckedChange={onSet} class={className} />

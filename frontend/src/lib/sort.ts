export type SortDir = "asc" | "desc";

/**
 * Stable-enough comparator for table headers: strings compare with
 * localeCompare, numbers arithmetically, dir flips the sign.
 */
export function sortBy<T>(items: T[], key: (t: T) => string | number, dir: SortDir): T[] {
	return [...items].sort((a, b) => {
		const ka = key(a);
		const kb = key(b);
		const c =
			typeof ka === "string" && typeof kb === "string"
				? ka.localeCompare(kb)
				: (ka as number) - (kb as number);
		return dir === "asc" ? c : -c;
	});
}

/** Click-cycle for a sortable header: none → asc → desc → asc … */
export function nextSortDir(current: SortDir | null): SortDir {
	return current === "asc" ? "desc" : "asc";
}

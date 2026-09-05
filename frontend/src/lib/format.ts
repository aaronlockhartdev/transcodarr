// Small presentation helpers. Pure functions, no dependencies.

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** 1234567 → "1.23 MB" */
export function formatBytes(bytes: number | null | undefined): string {
	if (bytes === null || bytes === undefined || Number.isNaN(bytes)) return "—";
	let n = bytes;
	let i = 0;
	while (n >= 1024 && i < BYTE_UNITS.length - 1) {
		n /= 1024;
		i += 1;
	}
	const digits = n >= 100 || i === 0 ? 0 : 1;
	return `${n.toFixed(digits)} ${BYTE_UNITS[i]}`;
}

/** Signed delta: -345000000 → "−330 MB" (space saved is positive here: input−output). */
export function formatDelta(deltaBytes: number): string {
	const sign = deltaBytes < 0 ? "+" : deltaBytes > 0 ? "−" : "±";
	return `${sign}${formatBytes(Math.abs(deltaBytes))}`;
}

/** Unix seconds → "2026-09-04 18:22" (local). */
export function formatUnixSeconds(ts: number | null | undefined): string {
	if (ts === null || ts === undefined) return "—";
	const d = new Date(ts * 1000);
	const p = (n: number) => String(n).padStart(2, "0");
	return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** "18:22" — time-of-day for compact table cells. */
export function formatTime(ts: number | null | undefined): string {
	if (ts === null || ts === undefined) return "—";
	const d = new Date(ts * 1000);
	const p = (n: number) => String(n).padStart(2, "0");
	return `${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Relative "3m ago" / "2h ago" / "5d ago". */
export function formatRelative(ts: number | null | undefined, now = Date.now()): string {
	if (ts === null || ts === undefined) return "—";
	const s = Math.max(0, Math.floor((now / 1000 - ts)));
	if (s < 5) return "just now";
	if (s < 60) return `${s}s ago`;
	if (s < 3600) return `${Math.floor(s / 60)}m ago`;
	if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
	return `${Math.floor(s / 86400)}d ago`;
}

/** "0:04:12" — human duration from seconds. */
export function formatDuration(secs: number): string {
	const s = Math.max(0, Math.round(secs));
	const p = (n: number) => String(n).padStart(2, "0");
	if (s < 3600) return `${p(Math.floor(s / 60))}:${p(s % 60)}`;
	return `${Math.floor(s / 3600)}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}

/** "/media/movies/a.mkv" → "a.mkv" */
export function basename(path: string): string {
	const i = path.lastIndexOf("/");
	return i === -1 ? path : path.slice(i + 1);
}

/** Parse a cached FileFacts JSON string, tolerating absent/broken values. */
export function parseFacts(
	factsJson: string | null,
): {
	container?: string;
	video?: { codec?: string; width?: number; height?: number; hdr?: string; pixel_format?: string };
	audio?: { codec: string; channels?: number; language?: string | null; atmos?: boolean }[];
} | null {
	if (!factsJson) return null;
	try {
		const f = JSON.parse(factsJson) as Record<string, unknown>;
		return f as never;
	} catch {
		return null;
	}
}

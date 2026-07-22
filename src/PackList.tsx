import { useMemo, useRef, useState } from "react";
import type { PackEntry } from "./types";
import { groupOf, GROUP_ORDER, showLunarBadge } from "./groups";
import { LocateIcon, MoonIcon } from "./icons";

const ROW_HEIGHT = 28;
const OVERSCAN = 6;

/// MC 1.8 lists packs in NTFS enumeration order: case-insensitive,
/// char-by-char — leading spaces sort first, which players use as priority.
export function mcMenuCompare(a: string, b: string): number {
  const A = a.toUpperCase();
  const B = b.toUpperCase();
  return A < B ? -1 : A > B ? 1 : 0;
}

export function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${bytes} B`;
}

export default function PackList({
  entries,
  emptyLabel,
  revealLabel,
  openLabel,
  lunarBadgeLabel,
  onReveal,
  onOpen,
}: {
  entries: PackEntry[];
  emptyLabel: string;
  revealLabel: string;
  openLabel: string;
  lunarBadgeLabel: string;
  onReveal: (name: string) => void;
  onOpen: (name: string) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);

  const sorted = useMemo(
    () =>
      [...entries].sort(
        (a, b) =>
          GROUP_ORDER.indexOf(groupOf(a)) - GROUP_ORDER.indexOf(groupOf(b)) ||
          mcMenuCompare(a.name, b.name),
      ),
    [entries],
  );

  const viewHeight = ref.current?.clientHeight || 560;
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const visible = Math.ceil(viewHeight / ROW_HEIGHT) + OVERSCAN * 2;
  const end = Math.min(sorted.length, start + visible);
  const slice = sorted.slice(start, end);

  return (
    <div
      className="pack-list"
      ref={ref}
      onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
    >
      <div style={{ height: start * ROW_HEIGHT }} />
      {sorted.length === 0 && <div className="empty placeholder">{emptyLabel}</div>}
      {slice.map((entry) => (
        <div className="pack-row" key={entry.name} style={{ height: ROW_HEIGHT }}>
          <span className={`dot cat-${groupOf(entry)}`} />
          <span
            className="name"
            title={`${entry.name}\n${openLabel}`}
            style={{ whiteSpace: "pre" }}
            onClick={() => onOpen(entry.name)}
          >
            {entry.name}
          </span>
          {showLunarBadge(entry) && (
            <span className="lunar-badge" title={lunarBadgeLabel} aria-label={lunarBadgeLabel}>
              <MoonIcon />
            </span>
          )}
          <span className="size">{formatSize(entry.size_bytes)}</span>
          <button
            className="row-btn"
            title={revealLabel}
            aria-label={revealLabel}
            onClick={() => onReveal(entry.name)}
          >
            <LocateIcon />
          </button>
        </div>
      ))}
      <div style={{ height: (sorted.length - end) * ROW_HEIGHT }} />
    </div>
  );
}

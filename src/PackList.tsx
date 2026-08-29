import { useEffect, useMemo, useRef, useState } from "react";
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

/// Depth by parent chain, not by raw path segment count — a shell wrapping a
/// nested zip is one logical row, not two.
function depthOf(entry: PackEntry, byPath: Map<string, PackEntry>): number {
  let depth = 0;
  let parent = entry.parent_path;
  while (parent) {
    depth += 1;
    const ancestor = byPath.get(parent);
    if (!ancestor) break;
    parent = ancestor.parent_path;
  }
  return depth;
}

export default function PackList({
  entries,
  emptyLabel,
  revealLabel,
  openLabel,
  lunarBadgeLabel,
  ignoreReasonLabel,
  expandLabel,
  collapseLabel,
  resetKey,
  expandSignal,
  mergedDescendantToTop,
  mergedLeafByTop,
  onReveal,
  onOpen,
}: {
  entries: PackEntry[];
  emptyLabel: string;
  revealLabel: string;
  openLabel: string;
  lunarBadgeLabel: string;
  ignoreReasonLabel: (entry: PackEntry) => string;
  expandLabel: (name: string) => string;
  collapseLabel: (name: string) => string;
  resetKey: object;
  expandSignal: { active: boolean; token: string };
  mergedDescendantToTop: Map<string, string>;
  mergedLeafByTop: Map<string, PackEntry>;
  onReveal: (name: string) => void;
  onOpen: (name: string) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const previousResetKey = useRef(resetKey);
  const previousSignal = useRef(expandSignal);
  const [scrollTop, setScrollTop] = useState(0);

  const topLevelFolders = useMemo(
    () =>
      entries
        .filter((entry) => entry.kind !== "pack" && entry.parent_path === null)
        .map((entry) => entry.relative_path),
    [entries],
  );

  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set(topLevelFolders),
  );

  // rescan → reset to top-level folders; if a filter/search is active, expand
  // every folder so the filtered tree is fully visible until the user collapses.
  useEffect(() => {
    if (previousResetKey.current === resetKey) return;
    previousResetKey.current = resetKey;
    setExpanded(new Set(topLevelFolders));
    setScrollTop(0);
  }, [resetKey, topLevelFolders]);

  // filter/search toggling on → expand every folder in the current view so
  // matches are visible; toggling off → leave state as-is (user-controlled).
  useEffect(() => {
    if (previousSignal.current.token === expandSignal.token) return;
    const wasActive = previousSignal.current.active;
    previousSignal.current = expandSignal;
    if (expandSignal.active && !wasActive) {
      setExpanded(
        new Set(
          entries
            .filter((entry) => entry.kind !== "pack")
            .map((entry) => entry.relative_path),
        ),
      );
    }
  }, [expandSignal, entries]);

  const { sorted, byPath } = useMemo(
    () => {
      const paths = new Set(entries.map((entry) => entry.relative_path));
      const map = new Map(entries.map((entry) => [entry.relative_path, entry]));
      const children = new Map<string | null, PackEntry[]>();
      for (const entry of entries) {
        const rawParent = entry.parent_path;
        const parent = rawParent && paths.has(rawParent) ? rawParent : null;
        const siblings = children.get(parent) ?? [];
        siblings.push(entry);
        children.set(parent, siblings);
      }
      // A descendant of a merged chain is skipped during rendering; the chain
      // top renders the collapsed row.
      const mergedDescendants = new Set<string>();
      for (const entry of entries) {
        if (mergedDescendantToTop.has(entry.relative_path)) {
          mergedDescendants.add(entry.relative_path);
        }
      }
      for (const siblings of children.values()) {
        siblings.sort((a, b) => {
          const aFolder = a.kind !== "pack";
          const bFolder = b.kind !== "pack";
          return (
            GROUP_ORDER.indexOf(groupOf(a)) - GROUP_ORDER.indexOf(groupOf(b)) ||
            (aFolder === bFolder ? mcMenuCompare(a.name, b.name) : aFolder ? -1 : 1)
          );
        });
      }
      const result: PackEntry[] = [];
      const mergedLeafByTopLocal = mergedLeafByTop;
      const visit = (parent: string | null) => {
        for (const entry of children.get(parent) ?? []) {
          // Skip chain descendants: the top renders the merged row.
          if (mergedDescendants.has(entry.relative_path)) continue;
          result.push(entry);
          const isMergedTop = mergedLeafByTopLocal.has(entry.relative_path);
          const expandable = entry.kind !== "pack" && !isMergedTop;
          if (expandable && expanded.has(entry.relative_path)) {
            visit(entry.relative_path);
          }
        }
      };
      visit(null);
      return { sorted: result, byPath: map };
    },
    [entries, expanded, mergedDescendantToTop, mergedLeafByTop],
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
      {slice.map((entry) => {
        const isMergedTop = mergedLeafByTop.has(entry.relative_path);
        const mergedLeaf: PackEntry = isMergedTop
          ? mergedLeafByTop.get(entry.relative_path)!
          : entry;
        const depth = depthOf(entry, byPath);
        return (
          <div
            className={`pack-row node-${entry.kind}`}
            key={entry.relative_path}
            style={{
              height: ROW_HEIGHT,
              paddingLeft: 10 + depth * 14,
            }}
          >
            {entry.kind !== "pack" && !isMergedTop && (
              <button
                className="tree-toggle"
                aria-label={
                  expanded.has(entry.relative_path)
                    ? collapseLabel(entry.name)
                    : expandLabel(entry.name)
                }
                onClick={() =>
                  setExpanded((current) => {
                    const next = new Set(current);
                    if (next.has(entry.relative_path)) next.delete(entry.relative_path);
                    else next.add(entry.relative_path);
                    return next;
                  })
                }
              >
                {expanded.has(entry.relative_path) ? "▾" : "▸"}
              </button>
            )}
            <span className={`dot cat-${isMergedTop ? "nested" : groupOf(entry)}`} />
            <span
              className="name"
              title={`${entry.name}\n${openLabel}`}
              style={{ whiteSpace: "pre" }}
              onClick={() => onOpen(entry.relative_path)}
            >
              {entry.name}
            </span>
            {showLunarBadge(mergedLeaf) && (
              <span className="lunar-badge" title={lunarBadgeLabel} aria-label={lunarBadgeLabel}>
                <MoonIcon />
              </span>
            )}
            {mergedLeaf.category === "ignored" && (
              <span className="ignore-reason" title={ignoreReasonLabel(mergedLeaf)}>
                {ignoreReasonLabel(mergedLeaf)}
              </span>
            )}
            <span className="size">{formatSize(entry.size_bytes)}</span>
            <button
              className="row-btn"
              title={revealLabel}
              aria-label={revealLabel}
              onClick={() => onReveal(entry.relative_path)}
            >
              <LocateIcon />
            </button>
          </div>
        );
      })}
      <div style={{ height: (sorted.length - end) * ROW_HEIGHT }} />
    </div>
  );
}

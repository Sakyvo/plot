import type { Category, PackEntry } from "./types";

/// Card / list display order: 非法 → 嵌套 → Lunar非法 → 文件夹 → 臃肿 → 正常 → 忽略.
export const GROUP_ORDER: Category[] = [
  "illegal",
  "nested",
  "lunar_illegal",
  "folder",
  "bloated",
  "normal",
  "ignored",
];

/** List-anchor group (primary category from the engine). */
export function groupOf(entry: PackEntry): Category {
  return entry.category;
}

export function hasLunarTag(entry: PackEntry): boolean {
  return (
    entry.category === "lunar_illegal" ||
    entry.causes.includes("lunar_escape")
  );
}

/** Card filter: pack belongs to the group (multi-tag inclusive). */
export function hasGroupTag(entry: PackEntry, filter: Category): boolean {
  if (filter === "lunar_illegal") {
    return hasLunarTag(entry);
  }
  return entry.category === filter;
}

/** Stacked Lunar: structure primary + lunar cause — show moon badge. */
export function showLunarBadge(entry: PackEntry): boolean {
  return entry.category !== "lunar_illegal" && entry.causes.includes("lunar_escape");
}

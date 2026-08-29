import { useCallback, useEffect, useRef, useState } from "react";
import {
  appVersion,
  browseFolder,
  checkForUpdate,
  checkLocks,
  defaultDir,
  GITHUB_REPO_URL,
  getSettings,
  onProcessProgress,
  onScanProgress,
  openPlotTemp,
  openPack,
  openUrl,
  processPacks,
  revealPack,
  saveSettings,
  scanDefault,
  scanPath,
} from "./ipc";
import { detectLang, ignoreReasonText, isLang, t, LANGS, type Lang } from "./i18n";
import {
  FolderIcon,
  GearIcon,
  GlobeIcon,
  MoonIcon,
  RefreshIcon,
  UpdateCheckIcon,
} from "./icons";
import { GROUP_ORDER, hasGroupTag, hasLunarTag, showLunarBadge } from "./groups";
import PackList, { mcMenuCompare } from "./PackList";
import githubIcon from "./assets/github.ico";
import type {
  Category,
  PackEntry,
  ProcessReport,
  ProcessNotice,
  ProgressEvent,
  ScanReport,
  Settings,
  UpdateInfo,
} from "./types";

/** Manual check only — startup never sets these. */
type ManualNotice =
  | { kind: "latest"; current: string }
  | { kind: "failed" };

type ResultIgnored = PackEntry & { sourceParent?: string };

/** A single-pack shell chain: chain[0] is the outermost shell, last is the
 * leaf pack. Collapsed to one orange nested row in the list (display only). */
type MergeChain = { top: PackEntry; leaf: PackEntry };

const MERGEABLE_LEAF_CATEGORY: ReadonlySet<Category> = new Set([
  "normal",
  "nested",
  "bloated",
  "lunar_illegal",
]);

/** Build display merge chains: a shell line whose every level has exactly one
 * child, ending in a mergeable pack leaf, collapses to the top shell row.
 * Ignored / illegal leaves, attachments, classification folders, or any branch
 * keep the tree. Pure display; engine counts and processing are untouched. */
function mergeChainsOf(entries: PackEntry[]): {
  chains: MergeChain[];
  descendantToTop: Map<string, string>;
} {
  const childrenOf = new Map<string, PackEntry[]>();
  for (const entry of entries) {
    if (!entry.parent_path) continue;
    const siblings = childrenOf.get(entry.parent_path) ?? [];
    siblings.push(entry);
    childrenOf.set(entry.parent_path, siblings);
  }
  const byPath = new Map(entries.map((entry) => [entry.relative_path, entry]));

  const chains: MergeChain[] = [];
  const descendantToTop = new Map<string, string>();

  for (const entry of entries) {
    if (entry.kind !== "shell") continue;
    // Walk down a shell line: every level has exactly one child, itself a shell.
    const line: PackEntry[] = [entry];
    let current: PackEntry = entry;
    while (current.kind === "shell") {
      const kids = childrenOf.get(current.relative_path) ?? [];
      if (kids.length !== 1) break;
      const only = kids[0];
      if (only.kind !== "shell") {
        line.push(only);
        current = only;
        break;
      }
      line.push(only);
      current = only;
    }
    const leaf = line[line.length - 1];
    if (!leaf || leaf.kind !== "pack") continue;
    if (!MERGEABLE_LEAF_CATEGORY.has(leaf.category)) continue;
    // The top must not itself be the inner level of another merge chain.
    const top = line[0];
    let parent = top.parent_path;
    let nested = false;
    while (parent) {
      const ancestor = byPath.get(parent);
      if (!ancestor) break;
      if (ancestor.kind === "shell") {
        const ancestorKids = childrenOf.get(ancestor.relative_path) ?? [];
        if (ancestorKids.length === 1 && ancestorKids[0].kind === "shell") {
          nested = true;
          break;
        }
      }
      parent = ancestor.parent_path;
    }
    if (nested) continue;
    chains.push({ top, leaf });
    for (let i = 1; i < line.length; i++) {
      descendantToTop.set(line[i].relative_path, top.relative_path);
    }
  }
  return { chains, descendantToTop };
}

/** Confirm dialog: same severity order as main page, without normal. */
const CONFIRM_GROUP_ORDER: Category[] = GROUP_ORDER.filter((c) => c !== "normal");

function confirmGroupOf(e: PackEntry): Category {
  return e.category === "normal" && hasLunarTag(e) ? "lunar_illegal" : e.category;
}

/** Everything the process run will touch. */
function problemsOf(report: ScanReport): PackEntry[] {
  const byPath = new Map(report.entries.map((entry) => [entry.relative_path, entry]));
  const hasShellAncestor = (entry: PackEntry) => {
    let parent = entry.parent_path;
    while (parent) {
      const ancestor = byPath.get(parent);
      if (!ancestor) break;
      if (ancestor.kind === "shell") return true;
      parent = ancestor.parent_path;
    }
    return false;
  };
  return report.entries.filter(
    (entry) =>
      (entry.kind === "shell" && !hasShellAncestor(entry)) ||
      (entry.kind === "pack" &&
        !hasShellAncestor(entry) &&
        entry.category !== "ignored" &&
        (entry.category !== "normal" || hasLunarTag(entry))),
  );
}

function lockCandidatesOf(report: ScanReport, problems: PackEntry[]): string[] {
  const paths = new Set<string>();
  for (const entry of problems) {
    if (entry.kind === "shell") {
      if (entry.causes.includes("archive_shell")) {
        paths.add(entry.relative_path);
        continue;
      }
      const prefix = `${entry.relative_path}/`;
      for (const child of report.entries) {
        if (child.kind === "pack" && child.relative_path.startsWith(prefix)) {
          paths.add(child.relative_path);
        }
      }
    } else {
      paths.add(entry.relative_path);
    }
  }
  return [...paths];
}

function confirmEntryLabel(report: ScanReport, entry: PackEntry): string {
  if (entry.kind !== "shell") return entry.relative_path;
  const prefix = `${entry.relative_path}/`;
  const children = report.entries
    .filter(
      (candidate) =>
        candidate.kind === "pack" && candidate.relative_path.startsWith(prefix),
    )
    .map((candidate) => candidate.relative_path);
  return children.length ? `${entry.relative_path} → ${children.join(", ")}` : entry.relative_path;
}

function visibleEntries(
  entries: PackEntry[],
  filter: Category | null,
  query: string,
): PackEntry[] {
  if (!filter && !query) return entries;

  const byPath = new Map(entries.map((entry) => [entry.relative_path, entry]));
  const children = new Map<string, PackEntry[]>();
  for (const entry of entries) {
    if (!entry.parent_path) continue;
    const siblings = children.get(entry.parent_path) ?? [];
    siblings.push(entry);
    children.set(entry.parent_path, siblings);
  }
  const addSubtree = (entry: PackEntry, set: Set<string>) => {
    set.add(entry.relative_path);
    for (const child of children.get(entry.relative_path) ?? []) addSubtree(child, set);
  };

  const filterPaths = new Set<string>();
  if (!filter) {
    for (const entry of entries) filterPaths.add(entry.relative_path);
  } else if (filter === "folder") {
    for (const entry of entries) {
      if (entry.kind === "classification_folder") addSubtree(entry, filterPaths);
    }
  } else {
    for (const entry of entries) {
      if (!hasGroupTag(entry, filter)) continue;
      if (filter === "nested" && entry.kind === "shell") addSubtree(entry, filterPaths);
      else filterPaths.add(entry.relative_path);
    }
  }

  const queryPaths = new Set<string>();
  if (!query) {
    for (const entry of entries) queryPaths.add(entry.relative_path);
  } else {
    for (const entry of entries) {
      if (!entry.name.toLowerCase().includes(query)) continue;
      if (entry.kind === "pack") queryPaths.add(entry.relative_path);
      else addSubtree(entry, queryPaths);
    }
  }

  const shown = new Set(
    [...filterPaths].filter((relativePath) => queryPaths.has(relativePath)),
  );
  for (const relativePath of [...shown]) {
    let parent = byPath.get(relativePath)?.parent_path ?? null;
    while (parent) {
      shown.add(parent);
      parent = byPath.get(parent)?.parent_path ?? null;
    }
  }
  return entries.filter((entry) => shown.has(entry.relative_path));
}

const GROUP_LABELS: Record<Category, string> = {
  illegal: "catIllegal",
  nested: "catNested",
  lunar_illegal: "catLunarIllegal",
  folder: "catFolder",
  bloated: "catBloated",
  normal: "catNormal",
  ignored: "catIgnored",
};

const ACTION_GROUPS: { action: string[]; labelKey: string }[] = [
  { action: ["converted"], labelKey: "groupConverted" },
  { action: ["moved_to_illegal"], labelKey: "groupIllegal" },
  {
    action: ["failed", "skipped_locked", "skipped_unsupported"],
    labelKey: "groupSkipped",
  },
];

function resultText(result: ProcessReport, ignored: ResultIgnored[], lang: Lang): string {
  const outcomes = result.outcomes
    .map((o) => {
      const products = o.products.length ? ` -> ${o.products.join(", ")}` : "";
      const detail = o.detail ? ` [${o.detail}]` : "";
      return `${o.action}: ${o.original_name}${products}${detail}`;
    })
    .join("\n");
  const ignoredText = ignored
    .map((entry) => {
      const source = entry.sourceParent
        ? `（${t(lang, "separatedFrom", { parent: entry.sourceParent })}）`
        : "";
      return `${entry.relative_path}: ${ignoreReasonText(lang, entry.ignore)}${source}`;
    })
    .join("\n");
  const notices = (result.notices ?? []).map((notice) => processNoticeText(lang, notice)).join("\n");
  return [outcomes, notices, ignoredText].filter(Boolean).join("\n");
}

function processNoticeText(lang: Lang, notice: ProcessNotice): string {
  if (notice.key === "attachments_kept_in_original_archive") {
    const source = notice.values.join(lang === "en" ? ", " : "、");
    const separator = lang === "en" ? ": " : "：";
    return `${source}${separator}${t(lang, "attachmentsKeptInOriginalArchive")}`;
  }
  return notice.key;
}

function uniqueIgnored(entries: ResultIgnored[]): ResultIgnored[] {
  const seen = new Set<string>();
  return entries.filter((entry) => {
    const key = entry.relative_path.toLowerCase();
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export default function App() {
  const [lang, setLang] = useState<Lang>("zh-CN");
  const [settings, setSettings] = useState<Settings>({});
  const [report, setReport] = useState<ScanReport | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ProcessReport | null>(null);
  const [resultIgnored, setResultIgnored] = useState<ResultIgnored[]>([]);
  const [resultRescanError, setResultRescanError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Category | null>(null);
  const [scanProgress, setScanProgress] = useState<ProgressEvent | null>(null);
  const [query, setQuery] = useState("");
  const [pathInput, setPathInput] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [locked, setLocked] = useState<string[]>([]);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [manualNotice, setManualNotice] = useState<ManualNotice | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // Settings loaded → true; distinguishes the landing page from the scan screen.
  const [booted, setBooted] = useState(false);
  // A scan was kicked from the landing page — show the scan screen meanwhile.
  const [scanKicked, setScanKicked] = useState(false);
  // Landing-page path-box prefill: stored custom path, else the default dir.
  const [homePath, setHomePath] = useState("");
  const reportRef = useRef<ScanReport | null>(null);
  reportRef.current = report;
  // Captured once at boot: mid-session toggle flips only the next launch.
  const autoScanAtBoot = useRef(false);

  useEffect(() => {
    (async () => {
      const stored = await getSettings().catch(() => ({}) as Settings);
      setSettings(stored);
      setLang(
        isLang(stored.language) ? stored.language : detectLang(navigator.language),
      );
      autoScanAtBoot.current = stored.auto_scan_on_start === true;
      setBooted(true);
      if (autoScanAtBoot.current) {
        const first = stored.custom_path
          ? await scanPath(stored.custom_path)
          : await scanDefault();
        setReport(first);
        setScanProgress(null);
      } else if (!stored.custom_path) {
        setHomePath(await defaultDir().catch(() => ""));
      } else {
        setHomePath(stored.custom_path);
      }
    })();
    // Fire-and-forget: never block UI; timeout / error / no update → no dialog.
    checkForUpdate()
      .then((info) => {
        if (info) setUpdate(info);
      })
      .catch(() => {});
    const cleanups: (() => void)[] = [];
    onProcessProgress(setProgress).then((un) => cleanups.push(un));
    onScanProgress(setScanProgress).then((un) => cleanups.push(un));
    // Replace the WebView2 context menu with our own rescan-only menu.
    const onCtx = (e: MouseEvent) => {
      e.preventDefault();
      // Rescan-only menu needs a report to rescan.
      if (!reportRef.current) return;
      setMenu({ x: e.clientX, y: e.clientY });
    };
    document.addEventListener("contextmenu", onCtx);
    cleanups.push(() => document.removeEventListener("contextmenu", onCtx));
    return () => cleanups.forEach((un) => un());
  }, []);

  const runManualCheck = useCallback(async () => {
    if (checkingUpdate) return;
    setCheckingUpdate(true);
    setManualNotice(null);
    try {
      const info = await checkForUpdate();
      if (info) {
        setUpdate(info);
      } else {
        const current = await appVersion().catch(() => "?");
        setManualNotice({ kind: "latest", current });
      }
    } catch {
      setManualNotice({ kind: "failed" });
    } finally {
      setCheckingUpdate(false);
    }
  }, [checkingUpdate]);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    document.addEventListener("click", close);
    return () => document.removeEventListener("click", close);
  }, [menu]);

  const persist = useCallback((next: Settings) => {
    setSettings(next);
    saveSettings(next).catch(() => {});
  }, []);

  const switchLang = useCallback(
    (value: string) => {
      if (isLang(value)) {
        setLang(value);
        persist({ ...settings, language: value });
      }
    },
    [settings, persist],
  );

  const rescan = useCallback(() => {
    if (report) {
      scanPath(report.path).then((r) => {
        setReport(r);
        setScanProgress(null);
      });
    }
  }, [report]);

  const scanInto = useCallback(
    async (dir: string) => {
      setReport(await scanPath(dir));
      setScanProgress(null);
      setFilter(null);
      persist({ ...settings, custom_path: dir });
    },
    [settings, persist],
  );

  const browse = useCallback(async () => {
    const picked = await browseFolder();
    if (picked) await scanInto(picked);
  }, [scanInto]);

  const commitPath = useCallback(() => {
    if (pathInput === null) return;
    const next = pathInput.trim();
    setPathInput(null);
    if (!report) {
      // Landing page: committing a path is the user's start signal.
      if (next && next !== homePath) scanInto(next);
      return;
    }
    if (next && next !== report.path) scanInto(next);
  }, [pathInput, report, homePath, scanInto]);

  /** Landing-page primary action: scan the remembered/default directory. */
  const startScan = useCallback(() => {
    setScanKicked(true);
    if (settings.custom_path) {
      // Remembered path wins; scan as-is without touching settings.
      scanPath(settings.custom_path).then((r) => {
        setReport(r);
        setScanProgress(null);
      });
    } else {
      scanDefault().then((r) => {
        setReport(r);
        setScanProgress(null);
      });
    }
  }, [settings]);

  /** Re-probe locks only — never rescans the directory. */
  const recheck = useCallback(() => {
    if (!report) return;
    const problems = problemsOf(report);
    Promise.resolve(checkLocks(report.path, lockCandidatesOf(report, problems)))
      .then((l) => setLocked(l ?? []))
      .catch(() => {});
  }, [report]);

  const openConfirm = useCallback(() => {
    if (!report) return;
    setLocked([]);
    setConfirming(true);
    // Probe is ms-fast; fill in lock marks as soon as they arrive.
    recheck();
  }, [report, recheck]);

  const runProcess = useCallback(async () => {
    if (!report) return;
    setConfirming(false);
    setBusy(true);
    setError(null);
    const ignored = uniqueIgnored(report.entries.filter((entry) => entry.category === "ignored"));
    setResultRescanError(null);
    try {
      const outcome = await processPacks(report.path);
      // Keep completed work visible even when the post-process scan fails.
      setResultIgnored(ignored);
      setResult(outcome);
      try {
        const rescanned = await scanPath(report.path);
        const separated = new Map<string, string>();
        for (const item of outcome.outcomes.flatMap((item) => item.separated ?? [])) {
          separated.set(item.name.toLowerCase(), item.parent);
        }
        const newlyIgnored: ResultIgnored[] = rescanned.entries
          .filter(
            (entry) =>
              entry.category === "ignored" &&
              separated.has(entry.relative_path.toLowerCase()),
          )
          .map((entry) => ({
            ...entry,
            sourceParent: separated.get(entry.relative_path.toLowerCase()),
          }));
        setResultIgnored(uniqueIgnored([...ignored, ...newlyIgnored]));
        setReport(rescanned);
      } catch (e) {
        setResultRescanError(String(e));
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }, [report]);

  const shownPath = pathInput ?? report?.path ?? homePath;

  const toolbar = (
    <div className="toolbar">
      <span className="scan-path">
        <button
          className="icon-btn"
          title={t(lang, "browse")}
          aria-label={t(lang, "browse")}
          onClick={browse}
        >
          <FolderIcon />
        </button>
        <span className="path-label">{t(lang, "scanDirLabel")}</span>
        <input
          className="path-box"
          type="text"
          aria-label={t(lang, "scanDirLabel")}
          title={shownPath}
          value={shownPath}
          onChange={(e) => setPathInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && commitPath()}
          onBlur={commitPath}
        />
      </span>
      <span className="search-area">
        <span className="search-box">
          <input
            type="text"
            aria-label={t(lang, "searchPlaceholder")}
            placeholder={t(lang, "searchPlaceholder")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          {query && (
            <button
              className="clear-btn"
              title={t(lang, "clearSearch")}
              aria-label={t(lang, "clearSearch")}
              onClick={() => setQuery("")}
            >
              ×
            </button>
          )}
        </span>
        <button
          className="icon-btn"
          title={t(lang, "rescan")}
          aria-label={t(lang, "rescan")}
          disabled={!report}
          onClick={rescan}
        >
          <RefreshIcon />
        </button>
        <button
          className="icon-btn"
          title={t(lang, "settings")}
          aria-label={t(lang, "settings")}
          onClick={() => setSettingsOpen(true)}
        >
          <GearIcon />
        </button>
      </span>
    </div>
  );

  const settingsDialog = settingsOpen ? (
    <div className="modal-backdrop">
      <div className="modal" role="dialog" aria-label={t(lang, "settings")}>
        <h2>{t(lang, "settings")}</h2>
        <div className="modal-body">
          <div className="settings-row">
            <span className="settings-label">
              <GlobeIcon />
              {t(lang, "language")}
            </span>
            <select
              className="settings-select"
              aria-label={t(lang, "language")}
              value={lang}
              onChange={(e) => switchLang(e.target.value)}
            >
              {LANGS.map(({ value, label }) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </div>
          <div className="settings-row">
            <span className="settings-label">{t(lang, "autoScanOnStart")}</span>
            <input
              type="checkbox"
              aria-label={t(lang, "autoScanOnStart")}
              checked={settings.auto_scan_on_start === true}
              onChange={(e) =>
                persist({ ...settings, auto_scan_on_start: e.target.checked })
              }
            />
          </div>
        </div>
        <div className="modal-actions">
          <button className="primary" onClick={() => setSettingsOpen(false)}>
            {t(lang, "close")}
          </button>
        </div>
      </div>
    </div>
  ) : null;

  if (!report) {
    if (!booted) {
      // Mirrors the index.html static skeleton — seamless first paint handoff.
      return (
        <div className="app">
          <div className="boot-skeleton" data-testid="boot-skeleton">
            <div className="skel-toolbar">
              <span className="skel-block skel-icon" />
              <span className="skel-block skel-path" />
              <span className="skel-block skel-search" />
              <span className="skel-block skel-icon" />
              <span className="skel-block skel-icon" />
            </div>
            <div className="skel-landing">
              <span className="skel-block skel-line" />
              <span className="skel-block skel-btn" />
            </div>
          </div>
        </div>
      );
    }
    if (!scanKicked && !autoScanAtBoot.current) {
      // Landing page: auto-scan disabled — the user starts the first scan.
      return (
        <div className="app">
          {toolbar}
          <div className="landing">
            <p className="placeholder">{t(lang, "landingHint")}</p>
            <button className="primary" onClick={startScan}>
              {t(lang, "startScan")}
            </button>
          </div>
          {settingsDialog}
        </div>
      );
    }
    return (
      <div className="app loading">
        {scanProgress ? (
          <>
            <progress value={scanProgress.index} max={scanProgress.total} />
            <p className="placeholder">
              {t(lang, "scanProgress", {
                index: scanProgress.index,
                total: scanProgress.total,
              })}
            </p>
          </>
        ) : (
          <p className="placeholder">{t(lang, "scanning")}</p>
        )}
      </div>
    );
  }

  const { chains, descendantToTop } = mergeChainsOf(report.entries);
  const mergedLeafByTop = new Map<string, PackEntry>();
  for (const { top, leaf } of chains) mergedLeafByTop.set(top.relative_path, leaf);

  const problems = problemsOf(report);
  const confirmEntries = problems
    .slice()
    .sort(
      (a, b) =>
        CONFIRM_GROUP_ORDER.indexOf(confirmGroupOf(a)) -
          CONFIRM_GROUP_ORDER.indexOf(confirmGroupOf(b)) ||
        mcMenuCompare(a.name, b.name),
    );
  const lockedSet = new Set(locked);
  const isLocked = (entry: PackEntry) => {
    if (entry.kind !== "shell") return lockedSet.has(entry.relative_path);
    const prefix = `${entry.relative_path}/`;
    return locked.some((path) => path.startsWith(prefix));
  };
  const occupied = confirmEntries.filter(isLocked);
  const confirmByGroup = CONFIRM_GROUP_ORDER.map((key) => ({
    key,
    entries: confirmEntries.filter(
      (e) => confirmGroupOf(e) === key && !isLocked(e),
    ),
  })).filter((g) => g.entries.length > 0);
  const lowerQuery = query.toLowerCase();
  const filterActive = Boolean(filter || lowerQuery);
  const filterToken = `${filter ?? ""}|${lowerQuery}`;
  const shown = visibleEntries(report.entries, filter, lowerQuery);

  return (
    <div className="app">
      {toolbar}
      {report.status === "missing_dir" && (
        <p className="alert">{t(lang, "missingDir")}</p>
      )}
      {report.status === "no_packs" && <p className="alert">{t(lang, "noPacks")}</p>}
      {report.status === "ok" && (
        <>
          <div className="overview" role="group" aria-label={t(lang, "overview")}>
            {GROUP_ORDER.map((key) => (
              <button
                key={key}
                className={`card cat-card-${key} ${filter === key ? "active" : ""}`}
                onClick={() => setFilter(filter === key ? null : key)}
              >
                <span className="card-label">{t(lang, GROUP_LABELS[key])}</span>
                <span className="card-count">
                  {key === "lunar_illegal" ? report.counts.lunar : report.counts[key]}
                </span>
              </button>
            ))}
          </div>
          <PackList
            entries={shown}
            emptyLabel={t(lang, "emptyList")}
            revealLabel={t(lang, "reveal")}
            openLabel={t(lang, "clickToOpen")}
            lunarBadgeLabel={t(lang, "lunarBadge")}
            ignoreReasonLabel={(entry) => ignoreReasonText(lang, entry.ignore)}
            expandLabel={(name) => t(lang, "expandFolder", { name })}
            collapseLabel={(name) => t(lang, "collapseFolder", { name })}
            resetKey={report}
            expandSignal={{ active: filterActive, token: filterToken }}
            mergedDescendantToTop={descendantToTop}
            mergedLeafByTop={mergedLeafByTop}
            onReveal={(relativePath) => revealPack(report.path, relativePath)}
            onOpen={(relativePath) => openPack(report.path, relativePath)}
          />
          <div className="actions">
            <button
              type="button"
              className="icon-btn github-btn"
              title={t(lang, "githubOpen")}
              aria-label={t(lang, "githubOpen")}
              onClick={() => openUrl(GITHUB_REPO_URL)}
            >
              <img src={githubIcon} alt="" />
            </button>
            <button
              type="button"
              className="icon-btn"
              title={t(lang, "checkUpdate")}
              aria-label={t(lang, "checkUpdate")}
              aria-busy={checkingUpdate}
              disabled={checkingUpdate}
              onClick={runManualCheck}
            >
              <UpdateCheckIcon />
            </button>
            <span className="actions-spacer" />
            {/* Fixed English per user request — not i18n */}
            <span className="files-total">
              {report.total_packs} files in total
            </span>
            {scanProgress && !busy && (
              <span className="placeholder">
                {t(lang, "scanProgress", {
                  index: scanProgress.index,
                  total: scanProgress.total,
                })}
              </span>
            )}
            {error && <span className="alert">{t(lang, "processError", { msg: error })}</span>}
            <button
              className="primary"
              disabled={problems.length === 0 || busy}
              onClick={openConfirm}
            >
              {t(lang, "processBtn")}
            </button>
          </div>
        </>
      )}

      {(report.status === "missing_dir" || report.status === "no_packs") && (
        <div className="actions">
          <button
            type="button"
            className="icon-btn github-btn"
            title={t(lang, "githubOpen")}
            aria-label={t(lang, "githubOpen")}
            onClick={() => openUrl(GITHUB_REPO_URL)}
          >
            <img src={githubIcon} alt="" />
          </button>
          <button
            type="button"
            className="icon-btn"
            title={t(lang, "checkUpdate")}
            aria-label={t(lang, "checkUpdate")}
            aria-busy={checkingUpdate}
            disabled={checkingUpdate}
            onClick={runManualCheck}
          >
            <UpdateCheckIcon />
          </button>
          <span className="actions-spacer" />
          <span className="files-total">
            {report.total_packs} files in total
          </span>
        </div>
      )}

      {busy && (
        <div className="modal-backdrop">
          <div
            className="modal processing-modal"
            role="dialog"
            aria-label={t(lang, "processingTitle")}
          >
            <h2>{t(lang, "processingTitle")}</h2>
            <progress
              value={progress ? progress.index + 1 : undefined}
              max={progress ? progress.total : undefined}
            />
            {progress && (
              <p className="placeholder">
                {t(lang, "processing", {
                  name: progress.name,
                  index: progress.index + 1,
                  total: progress.total,
                })}
              </p>
            )}
          </div>
        </div>
      )}

      {confirming && (
        <div className="modal-backdrop">
          <div className="modal wide" role="dialog" aria-label={t(lang, "confirmTitle")}>
            <h2>{t(lang, "confirmTitle")}</h2>
            <div className="modal-body">
              {occupied.length > 0 && (
                <section className="confirm-group">
                  <h3 className="confirm-group-title occupied-title">
                    {t(lang, "occupiedTitle")}
                  </h3>
                  <ul className="confirm-list">
                    {occupied.map((e) => (
                      <li key={e.relative_path} className="confirm-row occupied-row">
                        <span className={`dot cat-${confirmGroupOf(e)}`} />
                        <span className="confirm-name">{confirmEntryLabel(report, e)}</span>
                        {showLunarBadge(e) && (
                          <span
                            className="lunar-badge"
                            title={t(lang, "lunarBadge")}
                            aria-label={t(lang, "lunarBadge")}
                          >
                            <MoonIcon />
                          </span>
                        )}
                      </li>
                    ))}
                  </ul>
                  <div className="occupied-actions">
                    <p className="confirm-hint occupied-hint">
                      {t(lang, "occupiedHint", { n: occupied.length })}
                    </p>
                    <button className="recheck-btn" onClick={recheck}>
                      {t(lang, "recheckLocks")}
                    </button>
                  </div>
                </section>
              )}
              {confirmByGroup.map(({ key, entries }) => (
                <section key={key} className="confirm-group">
                  <h3 className={`confirm-group-title cat-text-${key}`}>
                    {t(lang, GROUP_LABELS[key])}
                  </h3>
                  <ul className="confirm-list">
                    {entries.map((e) => (
                      <li key={e.relative_path} className="confirm-row">
                        <span className={`dot cat-${key}`} />
                        <span className="confirm-name">{confirmEntryLabel(report, e)}</span>
                      </li>
                    ))}
                  </ul>
                  {key === "illegal" && (
                    <p className="confirm-hint placeholder">
                      {t(lang, "illegalManualHint")}
                    </p>
                  )}
                </section>
              ))}
            </div>
            <div className="modal-actions">
              <button onClick={() => setConfirming(false)}>{t(lang, "cancel")}</button>
              <button className="primary" onClick={runProcess}>
                {t(lang, "confirm")}
              </button>
            </div>
          </div>
        </div>
      )}

      {result && (
        <div className="modal-backdrop">
          <div className="modal wide" role="dialog" aria-label={t(lang, "resultTitle")}>
            <h2>{t(lang, "resultTitle")}</h2>
            {resultRescanError && (
              <p className="alert">{t(lang, "resultRescanFailed", { msg: resultRescanError })}</p>
            )}
            <div className="modal-body scroll-both">
              {ACTION_GROUPS.map(({ action, labelKey }) => {
                const items = result.outcomes.filter((o) => action.includes(o.action));
                if (items.length === 0) return null;
                return (
                  <section key={labelKey}>
                    <h3>{t(lang, labelKey)}</h3>
                    <ul>
                      {items.map((o) => (
                        <li key={o.original_name}>
                          {o.original_name}
                          {o.products.length > 0 && ` → ${o.products.join(", ")}`}
                          {o.detail && <span className="placeholder"> ({o.detail})</span>}
                          {(o.causes.includes("rar_archive") ||
                            o.causes.includes("sevenz_archive")) && (
                            <span className="hint"> — {t(lang, "rarHint")}</span>
                          )}
                        </li>
                      ))}
                    </ul>
                    {labelKey === "groupIllegal" && (
                      <p className="confirm-hint placeholder">
                        {t(lang, "illegalManualHint")}
                      </p>
                    )}
                  </section>
                );
              })}
              {resultIgnored.length > 0 && (
                <section className="result-ignored">
                  <h3>{t(lang, "groupIgnored")}</h3>
                  <ul>
                    {resultIgnored.map((entry) => (
                      <li key={entry.relative_path}>
                        {entry.relative_path}: {ignoreReasonText(lang, entry.ignore)}
                        {entry.sourceParent &&
                          `（${t(lang, "separatedFrom", { parent: entry.sourceParent })}）`}
                      </li>
                    ))}
                  </ul>
                </section>
              )}
              {(result.notices ?? []).length > 0 && (
                <section className="result-notices">
                  <ul>
                    {(result.notices ?? []).map((notice, index) => (
                      <li key={`${notice.key}-${index}`}>
                        {processNoticeText(lang, notice)}
                      </li>
                    ))}
                  </ul>
                </section>
              )}
            </div>
            <div className="modal-actions">
              <button
                onClick={() => navigator.clipboard.writeText(resultText(result, resultIgnored, lang))}
              >
                {t(lang, "copyResult")}
              </button>
              <button onClick={() => openPlotTemp(result.run_dir ?? undefined)}>
                {t(lang, "openBatchOutput")}
              </button>
              <button
                className="primary"
                onClick={() => {
                  setResult(null);
                  setResultRescanError(null);
                }}
              >
                {t(lang, "close")}
              </button>
            </div>
          </div>
        </div>
      )}

      {update && (
        <div className="modal-backdrop">
          <div
            className="modal wide"
            role="dialog"
            aria-label={t(lang, "updateTitle", { version: update.version })}
          >
            <h2>{t(lang, "updateTitle", { version: update.version })}</h2>
            <p className="update-meta">
              {t(lang, "updateCurrent", { version: update.current_version })}
            </p>
            <div className="modal-body scroll-both">
              <p className="update-notes-label">{t(lang, "updateNotes")}</p>
              <pre className="update-notes">
                {update.body.trim() ? update.body : t(lang, "updateNoNotes")}
              </pre>
            </div>
            <div className="modal-actions">
              <button onClick={() => setUpdate(null)}>{t(lang, "updateLater")}</button>
              <button
                className="primary"
                onClick={() => openUrl(update.download_url)}
              >
                {t(lang, "updateDownload")}
              </button>
            </div>
          </div>
        </div>
      )}

      {manualNotice && (
        <div className="modal-backdrop">
          <div
            className="modal"
            role="dialog"
            aria-label={
              manualNotice.kind === "latest"
                ? t(lang, "updateLatestTitle")
                : t(lang, "updateFailedTitle")
            }
          >
            <h2>
              {manualNotice.kind === "latest"
                ? t(lang, "updateLatestTitle")
                : t(lang, "updateFailedTitle")}
            </h2>
            <p className="update-meta">
              {manualNotice.kind === "latest"
                ? t(lang, "updateLatestBody", { version: manualNotice.current })
                : t(lang, "updateFailedBody")}
            </p>
            <div className="modal-actions">
              <button className="primary" onClick={() => setManualNotice(null)}>
                {t(lang, "close")}
              </button>
            </div>
          </div>
        </div>
      )}
      {settingsDialog}
      {menu && (
        <ul className="context-menu" role="menu" style={{ left: menu.x, top: menu.y }}>
          <li role="none">
            <button
              role="menuitem"
              onClick={() => {
                setMenu(null);
                rescan();
              }}
            >
              {t(lang, "rescan")}
            </button>
          </li>
        </ul>
      )}
    </div>
  );
}

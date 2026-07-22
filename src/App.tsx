import { useCallback, useEffect, useState } from "react";
import {
  browseFolder,
  checkLocks,
  getSettings,
  onProcessProgress,
  onScanProgress,
  openPlotTemp,
  openPack,
  processPacks,
  revealPack,
  saveSettings,
  scanDefault,
  scanPath,
} from "./ipc";
import { detectLang, isLang, t, LANGS, type Lang } from "./i18n";
import { FolderIcon, GlobeIcon, MoonIcon, RefreshIcon } from "./icons";
import { GROUP_ORDER, hasGroupTag, hasLunarTag, showLunarBadge } from "./groups";
import PackList, { mcMenuCompare } from "./PackList";
import type {
  Category,
  PackEntry,
  ProcessReport,
  ProgressEvent,
  ScanReport,
  Settings,
} from "./types";

/** Confirm dialog: same severity order as main page, without normal. */
const CONFIRM_GROUP_ORDER: Category[] = GROUP_ORDER.filter((c) => c !== "normal");

function confirmGroupOf(e: PackEntry): Category {
  return e.category === "normal" && hasLunarTag(e) ? "lunar_illegal" : e.category;
}

/** Everything the process run will touch. */
function problemsOf(report: ScanReport): PackEntry[] {
  return report.entries.filter((e) => e.category !== "normal" || hasLunarTag(e));
}

const GROUP_LABELS: Record<Category, string> = {
  illegal: "catIllegal",
  nested: "catNested",
  lunar_illegal: "catLunarIllegal",
  folder: "catFolder",
  bloated: "catBloated",
  normal: "catNormal",
};

const ACTION_GROUPS: { action: string[]; labelKey: string }[] = [
  { action: ["converted"], labelKey: "groupConverted" },
  { action: ["moved_to_illegal"], labelKey: "groupIllegal" },
  {
    action: ["failed", "skipped_locked", "skipped_unsupported"],
    labelKey: "groupSkipped",
  },
];

function resultText(result: ProcessReport): string {
  return result.outcomes
    .map((o) => {
      const products = o.products.length ? ` -> ${o.products.join(", ")}` : "";
      const detail = o.detail ? ` [${o.detail}]` : "";
      return `${o.action}: ${o.original_name}${products}${detail}`;
    })
    .join("\n");
}

export default function App() {
  const [lang, setLang] = useState<Lang>("zh-CN");
  const [settings, setSettings] = useState<Settings>({});
  const [report, setReport] = useState<ScanReport | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ProcessReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Category | null>(null);
  const [scanProgress, setScanProgress] = useState<ProgressEvent | null>(null);
  const [query, setQuery] = useState("");
  const [pathInput, setPathInput] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [locked, setLocked] = useState<string[]>([]);

  useEffect(() => {
    (async () => {
      const stored = await getSettings().catch(() => ({}) as Settings);
      setSettings(stored);
      setLang(
        isLang(stored.language) ? stored.language : detectLang(navigator.language),
      );
      const first = stored.custom_path
        ? await scanPath(stored.custom_path)
        : await scanDefault();
      setReport(first);
      setScanProgress(null);
    })();
    const cleanups: (() => void)[] = [];
    onProcessProgress(setProgress).then((un) => cleanups.push(un));
    onScanProgress(setScanProgress).then((un) => cleanups.push(un));
    // Replace the WebView2 context menu with our own rescan-only menu.
    const onCtx = (e: MouseEvent) => {
      e.preventDefault();
      setMenu({ x: e.clientX, y: e.clientY });
    };
    document.addEventListener("contextmenu", onCtx);
    cleanups.push(() => document.removeEventListener("contextmenu", onCtx));
    return () => cleanups.forEach((un) => un());
  }, []);

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
    if (pathInput === null || !report) return;
    const next = pathInput.trim();
    setPathInput(null);
    if (next && next !== report.path) scanInto(next);
  }, [pathInput, report, scanInto]);

  /** Re-probe locks only — never rescans the directory. */
  const recheck = useCallback(() => {
    if (!report) return;
    Promise.resolve(checkLocks(report.path, problemsOf(report).map((e) => e.name)))
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
    try {
      const outcome = await processPacks(report.path);
      setResult(outcome);
      setReport(await scanPath(report.path));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }, [report]);

  if (!report) {
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
  const occupied = confirmEntries.filter((e) => lockedSet.has(e.name));
  const confirmByGroup = CONFIRM_GROUP_ORDER.map((key) => ({
    key,
    entries: confirmEntries.filter(
      (e) => confirmGroupOf(e) === key && !lockedSet.has(e.name),
    ),
  })).filter((g) => g.entries.length > 0);
  const lowerQuery = query.toLowerCase();
  const shown = report.entries.filter(
    (e) =>
      (!filter || hasGroupTag(e, filter)) &&
      (!lowerQuery || e.name.toLowerCase().includes(lowerQuery)),
  );

  return (
    <div className="app">
      <div className="toolbar">
        <span className="scan-path">
          <span className="path-label">{t(lang, "scanDirLabel")}</span>
          <input
            className="path-box"
            type="text"
            aria-label={t(lang, "scanDirLabel")}
            title={report.path}
            value={pathInput ?? report.path}
            onChange={(e) => setPathInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && commitPath()}
            onBlur={commitPath}
          />
          <button
            className="icon-btn"
            title={t(lang, "browse")}
            aria-label={t(lang, "browse")}
            onClick={browse}
          >
            <FolderIcon />
          </button>
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
          <span className="icon-select" title={t(lang, "language")}>
            <GlobeIcon />
            <select
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
          </span>
          <button
            className="icon-btn"
            title={t(lang, "rescan")}
            aria-label={t(lang, "rescan")}
            onClick={rescan}
          >
            <RefreshIcon />
          </button>
        </span>
      </div>
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
            onReveal={(name) => revealPack(report.path, name)}
            onOpen={(name) => openPack(report.path, name)}
          />
          <div className="actions">
            {/* Fixed English per user request — not i18n */}
            <span className="files-total">
              {report.entries.length} files in total
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
          <span className="files-total">
            {report.entries.length} files in total
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
                      <li key={e.name} className="confirm-row occupied-row">
                        <span className={`dot cat-${confirmGroupOf(e)}`} />
                        <span className="confirm-name">{e.name}</span>
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
                      <li key={e.name} className="confirm-row">
                        <span className={`dot cat-${key}`} />
                        <span className="confirm-name">{e.name}</span>
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
            </div>
            <div className="modal-actions">
              <button
                onClick={() => navigator.clipboard.writeText(resultText(result))}
              >
                {t(lang, "copyResult")}
              </button>
              <button onClick={() => openPlotTemp()}>{t(lang, "openPlotTemp")}</button>
              <button className="primary" onClick={() => setResult(null)}>
                {t(lang, "close")}
              </button>
            </div>
          </div>
        </div>
      )}
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

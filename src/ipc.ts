// SDK-style wrappers around the Tauri IPC boundary — the mockable seam for UI tests.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  ProcessReport,
  ProgressEvent,
  ScanReport,
  Settings,
  UpdateInfo,
} from "./types";

export const GITHUB_REPO_URL = "https://github.com/Sakyvo/plot";

export function scanDefault(): Promise<ScanReport> {
  return invoke<ScanReport>("scan_default");
}

export function scanPath(path: string): Promise<ScanReport> {
  return invoke<ScanReport>("scan_path", { path });
}

/** Default resourcepacks path as text — no IO, for pre-filling the landing page. */
export function defaultDir(): Promise<string> {
  return invoke("default_dir");
}

export async function browseFolder(): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

export function processPacks(path: string): Promise<ProcessReport> {
  return invoke<ProcessReport>("process_packs", { path });
}

/** Write-conflict precheck: which of `names` are held open (usually by a running MC). */
export function checkLocks(path: string, names: string[]): Promise<string[]> {
  return invoke<string[]>("check_locks", { path, names });
}

export function openPlotTemp(runDir?: string): Promise<void> {
  return invoke("open_plot_temp", { runDir });
}

export function revealPack(dir: string, name: string): Promise<void> {
  return invoke("reveal_pack", { dir, name });
}

export function openPack(dir: string, name: string): Promise<void> {
  return invoke("open_pack", { dir, name });
}

export function onProcessProgress(
  cb: (event: ProgressEvent) => void,
): Promise<() => void> {
  return listen<ProgressEvent>("process-progress", (ev) => cb(ev.payload));
}

export function onScanProgress(
  cb: (event: ProgressEvent) => void,
): Promise<() => void> {
  return listen<ProgressEvent>("scan-progress", (ev) => cb(ev.payload));
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function saveSettings(newSettings: Settings): Promise<void> {
  return invoke("save_settings", { newSettings });
}

export function openUrl(url: string): Promise<void> {
  return invoke("open_url", { url });
}

/** `null` when already latest / no release; rejects on hard failure — UI stays silent either way. */
export function checkForUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_for_update");
}

export function appVersion(): Promise<string> {
  return invoke<string>("app_version");
}

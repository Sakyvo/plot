export type ScanStatus = "ok" | "missing_dir" | "no_packs";

export type Category =
  | "normal"
  | "nested"
  | "folder"
  | "bloated"
  | "illegal"
  | "lunar_illegal"
  | "ignored";

export type NodeKind =
  | "pack"
  | "classification_folder"
  | "supporting_folder"
  | "shell";

export interface IgnoreReason {
  key: string;
  values: string[];
}

export interface PackEntry {
  name: string;
  relative_path: string;
  parent_path: string | null;
  kind: NodeKind;
  category: Category;
  causes: string[];
  size_bytes: number;
  ignore?: IgnoreReason | null;
}

export interface Counts {
  normal: number;
  nested: number;
  folder: number;
  bloated: number;
  illegal: number;
  lunar: number;
  ignored: number;
}

export interface ScanReport {
  path: string;
  status: ScanStatus;
  total_packs: number;
  entries: PackEntry[];
  counts: Counts;
}

export interface PackOutcome {
  original_name: string;
  action: string;
  products: string[];
  causes: string[];
  detail: string | null;
  separated?: SeparatedPack[];
}

export interface SeparatedPack {
  name: string;
  parent: string;
}

export interface ProcessReport {
  outcomes: PackOutcome[];
  notices?: ProcessNotice[];
  /** Run folder actually used (name inside plot_temp), null for an empty batch. */
  run_dir?: string | null;
}

export interface ProcessNotice {
  key: string;
  values: string[];
}

export interface ProgressEvent {
  name: string;
  index: number;
  total: number;
}

export interface Settings {
  language?: string | null;
  custom_path?: string | null;
  auto_scan_on_start?: boolean | null;
}

/** Non-null only when GitHub latest release is newer than this build. */
export interface UpdateInfo {
  version: string;
  tag: string;
  body: string;
  download_url: string;
  html_url: string;
  current_version: string;
}

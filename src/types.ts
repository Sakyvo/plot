export type ScanStatus = "ok" | "missing_dir" | "no_packs";

export type Category =
  | "normal"
  | "nested"
  | "folder"
  | "bloated"
  | "illegal"
  | "lunar_illegal";

export interface PackEntry {
  name: string;
  category: Category;
  causes: string[];
  size_bytes: number;
}

export interface Counts {
  normal: number;
  nested: number;
  folder: number;
  bloated: number;
  illegal: number;
  lunar: number;
}

export interface ScanReport {
  path: string;
  status: ScanStatus;
  entries: PackEntry[];
  counts: Counts;
}

export interface PackOutcome {
  original_name: string;
  action: string;
  products: string[];
  causes: string[];
  detail: string | null;
}

export interface ProcessReport {
  outcomes: PackOutcome[];
}

export interface ProgressEvent {
  name: string;
  index: number;
  total: number;
}

export interface Settings {
  language?: string | null;
  custom_path?: string | null;
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

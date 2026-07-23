export type Lang = "zh-CN" | "zh-TW" | "en";

export const LANGS: { value: Lang; label: string }[] = [
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "en", label: "English" },
];

/// zh-Hant regions get Traditional, any other zh gets Simplified, rest English.
export function detectLang(tag: string): Lang {
  const lower = tag.toLowerCase();
  if (
    lower.startsWith("zh-tw") ||
    lower.startsWith("zh-hk") ||
    lower.startsWith("zh-mo") ||
    lower.startsWith("zh-hant")
  ) {
    return "zh-TW";
  }
  if (lower.startsWith("zh")) {
    return "zh-CN";
  }
  return "en";
}

export function isLang(value: string | null | undefined): value is Lang {
  return value === "zh-CN" || value === "zh-TW" || value === "en";
}

type Messages = Record<string, string>;

const catalogs: Record<Lang, Messages> = {
  "zh-CN": {
    appName: "Plot",
    scanning: "扫描中…",
    scanProgress: "扫描中… {index}/{total}",
    missingDir: "未找到 .minecraft\\resourcepacks 目录，请手动选择材质包文件夹",
    noPacks: "未检测到材质包",
    browse: "浏览文件夹",
    rescan: "重新扫描",
    scanDirLabel: "扫描目录：",
    processingTitle: "处理中",
    overview: "分类概览",
    catNormal: "正常",
    catNested: "嵌套",
    catFolder: "文件夹",
    catBloated: "臃肿",
    catIllegal: "非法",
    catLunarIllegal: "Lunar非法",
    lunarBadge: "含非法 JSON 转义，Lunar 可能读不进",
    processBtn: "处理",
    confirmTitle: "确认处理",
    occupiedTitle: "被占用",
    occupiedHint:
      "检测到 {n} 个材质包被占用（通常是运行中的 Minecraft）。可关闭游戏后重试；直接确认则这些包将被跳过。",
    recheckLocks: "重新检测",
    willConvert: "将转换",
    willMove: "将移入非法区",
    illegalManualHint: "处理完成后请在 illegal_packs 中手动处理",
    confirm: "确认",
    cancel: "取消",
    processing: "处理中… {name} ({index}/{total})",
    resultTitle: "处理结果",
    groupConverted: "转换成功",
    groupIllegal: "已移入非法区",
    groupSkipped: "失败 / 跳过",
    copyResult: "复制结果",
    openPlotTemp: "打开 plot_temp",
    close: "关闭",
    rarHint: "检测到 RAR/7z 压缩包，Plot v1 无法解析，请手动解压后重新扫描",
    processError: "处理失败：{msg}",
    language: "语言",
    reveal: "定位文件位置",
    searchPlaceholder: "搜索材质名",
    clearSearch: "清除搜索",
    emptyList: "无",
    clickToOpen: "点击打开",
    githubOpen: "在 GitHub 打开",
    checkUpdate: "检测更新",
    updateTitle: "发现新版本 {version}",
    updateCurrent: "当前版本：{version}",
    updateNotes: "更新说明",
    updateNoNotes: "无更新说明",
    updateDownload: "下载",
    updateLater: "稍后",
    updateLatestTitle: "已是最新版本",
    updateLatestBody: "当前已是最新版本（{version}）。",
    updateFailedTitle: "无法检查更新",
    updateFailedBody: "连接 GitHub 失败或超时，请检查网络后重试。",
  },
  "zh-TW": {
    appName: "Plot",
    scanning: "掃描中…",
    scanProgress: "掃描中… {index}/{total}",
    missingDir: "未找到 .minecraft\\resourcepacks 目錄，請手動選擇材質包資料夾",
    noPacks: "未偵測到材質包",
    browse: "瀏覽資料夾",
    rescan: "重新掃描",
    scanDirLabel: "掃描目錄：",
    processingTitle: "處理中",
    overview: "分類概覽",
    catNormal: "正常",
    catNested: "嵌套",
    catFolder: "資料夾",
    catBloated: "臃腫",
    catIllegal: "非法",
    catLunarIllegal: "Lunar非法",
    lunarBadge: "含非法 JSON 跳脫，Lunar 可能讀不進",
    processBtn: "處理",
    confirmTitle: "確認處理",
    occupiedTitle: "被佔用",
    occupiedHint:
      "偵測到 {n} 個材質包被佔用（通常是執行中的 Minecraft）。可關閉遊戲後重試；直接確認則這些包將被跳過。",
    recheckLocks: "重新檢測",
    willConvert: "將轉換",
    willMove: "將移入非法區",
    illegalManualHint: "處理完成後請在 illegal_packs 中手動處理",
    confirm: "確認",
    cancel: "取消",
    processing: "處理中… {name} ({index}/{total})",
    resultTitle: "處理結果",
    groupConverted: "轉換成功",
    groupIllegal: "已移入非法區",
    groupSkipped: "失敗 / 跳過",
    copyResult: "複製結果",
    openPlotTemp: "開啟 plot_temp",
    close: "關閉",
    rarHint: "偵測到 RAR/7z 壓縮檔，Plot v1 無法解析，請手動解壓後重新掃描",
    processError: "處理失敗：{msg}",
    language: "語言",
    reveal: "定位檔案位置",
    searchPlaceholder: "搜尋材質名",
    clearSearch: "清除搜尋",
    emptyList: "無",
    clickToOpen: "點擊開啟",
    githubOpen: "在 GitHub 開啟",
    checkUpdate: "檢查更新",
    updateTitle: "發現新版本 {version}",
    updateCurrent: "目前版本：{version}",
    updateNotes: "更新說明",
    updateNoNotes: "無更新說明",
    updateDownload: "下載",
    updateLater: "稍後",
    updateLatestTitle: "已是最新版本",
    updateLatestBody: "目前已是最新版本（{version}）。",
    updateFailedTitle: "無法檢查更新",
    updateFailedBody: "連線 GitHub 失敗或逾時，請檢查網路後重試。",
  },
  en: {
    appName: "Plot",
    scanning: "Scanning…",
    scanProgress: "Scanning… {index}/{total}",
    missingDir:
      ".minecraft\\resourcepacks not found — pick your resource pack folder manually",
    noPacks: "No resource packs detected",
    browse: "Browse folder",
    rescan: "Rescan",
    scanDirLabel: "Scanning:",
    processingTitle: "Processing",
    overview: "Category overview",
    catNormal: "Normal",
    catNested: "Nested",
    catFolder: "Folder",
    catBloated: "Bloated",
    catIllegal: "Illegal",
    catLunarIllegal: "Illegal in LC",
    lunarBadge: "Invalid JSON escape — Lunar may hide this pack",
    processBtn: "Fix",
    confirmTitle: "Confirm processing",
    occupiedTitle: "In use",
    occupiedHint:
      "{n} pack(s) are in use (usually by a running Minecraft). Close the game and retry, or confirm to skip them.",
    recheckLocks: "Recheck",
    willConvert: "Will convert",
    willMove: "Will quarantine as illegal",
    illegalManualHint: "After processing, handle these manually in illegal_packs",
    confirm: "Confirm",
    cancel: "Cancel",
    processing: "Processing… {name} ({index}/{total})",
    resultTitle: "Results",
    groupConverted: "Converted",
    groupIllegal: "Quarantined",
    groupSkipped: "Failed / skipped",
    copyResult: "Copy results",
    openPlotTemp: "Open plot_temp",
    close: "Close",
    rarHint:
      "RAR/7z archive detected — Plot v1 cannot parse it, extract it manually and rescan",
    processError: "Processing failed: {msg}",
    language: "Language",
    reveal: "Reveal in Explorer",
    searchPlaceholder: "Search packs",
    clearSearch: "Clear search",
    emptyList: "None",
    clickToOpen: "Click to open",
    githubOpen: "Open on GitHub",
    checkUpdate: "Check for updates",
    updateTitle: "Update available: {version}",
    updateCurrent: "Current version: {version}",
    updateNotes: "Release notes",
    updateNoNotes: "No release notes",
    updateDownload: "Download",
    updateLater: "Later",
    updateLatestTitle: "You're up to date",
    updateLatestBody: "You already have the latest version ({version}).",
    updateFailedTitle: "Could not check for updates",
    updateFailedBody: "Could not reach GitHub (network or timeout). Try again later.",
  },
};

export function t(lang: Lang, key: string, vars?: Record<string, string | number>): string {
  let text = catalogs[lang][key] ?? catalogs["zh-CN"][key] ?? key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      text = text.replaceAll(`{${k}}`, String(v));
    }
  }
  return text;
}

/// Every catalog must cover every key — checked by tests.
export function catalogKeys(lang: Lang): string[] {
  return Object.keys(catalogs[lang]);
}

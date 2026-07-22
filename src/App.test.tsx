import { render, screen, fireEvent, within, act } from "@testing-library/react";
import App from "./App";
import * as ipc from "./ipc";
import type {
  PackEntry,
  PackOutcome,
  ProcessReport,
  ProgressEvent,
  ScanReport,
} from "./types";

vi.mock("./ipc");

const mocked = vi.mocked(ipc);

beforeEach(() => {
  vi.clearAllMocks();
  mocked.onProcessProgress.mockResolvedValue(() => {});
  mocked.onScanProgress.mockResolvedValue(() => {});
  mocked.getSettings.mockResolvedValue({ language: "zh-CN" });
  mocked.saveSettings.mockResolvedValue(undefined);
  mocked.checkLocks.mockResolvedValue([]);
  setNavigatorLanguage("zh-CN");
});

function setNavigatorLanguage(tag: string) {
  Object.defineProperty(window.navigator, "language", {
    value: tag,
    configurable: true,
  });
}

function entry(partial: Partial<PackEntry>): PackEntry {
  return { name: "p.zip", category: "normal", causes: [], size_bytes: 1024, ...partial };
}

function report(partial: Partial<ScanReport>): ScanReport {
  const entries = partial.entries ?? [];
  const counts = {
    normal: 0,
    nested: 0,
    folder: 0,
    bloated: 0,
    illegal: 0,
    lunar: 0,
  };
  for (const e of entries) {
    if (e.category === "lunar_illegal") {
      counts.lunar += 1;
    } else {
      counts[e.category] += 1;
    }
    if (
      e.category === "lunar_illegal" ||
      e.causes.includes("lunar_escape")
    ) {
      if (e.category !== "lunar_illegal") counts.lunar += 1;
    }
  }
  return { path: "C:\\rp", status: "ok", entries, counts, ...partial };
}

function outcome(partial: Partial<PackOutcome>): PackOutcome {
  return {
    original_name: "x",
    action: "moved_to_illegal",
    products: [],
    causes: [],
    detail: null,
    ...partial,
  };
}

test("missing default directory shows red notice, browse rescans the picked folder", async () => {
  mocked.scanDefault.mockResolvedValue(report({ status: "missing_dir" }));
  mocked.browseFolder.mockResolvedValue("D:\\mc\\resourcepacks");
  mocked.scanPath.mockResolvedValue(
    report({
      path: "D:\\mc\\resourcepacks",
      entries: [entry({ name: "A.zip" })],
    }),
  );

  render(<App />);

  expect(await screen.findByText(/请手动选择材质包文件夹/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "浏览文件夹" }));
  expect(await screen.findByText("A.zip")).toBeTruthy();
});

test("an empty resourcepacks folder shows the red no-packs notice", async () => {
  mocked.scanDefault.mockResolvedValue(report({ status: "no_packs" }));

  render(<App />);

  expect(await screen.findByText("未检测到材质包")).toBeTruthy();
});

test("counts overview shows six category cards in severity order", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "a.zip", category: "normal" }),
        entry({ name: "b.zip", category: "normal" }),
        entry({ name: "c.zip", category: "nested" }),
        entry({ name: "d", category: "folder" }),
        entry({ name: "e.zip", category: "bloated" }),
        entry({ name: "f.txt", category: "illegal" }),
        entry({ name: "otb.zip", category: "lunar_illegal", causes: ["lunar_escape"] }),
      ],
    }),
  );

  render(<App />);

  const overview = await screen.findByRole("group", { name: "分类概览" });
  const labels = [...overview.querySelectorAll(".card-label")].map((el) => el.textContent);
  expect(labels).toEqual(["非法", "嵌套", "Lunar非法", "文件夹", "臃肿", "正常"]);
  expect(within(overview).getByText("正常").parentElement!.textContent).toContain("2");
  expect(within(overview).getByText("嵌套").parentElement!.textContent).toContain("1");
  expect(within(overview).getByText("Lunar非法").parentElement!.textContent).toContain("1");
  expect(within(overview).getByText("文件夹").parentElement!.textContent).toContain("1");
  expect(within(overview).getByText("臃肿").parentElement!.textContent).toContain("1");
  expect(within(overview).getByText("非法").parentElement!.textContent).toContain("1");
});

test("pure lunar packs are processable and show no stacked badge", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({
          name: "OTB FPS.zip",
          category: "lunar_illegal",
          causes: ["lunar_escape"],
        }),
      ],
    }),
  );

  render(<App />);

  await screen.findByText("OTB FPS.zip");
  expect(document.querySelector(".lunar-badge")).toBeNull();
  const btn = screen.getByRole("button", { name: "处理" });
  expect((btn as HTMLButtonElement).disabled).toBe(false);
});

test("stacked lunar shows moon badge; lunar card filter includes them", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({
          name: "OTB FPS.zip",
          category: "lunar_illegal",
          causes: ["lunar_escape"],
        }),
        entry({
          name: "wrapped.zip",
          category: "nested",
          causes: ["nested_container", "lunar_escape"],
        }),
        entry({ name: "clean.zip", category: "normal" }),
      ],
      counts: {
        normal: 1,
        nested: 1,
        folder: 0,
        bloated: 0,
        illegal: 0,
        lunar: 2,
      },
    }),
  );

  render(<App />);

  await screen.findByText("wrapped.zip");
  expect(document.querySelector(".lunar-badge")).toBeTruthy();

  const overview = screen.getByRole("group", { name: "分类概览" });
  fireEvent.click(within(overview).getByText("Lunar非法"));
  expect(screen.getByText("OTB FPS.zip")).toBeTruthy();
  expect(screen.getByText("wrapped.zip")).toBeTruthy();
  expect(screen.queryByText("clean.zip")).toBeNull();
});

test("pack rows show name and human-readable size", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [entry({ name: "Big.zip", size_bytes: 20 * 1024 * 1024 })],
    }),
  );

  render(<App />);

  expect(await screen.findByText("Big.zip")).toBeTruthy();
  expect(screen.getByText("20.0 MB")).toBeTruthy();
});

test("clicking a category card filters the list to that category", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "good.zip", category: "normal" }),
        entry({ name: "wrapped.zip", category: "nested", causes: ["nested_container"] }),
      ],
    }),
  );

  render(<App />);

  await screen.findByText("good.zip");
  const overview = screen.getByRole("group", { name: "分类概览" });
  fireEvent.click(within(overview).getByText("嵌套"));
  expect(screen.queryByText("good.zip")).toBeNull();
  expect(screen.getByText("wrapped.zip")).toBeTruthy();
});

test("list orders by category severity, then MC menu order within a group", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "zeta.zip", category: "normal" }),
        entry({ name: "Banana.zip", category: "normal" }),
        entry({ name: "apple.zip", category: "normal" }),
        entry({ name: "  spaced.zip", category: "normal" }),
        entry({ name: "mid.zip", category: "bloated" }),
        entry({ name: "wrap.zip", category: "nested" }),
        entry({ name: "dir", category: "folder" }),
        entry({ name: "bad.txt", category: "illegal" }),
      ],
    }),
  );

  render(<App />);

  await screen.findByText("bad.txt");
  const names = [...document.querySelectorAll(".pack-row .name")].map(
    (el) => el.textContent,
  );
  expect(names).toEqual([
    "bad.txt", // illegal first
    "wrap.zip", // nested
    "dir", // folder
    "mid.zip", // bloated
    "  spaced.zip", // normal group: leading spaces sort first (MC menu order)
    "apple.zip", // case-insensitive: APPLE < BANANA < ZETA
    "Banana.zip",
    "zeta.zip",
  ]);
});

test("list places pure lunar after nested and before folder", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "n.zip", category: "normal" }),
        entry({ name: "f", category: "folder" }),
        entry({
          name: "L.zip",
          category: "lunar_illegal",
          causes: ["lunar_escape"],
        }),
        entry({ name: "w.zip", category: "nested" }),
      ],
    }),
  );

  render(<App />);

  await screen.findByText("w.zip");
  const names = [...document.querySelectorAll(".pack-row .name")].map(
    (el) => el.textContent,
  );
  expect(names).toEqual(["w.zip", "L.zip", "f", "n.zip"]);
});

test("pack names render runs of spaces without collapsing", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "!      six spaces.zip" })] }),
  );

  render(<App />);

  await screen.findByText(/six spaces/);
  const el = document.querySelector<HTMLElement>(".pack-row .name")!;
  expect(el.title).toBe("!      six spaces.zip\n点击打开");
  expect(el.style.whiteSpace).toBe("pre");
});

test("clicking a pack name opens it with the system default app", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "A.zip" })] }),
  );

  render(<App />);

  fireEvent.click(await screen.findByText("A.zip"));
  expect(mocked.openPack).toHaveBeenCalledWith("C:\\rp", "A.zip");
});

test("large lists render a window of rows instead of all of them", async () => {
  const many = Array.from({ length: 1000 }, (_, i) =>
    entry({ name: `pack-${i}.zip` }),
  );
  mocked.scanDefault.mockResolvedValue(report({ entries: many }));

  render(<App />);

  await screen.findByText("pack-0.zip");
  const rendered = document.querySelectorAll(".pack-row").length;
  expect(rendered).toBeLessThan(200);
});

test("each row has a locate button that reveals the pack in explorer", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "A.zip" })] }),
  );

  render(<App />);

  await screen.findByText("A.zip");
  fireEvent.click(screen.getByRole("button", { name: "定位文件位置" }));
  expect(mocked.revealPack).toHaveBeenCalledWith("C:\\rp", "A.zip");
});

test("toolbar controls are icon buttons with localized tooltips", async () => {
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({})] }));

  render(<App />);

  const browse = await screen.findByRole("button", { name: "浏览文件夹" });
  const rescan = screen.getByRole("button", { name: "重新扫描" });
  expect(browse.querySelector("svg")).toBeTruthy();
  expect(rescan.querySelector("svg")).toBeTruthy();
  expect(browse.title).toBe("浏览文件夹");
  expect(rescan.title).toBe("重新扫描");
  const langWrap = screen.getByRole("combobox", { name: "语言" }).parentElement!;
  expect(langWrap.querySelector("svg")).toBeTruthy();
});

test("scan progress shows a live count while scanning", async () => {
  let fire: (ev: ProgressEvent) => void = () => {};
  mocked.onScanProgress.mockImplementation(async (cb) => {
    fire = cb;
    return () => {};
  });
  mocked.scanDefault.mockReturnValue(new Promise(() => {}));

  render(<App />);

  await screen.findByText("扫描中…");
  act(() => fire({ name: "x.zip", index: 500, total: 1104 }));
  expect(await screen.findByText("扫描中… 500/1104")).toBeTruthy();
});

test("wake controls are gone (ADR-0002: mtime therapy removed)", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "OTB FPS.zip" })] }),
  );

  render(<App />);

  await screen.findByText("OTB FPS.zip");
  expect(screen.queryByRole("button", { name: "唤醒" })).toBeNull();
  expect(screen.queryByRole("button", { name: "全部唤醒" })).toBeNull();
  expect(screen.queryByText(/已唤醒/)).toBeNull();
  expect("wakePack" in ipc).toBe(false);
  expect("wakeAll" in ipc).toBe(false);
});

test("search box filters by name and intersects with the card filter", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "OTB FPS.zip", category: "normal" }),
        entry({ name: "otb red.zip", category: "nested", causes: ["nested_container"] }),
        entry({ name: "Mav War.zip", category: "normal" }),
      ],
    }),
  );

  render(<App />);

  await screen.findByText("Mav War.zip");
  const box = screen.getByRole("textbox", { name: "搜索材质名" });
  fireEvent.change(box, { target: { value: "otb" } });
  expect(screen.queryByText("Mav War.zip")).toBeNull();
  expect(screen.getByText("OTB FPS.zip")).toBeTruthy();
  expect(screen.getByText("otb red.zip")).toBeTruthy();

  const overview = screen.getByRole("group", { name: "分类概览" });
  fireEvent.click(within(overview).getByText("嵌套"));
  expect(screen.queryByText("OTB FPS.zip")).toBeNull();
  expect(screen.getByText("otb red.zip")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "清除搜索" }));
  fireEvent.click(within(overview).getByText("嵌套"));
  expect(screen.getByText("Mav War.zip")).toBeTruthy();
});

test("an empty filter result shows the none placeholder", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "a.zip", category: "normal" })] }),
  );

  render(<App />);

  await screen.findByText("a.zip");
  fireEvent.change(screen.getByRole("textbox", { name: "搜索材质名" }), {
    target: { value: "nothing-matches" },
  });
  expect(screen.queryByText("a.zip")).toBeNull();
  expect(screen.getByText("无")).toBeTruthy();
});

test("processing shows a modal with a determinate progress bar", async () => {
  mocked.scanDefault.mockResolvedValue(PROBLEM_REPORT());
  let fire: (ev: ProgressEvent) => void = () => {};
  mocked.onProcessProgress.mockImplementation(async (cb) => {
    fire = cb;
    return () => {};
  });
  mocked.processPacks.mockReturnValue(new Promise(() => {}));

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));

  const modal = await screen.findByRole("dialog", { name: "处理中" });
  act(() => fire({ name: "Stimpy Eum3.zip", index: 1, total: 4 }));
  const bar = modal.querySelector<HTMLProgressElement>("progress")!;
  expect(bar).toBeTruthy();
  expect(bar.max).toBe(4);
  expect(bar.value).toBe(2);
  expect(within(modal).getByText(/Stimpy Eum3\.zip \(2\/4\)/)).toBeTruthy();
});

test("toolbar groups the path box with browse, search sits in the right grid area", async () => {
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({})] }));

  render(<App />);

  const browse = await screen.findByRole("button", { name: "浏览文件夹" });
  const pathArea = browse.closest(".scan-path")!;
  expect(pathArea).toBeTruthy();
  expect(pathArea.textContent).toContain("扫描目录");
  const box = pathArea.querySelector<HTMLInputElement>(".path-box")!;
  expect(box.value).toBe("C:\\rp");
  expect(pathArea.lastElementChild).toBe(browse);
  const search = screen.getByRole("textbox", { name: "搜索材质名" });
  expect(search.closest(".search-area")).toBeTruthy();
});

test("typing a directory into the path box and pressing Enter scans it", async () => {
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({ name: "a.zip" })] }));
  mocked.scanPath.mockResolvedValue(
    report({ path: "D:\\other", entries: [entry({ name: "b.zip" })] }),
  );

  render(<App />);

  await screen.findByText("a.zip");
  const box = screen.getByRole("textbox", { name: "扫描目录：" });
  fireEvent.change(box, { target: { value: "D:\\other" } });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(await screen.findByText("b.zip")).toBeTruthy();
  expect(mocked.scanPath).toHaveBeenCalledWith("D:\\other");
  expect(mocked.saveSettings).toHaveBeenCalledWith(
    expect.objectContaining({ custom_path: "D:\\other" }),
  );
});

test("blurring the edited path box also triggers the scan", async () => {
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({ name: "a.zip" })] }));
  mocked.scanPath.mockResolvedValue(
    report({ path: "D:\\other", entries: [entry({ name: "b.zip" })] }),
  );

  render(<App />);

  await screen.findByText("a.zip");
  const box = screen.getByRole("textbox", { name: "扫描目录：" });
  fireEvent.change(box, { target: { value: "D:\\other" } });
  fireEvent.blur(box);
  expect(await screen.findByText("b.zip")).toBeTruthy();
  expect(mocked.scanPath).toHaveBeenCalledWith("D:\\other");
});

test("right-click opens a custom context menu with only rescan", async () => {
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({ name: "a.zip" })] }));
  mocked.scanPath.mockResolvedValue(report({ entries: [entry({ name: "a.zip" })] }));

  render(<App />);

  await screen.findByText("a.zip");
  fireEvent.contextMenu(screen.getByText("a.zip"));
  const menu = screen.getByRole("menu");
  const items = within(menu).getAllByRole("menuitem");
  expect(items).toHaveLength(1);
  expect(items[0].textContent).toBe("重新扫描");
  fireEvent.click(items[0]);
  expect(mocked.scanPath).toHaveBeenCalledWith("C:\\rp");
  expect(screen.queryByRole("menu")).toBeNull();
});

test("the loading screen shows a determinate progress bar driven by scan events", async () => {
  let captured: ((ev: ProgressEvent) => void) | undefined;
  mocked.onScanProgress.mockImplementation(async (cb) => {
    captured = cb;
    return () => {};
  });
  mocked.scanDefault.mockReturnValue(new Promise(() => {}));

  render(<App />);

  await act(async () => {
    captured?.({ name: "x.zip", index: 731, total: 1103 });
  });
  const bar = document.querySelector<HTMLProgressElement>("progress")!;
  expect(bar).toBeTruthy();
  expect(bar.max).toBe(1103);
  expect(bar.value).toBe(731);
});

const PROBLEM_REPORT = () =>
  report({
    entries: [
      entry({ name: "good.zip", category: "normal" }),
      entry({ name: "wrapped.zip", category: "nested", causes: ["nested_container"] }),
      entry({ name: "junk.txt", category: "illegal", causes: ["not_zip"] }),
    ],
  });

test("locked packs pin to a yellow occupied section atop the confirm dialog", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "wrapped.zip", category: "nested", causes: ["nested_container"] }),
        entry({ name: "fat.zip", category: "bloated", causes: ["extra_root_entries"] }),
      ],
    }),
  );
  mocked.checkLocks.mockResolvedValue(["fat.zip"]);

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));

  const dialog = screen.getByRole("dialog");
  expect(mocked.checkLocks).toHaveBeenCalledWith("C:\\rp", ["wrapped.zip", "fat.zip"]);

  const title = await within(dialog).findByText("被占用");
  const section = title.closest("section")!;
  expect(within(section).getByText("fat.zip")).toBeTruthy();
  expect(section.querySelector(".occupied-row")).toBeTruthy();
  expect(section.querySelector(".dot.cat-bloated")).toBeTruthy();
  const body = dialog.textContent ?? "";
  expect(body.indexOf("被占用")).toBeLessThan(body.indexOf("嵌套"));
  expect(within(dialog).getByText(/通常是运行中的 Minecraft/)).toBeTruthy();
  expect(within(dialog).getAllByText("fat.zip")).toHaveLength(1);
  expect(within(dialog).queryByText("臃肿")).toBeNull();
});

test("no locked packs leaves the confirm dialog exactly as before", async () => {
  mocked.scanDefault.mockResolvedValue(PROBLEM_REPORT());

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));

  const dialog = screen.getByRole("dialog");
  await within(dialog).findByText("wrapped.zip");
  expect(within(dialog).queryByText("被占用")).toBeNull();
  expect(dialog.querySelector(".occupied-row")).toBeNull();
});

test("recheck re-probes locks only; unlocked packs fall back to their groups", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "wrapped.zip", category: "nested", causes: ["nested_container"] }),
        entry({ name: "fat.zip", category: "bloated", causes: ["extra_root_entries"] }),
      ],
    }),
  );
  mocked.checkLocks.mockResolvedValue(["wrapped.zip", "fat.zip"]);

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  const dialog = screen.getByRole("dialog");
  await within(dialog).findByText("被占用");
  expect(dialog.querySelectorAll(".occupied-row")).toHaveLength(2);

  // one pack released: it falls back to its category group, the other stays pinned
  mocked.checkLocks.mockResolvedValue(["fat.zip"]);
  fireEvent.click(within(dialog).getByRole("button", { name: "重新检测" }));
  await within(dialog).findByText("嵌套");
  expect(dialog.querySelectorAll(".occupied-row")).toHaveLength(1);
  expect(within(dialog).getByText("被占用")).toBeTruthy();

  // all released: warning section disappears entirely, no rescan happened
  mocked.checkLocks.mockResolvedValue([]);
  fireEvent.click(within(dialog).getByRole("button", { name: "重新检测" }));
  await within(dialog).findByText("臃肿");
  expect(within(dialog).queryByText("被占用")).toBeNull();
  expect(dialog.querySelector(".occupied-row")).toBeNull();
  expect(mocked.checkLocks).toHaveBeenCalledTimes(3);
  expect(mocked.scanPath).not.toHaveBeenCalled();
  expect(mocked.scanDefault).toHaveBeenCalledTimes(1);
});

test("footer shows fixed English files-in-total count", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "a.zip" }),
        entry({ name: "b.zip", category: "nested" }),
        entry({ name: "c.zip", category: "folder" }),
      ],
    }),
  );

  render(<App />);
  expect(await screen.findByText("3 files in total")).toBeTruthy();
});

test("empty pack folder still shows 0 files in total", async () => {
  mocked.scanDefault.mockResolvedValue(report({ status: "no_packs", entries: [] }));

  render(<App />);
  expect(await screen.findByText("0 files in total")).toBeTruthy();
});

test("process button opens a confirm dialog listing conversions and removals; cancel does nothing", async () => {
  mocked.scanDefault.mockResolvedValue(PROBLEM_REPORT());

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));

  const dialog = screen.getByRole("dialog");
  expect(within(dialog).getByText("wrapped.zip")).toBeTruthy();
  expect(within(dialog).getByText("junk.txt")).toBeTruthy();
  expect(within(dialog).queryByText("good.zip")).toBeNull();
  expect(dialog.querySelector(".dot.cat-nested")).toBeTruthy();
  expect(dialog.querySelector(".dot.cat-illegal")).toBeTruthy();
  expect(within(dialog).getByText(/illegal_packs/)).toBeTruthy();

  fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(mocked.processPacks).not.toHaveBeenCalled();
});

test("confirm dialog groups packs by category order without normal", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "good.zip", category: "normal" }),
        entry({ name: "fat.zip", category: "bloated", causes: ["bloated_zip"] }),
        entry({ name: "nest.zip", category: "nested", causes: ["nested_container"] }),
        entry({ name: "dir", category: "folder", causes: ["folder_pack"] }),
        entry({ name: "bad.txt", category: "illegal", causes: ["not_zip"] }),
        entry({
          name: "lc.zip",
          category: "lunar_illegal",
          causes: ["lunar_escape"],
        }),
      ],
    }),
  );

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));

  const dialog = screen.getByRole("dialog");
  expect(within(dialog).queryByText("good.zip")).toBeNull();

  const body = dialog.textContent ?? "";
  const illegalAt = body.indexOf("bad.txt");
  const nestedAt = body.indexOf("nest.zip");
  const lunarAt = body.indexOf("lc.zip");
  const folderAt = body.indexOf("dir");
  const bloatedAt = body.indexOf("fat.zip");
  expect(illegalAt).toBeGreaterThanOrEqual(0);
  expect(nestedAt).toBeGreaterThan(illegalAt);
  expect(lunarAt).toBeGreaterThan(nestedAt);
  expect(folderAt).toBeGreaterThan(lunarAt);
  expect(bloatedAt).toBeGreaterThan(folderAt);
  expect(within(dialog).getByText(/illegal_packs/)).toBeTruthy();
});

test("confirming runs processing, shows grouped results, and rescans", async () => {
  mocked.scanDefault.mockResolvedValue(PROBLEM_REPORT());
  const processed: ProcessReport = {
    outcomes: [
      outcome({ original_name: "junk.txt", action: "moved_to_illegal" }),
      outcome({ original_name: "wrapped.zip", action: "skipped_unsupported" }),
    ],
  };
  mocked.processPacks.mockResolvedValue(processed);
  mocked.scanPath.mockResolvedValue(
    report({ entries: [entry({ name: "good.zip" })] }),
  );

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));

  const result = await screen.findByRole("dialog", { name: "处理结果" });
  const illegalGroup = within(result).getByText("已移入非法区").parentElement!;
  expect(within(illegalGroup).getByText(/junk\.txt/)).toBeTruthy();
  expect(within(result).getByText(/illegal_packs/)).toBeTruthy();
  expect(mocked.scanPath).toHaveBeenCalledWith("C:\\rp");
});

test("result dialog can copy the report and open plot_temp", async () => {
  mocked.scanDefault.mockResolvedValue(PROBLEM_REPORT());
  mocked.processPacks.mockResolvedValue({
    outcomes: [outcome({ original_name: "junk.txt" })],
  });
  mocked.scanPath.mockResolvedValue(report({ entries: [] }));
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.assign(navigator, { clipboard: { writeText } });

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));
  const result = await screen.findByRole("dialog", { name: "处理结果" });

  fireEvent.click(within(result).getByRole("button", { name: "复制结果" }));
  expect(writeText).toHaveBeenCalledWith(expect.stringContaining("junk.txt"));
  fireEvent.click(within(result).getByRole("button", { name: "打开 plot_temp" }));
  expect(mocked.openPlotTemp).toHaveBeenCalled();
});

test("converted outcomes list original and product names", async () => {
  mocked.scanDefault.mockResolvedValue(PROBLEM_REPORT());
  mocked.processPacks.mockResolvedValue({
    outcomes: [
      outcome({
        original_name: "Yokabi.zip",
        action: "converted",
        products: ["Yokabi.zip"],
      }),
    ],
  });
  mocked.scanPath.mockResolvedValue(report({ entries: [] }));

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));

  const result = await screen.findByRole("dialog", { name: "处理结果" });
  const group = within(result).getByText("转换成功").parentElement!;
  expect(within(group).getByText(/Yokabi\.zip → Yokabi\.zip/)).toBeTruthy();
});

test("rar and sevenz outcomes carry the manual-extraction hint", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [entry({ name: "pack.rar", category: "illegal", causes: ["rar_archive"] })],
    }),
  );
  mocked.processPacks.mockResolvedValue({
    outcomes: [
      outcome({ original_name: "pack.rar", causes: ["rar_archive"] }),
    ],
  });
  mocked.scanPath.mockResolvedValue(report({ entries: [] }));

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));

  const result = await screen.findByRole("dialog", { name: "处理结果" });
  expect(within(result).getByText(/请手动解压后重新扫描/)).toBeTruthy();
});

test("a traditional-chinese system locale renders zh-TW text", async () => {
  setNavigatorLanguage("zh-TW");
  mocked.getSettings.mockResolvedValue({});
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({})] }));

  render(<App />);

  expect(await screen.findByRole("button", { name: "瀏覽資料夾" })).toBeTruthy();
});

test("a saved language setting overrides the system locale", async () => {
  setNavigatorLanguage("zh-TW");
  mocked.getSettings.mockResolvedValue({ language: "en" });
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({})] }));

  render(<App />);

  expect(await screen.findByRole("button", { name: "Browse folder" })).toBeTruthy();
});

test("switching language takes effect immediately and persists", async () => {
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({})] }));

  render(<App />);
  await screen.findByRole("button", { name: "浏览文件夹" });
  fireEvent.change(screen.getByRole("combobox", { name: "语言" }), {
    target: { value: "en" },
  });

  expect(await screen.findByRole("button", { name: "Browse folder" })).toBeTruthy();
  expect(mocked.saveSettings).toHaveBeenCalledWith(
    expect.objectContaining({ language: "en" }),
  );
});

test("a saved custom path is scanned directly on startup", async () => {
  mocked.getSettings.mockResolvedValue({ language: "zh-CN", custom_path: "D:\\mc\\rp" });
  mocked.scanPath.mockResolvedValue(
    report({ path: "D:\\mc\\rp", entries: [entry({ name: "saved.zip" })] }),
  );

  render(<App />);

  expect(await screen.findByText("saved.zip")).toBeTruthy();
  expect(mocked.scanDefault).not.toHaveBeenCalled();
});

test("browsing a folder persists it as the custom path", async () => {
  mocked.scanDefault.mockResolvedValue(report({ status: "missing_dir" }));
  mocked.browseFolder.mockResolvedValue("E:\\packs");
  mocked.scanPath.mockResolvedValue(report({ path: "E:\\packs", entries: [entry({})] }));

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "浏览文件夹" }));

  await screen.findByDisplayValue("E:\\packs");
  expect(mocked.saveSettings).toHaveBeenCalledWith(
    expect.objectContaining({ custom_path: "E:\\packs" }),
  );
});

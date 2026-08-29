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
  mocked.getSettings.mockResolvedValue({
    language: "zh-CN",
    auto_scan_on_start: true,
  });
  mocked.defaultDir.mockResolvedValue("C:\\default\\resourcepacks");
  mocked.saveSettings.mockResolvedValue(undefined);
  mocked.checkLocks.mockResolvedValue([]);
  mocked.checkForUpdate.mockResolvedValue(null);
  mocked.openUrl.mockResolvedValue(undefined);
  mocked.appVersion.mockResolvedValue("0.1.0");
  setNavigatorLanguage("zh-CN");
});

function setNavigatorLanguage(tag: string) {
  Object.defineProperty(window.navigator, "language", {
    value: tag,
    configurable: true,
  });
}

function entry(partial: Partial<PackEntry>): PackEntry {
  const name = partial.name ?? "p.zip";
  return {
    name,
    relative_path: name,
    parent_path: null,
    kind: "pack",
    category: "normal",
    causes: [],
    size_bytes: 1024,
    ignore: null,
    ...partial,
  };
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
    ignored: 0,
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
  const total_packs = entries.filter((entry) => entry.kind === "pack").length;
  return { path: "C:\\rp", status: "ok", total_packs, entries, counts, ...partial };
}

function outcome(partial: Partial<PackOutcome>): PackOutcome {
  return {
    original_name: "x",
    action: "moved_to_illegal",
    products: [],
    causes: [],
    detail: null,
    separated: [],
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

test("counts overview shows seven category cards with ignored last", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "a.zip", category: "normal" }),
        entry({ name: "b.zip", category: "normal" }),
        entry({ name: "c.zip", category: "nested" }),
        entry({ name: "d", kind: "classification_folder", category: "folder" }),
        entry({ name: "e.zip", category: "bloated" }),
        entry({ name: "f.txt", category: "illegal" }),
        entry({ name: "otb.zip", category: "lunar_illegal", causes: ["lunar_escape"] }),
        entry({
          name: "meezoid.zip",
          category: "ignored",
          ignore: { key: "modern_texture_layout", values: ["assets/minecraft/textures/item"] },
        }),
      ],
    }),
  );

  render(<App />);

  const overview = await screen.findByRole("group", { name: "分类概览" });
  const labels = [...overview.querySelectorAll(".card-label")].map((el) => el.textContent);
  expect(labels).toEqual([
    "非法",
    "嵌套",
    "Lunar非法",
    "分类文件夹",
    "臃肿",
    "正常",
    "忽略",
  ]);
  expect(within(overview).getByText("正常").parentElement!.textContent).toContain("2");
  expect(within(overview).getByText("嵌套").parentElement!.textContent).toContain("1");
  expect(within(overview).getByText("Lunar非法").parentElement!.textContent).toContain("1");
  expect(within(overview).getByText("分类文件夹").parentElement!.textContent).toContain("1");
  expect(within(overview).getByText("臃肿").parentElement!.textContent).toContain("1");
  expect(within(overview).getByText("非法").parentElement!.textContent).toContain("1");
  expect(within(overview).getByText("忽略").parentElement!.textContent).toContain("1");
  expect(
    screen.getByText(
      "检测到高版本纹理目录 assets/minecraft/textures/item，可能是高版本材质",
    ),
  ).toBeTruthy();
});

test("a classification folder is a parent view and not a processing problem", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      total_packs: 2,
      counts: {
        normal: 2,
        nested: 0,
        folder: 1,
        bloated: 0,
        illegal: 0,
        lunar: 0,
        ignored: 0,
      },
      entries: [
        entry({
          name: "PotPvP",
          relative_path: "PotPvP",
          parent_path: null,
          kind: "classification_folder",
          category: "folder",
        }),
        entry({
          name: "A.zip",
          relative_path: "PotPvP/A.zip",
          parent_path: "PotPvP",
          kind: "pack",
        }),
        entry({
          name: "B.zip",
          relative_path: "PotPvP/B.zip",
          parent_path: "PotPvP",
          kind: "pack",
        }),
      ],
    }),
  );

  render(<App />);

  const overview = await screen.findByRole("group", { name: "分类概览" });
  expect(within(overview).getByText("分类文件夹").parentElement!.textContent).toContain("1");
  expect(screen.getByText("PotPvP")).toBeTruthy();
  expect(screen.getByText("A.zip")).toBeTruthy();
  expect(screen.getByText("B.zip")).toBeTruthy();
  expect(screen.getByText("2 files in total")).toBeTruthy();
  expect((screen.getByRole("button", { name: "处理" }) as HTMLButtonElement).disabled).toBe(true);
});

test("top-level folders start expanded while deeper classification folders collapse", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({
          name: "PotPvP",
          relative_path: "PotPvP",
          kind: "classification_folder",
          category: "folder",
        }),
        entry({
          name: "Melee",
          relative_path: "PotPvP/Melee",
          parent_path: "PotPvP",
          kind: "classification_folder",
          category: "folder",
        }),
        entry({
          name: "A.zip",
          relative_path: "PotPvP/Melee/A.zip",
          parent_path: "PotPvP/Melee",
        }),
        entry({
          name: "B.zip",
          relative_path: "PotPvP/Melee/B.zip",
          parent_path: "PotPvP/Melee",
        }),
        entry({
          name: "UHC",
          relative_path: "PotPvP/UHC",
          parent_path: "PotPvP",
          kind: "classification_folder",
          category: "folder",
        }),
      ],
    }),
  );

  render(<App />);

  expect(await screen.findByText("PotPvP")).toBeTruthy();
  expect(screen.getByText("Melee")).toBeTruthy();
  expect(screen.getByText("UHC")).toBeTruthy();
  expect(screen.queryByText("A.zip")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "展开 Melee" }));
  expect(screen.getByText("A.zip")).toBeTruthy();
  expect(screen.getByText("B.zip")).toBeTruthy();
});

test("tree search keeps ancestors for a pack and the full subtree for a folder", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({
          name: "PotPvP",
          relative_path: "PotPvP",
          kind: "classification_folder",
          category: "folder",
        }),
        entry({
          name: "Melee",
          relative_path: "PotPvP/Melee",
          parent_path: "PotPvP",
          kind: "classification_folder",
          category: "folder",
        }),
        entry({ name: "Needle.zip", relative_path: "PotPvP/Melee/Needle.zip", parent_path: "PotPvP/Melee" }),
        entry({ name: "Other.zip", relative_path: "PotPvP/Melee/Other.zip", parent_path: "PotPvP/Melee" }),
        entry({
          name: "UHC",
          relative_path: "PotPvP/UHC",
          parent_path: "PotPvP",
          kind: "classification_folder",
          category: "folder",
        }),
        entry({ name: "Bow.zip", relative_path: "PotPvP/UHC/Bow.zip", parent_path: "PotPvP/UHC" }),
      ],
    }),
  );

  render(<App />);
  const search = await screen.findByRole("textbox", { name: "搜索材质名" });

  fireEvent.change(search, { target: { value: "Needle" } });
  expect(screen.getByText("PotPvP")).toBeTruthy();
  expect(screen.getByText("Melee")).toBeTruthy();
  expect(screen.getByText("Needle.zip")).toBeTruthy();
  expect(screen.queryByText("Other.zip")).toBeNull();
  expect(screen.queryByText("UHC")).toBeNull();

  fireEvent.change(search, { target: { value: "Melee" } });
  expect(screen.getByText("PotPvP")).toBeTruthy();
  expect(screen.getByText("Melee")).toBeTruthy();
  expect(screen.getByText("Needle.zip")).toBeTruthy();
  expect(screen.getByText("Other.zip")).toBeTruthy();
  expect(screen.queryByText("UHC")).toBeNull();
});

test("tree card filters keep ancestors, show category trees, and include shell children", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "Collections", relative_path: "Collections", kind: "classification_folder", category: "folder" }),
        entry({ name: "Bad.zip", relative_path: "Collections/Bad.zip", parent_path: "Collections", category: "illegal" }),
        entry({ name: "Good.zip", relative_path: "Collections/Good.zip", parent_path: "Collections" }),
        entry({ name: "Parent.zip", relative_path: "Parent.zip", kind: "shell", category: "nested" }),
        entry({ name: "Inner.zip", relative_path: "Parent.zip/Inner.zip", parent_path: "Parent.zip" }),
        entry({ name: "Other.zip", relative_path: "Parent.zip/Other.zip", parent_path: "Parent.zip" }),
      ],
    }),
  );

  render(<App />);
  const overview = await screen.findByRole("group", { name: "分类概览" });

  fireEvent.click(within(overview).getByText("非法"));
  expect(screen.getByText("Collections")).toBeTruthy();
  expect(screen.getByText("Bad.zip")).toBeTruthy();
  expect(screen.queryByText("Good.zip")).toBeNull();

  fireEvent.click(within(overview).getByText("非法"));
  fireEvent.click(within(overview).getByText("分类文件夹"));
  expect(screen.getByText("Collections")).toBeTruthy();
  expect(screen.getByText("Bad.zip")).toBeTruthy();
  expect(screen.getByText("Good.zip")).toBeTruthy();
  expect(screen.queryByText("Parent.zip")).toBeNull();

  fireEvent.click(within(overview).getByText("分类文件夹"));
  fireEvent.click(within(overview).getByText("嵌套"));
  expect(screen.getByText("Parent.zip")).toBeTruthy();
  expect(screen.getByText("Inner.zip")).toBeTruthy();
  expect(screen.queryByText("Collections")).toBeNull();
});

test("a single-pack shell chain collapses to one orange nested row", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "Wrapper.zip", relative_path: "Wrapper.zip", kind: "shell", category: "nested", causes: ["archive_shell"] }),
        entry({ name: "Inner.zip", relative_path: "Wrapper.zip/Inner.zip", parent_path: "Wrapper.zip" }),
      ],
    }),
  );

  render(<App />);
  await screen.findByText("Wrapper.zip");

  // The leaf is absorbed; only the merged top row renders.
  expect(screen.queryByText("Inner.zip")).toBeNull();
  // No expand toggle on a merged row.
  expect(screen.queryByRole("button", { name: "展开 Wrapper.zip" })).toBeNull();
  // The merged row uses the orange nested dot.
  const row = screen.getByText("Wrapper.zip").closest(".pack-row") as HTMLElement;
  const dot = row.querySelector(".dot") as HTMLElement;
  expect(dot.className).toContain("cat-nested");
});

test("a merged row surfaces the leaf lunar badge", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "Wrapper.zip", relative_path: "Wrapper.zip", kind: "shell", category: "nested", causes: ["archive_shell"] }),
        entry({
          name: "Inner.zip",
          relative_path: "Wrapper.zip/Inner.zip",
          parent_path: "Wrapper.zip",
          category: "nested",
          causes: ["nested_container", "lunar_escape"],
        }),
      ],
    }),
  );

  render(<App />);
  await screen.findByText("Wrapper.zip");
  expect(document.querySelector(".lunar-badge")).toBeTruthy();
});

test("an ignored leaf keeps the shell tree expanded, not merged", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "Wrapper.zip", relative_path: "Wrapper.zip", kind: "shell", category: "nested", causes: ["archive_shell"] }),
        entry({
          name: "Inner.zip",
          relative_path: "Wrapper.zip/Inner.zip",
          parent_path: "Wrapper.zip",
          category: "ignored",
          ignore: { key: "modern_texture_layout", values: ["assets/minecraft/textures/item"] },
        }),
      ],
    }),
  );

  render(<App />);
  await screen.findByText("Wrapper.zip");
  // Ignored leaves do not merge — the shell stays a collapsible folder row,
  // and as a top-level folder it starts expanded with the leaf visible.
  expect(screen.getByRole("button", { name: "折叠 Wrapper.zip" })).toBeTruthy();
  expect(screen.getByText("Inner.zip")).toBeTruthy();
  // Collapsing hides the ignored leaf.
  fireEvent.click(screen.getByRole("button", { name: "折叠 Wrapper.zip" }));
  expect(screen.queryByText("Inner.zip")).toBeNull();
});

test("folders stay collapsible while a card filter is active", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "Root", relative_path: "Root", kind: "classification_folder", category: "folder" }),
        entry({ name: "A.zip", relative_path: "Root/A.zip", parent_path: "Root", category: "nested" }),
        entry({ name: "B.zip", relative_path: "Root/B.zip", parent_path: "Root" }),
      ],
    }),
  );

  render(<App />);
  const overview = await screen.findByRole("group", { name: "分类概览" });

  // Activating the nested card expands folders so matches are visible...
  fireEvent.click(within(overview).getByText("嵌套"));
  expect(screen.getByText("A.zip")).toBeTruthy();
  // ...but the user can still collapse the folder back.
  fireEvent.click(screen.getByRole("button", { name: "折叠 Root" }));
  expect(screen.queryByText("A.zip")).toBeNull();
  expect(screen.getByRole("button", { name: "展开 Root" })).toBeTruthy();
});

test("classification siblings sort folders by name and packs by severity then name", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "Root", relative_path: "Root", kind: "classification_folder", category: "folder" }),
        entry({ name: "z.zip", relative_path: "Root/z.zip", parent_path: "Root" }),
        entry({ name: "Beta", relative_path: "Root/Beta", parent_path: "Root", kind: "classification_folder", category: "folder" }),
        entry({ name: "b.zip", relative_path: "Root/b.zip", parent_path: "Root", category: "illegal" }),
        entry({ name: "Alpha", relative_path: "Root/Alpha", parent_path: "Root", kind: "classification_folder", category: "folder" }),
        entry({ name: "a.zip", relative_path: "Root/a.zip", parent_path: "Root", category: "illegal" }),
        entry({ name: "nested.zip", relative_path: "Root/nested.zip", parent_path: "Root", category: "nested" }),
      ],
    }),
  );

  const { container } = render(<App />);
  await screen.findByText("Root");
  const names = [...container.querySelectorAll(".pack-row .name")].map((node) => node.textContent);
  expect(names).toEqual(["Root", "a.zip", "b.zip", "nested.zip", "Alpha", "Beta", "z.zip"]);
});

test("folder rows open and reveal their own relative path", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "PotPvP", relative_path: "PotPvP", kind: "classification_folder", category: "folder" }),
        entry({ name: "A.zip", relative_path: "PotPvP/A.zip", parent_path: "PotPvP" }),
        entry({ name: "B.zip", relative_path: "PotPvP/B.zip", parent_path: "PotPvP" }),
      ],
    }),
  );

  render(<App />);
  const folderName = await screen.findByText("PotPvP");
  const row = folderName.closest(".pack-row") as HTMLElement;
  fireEvent.click(folderName);
  expect(mocked.openPack).toHaveBeenCalledWith("C:\\rp", "PotPvP");
  fireEvent.click(within(row).getByRole("button", { name: "定位文件位置" }));
  expect(mocked.revealPack).toHaveBeenCalledWith("C:\\rp", "PotPvP");
});

test("folder expansion survives view filters and resets after a rescan", async () => {
  const tree = report({
    entries: [
      entry({ name: "Root", relative_path: "Root", kind: "classification_folder", category: "folder" }),
      entry({ name: "Melee", relative_path: "Root/Melee", parent_path: "Root", kind: "classification_folder", category: "folder" }),
      entry({ name: "A.zip", relative_path: "Root/Melee/A.zip", parent_path: "Root/Melee" }),
      entry({ name: "B.zip", relative_path: "Root/Melee/B.zip", parent_path: "Root/Melee" }),
    ],
  });
  mocked.scanDefault.mockResolvedValue(tree);
  mocked.scanPath.mockResolvedValue({ ...tree, entries: tree.entries.map((item) => ({ ...item })) });

  render(<App />);
  await screen.findByText("Melee");
  fireEvent.click(screen.getByRole("button", { name: "展开 Melee" }));
  expect(screen.getByText("A.zip")).toBeTruthy();
  const normalCard = screen.getByText("正常").closest("button") as HTMLButtonElement;
  fireEvent.click(normalCard);
  fireEvent.click(normalCard);
  expect(screen.getByText("A.zip")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "重新扫描" }));
  await act(async () => {});
  expect(screen.queryByText("A.zip")).toBeNull();
  expect(screen.getByRole("button", { name: "展开 Melee" })).toBeTruthy();
});

test("ignored packs filter by their gray card and never enter processing", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({
          name: "meezoid.zip",
          category: "ignored",
          ignore: { key: "modern_texture_layout", values: ["assets/minecraft/textures/item"] },
        }),
        entry({ name: "clean.zip", category: "normal" }),
      ],
    }),
  );

  render(<App />);

  expect(
    await screen.findByText(
      "检测到高版本纹理目录 assets/minecraft/textures/item，可能是高版本材质",
    ),
  ).toBeTruthy();
  fireEvent.click(within(screen.getByRole("group", { name: "分类概览" })).getByText("忽略"));
  expect(screen.getByText("meezoid.zip")).toBeTruthy();
  expect(screen.queryByText("clean.zip")).toBeNull();
  expect((screen.getByRole("button", { name: "处理" }) as HTMLButtonElement).disabled).toBe(true);
  expect(mocked.checkLocks).not.toHaveBeenCalled();
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
        ignored: 0,
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
        entry({ name: "dir", kind: "classification_folder", category: "folder" }),
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
    "dir", // classification folder
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
        entry({ name: "f", kind: "classification_folder", category: "folder" }),
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
  // Language selection moved into the settings dialog — none in the toolbar.
  expect(screen.queryByRole("combobox", { name: "语言" })).toBeNull();
  const gear = screen.getByRole("button", { name: "设置" });
  expect(gear.querySelector("svg")).toBeTruthy();
});

test("before settings resolve the boot skeleton shows — never a misleading scanning text", async () => {
  mocked.getSettings.mockReturnValue(new Promise(() => {}));

  render(<App />);

  expect(
    document.querySelector('[data-testid="boot-skeleton"]'),
  ).toBeTruthy();
  expect(screen.queryByText(/扫描中/)).toBeNull();
  expect(screen.queryByRole("button", { name: "开始扫描" })).toBeNull();
});

test("the landing page takes over cleanly once settings resolve", async () => {
  mocked.getSettings.mockResolvedValue({ language: "zh-CN" });

  render(<App />);

  expect(screen.queryByText(/扫描中/)).toBeNull();
  expect(
    await screen.findByRole("button", { name: "开始扫描" }),
  ).toBeTruthy();
  expect(
    document.querySelector('[data-testid="boot-skeleton"]'),
  ).toBeNull();
});

test("without the auto-scan setting the app lands idle — no scan is kicked", async () => {
  mocked.getSettings.mockResolvedValue({ language: "zh-CN" });

  render(<App />);

  const start = await screen.findByRole("button", { name: "开始扫描" });
  expect(screen.getByText(/尚未扫描/)).toBeTruthy();
  expect(mocked.scanDefault).not.toHaveBeenCalled();
  expect(mocked.scanPath).not.toHaveBeenCalled();
  // Path box pre-filled with the default dir; rescan has nothing to refresh.
  const box = screen.getByRole("textbox", { name: "扫描目录：" });
  expect((box as HTMLInputElement).value).toBe("C:\\default\\resourcepacks");
  expect(mocked.defaultDir).toHaveBeenCalled();
  expect((screen.getByRole("button", { name: "重新扫描" }) as HTMLButtonElement).disabled).toBe(true);
  // Right-click offers no rescan menu before the first scan.
  fireEvent.contextMenu(start);
  expect(screen.queryByRole("menu")).toBeNull();
});

test("landing start button runs scanDefault; a stored custom path is preferred", async () => {
  mocked.getSettings.mockResolvedValue({ language: "zh-CN" });
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({ name: "a.zip" })] }));

  const { unmount } = render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "开始扫描" }));
  expect(await screen.findByText("a.zip")).toBeTruthy();
  expect(mocked.scanDefault).toHaveBeenCalledTimes(1);
  expect(mocked.scanPath).not.toHaveBeenCalled();
  unmount();

  vi.clearAllMocks();
  mocked.getSettings.mockResolvedValue({
    language: "zh-CN",
    custom_path: "D:\\mc\\resourcepacks",
  });
  mocked.scanPath.mockResolvedValue(
    report({ path: "D:\\mc\\resourcepacks", entries: [entry({ name: "b.zip" })] }),
  );
  render(<App />);
  const start = await screen.findByRole("button", { name: "开始扫描" });
  // Stored custom path prefills without asking the backend for the default.
  expect(mocked.defaultDir).not.toHaveBeenCalled();
  fireEvent.click(start);
  expect(await screen.findByText("b.zip")).toBeTruthy();
  expect(mocked.scanPath).toHaveBeenCalledWith("D:\\mc\\resourcepacks");
  expect(mocked.scanDefault).not.toHaveBeenCalled();
});

test("typing a path on the landing page and pressing Enter scans it", async () => {
  mocked.getSettings.mockResolvedValue({ language: "zh-CN" });
  mocked.scanPath.mockResolvedValue(
    report({ path: "D:\\other", entries: [entry({ name: "b.zip" })] }),
  );

  render(<App />);
  const box = await screen.findByRole("textbox", { name: "扫描目录：" });
  fireEvent.change(box, { target: { value: "D:\\other" } });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(await screen.findByText("b.zip")).toBeTruthy();
  expect(mocked.scanPath).toHaveBeenCalledWith("D:\\other");
  expect(mocked.saveSettings).toHaveBeenCalledWith(
    expect.objectContaining({ custom_path: "D:\\other" }),
  );
});

test("the settings dialog hosts the language select and the auto-scan toggle", async () => {
  mocked.scanDefault.mockResolvedValue(report({ entries: [entry({ name: "a.zip" })] }));

  render(<App />);
  await screen.findByText("a.zip");
  fireEvent.click(screen.getByRole("button", { name: "设置" }));

  const dialog = screen.getByRole("dialog", { name: "设置" });
  // Language select moved here; choosing persists immediately.
  fireEvent.change(within(dialog).getByRole("combobox", { name: "语言" }), {
    target: { value: "en" },
  });
  expect(mocked.saveSettings).toHaveBeenCalledWith(
    expect.objectContaining({ language: "en" }),
  );
  // Dialog copy follows the new language live.
  expect(
    within(dialog).getByRole("heading", { name: "Settings" }),
  ).toBeTruthy();
  // Auto-scan checkbox mirrors settings and persists on toggle.
  const checkbox = within(dialog).getByRole("checkbox", {
    name: "Scan automatically on startup",
  }) as HTMLInputElement;
  expect(checkbox.checked).toBe(true);
  fireEvent.click(checkbox);
  expect(mocked.saveSettings).toHaveBeenCalledWith(
    expect.objectContaining({ auto_scan_on_start: false }),
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
  expect(screen.queryByRole("dialog", { name: "Settings" })).toBeNull();
});

test("the settings dialog is reachable from the landing page too", async () => {
  mocked.getSettings.mockResolvedValue({ language: "zh-CN", auto_scan_on_start: false });

  render(<App />);
  await screen.findByRole("button", { name: "开始扫描" });
  fireEvent.click(screen.getByRole("button", { name: "设置" }));
  const dialog = screen.getByRole("dialog", { name: "设置" });
  const checkbox = within(dialog).getByRole("checkbox", {
    name: "启动时自动扫描",
  }) as HTMLInputElement;
  expect(checkbox.checked).toBe(false);
  fireEvent.click(checkbox);
  expect(mocked.saveSettings).toHaveBeenCalledWith(
    expect.objectContaining({ auto_scan_on_start: true }),
  );
  // Toggling mid-session never kicks a scan itself.
  expect(mocked.scanDefault).not.toHaveBeenCalled();
  expect(mocked.scanPath).not.toHaveBeenCalled();
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
  expect(pathArea.firstElementChild).toBe(browse);
  expect(browse.nextElementSibling?.classList.contains("path-label")).toBe(true);
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
        entry({ name: "category", kind: "classification_folder", category: "folder" }),
      ],
    }),
  );

  render(<App />);
  expect(await screen.findByText("2 files in total")).toBeTruthy();
});

test("footer GitHub button opens the repository URL", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "a.zip" })] }),
  );

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "在 GitHub 打开" }));
  expect(mocked.openUrl).toHaveBeenCalledWith(ipc.GITHUB_REPO_URL);
});

test("startup update dialog shows notes and download opens asset URL", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "a.zip" })] }),
  );
  mocked.checkForUpdate.mockResolvedValue({
    version: "0.2.0",
    tag: "v0.2.0",
    body: "## Fixed\n- nested unwrap",
    download_url: "https://github.com/Sakyvo/plot/releases/download/v0.2.0/plot.exe",
    html_url: "https://github.com/Sakyvo/plot/releases/tag/v0.2.0",
    current_version: "0.1.0",
  });

  render(<App />);
  const dialog = await screen.findByRole("dialog", { name: /发现新版本 0\.2\.0/ });
  expect(within(dialog).getByText(/nested unwrap/)).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "下载" }));
  expect(mocked.openUrl).toHaveBeenCalledWith(
    "https://github.com/Sakyvo/plot/releases/download/v0.2.0/plot.exe",
  );
});

test("startup update check failure stays silent", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "a.zip" })] }),
  );
  mocked.checkForUpdate.mockRejectedValue(new Error("timeout"));

  render(<App />);
  expect(await screen.findByText("1 files in total")).toBeTruthy();
  expect(screen.queryByRole("dialog", { name: /发现新版本/ })).toBeNull();
  expect(screen.queryByRole("dialog", { name: "无法检查更新" })).toBeNull();
});

test("manual check with no update shows latest dialog", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "a.zip" })] }),
  );
  mocked.checkForUpdate.mockResolvedValue(null);

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "检测更新" }));
  const dialog = await screen.findByRole("dialog", { name: "已是最新版本" });
  expect(within(dialog).getByText(/0\.1\.0/)).toBeTruthy();
});

test("manual check failure shows connection error dialog", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "a.zip" })] }),
  );
  // Startup call rejects silently; subsequent manual click rejects with dialog.
  mocked.checkForUpdate
    .mockRejectedValueOnce(new Error("startup-timeout"))
    .mockRejectedValueOnce(new Error("manual-timeout"));

  render(<App />);
  expect(await screen.findByText("1 files in total")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "检测更新" }));
  expect(await screen.findByRole("dialog", { name: "无法检查更新" })).toBeTruthy();
});

test("manual check with update shows download dialog", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "a.zip" })] }),
  );
  mocked.checkForUpdate
    .mockResolvedValueOnce(null)
    .mockResolvedValueOnce({
      version: "0.3.0",
      tag: "v0.3.0",
      body: "manual notes",
      download_url: "https://example.com/plot.exe",
      html_url: "https://example.com/rel",
      current_version: "0.1.0",
    });

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "检测更新" }));
  const dialog = await screen.findByRole("dialog", { name: /发现新版本 0\.3\.0/ });
  expect(within(dialog).getByText(/manual notes/)).toBeTruthy();
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
        entry({ name: "dir", kind: "classification_folder", category: "folder" }),
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
  const bloatedAt = body.indexOf("fat.zip");
  expect(illegalAt).toBeGreaterThanOrEqual(0);
  expect(nestedAt).toBeGreaterThan(illegalAt);
  expect(lunarAt).toBeGreaterThan(nestedAt);
  expect(bloatedAt).toBeGreaterThan(lunarAt);
  expect(body).not.toContain("dir");
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

test("result dialog can copy the report and open this batch's run folder", async () => {
  mocked.scanDefault.mockResolvedValue(PROBLEM_REPORT());
  mocked.processPacks.mockResolvedValue({
    outcomes: [outcome({ original_name: "junk.txt" })],
    run_dir: "Plot_2026-08-23_13.46.34",
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
  fireEvent.click(within(result).getByRole("button", { name: "打开本次输出" }));
  expect(mocked.openPlotTemp).toHaveBeenCalledWith("Plot_2026-08-23_13.46.34");
});

test("a null run_dir falls back to opening the plot_temp root", async () => {
  mocked.scanDefault.mockResolvedValue(PROBLEM_REPORT());
  mocked.processPacks.mockResolvedValue({
    outcomes: [outcome({ original_name: "junk.txt" })],
    run_dir: null,
  });
  mocked.scanPath.mockResolvedValue(report({ entries: [] }));

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));
  const result = await screen.findByRole("dialog", { name: "处理结果" });

  fireEvent.click(within(result).getByRole("button", { name: "打开本次输出" }));
  expect(mocked.openPlotTemp).toHaveBeenCalledWith(undefined);
});

test("archive attachment notices appear in results and copied text", async () => {
  mocked.scanDefault.mockResolvedValue(PROBLEM_REPORT());
  mocked.processPacks.mockResolvedValue({
    outcomes: [outcome({ original_name: "Download.zip/A.zip", action: "converted" })],
    notices: [
      {
        key: "attachments_kept_in_original_archive",
        values: ["Download.zip"],
      },
    ],
  });
  mocked.scanPath.mockResolvedValue(report({ entries: [] }));
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.assign(navigator, { clipboard: { writeText } });

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));
  const result = await screen.findByRole("dialog", { name: "处理结果" });

  expect(within(result).getByText("Download.zip：附件保留在原始压缩文件中")).toBeTruthy();
  fireEvent.click(within(result).getByRole("button", { name: "复制结果" }));
  expect(writeText).toHaveBeenCalledWith(
    expect.stringContaining("Download.zip：附件保留在原始压缩文件中"),
  );
});

test("classified pack confirmation and results preserve relative paths", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "PotPvP", relative_path: "PotPvP", kind: "classification_folder", category: "folder" }),
        entry({ name: "A.zip", relative_path: "PotPvP/A.zip", parent_path: "PotPvP" }),
        entry({ name: "B.zip", relative_path: "PotPvP/B.zip", parent_path: "PotPvP", category: "bloated", causes: ["root_extras"] }),
        entry({ name: "C.zip", relative_path: "PotPvP/C.zip", parent_path: "PotPvP", category: "illegal", causes: ["not_zip"] }),
      ],
    }),
  );
  mocked.processPacks.mockResolvedValue({
    outcomes: [
      outcome({ original_name: "PotPvP/B.zip", action: "converted", products: ["PotPvP/B.zip"] }),
      outcome({ original_name: "PotPvP/C.zip", action: "moved_to_illegal" }),
    ],
  });
  mocked.scanPath.mockResolvedValue(report({ entries: [] }));

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  const confirmation = screen.getByRole("dialog", { name: "确认处理" });
  expect(within(confirmation).getByText("PotPvP/B.zip")).toBeTruthy();
  expect(within(confirmation).getByText("PotPvP/C.zip")).toBeTruthy();
  expect(within(confirmation).queryByText("A.zip")).toBeNull();
  fireEvent.click(within(confirmation).getByRole("button", { name: "确认" }));

  const result = await screen.findByRole("dialog", { name: "处理结果" });
  expect(within(result).getByText(/PotPvP\/B\.zip → PotPvP\/B\.zip/)).toBeTruthy();
  expect(within(result).getByText(/PotPvP\/C\.zip/)).toBeTruthy();
  expect(mocked.checkLocks).toHaveBeenCalledWith("C:\\rp", ["PotPvP/B.zip", "PotPvP/C.zip"]);
});

test("a folder shell is processable and confirms its real child path", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({
          name: "Wrapper",
          relative_path: "Wrapper",
          kind: "shell",
          category: "nested",
          causes: ["folder_shell"],
        }),
        entry({
          name: "A.zip",
          relative_path: "Wrapper/A.zip",
          parent_path: "Wrapper",
        }),
      ],
    }),
  );

  render(<App />);
  const processButton = await screen.findByRole("button", { name: "处理" });
  expect((processButton as HTMLButtonElement).disabled).toBe(false);
  fireEvent.click(processButton);

  const confirmation = screen.getByRole("dialog", { name: "确认处理" });
  expect(within(confirmation).getByText("Wrapper → Wrapper/A.zip")).toBeTruthy();
  expect(mocked.checkLocks).toHaveBeenCalledWith("C:\\rp", ["Wrapper/A.zip"]);
});

test("an archive shell confirms every planned inner pack and probes the outer file", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({
          name: "Parent.zip",
          relative_path: "Parent.zip",
          kind: "shell",
          category: "nested",
          causes: ["archive_shell"],
        }),
        entry({ name: "A.zip", relative_path: "Parent.zip/A.zip", parent_path: "Parent.zip" }),
        entry({ name: "B.zip", relative_path: "Parent.zip/B.zip", parent_path: "Parent.zip", category: "bloated" }),
        entry({ name: "C.zip", relative_path: "Parent.zip/C.zip", parent_path: "Parent.zip", category: "ignored", ignore: { key: "modern_texture_layout", values: ["assets/minecraft/textures/item"] } }),
        entry({ name: "D.zip", relative_path: "Parent.zip/D.zip", parent_path: "Parent.zip", category: "illegal" }),
      ],
    }),
  );

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));

  const confirmation = screen.getByRole("dialog", { name: "确认处理" });
  expect(
    within(confirmation).getByText(
      "Parent.zip → Parent.zip/A.zip, Parent.zip/B.zip, Parent.zip/C.zip, Parent.zip/D.zip",
    ),
  ).toBeTruthy();
  expect(mocked.checkLocks).toHaveBeenCalledWith("C:\\rp", ["Parent.zip"]);
});

test("an archive classification tree confirms every descendant pack path", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({
          name: "Download.zip",
          relative_path: "Download.zip",
          kind: "shell",
          category: "nested",
          causes: ["archive_shell"],
        }),
        entry({
          name: "PotPvP",
          relative_path: "Download.zip/PotPvP",
          parent_path: "Download.zip",
          kind: "classification_folder",
          category: "folder",
        }),
        entry({
          name: "A.zip",
          relative_path: "Download.zip/PotPvP/A.zip",
          parent_path: "Download.zip/PotPvP",
        }),
        entry({
          name: "B.zip",
          relative_path: "Download.zip/PotPvP/B.zip",
          parent_path: "Download.zip/PotPvP",
          category: "bloated",
        }),
      ],
    }),
  );

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));

  const confirmation = screen.getByRole("dialog", { name: "确认处理" });
  expect(
    within(confirmation).getByText(
      "Download.zip → Download.zip/PotPvP/A.zip, Download.zip/PotPvP/B.zip",
    ),
  ).toBeTruthy();
  expect(mocked.checkLocks).toHaveBeenCalledWith("C:\\rp", ["Download.zip"]);
});

test("results append preexisting ignored packs with localized reasons", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [
        entry({ name: "junk.txt", category: "illegal", causes: ["not_zip"] }),
        entry({
          name: "meezoid.zip",
          category: "ignored",
          ignore: { key: "modern_texture_layout", values: ["assets/minecraft/textures/item"] },
        }),
      ],
    }),
  );
  mocked.processPacks.mockResolvedValue({
    outcomes: [outcome({ original_name: "junk.txt", action: "moved_to_illegal" })],
  });
  mocked.scanPath.mockResolvedValue(
    report({
      entries: [
        entry({
          name: "meezoid.zip",
          category: "ignored",
          ignore: { key: "modern_texture_layout", values: ["assets/minecraft/textures/item"] },
        }),
      ],
    }),
  );
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.assign(navigator, { clipboard: { writeText } });

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));

  const result = await screen.findByRole("dialog", { name: "处理结果" });
  const ignored = within(result).getByText("已忽略（未处理）").parentElement!;
  expect(
    within(ignored).getByText(
      /meezoid\.zip.*检测到高版本纹理目录 assets\/minecraft\/textures\/item，可能是高版本材质/,
    ),
  ).toBeTruthy();
  fireEvent.click(within(result).getByRole("button", { name: "复制结果" }));
  expect(writeText).toHaveBeenCalledWith(
    expect.stringContaining(
      "meezoid.zip: 检测到高版本纹理目录 assets/minecraft/textures/item，可能是高版本材质",
    ),
  );
});

test("results merge newly separated ignored pack with its parent source", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({
      entries: [entry({ name: "Parent.zip", category: "bloated", causes: ["root_extras"] })],
    }),
  );
  mocked.processPacks.mockResolvedValue({
    outcomes: [
      outcome({
        original_name: "Parent.zip",
        action: "converted",
        products: ["Parent.zip"],
        separated: [{ name: "bonus-modern.zip", parent: "Parent.zip" }],
      }),
    ],
  });
  mocked.scanPath.mockResolvedValue(
    report({
      entries: [
        entry({ name: "Parent.zip", category: "normal" }),
        entry({
          name: "bonus-modern.zip",
          category: "ignored",
          ignore: { key: "modern_texture_layout", values: ["assets/minecraft/textures/item"] },
        }),
        entry({
          name: "unrelated.zip",
          category: "ignored",
          ignore: { key: "modern_texture_layout", values: ["assets/minecraft/textures/block"] },
        }),
      ],
    }),
  );
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.assign(navigator, { clipboard: { writeText } });

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));

  const result = await screen.findByRole("dialog", { name: "处理结果" });
  expect(
    within(result).getByText(
      /bonus-modern\.zip.*检测到高版本纹理目录 assets\/minecraft\/textures\/item.*由 Parent\.zip 分离/,
    ),
  ).toBeTruthy();
  expect(within(result).queryByText(/unrelated\.zip/)).toBeNull();
  fireEvent.click(within(result).getByRole("button", { name: "复制结果" }));
  expect(writeText).toHaveBeenCalledWith(
    expect.stringContaining(
      "bonus-modern.zip: 检测到高版本纹理目录 assets/minecraft/textures/item，可能是高版本材质（由 Parent.zip 分离）",
    ),
  );
});

test("rescan failure still shows completed outcomes without guessing separated category", async () => {
  mocked.scanDefault.mockResolvedValue(
    report({ entries: [entry({ name: "Parent.zip", category: "bloated", causes: ["root_extras"] })] }),
  );
  mocked.processPacks.mockResolvedValue({
    outcomes: [
      outcome({
        original_name: "Parent.zip",
        action: "converted",
        products: ["Parent.zip"],
        separated: [{ name: "bonus.zip", parent: "Parent.zip" }],
      }),
    ],
  });
  mocked.scanPath.mockRejectedValue(new Error("offline"));

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "处理" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认" }));

  const result = await screen.findByRole("dialog", { name: "处理结果" });
  expect(within(result).getByText(/Parent\.zip → Parent\.zip/)).toBeTruthy();
  expect(within(result).getByText(/处理后重新扫描失败/)).toBeTruthy();
  expect(within(result).queryByText("已忽略（未处理）")).toBeNull();
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
  fireEvent.click(screen.getByRole("button", { name: "设置" }));
  fireEvent.change(screen.getByRole("combobox", { name: "语言" }), {
    target: { value: "en" },
  });

  expect(await screen.findByRole("button", { name: "Browse folder" })).toBeTruthy();
  expect(mocked.saveSettings).toHaveBeenCalledWith(
    expect.objectContaining({ language: "en" }),
  );
});

test("a saved custom path is scanned directly on startup", async () => {
  mocked.getSettings.mockResolvedValue({
    language: "zh-CN",
    custom_path: "D:\\mc\\rp",
    auto_scan_on_start: true,
  });
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

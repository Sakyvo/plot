import { catalogKeys, detectLang, ignoreReasonText } from "./i18n";

test("all three catalogs cover exactly the same keys", () => {
  const zhCN = catalogKeys("zh-CN").sort();
  expect(catalogKeys("zh-TW").sort()).toEqual(zhCN);
  expect(catalogKeys("en").sort()).toEqual(zhCN);
});

test("system locale maps to the right language", () => {
  expect(detectLang("zh-CN")).toBe("zh-CN");
  expect(detectLang("zh-SG")).toBe("zh-CN");
  expect(detectLang("zh")).toBe("zh-CN");
  expect(detectLang("zh-TW")).toBe("zh-TW");
  expect(detectLang("zh-HK")).toBe("zh-TW");
  expect(detectLang("zh-MO")).toBe("zh-TW");
  expect(detectLang("zh-Hant-TW")).toBe("zh-TW");
  expect(detectLang("en-US")).toBe("en");
  expect(detectLang("ja-JP")).toBe("en");
});

test("modern texture layout reasons show the detected paths", () => {
  expect(
    ignoreReasonText("zh-CN", {
      key: "modern_texture_layout",
      values: ["assets/minecraft/textures/item", "assets/minecraft/textures/block"],
    }),
  ).toBe(
    "检测到高版本纹理目录 assets/minecraft/textures/item、assets/minecraft/textures/block，可能是高版本材质",
  );
  expect(
    ignoreReasonText("en", {
      key: "modern_texture_layout",
      values: ["assets/minecraft/textures/block"],
    }),
  ).toBe("Detected modern texture path assets/minecraft/textures/block; this may be a modern pack");
});

import { catalogKeys, detectLang } from "./i18n";

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

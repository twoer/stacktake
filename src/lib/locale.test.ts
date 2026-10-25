import { describe, expect, it } from "vitest";
import {
  isAppLocale,
  normalizeLocaleTag,
  parseLocalePreference,
  resolveLocale,
} from "@/lib/locale";

// 系统语言 → 五种语言是 P0 的唯一判据，且它同时决定「兜底是不是英文」。
// 映射函数纯、不碰 DOM，所以这些断言与跑测试的机器无关。

describe("normalizeLocaleTag —— 系统 BCP-47 归到五种语言", () => {
  it("繁体看 hant 或地区：TW / HK / MO 都归 zh-TW", () => {
    expect(normalizeLocaleTag("zh-Hant-TW")).toBe("zh-TW");
    expect(normalizeLocaleTag("zh-Hant")).toBe("zh-TW");
    expect(normalizeLocaleTag("zh-TW")).toBe("zh-TW");
    expect(normalizeLocaleTag("zh-HK")).toBe("zh-TW");
    expect(normalizeLocaleTag("zh-MO")).toBe("zh-TW");
  });

  it("其余中文按简体：zh、zh-CN、zh-Hans 与地区写法", () => {
    expect(normalizeLocaleTag("zh")).toBe("zh-CN");
    expect(normalizeLocaleTag("zh-CN")).toBe("zh-CN");
    expect(normalizeLocaleTag("zh-Hans-CN")).toBe("zh-CN");
    expect(normalizeLocaleTag("zh-SG")).toBe("zh-CN");
  });

  it("大小写与下划线不影响判定（Windows 可能给 zh_TW）", () => {
    expect(normalizeLocaleTag("ZH-HANT-tw")).toBe("zh-TW");
    expect(normalizeLocaleTag("zh_tw")).toBe("zh-TW");
    expect(normalizeLocaleTag("JA-jp")).toBe("ja");
  });

  it("日 / 西按语言前缀归；地区变体（es-419）同样收敛", () => {
    expect(normalizeLocaleTag("ja-JP")).toBe("ja");
    expect(normalizeLocaleTag("ja")).toBe("ja");
    expect(normalizeLocaleTag("es-419")).toBe("es");
    expect(normalizeLocaleTag("es-MX")).toBe("es");
  });

  it("五种之外一律英文——兜底是拍板过的，不是「保持系统语言」", () => {
    expect(normalizeLocaleTag("de-DE")).toBe("en");
    expect(normalizeLocaleTag("fr")).toBe("en");
    expect(normalizeLocaleTag("en-GB")).toBe("en");
    expect(normalizeLocaleTag(null)).toBe("en");
    expect(normalizeLocaleTag(undefined)).toBe("en");
    expect(normalizeLocaleTag("")).toBe("en");
  });
});

describe("parseLocalePreference / resolveLocale —— 偏好与系统标签分开", () => {
  it("只认五种语言；非法值、缺省与 system 都回到跟随系统", () => {
    expect(parseLocalePreference("ja")).toBe("ja");
    expect(parseLocalePreference("zh-TW")).toBe("zh-TW");
    expect(parseLocalePreference("system")).toBe("system");
    expect(parseLocalePreference(null)).toBe("system");
    // 旧版本 / 手改 localStorage 写进来的值不该被信任
    expect(parseLocalePreference("fr")).toBe("system");
    expect(parseLocalePreference("zh-cn")).toBe("system");
  });

  it("显式偏好压过系统标签；system 时走归一化", () => {
    expect(resolveLocale("es", "ja-JP")).toBe("es");
    expect(resolveLocale("system", "ja-JP")).toBe("ja");
    expect(resolveLocale("system", "de-DE")).toBe("en");
    expect(resolveLocale("system", null)).toBe("en");
  });

  it("isAppLocale 只对五值返回 true", () => {
    expect(isAppLocale("zh-CN")).toBe(true);
    expect(isAppLocale("en")).toBe(true);
    expect(isAppLocale("system")).toBe(false);
    expect(isAppLocale("EN")).toBe(false);
  });
});

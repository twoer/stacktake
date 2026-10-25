import { beforeEach, describe, expect, it } from "vitest";
import {
  activeLocale,
  applyLocale,
  compareText,
  formatBytesLocalized,
  formatNumber,
  relativeTime,
  renderText,
  t,
  translate,
} from "@/lib/i18n";

// 语言是模块级状态，每个用例前显式钉一次，避免用例之间互相继承。
beforeEach(() => applyLocale("zh-CN"));

describe("t / translate —— 查表与命名插值", () => {
  it("静态键按当前语言取；切语言立即换", () => {
    expect(t("common.yes")).toBe("是");
    applyLocale("en");
    expect(t("common.yes")).toBe("Yes");
    expect(activeLocale.value).toBe("en");
  });

  it("命名参数替换 `{name}`，语言名本身不翻译（母语写法）", () => {
    expect(t("settings.language.systemWith", { language: "日本語" })).toBe(
      "跟随系统（日本語）",
    );
    applyLocale("ja");
    expect(t("settings.language.systemWith", { language: "日本語" })).toBe(
      "システムに従う（日本語）",
    );
  });

  it("缺参数时保留占位符、缺键时回显键名——不抛错也不显示 undefined", () => {
    expect(translate("settings.language.systemWith", {})).toBe("跟随系统（{language}）");
    expect(translate("no.such.key")).toBe("no.such.key");
  });

  it("数字参数按当前语言格式化（es 的小数点是逗号）", () => {
    applyLocale("es");
    expect(translate("n = {n}", { n: 1536.5 })).toBe("n = 1.536,5");
  });
});

describe("renderText —— 后端 Text 三种形态", () => {
  it("Raw 原样、Key 查表、Fmt 带命名参数（参数可以是嵌套 Text）", () => {
    expect(renderText({ t: "r", v: "main" })).toBe("main");
    expect(renderText({ t: "k", v: "common.yes" })).toBe("是");
    expect(
      renderText({
        t: "f",
        v: { k: "settings.language.systemWith", a: { language: { t: "k", v: "common.no" } } },
      }),
    ).toBe("跟随系统（否）");
  });

  it("空值返回空串（detail / notes 里可能缺字段）", () => {
    expect(renderText(null)).toBe("");
    expect(renderText(undefined)).toBe("");
  });

  it("数字 / 字节参数走对应格式化", () => {
    expect(
      renderText({ t: "f", v: { k: "{n} · {b}", a: { n: { t: "n", v: 1536 }, b: { t: "b", v: 1536 } } } }),
    ).toBe("1,536 · 1.5 KB");
  });
});

describe("复数：调用方写基名，按 count 选后缀", () => {
  it("en 区分 one / other；ja 只有 other", () => {
    applyLocale("en");
    expect(t("clean.totalItems", { count: 1 })).toBe("1 item");
    expect(t("clean.totalItems", { count: 3 })).toBe("3 items");
    applyLocale("ja");
    expect(t("clean.totalItems", { count: 1 })).toBe("1 件");
  });

  it("不带 count 不猜复数：基名不是真实键，回显键名", () => {
    applyLocale("en");
    expect(translate("clean.totalItems")).toBe("clean.totalItems");
  });

  it("句子里的数量词也分单复数（1 repositories 是错的）", () => {
    applyLocale("en");
    expect(t("clean.batchRepoBody", { count: 1 })).toContain("1 repository");
    expect(t("clean.batchRepoBody", { count: 2 })).toContain("2 repositories");
    expect(t("inspector.omitted", { count: 1 })).toContain("1 more folder omitted");
    applyLocale("es");
    expect(t("filter.active", { count: 1 })).toBe("Filtrar (1 activo)");
    expect(
      t("clean.artifact.selectedSummary", { count: 1, bytes: { t: "b", v: 1024 } }),
    ).toContain("Seleccionado:");
    expect(
      t("clean.artifact.selectedSummary", { count: 2, bytes: { t: "b", v: 1024 } }),
    ).toContain("Seleccionados:");
  });
});

describe("本地化读数", () => {
  it("formatNumber 千分位随语言", () => {
    expect(formatNumber(1234)).toBe("1,234");
    applyLocale("es");
    expect(formatNumber(1234)).toBe("1.234");
  });

  it("字节单位不变、尾数不打千分位（es 下 1023 MB 不能变成有歧义的 1.023 MB）", () => {
    expect(formatBytesLocalized(1536)).toBe("1.5 KB");
    expect(formatBytesLocalized(512)).toBe("512 B");
    expect(formatBytesLocalized(0)).toBe("0 B");
    applyLocale("es");
    expect(formatBytesLocalized(1536)).toBe("1,5 KB");
    expect(formatBytesLocalized(1023 * 1024 * 1024)).toBe("1023 MB");
  });

  it("相对时间交给 Intl：连单位与空格也跟着语言", () => {
    applyLocale("zh-TW");
    expect(relativeTime(-1, "minute")).toBe("1 分鐘前");
    applyLocale("es");
    expect(relativeTime(-1, "minute")).toBe("hace 1 minuto");
  });

  it("排序用当前语言的 Collator：zh-CN 按拼音，「大」在「陈」之后（码位序会反过来）", () => {
    expect(compareText("大", "陈")).toBeGreaterThan(0);
    expect(compareText("Alpha", "beta")).toBeLessThan(0);
  });
});

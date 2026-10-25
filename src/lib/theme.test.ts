import { describe, expect, it } from "vitest";
import { nativeThemeFor, parseThemePreference, resolveTheme } from "@/lib/theme";

// 三态外观：缺省必须回到跟随系统。老版本只写过 light/dark，读到非法值也不能当成
// 某一边钉住——那会让系统切色而应用不动，用户找不到原因。
describe("parseThemePreference", () => {
  it("light / dark 是显式钉住", () => {
    expect(parseThemePreference("light")).toBe("light");
    expect(parseThemePreference("dark")).toBe("dark");
  });

  it("system、缺省与非法值都回到跟随系统", () => {
    expect(parseThemePreference("system")).toBe("system");
    expect(parseThemePreference(null)).toBe("system");
    expect(parseThemePreference("")).toBe("system");
    expect(parseThemePreference("DARK")).toBe("system");
  });
});

describe("resolveTheme", () => {
  it("跟随系统时取系统值", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("显式选择不看系统", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });
});

describe("nativeThemeFor", () => {
  it("只有显式选择才通报原生窗口，跟随系统交还 null", () => {
    expect(nativeThemeFor("system")).toBeNull();
    expect(nativeThemeFor("light")).toBe("light");
    expect(nativeThemeFor("dark")).toBe("dark");
  });
});

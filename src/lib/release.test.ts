import { describe, expect, it } from "vitest";
import { isNewer } from "@/lib/release";

// 版本比较直接决定设置页「有没有新版」：漏报是错过修复，误报是让用户白装一遍。
// 本项目 tag 不搞预发布后缀，segments 刻意把后缀剥掉、缺失段视作 0。
describe("isNewer", () => {
  it("tag 带 v 前缀、getVersion 不带，照样能比", () => {
    expect(isNewer("v0.2.0", "0.1.8")).toBe(true);
  });

  it("同版本与更旧都不报", () => {
    expect(isNewer("v0.1.8", "0.1.8")).toBe(false);
    expect(isNewer("0.1.7", "0.1.8")).toBe(false);
  });

  it("逐段按数字比，不是字典序", () => {
    expect(isNewer("0.1.10", "0.1.9")).toBe(true);
    expect(isNewer("0.10.0", "0.9.0")).toBe(true);
  });

  it("缺失段视作 0；预发布后缀剥掉视同同版", () => {
    expect(isNewer("1.0", "1.0.0")).toBe(false);
    expect(isNewer("v1.0.0-beta", "1.0.0")).toBe(false);
    expect(isNewer("0.2", "0.1.9")).toBe(true);
  });
});

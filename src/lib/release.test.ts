import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyLocale } from "@/lib/i18n";
import { checkForUpdate, isNewer } from "@/lib/release";

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

describe("checkForUpdate —— 本工具唯一一次对外请求的状态机", () => {
  // 报错消息是查表结果，测试环境（node）的系统语言是 en；显式钉中文，
  // 让断言守住「错误文案确实来自语言包」而不是某台机器碰巧的语言。
  beforeEach(() => applyLocale("zh-CN"));
  afterEach(() => vi.unstubAllGlobals());

  it("404 = 仓库还没有 release，按无更新处理而不是出错", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: false, status: 404, json: async () => ({}) })));
    expect(await checkForUpdate("0.1.8")).toEqual({ kind: "up-to-date", current: "0.1.8" });
  });

  it("有更新返回 available，latest 保留 tag 原文（v 前缀交给 isNewer 剥）", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, status: 200, json: async () => ({ tag_name: "v0.2.0" }) })));
    expect(await checkForUpdate("0.1.8")).toEqual({ kind: "available", current: "0.1.8", latest: "v0.2.0" });
  });

  it("同版本不报更新；tag_name 缺失或空白报错误", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, status: 200, json: async () => ({ tag_name: "v0.1.8" }) })));
    expect(await checkForUpdate("0.1.8")).toEqual({ kind: "up-to-date", current: "0.1.8" });
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, status: 200, json: async () => ({}) })));
    expect(await checkForUpdate("0.1.8")).toEqual({ kind: "error", message: "响应里没有 tag_name" });
  });

  it("非 404 的非 2xx 报错误；fetch 抛错（断网 / 超时）转成 error 结果而不冒异常", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: false, status: 500, json: async () => ({}) })));
    expect(await checkForUpdate("0.1.8")).toEqual({ kind: "error", message: "GitHub 返回 500" });
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("network down"); }));
    expect(await checkForUpdate("0.1.8")).toEqual({ kind: "error", message: "network down" });
  });
});

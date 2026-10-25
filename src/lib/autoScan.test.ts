import { describe, expect, it } from "vitest";
import {
  autoScanDue,
  oldestScanTime,
  parseAutoScanPreference,
  STALE_AFTER_SECS,
} from "@/lib/autoScan";

// 启动自动重扫的边界：默认开、只按最旧分组计时、首扫不重复。
// 这三条任一条翻面，都会让「打开即看」变成「打开就啃磁盘」或让旧数据瞒过阈值。
describe("parseAutoScanPreference", () => {
  it("缺省、非法值都按开启——默认行为不能靠键存在与否来判", () => {
    expect(parseAutoScanPreference(null)).toBe(true);
    expect(parseAutoScanPreference("")).toBe(true);
    expect(parseAutoScanPreference("1")).toBe(true);
    expect(parseAutoScanPreference("yes")).toBe(true);
  });

  it("只有显式 0 才是关闭", () => {
    expect(parseAutoScanPreference("0")).toBe(false);
  });
});

describe("oldestScanTime", () => {
  it("取各分组里最旧的一个", () => {
    expect(oldestScanTime({ repos: 300, toolchain: 100, ai_tools: 200 }, 300)).toBe(100);
  });

  it("分组时间表为空（或全是坏值）时退回信封时间", () => {
    expect(oldestScanTime({}, 123)).toBe(123);
    expect(oldestScanTime({ repos: 0, toolchain: -1 }, 123)).toBe(123);
  });
});

describe("autoScanDue", () => {
  const now = 1_800_000_000;

  it("刚扫过不触发", () => {
    expect(autoScanDue(true, false, { repos: now - 60 }, now - 60, now)).toBe(false);
  });

  it("正好 24 小时不触发，超过才触发（与状态栏 stale 同为 >）", () => {
    expect(
      autoScanDue(true, false, { repos: now - STALE_AFTER_SECS }, now, now),
    ).toBe(false);
    expect(
      autoScanDue(true, false, { repos: now - STALE_AFTER_SECS - 1 }, now, now),
    ).toBe(true);
  });

  it("首次启动已经真扫过，不再补一次", () => {
    expect(autoScanDue(true, true, {}, now - 10 * STALE_AFTER_SECS, now)).toBe(false);
  });

  it("关掉设置后多旧都不触发", () => {
    expect(autoScanDue(false, false, { repos: 1 }, 1, now)).toBe(false);
  });

  it("单组刷新不重置全体时钟：信封很新但最旧分组过期，仍触发", () => {
    expect(
      autoScanDue(
        true,
        false,
        { repos: now - 3 * STALE_AFTER_SECS, ai_tools: now - 60 },
        now - 60,
        now,
      ),
    ).toBe(true);
  });
});

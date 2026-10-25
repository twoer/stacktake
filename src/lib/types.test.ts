import { beforeAll, describe, expect, it } from "vitest";
import type { AssetGroup, Item } from "@/lib/types";
import { applyLocale } from "@/lib/i18n";
import {
  artifactBytes,
  formatAgo,
  formatBytes,
  formatBytesParts,
  formatMetric,
  formatMetricBytes,
  groupReclaimable,
  groupSize,
  itemSize,
  metricValue,
  reclaimableOf,
} from "@/lib/types";

// D16 登记点名的盲区：字节换算与时间读数是账本上每一行都要过的两个纯函数，
// 还有「顶层 reclaimable 优先」「artifactBytes 只认 b 不认 i」这类口径差异。
// 这些断言钉的是中文读数口径；语言是全局状态，显式钉住，别跟着跑测试的机器走。
beforeAll(() => applyLocale("zh-CN"));

const item = (over: Partial<Item> = {}): Item => ({
  id: "/tmp/repo-a",
  tags: [],
  name: "repo-a",
  path: "/tmp/repo-a",
  metrics: {},
  ...over,
});

describe("metricValue / itemSize / artifactBytes / reclaimableOf —— 取值口径", () => {
  it("metricValue 取裸值，四种标签各一行", () => {
    expect(metricValue({ t: "s", v: "main" })).toBe("main");
    expect(metricValue({ t: "b", v: 1 })).toBe(1);
    expect(metricValue({ t: "i", v: 2 })).toBe(2);
    expect(metricValue({ t: "z", v: true })).toBe(true);
  });

  it("itemSize 认 b|i；类型不对或缺失算 0（WSL1「没有」与「0 字节」同形，是既有取舍）", () => {
    expect(itemSize(item({ metrics: { size: { t: "b", v: 100 } } }))).toBe(100);
    expect(itemSize(item({ metrics: { size: { t: "i", v: 100 } } }))).toBe(100);
    expect(itemSize(item({ metrics: { size: { t: "s", v: "100" } } }))).toBe(0);
    expect(itemSize(item())).toBe(0);
  });

  it("artifactBytes 只认 b，不认 i——比 itemSize 更严（P2 起的口径）", () => {
    expect(artifactBytes(item({ metrics: { artifacts: { t: "b", v: 7 } } }))).toBe(7);
    expect(artifactBytes(item({ metrics: { artifacts: { t: "i", v: 7 } } }))).toBe(0);
    expect(artifactBytes(item())).toBe(0);
  });

  it("reclaimableOf 顶层优先于 metrics（总览行没有顶层字段才走 metrics），顶层 0 也优先", () => {
    expect(reclaimableOf(item({ reclaimable: 5, metrics: { reclaimable: { t: "b", v: 999 } } }))).toBe(5);
    expect(reclaimableOf(item({ reclaimable: 0, metrics: { reclaimable: { t: "b", v: 999 } } }))).toBe(0);
    expect(reclaimableOf(item({ metrics: { reclaimable: { t: "b", v: 3 } } }))).toBe(3);
    expect(reclaimableOf(item({ metrics: { reclaimable: { t: "s", v: "3" } } }))).toBe(0);
    expect(reclaimableOf(item())).toBe(0);
  });

  it("groupSize 全加；groupReclaimable 只加 cleanable 行（与 Rust total_reclaimable 同口径）", () => {
    const g: AssetGroup = {
      id: "repos", label: { t: "k", v: "data.group.repos" }, summary: { t: "r", v: "" }, fields: [],
      items: [
        item({ metrics: { size: { t: "b", v: 10 } }, cleanable: true, reclaimable: 10 }),
        item({ metrics: { size: { t: "b", v: 20 } } }),
      ],
    };
    expect(groupSize(g)).toBe(30);
    expect(groupReclaimable(g)).toBe(10);
  });
});

describe("formatBytes / formatBytesParts —— 字节换算", () => {
  it("非正数归 0 B；B 段保持整数不挂小数", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-5)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
  });

  it("1024 进位、尾随 .0 抹掉、真正的非零小数保留（1.4 与 1.1 GB 必须分得出）", () => {
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1024 ** 2)).toBe("1 MB");
    expect(formatBytes(109 * 1024 ** 3)).toBe("109 GB");
    expect(formatBytes(1.4 * 1024 ** 3)).toBe("1.4 GB");
    expect(formatBytes(1.1 * 1024 ** 3)).toBe("1.1 GB");
  });

  it("TB 是最后一档，超出不再进位；上一档边界显示 1024 KB（既有怪癖，钉住）", () => {
    expect(formatBytes(1024 ** 4)).toBe("1 TB");
    expect(formatBytes(2048 * 1024 ** 4)).toBe("2048 TB");
    expect(formatBytes(1024 ** 2 - 1)).toBe("1024 KB");
  });

  it("parts 与 formatBytes 同一套换算（刻意复用，单位边界不许漂）", () => {
    expect(formatBytesParts(1024)).toEqual({ n: "1", u: "KB" });
    expect(formatBytesParts(512)).toEqual({ n: "512", u: "B" });
  });
});

describe("formatAgo —— 人话时间读数（传时间针，不依赖墙钟）", () => {
  const now = 1_800_000_000;

  it("未来与一分钟内都是「刚刚」", () => {
    expect(formatAgo(now + 10, now)).toBe("刚刚");
    expect(formatAgo(now, now)).toBe("刚刚");
    expect(formatAgo(now - 59, now)).toBe("刚刚");
  });

  it("分 / 时 / 天 / 月 / 年的档位与边界", () => {
    // 读数交给 Intl.RelativeTimeFormat：zh-CN 的数字与单位之间没有空格
    expect(formatAgo(now - 60, now)).toBe("1分钟前");
    expect(formatAgo(now - 3599, now)).toBe("59分钟前");
    expect(formatAgo(now - 3600, now)).toBe("1小时前");
    expect(formatAgo(now - 86399, now)).toBe("23小时前");
    expect(formatAgo(now - 86400, now)).toBe("1天前");
    expect(formatAgo(now - 29 * 86400, now)).toBe("29天前");
    expect(formatAgo(now - 30 * 86400, now)).toBe("1个月前");
    expect(formatAgo(now - 359 * 86400, now)).toBe("11个月前");
    expect(formatAgo(now - 365 * 86400, now)).toBe("1年前");
    expect(formatAgo(now - 730 * 86400, now)).toBe("2年前");
  });
});

describe("formatMetric / formatMetricBytes —— 按标签分派", () => {
  it("布尔走「是/否」，字符串透传，数字千分位，时间标签走 formatAgo", () => {
    expect(formatMetric({ t: "z", v: true })).toBe("是");
    expect(formatMetric({ t: "z", v: false })).toBe("否");
    expect(formatMetric({ t: "s", v: "main" })).toBe("main");
    expect(formatMetric({ t: "i", v: 1234 })).toBe("1,234");
    // temporal 路径走 formatAgo 的默认墙钟，取相对 now 的值（绝对值见 formatAgo 自己的用例）
    expect(formatMetric({ t: "i", v: Math.floor(Date.now() / 1000) - 60 }, true)).toBe("1分钟前");
  });

  it("体积列专用：b 走字节换算，其余按通用分派", () => {
    expect(formatMetricBytes({ t: "b", v: 1024 })).toBe("1 KB");
    expect(formatMetricBytes({ t: "s", v: "main" })).toBe("main");
  });
});

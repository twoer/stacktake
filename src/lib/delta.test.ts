import { beforeAll, describe, expect, it } from "vitest";
import { applyLocale } from "@/lib/i18n";
import type { Item } from "@/lib/types";
import {
  byteSizeStrict,
  deltaOf,
  formatSignedBytes,
  groupDeltaOf,
  itemDeltaOf,
  sumOf,
} from "@/lib/delta";

// 差额的口径边界（L1）：差额错了比没有差额更害人——超时行显示成假负差额
// 会让人以为目录缩了。这里钉住「严格 Bytes」「0 不渲染」「无基线不渲染」三条。

beforeAll(() => applyLocale("zh-CN"));

const item = (over: Partial<Item> = {}): Item => ({
  id: "toolchain:/home/pnpm-store",
  tags: [],
  name: "pnpm 内容存储",
  path: "/home/pnpm-store",
  metrics: {},
  ...over,
});

describe("byteSizeStrict", () => {
  it("只认严格 Bytes", () => {
    expect(byteSizeStrict(item({ metrics: { size: { t: "b", v: 100 } } }))).toBe(100);
  });
  it("Int 不是体积——itemSize 认它，这里刻意更严", () => {
    expect(byteSizeStrict(item({ metrics: { size: { t: "i", v: 100 } } }))).toBeNull();
  });
  it("超时行的 Text 与缺失的 size 都没有可比体积", () => {
    expect(byteSizeStrict(item({ metrics: { size: { t: "s", v: "—" } } }))).toBeNull();
    expect(byteSizeStrict(item())).toBeNull();
  });
});

describe("deltaOf", () => {
  it("两边都有才算差额", () => {
    expect(deltaOf(1500, 1000)).toBe(500);
    expect(deltaOf(800, 1000)).toBe(-200);
  });
  it("当前缺失或基线缺失都是 null——不渲染", () => {
    expect(deltaOf(null, 1000)).toBeNull();
    expect(deltaOf(1500, null)).toBeNull();
    expect(deltaOf(1500, undefined)).toBeNull();
  });
  it("差额为 0 是 null：界面约定 0 不渲染", () => {
    expect(deltaOf(1000, 1000)).toBeNull();
    expect(deltaOf(0, 0)).toBeNull();
  });
});

describe("formatSignedBytes", () => {
  it("带符号并复用 formatBytes 的换算", () => {
    expect(formatSignedBytes(1_572_864_000)).toBe("+1.5 GB");
    expect(formatSignedBytes(-2_097_152)).toBe("-2 MB");
    expect(formatSignedBytes(1_048_576 + 1024)).toBe("+1 MB");
  });
});

describe("sumOf", () => {
  it("跳过 null 求和", () => {
    expect(sumOf([1, null, 2])).toBe(3);
  });
  it("全 null（含空数组）是 null：没有基线不伪装成 0", () => {
    expect(sumOf([null, null])).toBeNull();
    expect(sumOf([])).toBeNull();
  });
});

describe("itemDeltaOf", () => {
  const prev = {
    item_sizes: { toolchain: { "toolchain:/home/pnpm-store": 1000 } },
  };
  it("同 id 有基线才算", () => {
    expect(
      itemDeltaOf(item({ metrics: { size: { t: "b", v: 1400 } } }), prev, "toolchain"),
    ).toBe(400);
  });
  it("新条目无基线 → null", () => {
    expect(itemDeltaOf(item({ id: "new", metrics: { size: { t: "b", v: 9 } } }), prev, "toolchain")).toBeNull();
  });
  it("基线里的超时行换成有体积了也不显示（旧值本来就没记）", () => {
    expect(
      itemDeltaOf(item({ id: "toolchain:/timeout", metrics: { size: { t: "b", v: 9 } } }), prev, "toolchain"),
    ).toBeNull();
  });
});

describe("groupDeltaOf", () => {
  it("对组基线合计求差额", () => {
    expect(groupDeltaOf(1200, { group_totals: { repos: 1000 } }, "repos")).toBe(200);
  });
  it("没有该组基线 → null", () => {
    expect(groupDeltaOf(1200, undefined, "repos")).toBeNull();
    expect(groupDeltaOf(1200, { group_totals: { toolchain: 1000 } }, "repos")).toBeNull();
  });
});

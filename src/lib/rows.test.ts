import { beforeAll, describe, expect, it } from "vitest";
import type { AssetGroup, Item, Tag } from "@/lib/types";
import { applyLocale } from "@/lib/i18n";
import {
  compareRows,
  countMatches,
  dimensionsFor,
  MAJORITY_MIN_ROWS,
  MAJORITY_RATIO,
  machineStatsOf,
  matchDim,
  matchesSearch,
  rowFullName,
  rowName,
  rowTagsOf,
  sectionReclaim,
  sectionSize,
  sectionsOf,
  shareLabelOf,
  suppressedTagsOf,
  visibleRowsOf,
} from "@/lib/rows";

// D16 拍板 A 的主战场：这些判据从前焊在 Assets.vue 里，vue-tsc 验不了。
// 重点钉四条有事故背景的语义：
// ① 分节读 group_key 数据、不解析名字（G1 起的契约，前端半边在此）；
// ② 维度「≥2 取值才出按钮」——只有一个取值的筛选等于没选；
// ③ 多数派抑制的两个护栏（行数下限 / 占比阈值）；
// ④ 菜单计数按「其它维度已筛过」口径，否则数字跟筛选结果对不上。

// 排序走当前界面语言的 Collator，钉住 zh-CN——断言不该跟着跑测试的机器走。
beforeAll(() => applyLocale("zh-CN"));

const item = (over: Partial<Item> = {}): Item => ({
  id: "/tmp/repo-a",
  tags: [],
  name: "repo-a",
  path: "/tmp/repo-a",
  metrics: {},
  ...over,
});

/** 标签夹具：与后端 `Tag::new` 同形（key 是稳定取值，label 是文案键）。 */
const tg = (key: string): Tag => ({ key, label: { t: "k", v: `data.tag.${key}` } });

const row = (it: Item) => ({ item: it });

const group = (over: Partial<AssetGroup> = {}): AssetGroup => ({
  id: "toolchain",
  label: { t: "k", v: "data.group.toolchain" },
  summary: { t: "r", v: "" },
  fields: [],
  items: [],
  ...over,
});

describe("sectionsOf / rowName —— group_key 分节契约（前端半边）", () => {
  it("全部行都有 group_key 时按键分节，节序 = 首现序", () => {
    const rows = [
      row(item({ name: "node · 20.11.0", group_key: "node" })),
      row(item({ name: "python · 3.12", group_key: "python" })),
      row(item({ name: "node · 18.20.1", group_key: "node" })),
    ];
    const secs = sectionsOf(rows);
    expect(secs.map((s) => s.key)).toEqual(["node", "python"]);
    expect(secs[0].rows).toHaveLength(2);
    expect(secs[1].rows).toHaveLength(1);
  });

  it("任一行没有 group_key 就整组不分节（repos / 鸿蒙 / WSL 的形态）", () => {
    const rows = [
      row(item({ name: "repo-a", group_key: "node" })),
      row(item({ name: "repo-b", group_key: null })),
    ];
    const secs = sectionsOf(rows);
    expect(secs).toHaveLength(1);
    expect(secs[0].key).toBeNull();
    expect(secs[0].rows).toHaveLength(2);
  });

  it("空行集合也是单个 null 节", () => {
    const secs = sectionsOf([]);
    expect(secs).toEqual([{ key: null, label: "", rows: [] }]);
  });

  it("rowName 优先 short_label，其次 label，都没有才剥「group_key · 」前缀", () => {
    expect(rowName(item({ name: "node · 20.11.0", group_key: "node" }))).toBe("20.11.0");
    expect(rowName(item({ name: "20.11.0", group_key: "node" }))).toBe("20.11.0");
    expect(rowName(item({ name: "repo-a" }))).toBe("repo-a");
    // 后端下发展示字段时不再靠切名字（英文界面下 name 里是中文词表，切不出东西）。
    // 同一份数据、不重扫，切语言即换名——国际化方案的整条前提就落在这一步上。
    applyLocale("en");
    expect(
      rowName(
        item({
          name: "Rust · Cargo 依赖缓存",
          group_key: "Rust",
          short_label: { t: "k", v: "data.item.cargoRegistry" },
        }),
      ),
    ).toBe("Cargo dependency cache");
    applyLocale("zh-CN");
    expect(
      rowName(
        item({
          name: "Rust · Cargo 依赖缓存",
          group_key: "Rust",
          short_label: { t: "k", v: "data.item.cargoRegistry" },
        }),
      ),
    ).toBe("Cargo 依赖缓存");
  });

  it("rowFullName 带回分节前缀——弹窗与失败回执只有一行文字，剥了前缀说不清是哪一行", () => {
    applyLocale("en");
    expect(
      rowFullName(
        item({
          name: "Rust · Cargo 依赖缓存",
          group_key: "Rust",
          short_label: { t: "k", v: "data.item.cargoRegistry" },
        }),
      ),
    ).toBe("Rust · Cargo dependency cache");
    applyLocale("zh-CN");
    // 没分节的行（repos）整名就是行名，不重复前缀
    expect(rowFullName(item({ name: "repo-a" }))).toBe("repo-a");
  });
});

describe("dimensionsFor —— 维度由分组声明，≥2 取值才出按钮", () => {
  it("PREFIX_LABEL 里的分组且有 ≥2 个 group_key 才出 prefix 维度（取值排序）", () => {
    const g = group({
      items: [
        item({ group_key: "python" }),
        item({ group_key: "node" }),
        item({ group_key: "node" }),
      ],
    });
    expect(dimensionsFor(g)).toEqual([
      {
        key: "prefix",
        label: "栈",
        options: [
          { value: "node", label: "node" },
          { value: "python", label: "python" },
        ],
      },
    ]);
  });

  it("prefix 维度的**取值**是 group_key 词表、**标签**是展示名（英文界面按展示名排）", () => {
    const g = group({
      items: [
        item({ group_key: "Node 生态", group_label: { t: "k", v: "data.section.node" } }),
        item({ group_key: "Node 生态", group_label: { t: "k", v: "data.section.node" } }),
        item({ group_key: "AI 运行时", group_label: { t: "k", v: "data.section.aiRuntime" } }),
      ],
    });
    applyLocale("en");
    expect(dimensionsFor(g)).toEqual([
      {
        key: "prefix",
        label: "Stack",
        options: [
          // 按展示名（"AI runtime" / "Node ecosystem"）排，不是按中文词表的码位排
          { value: "AI 运行时", label: "AI runtime" },
          { value: "Node 生态", label: "Node ecosystem" },
        ],
      },
    ]);
    applyLocale("zh-CN");
  });

  it("只有一个取值不出（选了等于没选）；不在 PREFIX_LABEL 的分组不出 prefix 维度", () => {
    expect(dimensionsFor(group({ items: [item({ group_key: "node" })] }))).toEqual([]);
    // repos 不在 PREFIX_LABEL：两百多个仓库的 group_key 全是 null，也不该出
    expect(
      dimensionsFor(group({ id: "repos", items: [item({ group_key: "a" }), item({ group_key: "b" })] })),
    ).toEqual([]);
  });

  it("kind 维度只认字符串指标，≥2 个取值才出；取值是词表、标签是展示名", () => {
    // 后端的 `metrics.kind` 存中文词表（筛选取值，与语言无关），展示走 `kind_label`
    // 文案键；同一份数据切语言只换标签、不换取值。
    const KIND_KEY: Record<string, string> = { 会话: "session", 缓存: "cache", session: "session", cache: "cache" };
    const kinds = (vs: string[]) =>
      group({
        items: vs.map((v) =>
          item({ metrics: { kind: { t: "s" as const, v } }, kind_label: { t: "k", v: `data.kind.${KIND_KEY[v]}` } }),
        ),
      });
    applyLocale("en");
    expect(dimensionsFor(kinds(["会话", "缓存"]))).toEqual([
      {
        key: "kind",
        label: "Category",
        options: [
          // 按展示名字典序：Cache 在 Sessions 前
          { value: "缓存", label: "Cache" },
          { value: "会话", label: "Sessions" },
        ],
      },
    ]);
    applyLocale("zh-CN");
    expect(dimensionsFor(kinds(["会话", "缓存"]))).toEqual([
      {
        key: "kind",
        label: "类别",
        options: [
          // 「缓存」(huǎn) 排在「会话」(huì) 前——按拼音，取值始终是中文词表
          { value: "缓存", label: "缓存" },
          { value: "会话", label: "会话" },
        ],
      },
    ]);
    expect(dimensionsFor(kinds(["会话", "会话"]))).toEqual([]);
  });

  it("tag 维度 ≥2 个取值才出；排序按**展示名**走当前语言字典序（zh-CN 是拼音，不是码位）", () => {
    expect(
      dimensionsFor(group({ items: [item({ tags: [tg("large")], name: "a" }), item({ tags: [tg("stale")], name: "b" })] })),
    ).toEqual([
      {
        key: "tag",
        label: "标签",
        options: [
          // 「陈旧」(chén jiù) 排在「大」(dà) 前——按拼音；按码位会反过来
          { value: "stale", label: "陈旧" },
          { value: "large", label: "大" },
        ],
      },
    ]);
    expect(dimensionsFor(group())).toEqual([]);
  });
});

describe("matchDim —— 三根轴的取值来源各不相同", () => {
  const it_ = item({
    group_key: "node",
    tags: [tg("large")],
    metrics: { kind: { t: "s", v: "缓存" } },
  });

  it("prefix 读 group_key；tag 读 tags[].key；kind 读 metrics.kind 原值", () => {
    expect(matchDim(it_, "prefix", "node")).toBe(true);
    expect(matchDim(it_, "prefix", "python")).toBe(false);
    expect(matchDim(it_, "tag", "large")).toBe(true);
    // 展示名不是取值：拿 label 文案去比，切换语言后筛选就会静默失效
    expect(matchDim(it_, "tag", "大")).toBe(false);
    expect(matchDim(it_, "kind", "缓存")).toBe(true);
    expect(matchDim(it_, "kind", "会话")).toBe(false);
  });

  it("认不出的维度键不拦行（不筛）", () => {
    expect(matchDim(it_, "unknown", "x")).toBe(true);
  });
});

describe("visibleRowsOf / matchesSearch / compareRows —— 搜索 + 筛选 + 排序管线", () => {
  const rows = [
    row(item({ id: "1", name: "Alpha", path: "/code/alpha", metrics: { size: { t: "b", v: 300 } } })),
    row(item({ id: "2", name: "beta", path: "/tmp/beta", metrics: { size: { t: "b", v: 100 } } })),
    row(item({ id: "3", name: "Gamma", path: "/code/gamma", metrics: { size: { t: "b", v: 200 } } })),
  ];

  it("搜索命中名字或路径，大小写与首尾空白不敏感", () => {
    expect(matchesSearch(rows[0].item, "alpha")).toBe(true);
    expect(matchesSearch(rows[1].item, "  /TMP  ")).toBe(true);
    expect(matchesSearch(rows[0].item, "beta")).toBe(false);
    expect(matchesSearch(rows[0].item, "")).toBe(true);
  });

  it("默认按体积降序", () => {
    const out = visibleRowsOf(rows, "", {}, "size", true);
    expect(out.map((r) => r.item.id)).toEqual(["1", "3", "2"]);
  });

  it("名称排序走 zh-CN 排序键：拉丁字母大小写不敏感按字母序（ICU 语义，用例钉住它）", () => {
    const asc = visibleRowsOf(rows, "", {}, "name", false);
    expect(asc.map((r) => r.item.name)).toEqual(["Alpha", "beta", "Gamma"]);
    // desc 翻转方向：zh-CN 下 Alpha 在 beta 前，desc=true 后者应排前
    expect(compareRows(rows[0], rows[1], "name", true)).toBeGreaterThan(0);
  });

  it("维度筛选与搜索叠加", () => {
    const withKeys = [
      row(item({ id: "1", name: "node-a", path: "/n/a", group_key: "node" })),
      row(item({ id: "2", name: "py-a", path: "/p/a", group_key: "python" })),
    ];
    const out = visibleRowsOf(withKeys, "a", { prefix: "node" }, null, false);
    expect(out.map((r) => r.item.id)).toEqual(["1"]);
  });
});

describe("countMatches —— 计数按「其它维度已筛过」口径", () => {
  const rows = [
    row(item({ name: "n1", path: "/x", group_key: "node", tags: [tg("large")] })),
    row(item({ name: "n2", path: "/x", group_key: "node", tags: [tg("stale")] })),
    row(item({ name: "p1", path: "/x", group_key: "python", tags: [tg("large")] })),
  ];

  it("值计数排除本维度自身、叠加其它维度与搜索", () => {
    const dims = { prefix: "node", tag: null };
    // tag 维度的「大」：在 prefix=node 的 2 行里数，不含 python 那行
    expect(countMatches(rows, "", dims, "tag", "large")).toBe(1);
    // prefix 维度的「node」：不计 prefix 自身，tag 未筛，3 行里 2 个 node
    expect(countMatches(rows, "", dims, "prefix", "node")).toBe(2);
    // null = 本维度不筛，只按其它维度数
    expect(countMatches(rows, "", dims, "tag", null)).toBe(2);
  });

  it("搜索同样作用于计数", () => {
    expect(countMatches(rows, "p1", { prefix: null }, "tag", "large")).toBe(1);
  });
});

describe("suppressedTagsOf / rowTagsOf —— 多数派抑制的两个护栏", () => {
  const rowsWith = (n: number, total: number) =>
    Array.from({ length: total }, (_, i) =>
      row(item({ id: String(i), name: String(i), path: `/${i}`, tags: i < n ? [tg("large")] : [] })),
    );

  it("可见行不足下限不抑制——筛到三两行时标签是主要信息", () => {
    expect(suppressedTagsOf(rowsWith(MAJORITY_MIN_ROWS - 1, MAJORITY_MIN_ROWS - 1))).toEqual(new Set());
  });

  it("占比恰好到阈值（0.7）抑制，差一点不抑制", () => {
    const at = suppressedTagsOf(rowsWith(7, 10));
    expect(MAJORITY_RATIO).toBe(0.7);
    expect(at.has("large")).toBe(true);
    expect(suppressedTagsOf(rowsWith(6, 10)).has("large")).toBe(false);
  });

  it("行内标签：抑制集剔除、「可清理」与绿字互斥（reclaimable=0 保留）、「大」恒不上行", () => {
    const suppressed = new Set(["artifacts"]);
    const it_ = item({
      tags: [tg("artifacts"), tg("cleanable"), tg("large"), tg("unpushed"), tg("stale")],
      cleanable: true,
      reclaimable: 100,
    });
    expect(rowTagsOf(it_, suppressed).map((tag) => tag.key)).toEqual(["unpushed", "stale"]);
    const zeroReclaim = item({ tags: [tg("cleanable"), tg("stale")], cleanable: true });
    // orderedTags 只把阻断项提前，不重排说明项之间的原有顺序
    expect(rowTagsOf(zeroReclaim, new Set()).map((tag) => tag.key)).toEqual(["cleanable", "stale"]);
  });
});

describe("sectionSize / sectionReclaim —— 小计口径", () => {
  it("可回收只累加标了 cleanable 的行（与 Rust total_reclaimable 同口径）", () => {
    const rows = [
      row(item({ metrics: { size: { t: "b", v: 100 } }, cleanable: true, reclaimable: 100 })),
      row(item({ metrics: { size: { t: "b", v: 50 } } })),
    ];
    expect(sectionSize(rows)).toBe(150);
    expect(sectionReclaim(rows)).toBe(100);
  });
});

describe("machineStatsOf —— 总览统计剔掉 overview 行", () => {
  const now = 1_800_000_000;
  const repos = group({
    id: "repos",
    items: [
      item({
        metrics: { size: { t: "b", v: 10 }, last_commit: { t: "i", v: now - 100 } },
        cleanable: true,
        reclaimable: 10,
      }),
      item({
        metrics: { size: { t: "b", v: 20 }, last_commit: { t: "i", v: now - 8 * 86400 } },
        cleanable: true,
        reclaimable: 20,
      }),
    ],
  });
  const tool = group({ id: "toolchain", items: [item({ metrics: { size: { t: "b", v: 5 } } })] });
  const overview = group({ id: "overview", items: [item({ metrics: { size: { t: "b", v: 999 } } })] });

  it("合计与分组数不含 overview；条目总数只累加真实分组；近 7 天只数 repos 且按严格小于一周", () => {
    const s = machineStatsOf([repos, tool, overview], now);
    expect(s).toEqual({ total: 35, reclaim: 30, count: 2, items: 3, recent: 1 });
  });

  it("没有 repos 组时 recent 为 0，不抛错", () => {
    expect(machineStatsOf([tool], now).recent).toBe(0);
  });
});

describe("shareLabelOf —— 占比带：≥10% 取整、≥0.5% 一位小数、再小 <0.1%", () => {
  const sized = (v: number) => item({ metrics: { size: { t: "b", v } } });

  it("三档带与空值", () => {
    expect(shareLabelOf(sized(25), 100)).toBe("25%");
    expect(shareLabelOf(sized(5), 100)).toBe("5.0%");
    expect(shareLabelOf(sized(0.4), 100)).toBe("<0.1%");
    expect(shareLabelOf(sized(0), 100)).toBe("");
  });

  it("分母为 0（全机合计 0）不除零，返回空", () => {
    expect(shareLabelOf(sized(25), 0)).toBe("");
  });
});

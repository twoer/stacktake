/**
 * 主表格的行管线：搜索 → 维度筛选 → 排序 → 分节 → 标签抑制 → 小计 → 总览统计。
 *
 * 2026-10-25 从 Assets.vue 下沉（T2，D16 拍板 A）：这半条管线是「数据的函数」，
 * 焊在视图里 vue-tsc 验不了判据（D16 登记的缺口），拆出来每一段都能喂样本。
 * 分节与筛选读 `group_key` 不解析名字（G1 起的数据契约）——Rust 侧
 * `assert_frontend_contract` 守数据的半边，前端这半边的断言在 rows.test.ts。
 * 模板零变化：Assets.vue 保留与模板同名的薄包装（rowTags / shareLabel /
 * optionCount），签名与下沉前一致。
 */
import type { AssetGroup, Item, Tag } from "@/lib/types";
import {
  groupReclaimable,
  groupSize,
  itemLabelOf,
  itemSize,
  kindKeyOf,
  kindLabelOf,
  reclaimableOf,
  sectionLabelOf,
} from "@/lib/types";
import { compareText, renderText, t, type MessageKey } from "@/lib/i18n";
import { orderedTags } from "@/lib/assets";
import type { Dimension, DimensionOption } from "@/lib/assets";

/** 一行 = 一条 item 的包装。模板以 `row.item.*` 取值，键名保持这个形状。 */
export interface Row {
  item: Item;
}

export interface Section {
  key: string | null;
  /** 分节标题的展示名（`group_label` 优先）；不分节时为空串。 */
  label: string;
  rows: Row[];
}

// ---- 搜索 ----

/**
 * 路径与**展示名**也参与匹配：搜 ~/Documents/Code 下的目录名往往比仓库名更好记；
 * 而英文界面里看到的是「npm cache」，`name` 原串仍是中文词表——只搜 `name`
 * 会让用户搜不到自己眼前那一行。
 */
export function matchesSearch(it: Item, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    it.name.toLowerCase().includes(q) ||
    itemLabelOf(it).toLowerCase().includes(q) ||
    it.path.toLowerCase().includes(q)
  );
}

// ---- 筛选维度 ----
//
// 一条 item 的维度取值在不同分组装的是不同的东西：toolchain 的 `group_key` 是
// 「栈」、ai_tools 是「工具名」、java 是「组件」。所以维度由分组声明、名字也由
// 分组给（PREFIX_LABEL）——不是同一根轴的三种叫法，是三种不同的轴。
// 分节与筛选都读 `group_key`（G1 起正式化成数据），不再解析名字里的 ` · `。
// 只有某个维度真有 ≥2 个取值时才做成筛选项：tag 只有一个「可清理」的分组，
// 做成筛选没有意义（选了等于没选），直接不出。

/**
 * 分组 → 「group_key 那根轴叫什么」的文案键。维度名由分组声明，不是同一根轴
 * 的三种叫法（toolchain 的 group_key 是栈、ai_tools 是工具名、java 是组件）。
 * `dimensionsFor` 在 computed 里调用 `t()`，切语言即重算。
 */
const PREFIX_LABEL_KEY: Record<string, MessageKey> = {
  toolchain: "dimension.prefix.toolchain",
  ai_tools: "dimension.prefix.ai_tools",
  java: "dimension.prefix.java",
  local_models: "dimension.prefix.local_models",
};

/**
 * 分节与筛选的**取值**用原始词表（`group_key` / `metrics.kind` / `tags[].key`），
 * **标签**用展示名——菜单里给人看的是「Node stack」，选中后比较的是 `group_key`
 * 那个中文串。所以每个维度项都要带 `(value, label)` 两半。
 *
 * 同一个取值在不同行里展示名相同（后端按 `group_key` 给 `group_label`），取第一条即可。
 */
export function dimensionsFor(g: AssetGroup | null): Dimension[] {
  if (!g || !g.items.length) return [];
  const dims: Dimension[] = [];

  const plKey = PREFIX_LABEL_KEY[g.id];
  if (plKey) {
    const opts = optionMap(
      g.items,
      (it): [string, string][] => (it.group_key ? [[it.group_key, sectionLabelOf(it)]] : []),
    );
    if (opts.length >= 2) dims.push({ key: "prefix", label: t(plKey), options: opts });
  }

  const kindOpts = optionMap(g.items, (it): [string, string][] => {
    const v = kindKeyOf(it);
    return v ? [[v, kindLabelOf(it)]] : [];
  });
  if (kindOpts.length >= 2) {
    dims.push({ key: "kind", label: t("dimension.kind"), options: kindOpts });
  }

  const tagOpts = optionMap(
    g.items,
    (it): [string, string][] => it.tags.map((tg): [string, string] => [tg.key, renderText(tg.label)]),
  );
  if (tagOpts.length >= 2) {
    dims.push({ key: "tag", label: t("dimension.tag"), options: tagOpts });
  }
  return dims;
}

/**
 * 收集某维度的 `(取值 → 展示名)` 去重表，按**展示名**用当前语言的字典序排。
 * 取值是词表（比较用），展示名是给人看的（排序也该按它排——英文界面里
 * 按中文词表排出来的顺序，用户看不出任何规律）。
 */
function optionMap(items: Item[], pairsFor: (it: Item) => [string, string][]): DimensionOption[] {
  const map = new Map<string, string>();
  for (const it of items) for (const [value, label] of pairsFor(it)) if (!map.has(value)) map.set(value, label);
  return [...map.entries()]
    .map(([value, label]) => ({ value, label }))
    .sort((a, b) => compareText(a.label, b.label));
}

export function matchDim(it: Item, key: string, val: string): boolean {
  if (key === "prefix") return it.group_key === val;
  if (key === "kind") return kindKeyOf(it) === val;
  if (key === "tag") return it.tags.some((tg) => tg.key === val);
  return true;
}

/** 命中「除 except 外的全部已激活维度」——菜单计数的口径是「其它维度已筛过」。 */
export function matchesDims(it: Item, dims: Record<string, string | null>, except?: string): boolean {
  return Object.entries(dims).every(([k, v]) => !v || k === except || matchDim(it, k, v));
}

// ---- 行管线：筛选 → 排序 ----

export function sortValue(it: Item, key: string): number | string {
  if (key === "name") return it.name;
  if (key === "reclaimable") return reclaimableOf(it);
  // 占比是体积的单调函数，排序直接用体积，省一次除法也避免浮点并列
  if (key === "share") return itemSize(it);
  const m = it.metrics[key];
  if (!m) return "";
  if (m.t === "i" || m.t === "b") return m.v;
  if (m.t === "z") return m.v ? 1 : 0;
  return m.v;
}

/** 数值列按数比，其余走当前界面语言的字典序；desc 翻转方向。 */
export function compareRows(a: Row, b: Row, key: string, desc: boolean): number {
  const dir = desc ? -1 : 1;
  const va = sortValue(a.item, key);
  const vb = sortValue(b.item, key);
  if (typeof va === "number" && typeof vb === "number") return (va - vb) * dir;
  return compareText(String(va), String(vb)) * dir;
}

export function visibleRowsOf(
  rows: Row[],
  query: string,
  dims: Record<string, string | null>,
  sortKey: string | null,
  sortDesc: boolean,
): Row[] {
  let out = rows.filter((r) => matchesSearch(r.item, query) && matchesDims(r.item, dims));
  if (sortKey) out = [...out].sort((a, b) => compareRows(a, b, sortKey, sortDesc));
  return out;
}

/** 菜单里每项的计数按「其它维度已经筛过」的口径算——否则数字跟筛选结果对不上。 */
export function countMatches(
  rows: Row[],
  query: string,
  dims: Record<string, string | null>,
  dimKey: string,
  value: string | null,
): number {
  return rows.filter(
    (r) =>
      matchesSearch(r.item, query) &&
      matchesDims(r.item, dims, dimKey) &&
      (value === null || matchDim(r.item, dimKey, value)),
  ).length;
}

// ---- 分节 ----

/**
 * 按 `group_key` 分节渲染：取值做小节标题（带小计），行名只显示后半截。
 * AI 工具分组一个工具就有多个类别行，名称列每行重复「WorkBuddy ·」是纯噪音。
 * 整组都没有 group_key（repos / 鸿蒙 / WSL）时走单组，不分节。
 */
export function sectionsOf(rows: Row[]): Section[] {
  if (!rows.length || !rows.every((r) => r.item.group_key)) {
    return [{ key: null, label: "", rows }];
  }
  const map = new Map<string, Row[]>();
  for (const r of rows) {
    const k = r.item.group_key!;
    if (!map.has(k)) map.set(k, []);
    map.get(k)!.push(r);
  }
  return [...map.entries()].map(([key, rs]) => ({ key, label: sectionLabelOf(rs[0].item), rows: rs }));
}

/**
 * 行名的展示形态：优先后端的 `short_label`（分节行去掉 `group_key · ` 前缀那半截），
 * 其次 `label`，都没有才按前缀切 `name`。老缓存没有这些字段，回落路径要留着。
 */
export function rowName(it: Item): string {
  if (it.short_label) return renderText(it.short_label);
  if (it.label) return renderText(it.label);
  const prefix = it.group_key ? `${it.group_key} · ` : "";
  return it.name.startsWith(prefix) ? it.name.slice(prefix.length) : it.name;
}

/**
 * 整行的完整展示名（带分节前缀），给提示、弹窗与清理失败回执用——那些地方
 * 只有一行文字，剥掉前缀就说不清「这是哪个栈下的哪个缓存」。
 */
export function rowFullName(it: Item): string {
  if (it.label) return renderText(it.label);
  const short = rowName(it);
  const sec = it.group_key ? sectionLabelOf(it) : "";
  return sec && !short.startsWith(sec) ? `${sec} · ${short}` : short;
}

// ---- 多数派标签抑制 ----
//
// 「大」在体积降序视图里几乎恒真（默认排序就是体积降序），「有产物」在本机仓库里
// 也是十之八九——这类标签每行都挂，信息量为零，还把每行撑成两行。规则是数据驱动的
// 一条：某标签在当前**可见行**里占了绝大多数，这一屏就不渲染它。
// 两个护栏：可见行太少时不抑制（筛到三两行时标签是主要信息）；只影响渲染——
// 筛选维度读的是 `item.tags` 原始数据（`dimensionsFor` / `matchDim`），菜单里照旧可筛。

export const MAJORITY_RATIO = 0.7;
export const MAJORITY_MIN_ROWS = 6;

export function suppressedTagsOf(rows: Row[]): Set<string> {
  const out = new Set<string>();
  if (rows.length < MAJORITY_MIN_ROWS) return out;
  const counts = new Map<string, number>();
  for (const r of rows) for (const tg of r.item.tags) counts.set(tg.key, (counts.get(tg.key) ?? 0) + 1);
  for (const [k, n] of counts) if (n / rows.length >= MAJORITY_RATIO) out.add(k);
  return out;
}

// ---- 标签释义 ----
//
// 标签是筛选维度的取值（Rust 侧定死，`scan/tests.rs` 断言钉着），所以不能靠
// 改名把它说清楚——只能在 hover 上补一句。补的这句只解释「这条是怎么判出来的」，
// 不重复行里已经有的数字。
//
// 两个数字（200 MB / 180 天）与 Rust 侧 `LARGE_REPO_BYTES` / `STALE_DAYS`
// （`src-tauri/src/scan/repos.rs`）对齐。改那边要同步改这里，否则提示会撒谎。

export function visibleTagsOf(it: Item, suppressed: ReadonlySet<string>): Tag[] {
  return it.tags.filter((tg) => {
    if (suppressed.has(tg.key)) return false;
    // 「可清理」chip 与绿字可回收是同一件事的两次陈述：三个可清理分组的账面上
    // reclaimable>0 ⟺ 有删除入口。绿字已经在体积列里（可回收合并进体积列，
    // 见 Assets.vue 的 splitReclaim），chip 不再挂第二行——它一挂，行就变两行，
    // 整列一多半行都是同一块灰牌子。
    // reclaimable==0 却标了可清理的异常行保留 chip；筛选维度仍读原始 tags。
    if (tg.key === "cleanable" && reclaimableOf(it) > 0) return false;
    // 「大」只是体积过阈值的别名，体积列就在同一行右边——行内再写一遍是同一事实
    // 的第二遍陈述（U15 去药丸时顺手收掉；筛选菜单仍读原始 tags，照常可按「大」筛）。
    if (tg.key === "large") return false;
    return true;
  });
}

/** 行内渲染口径：先抑制，再把阻断项提到最前——限宽截断时牺牲的是说明项。 */
export function rowTagsOf(it: Item, suppressed: ReadonlySet<string>): Tag[] {
  return orderedTags(visibleTagsOf(it, suppressed));
}

// ---- 小计 ----
//
// 分节小计与状态栏「可见合计」是同一口径的两处显示，共用这两个归约；
// 可回收只累加标了 cleanable 的行，与 Rust 侧 total_reclaimable 同口径。

export function sectionSize(rows: Row[]): number {
  return rows.reduce((s, r) => s + itemSize(r.item), 0);
}

export function sectionReclaim(rows: Row[]): number {
  return rows.reduce((s, r) => s + (r.item.cleanable ? reclaimableOf(r.item) : 0), 0);
}

// ---- 总览统计 ----

/** 总览首块「本机合计」：总占用 / 可回收 / 分组数 / 条目总数 / 近 7 天有提交。
 *  nowSecs 由调用方传（Assets 的 now 是跟着时间针走的 ref），本模块保持纯函数可测。 */
export function machineStatsOf(
  groups: AssetGroup[],
  nowSecs: number,
): { total: number; reclaim: number; count: number; items: number; recent: number } {
  const real = groups.filter((g) => g.id !== "overview");
  const total = real.reduce((s, g) => s + groupSize(g), 0);
  const reclaim = real.reduce((s, g) => s + groupReclaimable(g), 0);
  const items = real.reduce((s, g) => s + g.items.length, 0);
  const repos = real.find((g) => g.id === "repos");
  const week = 7 * 86400;
  const recent = repos
    ? repos.items.filter((it) => {
        const m = it.metrics.last_commit;
        return m && (m.t === "b" || m.t === "i") && nowSecs - m.v < week;
      }).length
    : 0;
  return { total, reclaim, count: real.length, items, recent };
}

/**
 * 总览「占比」列（U15）的标注：前端派生列，不是 Rust 下发的 field——fields 契约
 * （最多三个）不动。分母 total 由调用方给（machineStats.total），不是当前可见
 * 行之和：筛掉几行后占比不该跟着变（它说的是「占整机多少」）。
 * 标注档位沿用原占比带：≥10% 取整、≥0.5% 一位小数、再小只写 <0.1%。
 */
export function shareLabelOf(it: Item, total: number): string {
  const share = total > 0 ? itemSize(it) / total : 0;
  if (share <= 0) return "";
  if (share >= 0.1) return `${Math.round(share * 100)}%`;
  if (share >= 0.005) return `${(share * 100).toFixed(1)}%`;
  return "<0.1%";
}

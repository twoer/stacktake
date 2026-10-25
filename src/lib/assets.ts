/**
 * 表格与检查器共用的行渲染辅助（2026-10-24 拆分计划阶段 0 从 Assets.vue 下沉）。
 *
 * `cell` 与 `artifactRowsOf` 各有两个消费方：主表格（Assets.vue）与检查器
 * （后续拆出的 InspectorPanel）。「渲染一条 item 的某列 / 一条 item 的产物行」
 * 是数据形状的函数，不是某个组件的私事，放 lib 让两边 import 同一份。
 */
import type { ArtifactEntry, AssetGroup, Item, Tag } from "@/lib/types";
import {
  artifactBytes,
  formatMetric,
  formatMetricBytes,
  kindKeyOf,
  kindLabelOf,
  reclaimableOf,
} from "@/lib/types";
import { t, type MessageKey } from "@/lib/i18n";
import { isLocalPath } from "@/lib/path";

export function cell(it: Item, key: string, temporal?: boolean): string {
  const m = it.metrics[key];
  if (!m) return "—";
  if (key === "size") {
    // 0 B 在本地模型这类分组里是「账面为 0」（模型缺失、只留了元数据），一排
    // `0 B` 看着像扫描失败。改成与空值同形态的「—」，排序仍按 0 走，不受影响。
    if ((m.t === "b" || m.t === "i") && m.v <= 0) return "—";
    return formatMetricBytes(m);
  }
  // 类别列显示展示名，不显示 `metrics.kind` 的原值：那个中文词表是筛选与分节
  // 的**取值**（语言无关才正确），给人看的是 `kind_label`。老缓存没有
  // kind_label，kindLabelOf 会退回原值，与改前一致。
  if (key === "kind") return kindLabelOf(it) || "—";
  return formatMetric(m, temporal);
}

export type ArtifactRow = ArtifactEntry & { dir: string; base: string };

export function artifactRowsOf(item: Item | null): ArtifactRow[] {
  return (item?.artifact_entries ?? []).map((entry) => {
    const rel = entry.relative_path;
    const slash = Math.max(rel.lastIndexOf("/"), rel.lastIndexOf("\\"));
    return {
      ...entry,
      dir: slash < 0 ? "" : rel.slice(0, slash + 1),
      base: slash < 0 ? rel : rel.slice(slash + 1),
    };
  });
}

/** 筛选维度：一条 item 的维度取值在不同分组装的是不同的东西（toolchain 的
 *  group_key 是「栈」、ai_tools 是「工具名」…），维度由分组声明。
 *
 * `options` 每项两半：`value` 是后端词表里的取值（比较用它，语言无关），
 * `label` 是给人看的展示名（P2 起由后端 `Text` 下发，切语言只重渲染）。 */
export interface DimensionOption {
  value: string;
  label: string;
}

export interface Dimension {
  key: string;
  label: string;
  options: DimensionOption[];
}

/**
 * 标签 → 悬停提示的键。按标签的 **`key`**（ASCII，Rust 侧 `Tag::new` 定死）映射，
 * 不按展示名——展示名现在随语言变，用它当键会在英文界面下一条提示都给不出。
 */
const TAG_HINT_KEYS: Record<string, MessageKey> = {
  large: "tagHint.large",
  stale: "tagHint.stale",
  unpushed: "tagHint.unpushed",
  modified: "tagHint.modified",
  stashed: "tagHint.stashed",
  nested: "tagHint.nested",
  artifacts: "tagHint.artifacts",
  cleanable: "tagHint.cleanable",
};

/** 认不出的标签不给提示（Hint 没文案就不挂实例），宁可不说话也别瞎解释。 */
export function tagHint(tag: Tag): string | undefined {
  const key = TAG_HINT_KEYS[tag.key];
  return key ? t(key) : undefined;
}

/**
 * 两档墨色的分界：这三条意味着「整仓清理会被拒」（repos 复核的 fail-closed 三态），
 * 是行动预警；嵌套 / 有产物 / 陈旧 / 大只是口径说明。预警不能长得跟脚注一样——
 * 落的是「值近黑、元数据灰」这条层级原则（Assets.vue 数据列注释），不引入新颜色。
 * 名单钉的是标签的 `key`（Rust 侧 `Tag::new` 的字面量），新增阻断性标签时在这里登记。
 */
export const BLOCKER_TAGS: ReadonlySet<string> = new Set(["unpushed", "modified", "stashed"]);

export function isBlockerTag(tag: Tag): boolean {
  return BLOCKER_TAGS.has(tag.key);
}

/** 阻断项排最前：标签段限宽会静默截断，被裁掉的必须是说明项，不能是预警。 */
export function orderedTags(tags: Tag[]): Tag[] {
  return [...tags.filter(isBlockerTag), ...tags.filter((tag) => !isBlockerTag(tag))];
}

/**
 * 「清理」按钮的判据（2026-10-24 随检查器拆分下沉，表格无关）。三个条件都要
 * 满足：本组有删除实现（后端下发的 `supports_clean`）、行本身 `cleanable`、
 * 且是宿主机路径。
 *
 * **为什么不写死分组 id**（S3 起）：S2 之前判的是 `id === "repos"`。加第二个
 * 分组（toolchain）就得去改那个字符串，漏改的后果是「界面给按钮、服务端拒绝」
 * 或反过来——后者更坏，用户会以为这个工具没有清理功能。能力的唯一真相在
 * Rust 侧（`Scanner::supports_clean` → `AssetGroup` 字段）。
 *
 * 行级 `cleanable` 与分组级是两个粒度：toolchain 组 `supports_clean` 为 true，
 * 但有官方清理命令的那 6 条 `cleanable` 是 false——它们要用人自己的命令清，
 * 而 `pnpm store prune` 只删没被引用的包，比搬走整个目录精确。
 */
export function canCleanItem(group: AssetGroup | null, item: Item): boolean {
  return !!group?.supports_clean && !!item.cleanable && isLocalPath(item.path);
}

/**
 * 批量清理的开放范围（2026-10-25 拍板）：只做整条目，且先开 toolchain /
 * ai_tools。repos 整仓保持单次确认——一次勾十个仓库一起删，确认弹窗能承载
 * 的注意力不够；构建产物跨仓库的范围口径也不同（相对路径按仓库根解析）。
 *
 * **这不是「能不能删」的判据**：那是 `supports_clean`（组）+ `Item.cleanable`
 * （行），真相在 Rust 侧。这里只是「给不给多选界面」的产品范围——后端
 * `clean_items` 对 `item_ids` 本来就按数组逐项复核，放宽范围不会出现
 * 「界面给按钮、服务端拒绝」。
 */
export const BATCH_CLEAN_GROUP_IDS: ReadonlySet<string> = new Set(["toolchain", "ai_tools"]);

/** 批量清理确认弹窗的目标：打开那一刻锁定的组与条目快照。 */
export interface BatchCleanTarget {
  groupId: string;
  items: Item[];
}

export function supportsBatchClean(group: AssetGroup | null): boolean {
  return !!group?.supports_clean && BATCH_CLEAN_GROUP_IDS.has(group.id);
}

/** 批量模式可勾选的行：与单条清理同一判据（`canCleanItem`），不另开一套。 */
export function batchCleanRows(group: AssetGroup | null, items: Item[]): Item[] {
  if (!supportsBatchClean(group)) return [];
  return items.filter((it) => canCleanItem(group, it));
}

/** 已选条目的可回收合计；只计仍在可勾选集合里的 id（筛掉的不会被算进来）。 */
export function batchReclaimOf(
  group: AssetGroup | null,
  items: Item[],
  selected: ReadonlySet<string>,
): number {
  return batchCleanRows(group, items)
    .filter((it) => selected.has(it.id))
    .reduce((sum, it) => sum + reclaimableOf(it), 0);
}

/**
 * 可见集合变化后裁剪选择。筛选/搜索隐藏的条目不能留在选中集里——确认弹窗
 * 列的是快照，但「选中了但看不见」本身就是误删风险；宁可在筛选时丢掉选择。
 * 集合没变化时返回原引用，避免 watch 里无谓地触发渲染。
 */
export function pruneSelection(
  selected: ReadonlySet<string>,
  allowed: ReadonlySet<string>,
): ReadonlySet<string> {
  let changed = false;
  const out = new Set<string>();
  for (const id of selected) {
    if (allowed.has(id)) out.add(id);
    else changed = true;
  }
  return changed ? out : selected;
}

/**
 * 「清理产物」的判据与 `canCleanItem` 是两个动作：整仓删除看 `cleanable`
 * （复核未推送/改动），产物清理看 `metrics.artifacts`（复核白名单）。
 * 没有 `supports_clean` 的组、非宿主机路径一律不给。
 */
export function canCleanArtifactsItem(group: AssetGroup | null, item: Item): boolean {
  return (
    !!group?.supports_clean &&
    artifactBytes(item) > 0 &&
    !!item.artifact_entries?.length &&
    isLocalPath(item.path)
  );
}

/**
 * 清理确认弹窗「会怎样」文案的性质分岔。按分组 id 分岔的只是**呈现语气**——
 * 删仓库丢源码、删缓存只花重下的时间、删 AI 数据里的会话丢历史；能力判据仍在
 * Rust 侧（`supports_clean` → `canCleanItem`），这里登记的永远是文案不是权限。
 * 新开清理入口的分组若不登记，会落到「缓存」措辞——那句话对它未必准，登记一下。
 */
export type CleanKind = "repo" | "ai" | "cache";

const CLEAN_KIND_BY_GROUP: Readonly<Record<string, CleanKind>> = {
  repos: "repo",
  ai_tools: "ai",
};

export function cleanKindOf(groupId: string | null): CleanKind {
  return (groupId && CLEAN_KIND_BY_GROUP[groupId]) || "cache";
}

/** AI 工具行的类别标签（展示名，`kind_label` 优先）；缺失时给中性词，不猜。 */
export function aiKindLabel(item: Item | null): string {
  return kindLabelOf(item) || t("common.data");
}

/**
 * 这一行是不是「会话」类（删了不会再生，确认弹窗要单独说这一条）。
 *
 * 比的是 `metrics.kind` 的**原始值**，不是展示名——展示名现在随语言变，
 * `aiKindLabel(it) === "会话"` 在英文界面下永远为假，那条警告就静默消失了。
 * 原始值是后端词表（`ai_tools.rs` 的 `Category::label()`），与语言无关。
 */
export function isSessionKind(item: Item | null | undefined): boolean {
  return kindKeyOf(item) === "会话";
}

/**
 * 表格与检查器共用的行渲染辅助（2026-10-24 拆分计划阶段 0 从 Assets.vue 下沉）。
 *
 * `cell` 与 `artifactRowsOf` 各有两个消费方：主表格（Assets.vue）与检查器
 * （后续拆出的 InspectorPanel）。「渲染一条 item 的某列 / 一条 item 的产物行」
 * 是数据形状的函数，不是某个组件的私事，放 lib 让两边 import 同一份。
 */
import type { ArtifactEntry, AssetGroup, Item } from "@/lib/types";
import { artifactBytes, formatMetric, formatMetricBytes } from "@/lib/types";
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
 *  group_key 是「栈」、ai_tools 是「工具名」…），维度由分组声明。 */
export interface Dimension {
  key: string;
  label: string;
  options: string[];
}

/** 标签 → 悬停提示的文案。表格行与检查器共用（2026-10-24 随检查器拆分下沉）。 */
const TAG_HINTS: Record<string, string> = {
  大: "体积超过 200 MB",
  陈旧: "超过 180 天没有提交",
  未推送:
    "有未推送提交，或推送状态核查不出来（没配 upstream / 游离 HEAD / git 失败）。核查不出的按有未推送处理，整仓删除会被拒绝",
  有改动: "工作区有未提交改动，整仓删除会被拒绝",
  "有 stash": "git stash 非空。stash 藏在 .git 里，整仓进废纸篓会连它一起没",
  嵌套: "位于另一个仓库目录内；它的体积已从父仓库扣除，不重复计入",
  有产物: "含 node_modules / target / dist 等构建产物目录，可单独清理，不动源码",
  可清理: "有删除入口：移入废纸篓，可从废纸篓恢复",
};

/** 认不出的标签不给提示（Hint 没文案就不挂实例），宁可不说话也别瞎解释。 */
export function tagHint(t: string): string | undefined {
  return TAG_HINTS[t];
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

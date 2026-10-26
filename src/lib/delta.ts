/**
 * 差额台账的纯函数（L1 / D18）。基线在 Rust 侧轮转（`cache::rotate`），
 * 这里只负责「拿当前值与基线算差额、渲染成文案」。
 *
 * **体积判定用严格 Bytes**（同 `artifactBytes` 只认 `b` 的口径，比 `itemSize`
 * 认 `b`/`i` 更严）：repos 遍历超时行写的是 `Text("—")`，`itemSize` 对它返回
 * 0——拿 0 当体积算差额会把超时行显示成假负差额。没有可比体积就是没有差额，
 * 两边（`cache::PrevLedger` 只记 Bytes）一致地不显示。
 */

import { formatBytes } from "@/lib/types";
import type { Item, PrevLedger } from "@/lib/types";

/** 条目体积，严格 Bytes。`Text` / `Int` / 缺失都不是体积 → null。 */
export function byteSizeStrict(it: Item): number | null {
  const m = it.metrics.size;
  return m && m.t === "b" ? m.v : null;
}

/**
 * 差额：两边都有可比值才算；**0 不算差额**——界面约定「差额为 0 不渲染」
 * （D18），调用方拿 null 与 undefined 同义处理（都不渲染）。
 */
export function deltaOf(
  current: number | null,
  baseline: number | null | undefined,
): number | null {
  if (current === null || baseline == null) return null;
  const d = current - baseline;
  return d === 0 ? null : d;
}

/** 带符号体积文案：`+1.2 GB` / `-512 MB`。换算复用 `formatBytes`——两处各算
 *  一次，单位边界迟早会漂（`formatBytesParts` 同一条教训）。 */
export function formatSignedBytes(delta: number): string {
  return (delta > 0 ? "+" : "-") + formatBytes(Math.abs(delta));
}

/** 求和，跳过 null；全 null（含空数组）返回 null——「没有基线」不该伪装成 0。 */
export function sumOf(values: ReadonlyArray<number | null>): number | null {
  let total: number | null = null;
  for (const v of values) {
    if (v !== null) total = (total ?? 0) + v;
  }
  return total;
}

/** 条目级差额：当前体积对该组基线里的同 id 条目。新条目无基线 → null。 */
export function itemDeltaOf(
  it: Item,
  prev: PrevLedger | null | undefined,
  groupId: string,
): number | null {
  return deltaOf(byteSizeStrict(it), prev?.item_sizes?.[groupId]?.[it.id]);
}

/** 整组差额：当前合计对该组基线合计。 */
export function groupDeltaOf(
  groupSize: number,
  prev: PrevLedger | null | undefined,
  groupId: string,
): number | null {
  return deltaOf(groupSize, prev?.group_totals?.[groupId]);
}

/**
 * 启动自动重扫的策略。设置页的勾选只决定「要不要」，这里的判据决定「够不够旧」。
 *
 * 默认开启（只有显式写 "0" 才关）：缓存的用途是「打开即看」，不是「永远不更新」；
 * 24 小时是给「每天打开一次」的人兜底，不是给「每次启动都重扫」开的口子。
 */
export const AUTO_SCAN_STORAGE_KEY = "stacktake-auto-scan";

/** 与 Assets 状态栏「建议重扫」共用同一个阈值，别在两处各写一个 86400。 */
export const STALE_AFTER_SECS = 24 * 3600;

/** 缺省、非法值都按开启——它是默认行为；用户显式关掉才会写入 "0"。 */
export function parseAutoScanPreference(raw: string | null): boolean {
  return raw !== "0";
}

/**
 * 快照的「最旧分组」时间。信封级 saved_at 会被单组重扫刷新成当前时间，
 * 拿它判全体新鲜度会让其余旧分组永远等不到自动重扫。
 */
export function oldestScanTime(
  groupSavedAt: Record<string, number>,
  savedAt: number | null,
): number | null {
  const times = Object.values(groupSavedAt).filter((t) => Number.isFinite(t) && t > 0);
  return times.length ? Math.min(...times) : savedAt;
}

/** 启动时是否该自动重扫一次。`bootScanned` 为真（无缓存首扫）时不重复触发。 */
export function autoScanDue(
  enabled: boolean,
  bootScanned: boolean,
  groupSavedAt: Record<string, number>,
  savedAt: number | null,
  nowSecs: number,
): boolean {
  if (!enabled || bootScanned) return false;
  const oldest = oldestScanTime(groupSavedAt, savedAt);
  return oldest != null && nowSecs - oldest > STALE_AFTER_SECS;
}

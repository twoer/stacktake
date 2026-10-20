/**
 * 版本检查：查 GitHub 上最新的 release tag，只告诉用户「有没有新版」，
 * 不下载、不安装、不校验签名——那是 C 方案（tauri-plugin-updater）的事，
 * 本机没有 Apple Developer ID，也不打算背一把「丢了就永久无法给存量用户
 * 推送更新」的 minisign 私钥。
 *
 * 这是本工具**唯一一次对外网络请求**，也是它唯一一次离开本机：
 * GET api.github.com，请求体为空，URL 里只有仓库名，不带任何本机数据。
 * 代价是 GitHub 会看到一次来源 IP（未认证 API 60 次/小时/IP，够用）。
 * 想彻底掐掉：不调用 checkForUpdate 即可，代码里没有别的网络出口。
 *
 * tauri.conf.json 的 CSP 为此加了 `connect-src ... https://api.github.com`。
 * 不开会被 `default-src 'self'` 拦掉，而且是**静默失败**——控制台一行 CSP
 * 报错，界面上什么都没有，看起来就像网络不通。改 CSP 时别只删这一句。
 */

const REPO = "twoer/stacktake";
const LATEST_API = `https://api.github.com/repos/${REPO}/releases/latest`;
/** 跳的是页面不是资源：让用户自己在 GitHub 上挑 dmg / exe 下载。 */
export const RELEASES_URL = `https://github.com/${REPO}/releases/latest`;

/** 启动那次检查不能拖慢首屏，也别在断网时干等。 */
const TIMEOUT_MS = 6000;

export type CheckResult =
  | { kind: "up-to-date"; current: string }
  | { kind: "available"; current: string; latest: string }
  | { kind: "error"; message: string };

/** 剥掉两侧可能不一致的 `v` 前缀（tag 写 v0.2.0，getVersion() 回 0.1.0）。 */
function segments(v: string): number[] {
  return v
    .replace(/^v/i, "")
    .split(".")
    .map((n) => Number.parseInt(n, 10) || 0);
}

/** 只比数字段，不认预发布后缀（v1.0.0-beta 视同 1.0.0）——本项目的 tag 不搞这套。 */
export function isNewer(latest: string, current: string): boolean {
  const a = segments(latest);
  const b = segments(current);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const d = (a[i] ?? 0) - (b[i] ?? 0);
    if (d !== 0) return d > 0;
  }
  return false;
}

export async function checkForUpdate(current: string): Promise<CheckResult> {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), TIMEOUT_MS);
  try {
    const res = await fetch(LATEST_API, {
      signal: ctrl.signal,
      headers: { Accept: "application/vnd.github+json" },
    });
    // 404 的语义是「这个仓库还没有 release」，不是出错：仓库刚公开、tag 还没推
    // 上去、或网络被代理挡了都可能是这个码。此时按「没有可用更新」处理，
    // 让启动那次检查彻底无声；真要排查，设置页手动点一次能看到具体状态。
    if (res.status === 404) return { kind: "up-to-date", current };
    if (!res.ok) return { kind: "error", message: `GitHub 返回 ${res.status}` };
    const data = (await res.json()) as { tag_name?: string };
    const latest = data.tag_name?.trim();
    if (!latest) return { kind: "error", message: "响应里没有 tag_name" };
    return isNewer(latest, current)
      ? { kind: "available", current, latest }
      : { kind: "up-to-date", current };
  } catch (e) {
    // 断网 / 超时 / CSP 拦截都落在这里，由调用方决定要不要显示。
    return { kind: "error", message: e instanceof Error ? e.message : String(e) };
  } finally {
    clearTimeout(timer);
  }
}

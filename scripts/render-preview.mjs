// 渲染台：把 dist/ 构建产物用真实扫描缓存跑起来，供无头浏览器截图。
// 为什么要有它：UI 改动的验收始终需要人眼，而 Tauri 窗口不在浏览器里；
// 这份桩让前端在浏览器里拿到与本机 app 同一份 bootstrap 数据，所见即 app 所见
// （D15 勘测用过同类手段，这里落成常驻脚本）。
//
// 用法：
//   node scripts/render-preview.mjs            # 起服务，打印可截图的地址
//   然后另开终端用 Chrome headless 截图：
//   "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \
//     --disable-gpu --hide-scrollbars --window-size=1180,760 \
//     --screenshot=/tmp/overview.png "http://127.0.0.1:4173/#/assets/overview"
//
// 桩的口径：invoke 只认 bootstrap（喂真实缓存 + 现算总览，与 Rust 的
// synthesize_overview 同构）与 plugin:app|version；listen/set_theme 一律
// resolve(null)——进度事件在浏览器里本来就不会来。
import { createServer } from "node:http";
import { readFileSync, existsSync } from "node:fs";
import { extname, join, normalize } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("../dist", import.meta.url));
const CACHE =
  process.env.STACKTAKE_CACHE ??
  join(homedir(), "Library/Application Support/com.twoer.stacktake/scan-cache.json");

if (!existsSync(join(ROOT, "index.html"))) {
  console.error("dist/ 不存在——先 npm run build");
  process.exit(1);
}
if (!existsSync(CACHE)) {
  console.error(`找不到扫描缓存 ${CACHE}（可用 STACKTAKE_CACHE 覆盖）`);
  process.exit(1);
}

const cache = JSON.parse(readFileSync(CACHE, "utf8"));

// 与 src-tauri/src/scan/mod.rs::synthesize_overview 同构：纯函数、不碰磁盘。
// 口径漂移的风险由「体积/可回收取自同一份缓存」压住；字段表抄自 Rust（体积/可回收/条目）。
function synthesizeOverview(groups) {
  const total = (g) => g.items.reduce((s, it) => s + (it.metrics?.size?.v ?? 0), 0);
  const reclaim = (g) =>
    g.items.reduce((s, it) => s + (it.cleanable ? (it.reclaimable ?? 0) : 0), 0);
  const items = groups.map((g) => ({
    id: `overview:${g.id}`,
    tags: [],
    name: g.label,
    group_key: null,
    path: `overview://${g.id}`,
    metrics: {
      size: { t: "b", v: total(g) },
      reclaimable: { t: "b", v: reclaim(g) },
      count: { t: "i", v: g.items.length },
    },
    detail: null,
    cleanable: false,
    reclaimable: null,
  }));
  const sum = (f) => groups.reduce((s, g) => s + f(g), 0);
  const hb = (n) => {
    if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
    if (n >= 1024 ** 2) return `${(n / 1024 ** 2).toFixed(1)} MB`;
    return `${(n / 1024).toFixed(1)} KB`;
  };
  return {
    id: "overview",
    label: "总览",
    summary: `${groups.length} 个分组 · 合计 ${hb(sum(total))} · 可回收 ${hb(sum(reclaim))}`,
    fields: [
      { key: "size", label: "体积", numeric: true, temporal: null },
      { key: "reclaimable", label: "可回收", numeric: true, temporal: null },
      { key: "count", label: "条目", numeric: true, temporal: null },
    ],
    items,
    notes: [],
    supports_clean: false,
  };
}

const boot = {
  groups: [synthesizeOverview(cache.groups), ...cache.groups],
  saved_at: cache.saved_at,
};

// 模拟 macOS 侧栏 vibrancy 材质：浅色 236 灰 / 深色 65 灰（U12、U13 真机实测值）。
// 没有这一层，透明 body 下的侧栏会压在无头浏览器的白底上，深色模式侧栏文字看不见——
// 那是桩环境的假象，不是 app 的 bug；补上材质才能让侧栏截图与真机同构。
const STUB = `<style>html.macos{background:#ececec}html.macos.dark{background:#414141}</style>
<script>
// 地址带 ?dark（如 /?dark#/assets/repos）即强制深色：useTheme 读的就是这个键，
// 且 index.html 的首帧判色脚本在 body 里、晚于本桩执行，同帧生效不闪。
if (location.search.includes("dark")) localStorage.setItem("stacktake-theme", "dark");
else localStorage.removeItem("stacktake-theme");
window.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
  transformCallback(cb) { (this._cbs ??= {}); this._cbs[Math.random()] = cb; return 0; },
  invoke(cmd) {
    if (cmd === "bootstrap") return Promise.resolve(${JSON.stringify(boot)});
    if (cmd === "plugin:app|version") return Promise.resolve("0.1.0");
    return Promise.resolve(null);
  },
};
</script>`;

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
  ".json": "application/json",
};

createServer((req, res) => {
  let path = normalize(decodeURIComponent((req.url ?? "/").split("?")[0]));
  if (path.includes("..")) path = "/";
  let file = join(ROOT, path);
  if (!existsSync(file) || extname(file) === "") file = join(ROOT, "index.html");
  let body = readFileSync(file);
  if (file.endsWith("index.html")) {
    // 桩必须在模块脚本之前同步执行：加在 <head> 最前面
    body = body.toString().replace("<head>", "<head>" + STUB);
  }
  res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
  res.end(body);
}).listen(4173, "127.0.0.1", () => {
  console.log("渲染台就绪：");
  console.log("  总览   http://127.0.0.1:4173/#/assets/overview");
  console.log("  分组   http://127.0.0.1:4173/#/assets/repos");
  console.log("  设置   http://127.0.0.1:4173/#/settings");
});

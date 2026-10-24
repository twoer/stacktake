/**
 * 截图基线台（2026-10-24 拆分回路的配套设施）。
 *
 * 为什么不用真 app 截图：dist 产物在纯浏览器里跑不起 invoke，首屏只会停在
 * 「扫描失败」——表格 / 弹窗 / 检查器都出不来。mockIPC + 夹具把后端钉死成一份
 * 确定数据，前后两轮截图的差异才只可能来自前端改动本身。
 *
 * 只进 dist-shot（vite.screenshot.config.ts 独立构建），不进 tauri 的 dist。
 * 夹具全部用占位名（repo-a / WorkBuddy / fixture 用户），不写本机可识别的
 * 真实名与路径（AGENTS 代码风格约束）。fields 形状对照各扫描器的真实声明：
 * repos（体积 / 最后提交 / 分支）、toolchain（体积 / 最后修改 / 类别）、
 * ai_tools（体积 / 最后活动 / 类别）、local_models（体积 / 最后修改）。
 */
import { mockIPC } from "@tauri-apps/api/mocks";
import { createApp } from "vue";
import App from "@/App.vue";
import { router } from "@/router";
import { formatBytes, groupReclaimable, groupSize } from "@/lib/types";
import type {
  AssetGroup,
  BootPayload,
  Field,
  Item,
  MetricValue,
  RepoRoots,
  ScanResult,
} from "@/lib/types";
import "@/style.css";

// 截图确定性：桩掉 prefers-reduced-motion。JS 动效（motion.ts 的 tweenNumber /
// playRowFlip）都尊重它，桩成 reduce 后数字补间直接落终值——后台窗口里 rAF 不走，
// 补间会冻在中途值，两次截图就不可比。CSS 动画不受 JS 桩影响，由采集侧的
// getAnimations().finish() 收尾。只桩 reduced-motion 这一个查询，颜色等其余原样透传。
const nativeMatchMedia = window.matchMedia.bind(window);
window.matchMedia = (query: string): MediaQueryList => {
  const mql = nativeMatchMedia(query);
  if (!query.includes("prefers-reduced-motion")) return mql;
  return {
    matches: true,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  } as MediaQueryList;
};

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;
const GB = 1024 ** 3;
const MB = 1024 ** 2;

const bytes = (v: number): MetricValue => ({ t: "b", v });
const text = (v: string): MetricValue => ({ t: "s", v });
const count = (v: number): MetricValue => ({ t: "i", v });
const ago = (days: number) => NOW - days * DAY;

function field(key: string, label: string, numeric = false, temporal = false): Field {
  return { key, label, numeric: numeric || undefined, temporal: temporal || undefined };
}

type ItemSpec = Omit<Item, "tags"> & { tags?: string[] };

function item(spec: ItemSpec): Item {
  return { tags: [], ...spec };
}

const HOME = "C:\\Users\\fixture";

// ---- repos：既有可清理行（弹干净弹窗）、也有带产物的行（弹产物弹窗）----

const repos: AssetGroup = {
  id: "repos",
  label: "Git 仓库",
  summary: "6 个仓库 · 2 个有构建产物",
  fields: [
    field("size", "体积", true),
    field("last_commit", "最后提交", true, true),
    field("branch", "分支"),
  ],
  items: [
    item({
      id: "X:\\dev\\fixtures\\repo-a",
      name: "repo-a",
      path: "X:\\dev\\fixtures\\repo-a",
      tags: ["大", "有产物"],
      metrics: {
        size: bytes(2.4 * GB),
        last_commit: count(ago(2)),
        branch: text("main"),
        artifacts: bytes(1.31 * GB),
      },
      artifact_entries: [
        { relative_path: "node_modules", size_bytes: 1.28 * GB },
        { relative_path: "apps/desktop/.output/server/node_modules", size_bytes: 64.1 * MB },
        { relative_path: "dist", size_bytes: 58 * MB },
      ],
      detail: {
        "1. 未推送": "无",
        "2. 工作区": "干净",
        "3. 说明": "构建产物目录 3 个，可单独清理（源码与 .git 保留）",
      },
    }),
    item({
      id: "X:\\dev\\fixtures\\repo-b",
      name: "repo-b",
      path: "X:\\dev\\fixtures\\repo-b",
      tags: ["可清理"],
      metrics: {
        size: bytes(812 * MB),
        last_commit: count(ago(5)),
        branch: text("feature/login"),
      },
      cleanable: true,
      reclaimable: 812 * MB,
    }),
    item({
      id: "X:\\dev\\fixtures\\repo-c",
      name: "repo-c",
      path: "X:\\dev\\fixtures\\repo-c",
      tags: ["大", "陈旧"],
      metrics: {
        size: bytes(3.1 * GB),
        last_commit: count(ago(200)),
        branch: text("master"),
      },
    }),
    item({
      id: "X:\\dev\\fixtures\\repo-docs",
      name: "repo-docs",
      path: "X:\\dev\\fixtures\\repo-docs",
      metrics: {
        size: bytes(12 * MB),
        last_commit: count(ago(30)),
        branch: text("main"),
      },
    }),
    item({
      id: "X:\\dev\\fixtures\\repo-legacy",
      name: "repo-legacy",
      path: "X:\\dev\\fixtures\\repo-legacy",
      tags: ["可清理", "陈旧"],
      metrics: {
        size: bytes(402 * MB),
        last_commit: count(ago(400)),
        branch: text("develop"),
      },
      cleanable: true,
      reclaimable: 402 * MB,
    }),
    item({
      id: "X:\\dev\\fixtures\\repo-web",
      name: "repo-web",
      path: "X:\\dev\\fixtures\\repo-web",
      tags: ["有产物"],
      metrics: {
        size: bytes(1.1 * GB),
        last_commit: count(ago(1)),
        branch: text("main"),
        artifacts: bytes(704 * MB),
      },
      artifact_entries: [
        { relative_path: "node_modules", size_bytes: 704 * MB },
        { relative_path: ".next", size_bytes: 0 },
      ],
    }),
  ],
  notes: ["扫描根目录：X:\\dev\\fixtures"],
  supports_clean: true,
};

// ---- toolchain：按栈分节（group_key）；有官方命令的行不标可清理 ----

const toolchain: AssetGroup = {
  id: "toolchain",
  label: "工具链缓存",
  summary: "7 条缓存 · 其中 2 条有官方清理命令",
  fields: [
    field("size", "体积", true),
    field("last_modified", "最后修改", true, true),
    field("kind", "类别"),
  ],
  items: [
    item({
      id: "toolchain:cargo",
      name: "Rust · cargo 下载缓存",
      group_key: "Rust",
      path: `${HOME}\\.cargo\\registry`,
      metrics: { size: bytes(6.2 * GB), last_modified: count(ago(1)), kind: text("下载缓存") },
      cleanable: true,
      reclaimable: 6.2 * GB,
      detail: { "1. 位置": `${HOME}\\.cargo\\registry`, "2. 条目": "crate 源码与二进制 1840 个" },
    }),
    item({
      id: "toolchain:rustup",
      name: "Rust · rustup 工具链",
      group_key: "Rust",
      path: `${HOME}\\.rustup\\toolchains`,
      metrics: { size: bytes(4.8 * GB), last_modified: count(ago(45)), kind: text("已安装") },
      detail: { "1. 位置": `${HOME}\\.rustup\\toolchains`, "2. 版本": "1.82 / 1.75 / 1.71" },
    }),
    item({
      id: "toolchain:pnpm",
      name: "Node · pnpm store",
      group_key: "Node",
      path: `${HOME}\\AppData\\Local\\pnpm-cache`,
      metrics: { size: bytes(3.4 * GB), last_modified: count(ago(3)), kind: text("下载缓存") },
      detail: {
        "1. 位置": `${HOME}\\AppData\\Local\\pnpm-cache`,
        "2. 条目": "包 2100 个",
        "3. 说明": "官方清理命令：pnpm store prune（只删未引用的包，更精确）",
      },
    }),
    item({
      id: "toolchain:npm",
      name: "Node · npm cache",
      group_key: "Node",
      path: `${HOME}\\AppData\\Local\\npm-cache`,
      metrics: { size: bytes(1.2 * GB), last_modified: count(ago(9)), kind: text("下载缓存") },
      detail: { "3. 说明": "官方清理命令：npm cache clean --force" },
    }),
    item({
      id: "toolchain:pip",
      name: "Python · pip cache",
      group_key: "Python",
      path: `${HOME}\\AppData\\Local\\pip\\cache`,
      metrics: { size: bytes(890 * MB), last_modified: count(ago(7)), kind: text("下载缓存") },
      cleanable: true,
      reclaimable: 890 * MB,
    }),
    item({
      id: "toolchain:go",
      name: "Go · 构建缓存",
      group_key: "Go",
      path: `${HOME}\\AppData\\Local\\go-build`,
      metrics: { size: bytes(1.8 * GB), last_modified: count(ago(2)), kind: text("构建缓存") },
      cleanable: true,
      reclaimable: 1.8 * GB,
    }),
    item({
      id: "toolchain:gradle",
      name: "Java · Gradle 缓存",
      group_key: "Java",
      path: `${HOME}\\.gradle\\caches`,
      metrics: { size: bytes(6.5 * GB), last_modified: count(ago(10)), kind: text("下载缓存") },
      cleanable: true,
      reclaimable: 6.5 * GB,
    }),
  ],
  notes: ["白名单扫描，不是全盘搜索；环境变量改过的位置不识别。"],
  supports_clean: true,
};

// ---- ai_tools：一行 = 工具 × 类别；会话/日志/缓存可清，索引/配置只读 ----

const aiTools: AssetGroup = {
  id: "ai_tools",
  label: "编辑器与 AI 工具",
  summary: "2 个工具 · 6 个类别",
  fields: [
    field("size", "体积", true),
    field("last_activity", "最后活动", true, true),
    field("kind", "类别"),
  ],
  items: [
    item({
      id: `ai:chat:${HOME}\\AppData\\Roaming\\WorkBuddy`,
      name: "WorkBuddy · 会话",
      group_key: "WorkBuddy",
      path: `${HOME}\\AppData\\Roaming\\WorkBuddy\\sessions`,
      metrics: { size: bytes(2.1 * GB), last_activity: count(ago(1)), kind: text("会话") },
      cleanable: true,
      reclaimable: 2.1 * GB,
    }),
    item({
      id: `ai:log:${HOME}\\AppData\\Roaming\\WorkBuddy`,
      name: "WorkBuddy · 日志",
      group_key: "WorkBuddy",
      path: `${HOME}\\AppData\\Roaming\\WorkBuddy\\logs`,
      metrics: { size: bytes(340 * MB), last_activity: count(ago(1)), kind: text("日志") },
      cleanable: true,
      reclaimable: 340 * MB,
    }),
    item({
      id: `ai:cache:${HOME}\\AppData\\Roaming\\WorkBuddy`,
      name: "WorkBuddy · 缓存",
      group_key: "WorkBuddy",
      path: `${HOME}\\AppData\\Roaming\\WorkBuddy\\cache`,
      metrics: { size: bytes(1.2 * GB), last_activity: count(ago(1)), kind: text("缓存") },
      cleanable: true,
      reclaimable: 1.2 * GB,
    }),
    item({
      id: `ai:chat:${HOME}\\AppData\\Roaming\\CodePal`,
      name: "CodePal · 会话",
      group_key: "CodePal",
      path: `${HOME}\\AppData\\Roaming\\CodePal\\sessions`,
      metrics: { size: bytes(890 * MB), last_activity: count(ago(6)), kind: text("会话") },
      cleanable: true,
      reclaimable: 890 * MB,
    }),
    item({
      id: `ai:index:${HOME}\\AppData\\Roaming\\CodePal`,
      name: "CodePal · 索引",
      group_key: "CodePal",
      path: `${HOME}\\AppData\\Roaming\\CodePal\\index`,
      metrics: { size: bytes(5.6 * GB), last_activity: count(ago(6)), kind: text("索引") },
    }),
    item({
      id: `ai:config:${HOME}\\AppData\\Roaming\\CodePal`,
      name: "CodePal · 配置",
      group_key: "CodePal",
      path: `${HOME}\\AppData\\Roaming\\CodePal`,
      metrics: { size: bytes(28 * MB), last_activity: count(ago(30)), kind: text("配置") },
    }),
  ],
  notes: ["只按文件名归类，不读任何文件内容。"],
  supports_clean: true,
};

// ---- local_models：按来源分节，整组只读 ----

const localModels: AssetGroup = {
  id: "local_models",
  label: "本地模型",
  summary: "3 个来源 · 4 条",
  fields: [field("size", "体积", true), field("last_modified", "最后修改", true, true)],
  items: [
    item({
      id: "ollama",
      name: "Ollama · 已装模型",
      group_key: "Ollama",
      path: `${HOME}\\.ollama\\models`,
      metrics: { size: bytes(18.6 * GB), last_modified: count(ago(12)) },
      detail: { "1. 模型": "qwen3:32b · codellama:13b" },
    }),
    item({
      id: "lmstudio",
      name: "LM Studio · 已装模型",
      group_key: "LM Studio",
      path: `${HOME}\\.lmstudio\\models`,
      metrics: { size: bytes(12.4 * GB), last_modified: count(ago(20)) },
    }),
    item({
      id: "huggingface",
      name: "HuggingFace · 下载缓存",
      group_key: "HuggingFace",
      path: `${HOME}\\.cache\\huggingface`,
      metrics: { size: bytes(22.8 * GB), last_modified: count(ago(4)) },
    }),
  ],
  notes: ["模型是资产不是缓存，整组不提供清理入口。"],
};

// ---- 总览：同 Rust 侧 synthesize_overview 的口径，前端用同一套 helper 合成 ----

function synthesizeOverview(list: AssetGroup[]): AssetGroup {
  const total = list.reduce((s, g) => s + groupSize(g), 0);
  const reclaim = list.reduce((s, g) => s + groupReclaimable(g), 0);
  return {
    id: "overview",
    label: "总览",
    summary: `${list.length} 个分组 · 合计 ${formatBytes(total)} · 可回收 ${formatBytes(reclaim)}`,
    fields: [
      field("size", "体积", true),
      field("reclaimable", "可回收", true),
      field("count", "条目", true),
    ],
    items: list.map((g) =>
      item({
        id: `overview:${g.id}`,
        name: g.label,
        path: `overview://${g.id}`,
        metrics: {
          size: bytes(groupSize(g)),
          reclaimable: bytes(groupReclaimable(g)),
          count: count(g.items.length),
        },
      }),
    ),
    notes: ["总占用只统计侧栏已注册的分组。", "点任意一行进入对应分组。"],
  };
}

const realGroups = [repos, toolchain, aiTools, localModels];
const groups: AssetGroup[] = [synthesizeOverview(realGroups), ...realGroups];

// 「数据更新于 3 小时前」稳定可断言；所有分组同一时间（全量口径）。
const savedAt = NOW - 3 * 3600;
const groupSavedAt = Object.fromEntries(groups.map((g) => [g.id, savedAt]));

const snapshot = (): ScanResult => ({ groups, saved_at: savedAt, group_saved_at: groupSavedAt });

mockIPC((cmd: string) => {
  switch (cmd) {
    case "bootstrap":
      return { ...snapshot(), scanned: false } satisfies BootPayload;
    case "rescan_now":
    case "rescan_group":
      return snapshot();
    case "get_repo_roots":
      return {
        default_roots: ["X:\\dev\\fixtures"],
        extra_roots: [],
      } satisfies RepoRoots;
    case "reveal_path":
      return null;
    case "plugin:app|version":
      return "0.1.7";
    default:
      // 未mock 的命令（事件 listen、窗口 set_theme 等）静默成功即可
      return null;
  }
});

createApp(App).use(router).mount("#app");

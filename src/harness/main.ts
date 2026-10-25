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
import { groupReclaimable, groupSize, itemSize, reclaimableOf } from "@/lib/types";
import type {
  AssetGroup,
  BootPayload,
  Field,
  Item,
  MetricValue,
  RepoRoots,
  ScanResult,
  Tag,
  Text,
  TextArg,
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

/**
 * 展示文案的夹具构造器。
 *
 * **夹具下发的是文案键，不是成品中文**——与 Rust 侧同一套契约（`Text::Key` /
 * `Text::Fmt`）。否则截图台验的是「前端怎么把中文拼起来」，五语矩阵里漏一条
 * 译文在这边照样绿，用户看到的却是界面上印着 `data.group.repos`。
 */
const rtxt = (v: string): Text => ({ t: "r", v });
const ktxt = (v: string): Text => ({ t: "k", v });
const ftxt = (k: string, a: Record<string, TextArg>): Text => ({ t: "f", v: { k, a } });
const narg = (v: number): TextArg => ({ t: "n", v });
const barg = (v: number): TextArg => ({ t: "b", v });

/** 标签：`key` 是稳定取值（筛选 / 抑制 / 阻断判据读它），展示名走 `data.tag.*`。 */
const tg = (key: string): Tag => ({ key, label: ktxt(`data.tag.${key}`) });

/** 字段标签的键与后端 `field_text` 同源：key 是指标名，label 键是驼峰。 */
const FIELD_LABEL_KEY: Record<string, string> = {
  size: "size",
  last_modified: "lastModified",
  last_commit: "lastCommit",
  last_activity: "lastActivity",
  branch: "branch",
  kind: "kind",
  reclaimable: "reclaimable",
  count: "count",
};

function field(key: string, numeric = false, temporal = false): Field {
  const labelKey = FIELD_LABEL_KEY[key];
  if (!labelKey) throw new Error(`夹具字段没有 label 键：${key}`);
  return {
    key,
    label: ktxt(`data.field.${labelKey}`),
    numeric: numeric || undefined,
    temporal: temporal || undefined,
  };
}

type ItemSpec = Omit<Item, "tags"> & { tags?: string[] };

function item(spec: ItemSpec): Item {
  const { tags = [], ...rest } = spec;
  return { ...rest, tags: tags.map(tg) };
}

const HOME = "C:\\Users\\fixture";

/**
 * 摘要按夹具的**实际数据**算，不写死字面量：写死的数与行不符时，截图里
 * 看不出来，而这一行每屏都在。模板与参数名同 Rust 侧 `data.summary.*`——
 * 各语言用哪几个参数由译文自己决定，多给的参数无害。
 */
function summaryFor(items: Item[], key: string, extra: Record<string, TextArg> = {}): Text {
  return ftxt(key, {
    count: narg(items.length),
    items: narg(items.length),
    total: barg(items.reduce((s, it) => s + itemSize(it), 0)),
    clean: narg(items.filter((it) => it.cleanable).length),
    reclaim: barg(
      items.reduce((s, it) => (it.cleanable ? s + reclaimableOf(it) : s), 0),
    ),
    locations: narg(new Set(items.map((it) => it.group_key ?? "")).size),
    ...extra,
  });
}

// ---- repos：既有可清理行（弹干净弹窗）、也有带产物的行（弹产物弹窗）----

const reposItems: Item[] = [
  item({
      id: "X:\\dev\\fixtures\\repo-a",
      name: "repo-a",
      path: "X:\\dev\\fixtures\\repo-a",
      tags: ["large", "artifacts"],
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
      tags: ["cleanable"],
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
      // 唯一带阻断标签（modified）的行：基线截图里两档墨色与「阻断项排最前」靠它自验
      tags: ["large", "stale", "modified"],
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
      tags: ["cleanable", "stale"],
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
      tags: ["artifacts"],
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
];

const repos: AssetGroup = {
  id: "repos",
  label: ktxt("data.group.repos"),
  summary: summaryFor(reposItems, "data.summary.repos"),
  fields: [field("size", true), field("last_commit", true, true), field("branch")],
  items: reposItems,
  notes: [ftxt("data.note.reposRoots", { roots: rtxt("X:\\dev\\fixtures") })],
  supports_clean: true,
};

// ---- toolchain：按栈分节（group_key）；有官方命令的行不标可清理 ----
//
// 行的形状与 Rust 侧一致：`name` 是「section · short」的稳定身份（不参与展示），
// 展示走 `group_label` / `short_label` / `kind_label` 三个 Text 字段；
// `metrics.kind` 保持中文原值——它是筛选与口径测试读的词汇，不翻译。

const toolchainItems: Item[] = [
  item({
      id: "toolchain:cargo",
      name: "Rust · Cargo 依赖缓存",
      group_key: "Rust",
      group_label: rtxt("Rust"),
      short_label: ktxt("data.item.cargoRegistry"),
      kind_label: ktxt("data.kind.downloadCache"),
      path: `${HOME}\\.cargo\\registry`,
      metrics: { size: bytes(6.2 * GB), last_modified: count(ago(1)), kind: text("下载缓存") },
      cleanable: true,
      reclaimable: 6.2 * GB,
      detail: { "1. 位置": `${HOME}\\.cargo\\registry`, "2. 条目": "crate 源码与二进制 1840 个" },
    }),
  item({
      id: "toolchain:rustup",
      name: "Rust · Rustup 工具链",
      group_key: "Rust",
      group_label: rtxt("Rust"),
      short_label: ktxt("data.item.rustupToolchains"),
      kind_label: ktxt("data.kind.installed"),
      path: `${HOME}\\.rustup\\toolchains`,
      metrics: { size: bytes(4.8 * GB), last_modified: count(ago(45)), kind: text("已安装") },
      detail: { "1. 位置": `${HOME}\\.rustup\\toolchains`, "2. 版本": "1.82 / 1.75 / 1.71" },
    }),
  item({
      id: "toolchain:pnpm",
      name: "Node · pnpm 内容存储",
      group_key: "Node",
      group_label: ktxt("data.section.node"),
      short_label: ktxt("data.item.pnpmStore"),
      kind_label: ktxt("data.kind.downloadCache"),
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
      name: "Node · npm",
      group_key: "Node",
      group_label: ktxt("data.section.node"),
      short_label: rtxt("npm"),
      kind_label: ktxt("data.kind.downloadCache"),
      path: `${HOME}\\AppData\\Local\\npm-cache`,
      metrics: { size: bytes(1.2 * GB), last_modified: count(ago(9)), kind: text("下载缓存") },
      detail: { "3. 说明": "官方清理命令：npm cache clean --force" },
    }),
  item({
      id: "toolchain:pip",
      name: "Python · pip",
      group_key: "Python",
      group_label: rtxt("Python"),
      short_label: rtxt("pip"),
      kind_label: ktxt("data.kind.downloadCache"),
      path: `${HOME}\\AppData\\Local\\pip\\cache`,
      metrics: { size: bytes(890 * MB), last_modified: count(ago(7)), kind: text("下载缓存") },
      cleanable: true,
      reclaimable: 890 * MB,
    }),
  item({
      id: "toolchain:go",
      name: "Go · 构建缓存",
      group_key: "Go",
      group_label: rtxt("Go"),
      short_label: ktxt("data.item.goBuildCache"),
      kind_label: ktxt("data.kind.buildCache"),
      path: `${HOME}\\AppData\\Local\\go-build`,
      metrics: { size: bytes(1.8 * GB), last_modified: count(ago(2)), kind: text("构建缓存") },
      cleanable: true,
      reclaimable: 1.8 * GB,
    }),
  item({
      id: "toolchain:coursier",
      name: "Java · Coursier",
      group_key: "Java",
      group_label: rtxt("Java"),
      short_label: rtxt("Coursier"),
      kind_label: ktxt("data.kind.dependencyCache"),
      path: `${HOME}\\AppData\\Local\\coursier\\cache`,
      metrics: { size: bytes(6.5 * GB), last_modified: count(ago(10)), kind: text("依赖缓存") },
      cleanable: true,
      reclaimable: 6.5 * GB,
    }),
];

const toolchain: AssetGroup = {
  id: "toolchain",
  label: ktxt("data.group.toolchain"),
  summary: summaryFor(toolchainItems, "data.summary.toolchain"),
  fields: [field("size", true), field("last_modified", true, true), field("kind")],
  items: toolchainItems,
  notes: [ktxt("data.note.toolchain.reclaimRule"), ktxt("data.note.toolchain.installedNotReclaimable")],
  supports_clean: true,
};

// ---- ai_tools：一行 = 工具 × 类别；会话/日志/缓存可清，索引/配置只读 ----

const aiToolItems: Item[] = [
  item({
      id: `ai:chat:${HOME}\\AppData\\Roaming\\WorkBuddy`,
      name: "WorkBuddy · 会话",
      group_key: "WorkBuddy",
      group_label: rtxt("WorkBuddy"),
      short_label: ktxt("data.kind.session"),
      kind_label: ktxt("data.kind.session"),
      path: `${HOME}\\AppData\\Roaming\\WorkBuddy\\sessions`,
      metrics: { size: bytes(2.1 * GB), last_activity: count(ago(1)), kind: text("会话") },
      cleanable: true,
      reclaimable: 2.1 * GB,
    }),
  item({
      id: `ai:log:${HOME}\\AppData\\Roaming\\WorkBuddy`,
      name: "WorkBuddy · 日志",
      group_key: "WorkBuddy",
      group_label: rtxt("WorkBuddy"),
      short_label: ktxt("data.kind.log"),
      kind_label: ktxt("data.kind.log"),
      path: `${HOME}\\AppData\\Roaming\\WorkBuddy\\logs`,
      metrics: { size: bytes(340 * MB), last_activity: count(ago(1)), kind: text("日志") },
      cleanable: true,
      reclaimable: 340 * MB,
    }),
  item({
      id: `ai:cache:${HOME}\\AppData\\Roaming\\WorkBuddy`,
      name: "WorkBuddy · 缓存",
      group_key: "WorkBuddy",
      group_label: rtxt("WorkBuddy"),
      short_label: ktxt("data.kind.cache"),
      kind_label: ktxt("data.kind.cache"),
      path: `${HOME}\\AppData\\Roaming\\WorkBuddy\\cache`,
      metrics: { size: bytes(1.2 * GB), last_activity: count(ago(1)), kind: text("缓存") },
      cleanable: true,
      reclaimable: 1.2 * GB,
    }),
  item({
      id: `ai:chat:${HOME}\\AppData\\Roaming\\CodePal`,
      name: "CodePal · 会话",
      group_key: "CodePal",
      group_label: rtxt("CodePal"),
      short_label: ktxt("data.kind.session"),
      kind_label: ktxt("data.kind.session"),
      path: `${HOME}\\AppData\\Roaming\\CodePal\\sessions`,
      metrics: { size: bytes(890 * MB), last_activity: count(ago(6)), kind: text("会话") },
      cleanable: true,
      reclaimable: 890 * MB,
    }),
  item({
      id: `ai:index:${HOME}\\AppData\\Roaming\\CodePal`,
      name: "CodePal · 索引",
      group_key: "CodePal",
      group_label: rtxt("CodePal"),
      short_label: ktxt("data.kind.index"),
      kind_label: ktxt("data.kind.index"),
      path: `${HOME}\\AppData\\Roaming\\CodePal\\index`,
      metrics: { size: bytes(5.6 * GB), last_activity: count(ago(6)), kind: text("索引") },
    }),
  item({
      id: `ai:config:${HOME}\\AppData\\Roaming\\CodePal`,
      name: "CodePal · 配置",
      group_key: "CodePal",
      group_label: rtxt("CodePal"),
      short_label: ktxt("data.kind.config"),
      kind_label: ktxt("data.kind.config"),
      path: `${HOME}\\AppData\\Roaming\\CodePal`,
      metrics: { size: bytes(28 * MB), last_activity: count(ago(30)), kind: text("配置") },
    }),
];

const aiTools: AssetGroup = {
  id: "ai_tools",
  label: ktxt("data.group.ai_tools"),
  summary: summaryFor(aiToolItems, "data.summary.aiTools"),
  fields: [field("size", true), field("last_activity", true, true), field("kind")],
  items: aiToolItems,
  notes: [ktxt("data.note.aiTools.noContent"), ktxt("data.note.aiTools.session")],
  supports_clean: true,
};

// ---- local_models：按来源分节，整组只读 ----
//
// 行名是**模型名**（数据，不是文案），所以这三行不给 short_label——与 Rust 侧
// 同形：`name` 形如 `Ollama · qwen3:32b`，前端按 `group_key · ` 前缀回落切名。

const localModelItems: Item[] = [
  item({
      id: "ollama",
      name: "Ollama · qwen3:32b",
      group_key: "Ollama",
      group_label: rtxt("Ollama"),
      path: `${HOME}\\.ollama\\models\\qwen3-32b`,
      metrics: { size: bytes(18.6 * GB), last_modified: count(ago(12)) },
      detail: { "1. 模型": "qwen3:32b · codellama:13b" },
    }),
  item({
      id: "lmstudio",
      name: "LM Studio · llama-3.1-8b",
      group_key: "LM Studio",
      group_label: rtxt("LM Studio"),
      path: `${HOME}\\.lmstudio\\models\\llama-3.1-8b`,
      metrics: { size: bytes(12.4 * GB), last_modified: count(ago(20)) },
    }),
  item({
      id: "huggingface",
      name: "HuggingFace · org-a/bert-base",
      group_key: "HuggingFace",
      group_label: rtxt("HuggingFace"),
      path: `${HOME}\\.cache\\huggingface\\hub\\models--org-a--bert-base`,
      metrics: { size: bytes(22.8 * GB), last_modified: count(ago(4)) },
    }),
];

const localModels: AssetGroup = {
  id: "local_models",
  label: ktxt("data.group.local_models"),
  summary: summaryFor(localModelItems, "data.summary.localModels"),
  fields: [field("size", true), field("last_modified", true, true)],
  items: localModelItems,
  notes: [ktxt("data.note.localModels.assetNotCache")],
};

// ---- 总览：同 Rust 侧 synthesize_overview 的口径，前端用同一套 helper 合成 ----

function synthesizeOverview(list: AssetGroup[]): AssetGroup {
  const total = list.reduce((s, g) => s + groupSize(g), 0);
  const reclaim = list.reduce((s, g) => s + groupReclaimable(g), 0);
  return {
    id: "overview",
    label: ktxt("data.group.overview"),
    summary: ftxt("data.summary.overview", {
      count: narg(list.length),
      total: barg(total),
      reclaim: barg(reclaim),
    }),
    fields: [field("size", true), field("reclaimable", true), field("count", true)],
    items: list.map((g) =>
      item({
        id: `overview:${g.id}`,
        // `name` 只做稳定身份（分组 id），展示名是分组自己的 label——与 Rust 侧
        // `synthesize_overview` 同形：把某语言的分组名抄进 name 会跟着数据冻在缓存里。
        name: g.id,
        label: g.label,
        path: `overview://${g.id}`,
        metrics: {
          size: bytes(groupSize(g)),
          reclaimable: bytes(groupReclaimable(g)),
          count: count(g.items.length),
        },
      }),
    ),
    notes: [],
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

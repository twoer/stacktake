/** 与 Rust 侧 `scan::AssetGroup` 对应的类型定义。 */

import { formatBytesLocalized, formatNumber, relativeTime, renderText, t, translate } from "@/lib/i18n";

/**
 * 展示文案（P2 起后端各分组下发的就是它）。
 *
 * 缓存里存的是**键与参数**而不是某一种语言的成品文案：切语言只重渲染，
 * 不需要重扫；五种语言的译文都在 `src/locales/`。
 * - `r` = 原始数据（路径、仓库名、品牌名），不翻译
 * - `k` = 静态文案键
 * - `f` = 带命名参数的文案键
 */
export type Text =
  | { t: "r"; v: string }
  | { t: "k"; v: string }
  | { t: "f"; v: { k: string; a: Record<string, TextArg> } };

export type TextArg = Text | { t: "n"; v: number } | { t: "b"; v: number };

/** Rust `scan::Tag`：`key` 参与筛选 / 抑制 / 阻断判据（语言无关），`label` 只展示。 */
export interface Tag {
  key: string
  label: Text
}

export interface Field {
  key: string
  label: Text
  numeric?: boolean
  temporal?: boolean
}

export interface ArtifactEntry {
  relative_path: string
  size_bytes: number
}

export interface Item {
  /**
   * 后端条目 id，**前端只透传、不解析**。形态因组而异：repos 是裸路径、
   * toolchain 带 `toolchain:` 前缀、ai_tools 是 `ai:<类别键>:<工具根路径>`
   * （B1 起；此前不含路径，清理入口因此拿不到删除目标）。
   */
  id: string
  tags: Tag[]
  /**
   * 原始名：**搜索、排序、分节前缀与 `group_key` 约定都读它**，不随语言变。
   * 渲染请用 `itemDisplay()`（有 `label` / `short_label` 时走展示字段）。
   */
  name: string
  /** 整行展示名。缺省 = `name`（仓库 / 模型这类纯数据名不需要翻译）。 */
  label?: Text | null
  /** 分节行里去掉 `group_key · ` 前缀后的展示名。缺省时前端按前缀切分 `name`。 */
  short_label?: Text | null
  /** 分节标题的展示名。缺省 = `group_key` 原值。 */
  group_label?: Text | null
  /** 类别列的展示名。缺省 = `metrics.kind` 原值（那是筛选词表，不是文案）。 */
  kind_label?: Text | null
  /**
   * 分节依据：这一条属于本组哪个「维度取值」（G1 起正式化）。
   *
   * 在此之前前端靠解析 `name` 里的 ` · ` 前缀分节——那是显示约定不是数据，
   * 改一次名字分节就会散。`null`/缺省 = 本组不分节（repos / 鸿蒙 / WSL 都是）。
   * 同一个字段在 toolchain 是「栈」、ai_tools 是「工具名」、java 是「组件」，
   * 所以维度名由前端按分组声明（见 Assets.vue 的 PREFIX_LABEL）。
   *
   * 字段名与 Rust 侧逐字一致（snake_case，没有 rename_all）——这个文件是镜像，
   * 改 Rust 字段名必须同步这里，且`group_key` 是第一个多单词字段，前面那些
   * 单单词字段大小写无差异，容易让人误以为全栈都在用驼峰。
   */
  group_key?: string | null
  path: string
  metrics: Record<string, MetricValue>
  /** 检查器明细。P2b 前是 `Record<string, string>`（键带数字前缀排序）。 */
  detail?: DetailRow[]
  artifact_entries?: ArtifactEntry[]
  cleanable?: boolean
  reclaimable?: number
}

export interface AssetGroup {
  id: string
  label: Text
  summary: Text
  fields: Field[]
  items: Item[]
  notes?: Text[]
  error?: Text
  /**
   * 本组有没有删除实现（S3 起由后端下发）。
   *
   * **前端不要写死分组 id 来判断能不能清理**——S3 之前 `canClean` 判的是
   * `current.id === "repos"`，加第二个分组就得去改那个字符串，漏改就会出现
   * 「界面给按钮、服务端拒绝」或反过来。能力的唯一真相在 Rust 侧。
   *
   * 与行级 `Item.cleanable` 是两个粒度：本字段说「这一组能不能清」，
   * 行级说「这一行给不给按钮」——toolchain 组本字段为 true，但有官方清理
   * 命令的那 6 条 `cleanable` 恒 false；ai_tools 组为 true，但只有日志 /
   * 会话 / 缓存三类行标 `cleanable`。缺省视作 false（后端只在 true 时下发）。
   */
  supports_clean?: boolean
}

// ---- 展示名 ----
//
// 渲染读这三个函数，**不读 `name` / `group_key` / `metrics.kind` 的原串**：
// 后三者是搜索、筛选、分节与后端复核的词表，语言无关才正确（切英文时
// 「npm 缓存」这一行的 `name` 仍是中文，否则英文界面里筛完的口径就对不上账）。
// 后端为每个展示位下发了对应的 `Text`（`label` / `short_label` / `group_label` /
// `kind_label`），缺省才回落原串——品牌名、版本号、模型名这类本来就不用翻译。

/** 分组展示名（侧栏、页面标题、空态）。 */
export function groupLabelOf(g: AssetGroup): string {
  return renderText(g.label) || g.id;
}

/** 行的展示名（整行形态，不剥分节前缀）。 */
export function itemLabelOf(it: Item | null | undefined): string {
  if (!it) return "";
  return it.label ? renderText(it.label) : it.name;
}

/** 分节标题的展示名：`group_label` 优先，缺省用 `group_key` 原值。 */
export function sectionLabelOf(it: Item): string {
  return it.group_label ? renderText(it.group_label) : (it.group_key ?? "");
}

/** 类别列的展示名：`kind_label` 优先，缺省回落 `metrics.kind` 的原值。 */
export function kindLabelOf(it: Item | null | undefined): string {
  if (it?.kind_label) return renderText(it.kind_label);
  const m = it?.metrics.kind;
  if (!m) return "";
  if (m.t === "k") return translate(m.v);
  return m.t === "s" ? String(m.v) : "";
}

/**
 * 按 label 键找明细行的值（键形如 `data.detail.note`）。目前只有清理弹窗
 * 需要按行寻址（把工具链的「说明」逐条摆出来）；检查器是整列顺序渲染，不走这里。
 */
export function detailValueOf(
  it: Item | null | undefined,
  labelKey: string,
): Text | undefined {
  return it?.detail?.find((r) => r.label.t === "k" && r.label.v === labelKey)?.value;
}

/** `kind` 指标的**原始值**（筛选词表，与语言无关）。判「这一行是不是某类别」用它，别比展示名。 */
export function kindKeyOf(it: Item | null | undefined): string {
  const m = it?.metrics.kind;
  return m && m.t === "s" ? String(m.v) : "";
}

/**
 * Rust `cache::PrevLedger` 的对应结构——上一版基线（差额台账，L1 / D18）。
 *
 * Rust 侧三个映射**全空时整个字段省略**，单组轮转只动该组那一份，所以三个
 * 字段在 `undefined` 语义上等同：没有这个键 = 没有基线 = 不显示差额。
 * 判读口径在 `lib/delta.ts`，这里只做镜像。
 */
export interface PrevLedger {
  /** 基线时刻各分组的扫描时间（检查器整组差额提示按它渲染「与 X 前的扫描对比」）。 */
  group_saved_at?: Record<string, number>
  /** 组 → 条目 id → 体积（只记 size 为严格 Bytes 的条目）。 */
  item_sizes?: Record<string, Record<string, number>>
  /** 组 → 合计体积（Rust `AssetGroup::total_size` 口径）。 */
  group_totals?: Record<string, number>
}

/** Rust `cache::CachedSnapshot` 的对应结构。 */
export interface CachedSnapshot {
  groups: AssetGroup[]
  saved_at: number
  /** 各分组各自的写入时间，语义见 ScanResult */
  group_saved_at: Record<string, number>
  age_seconds: number
  extra_repo_roots: string[]
  /** 上一版基线（差额台账）。缺省 = 无基线（首次运行 / 清缓存后的合法态）。 */
  prev?: PrevLedger | null
}

/** Rust `lib.rs::BootPayload` 的对应结构——启动时的统一响应。 */
export interface BootPayload {
  groups: AssetGroup[]
  saved_at: number
  /** 各分组各自的写入时间，语义见 ScanResult */
  group_saved_at: Record<string, number>
  /** 上一版基线（差额台账）。缺省 = 无基线。 */
  prev?: PrevLedger | null
  /** 本次是否真的扫描了。false = 吃的缓存 */
  scanned: boolean
}

/**
 * Rust `lib.rs::ScanResult` 的对应结构——扫描命令（`rescan_now` / `rescan_group`）的返回。
 *
 * `saved_at` 以后端为准：前端自己用 `Date.now()` 近似会带上一次扫描的时长偏差
 * （全量扫描几十秒到几分钟），而它就是缓存文件里真正写下的那个时间戳。
 *
 * `group_saved_at` 是各分组各自的写入时间：单组重扫后不同分组的新鲜度不同，
 * 「数据更新于」要按当前视图取对应组的时间，不能拿全体写入时间冒充。
 * 全量扫描时所有值都等于 `saved_at`。
 */
export interface ScanResult {
  groups: AssetGroup[]
  saved_at: number
  group_saved_at: Record<string, number>
  /** 上一版基线（差额台账）。缺省 = 无基线。 */
  prev?: PrevLedger | null
}

/** 补充扫描目录；离线目录仍保留在配置中，允许移除。 */
export interface RepoRoot {
  path: string
  available: boolean
}

export interface RepoRoots {
  default_roots: string[]
  extra_roots: RepoRoot[]
}

export interface RepoRootsError {
  message: string
  snapshot: ScanResult | null
}

/**
 * Rust `scan::CleanPlan` 的对应结构——清理命令（`clean_items`）的入参（S2 / D1）。
 *
 * `confirmed` 是**必填**的：Rust 侧 `Deserialize` 没有 `#[serde(default)]`，
 * 缺这个字段反序列化直接失败；显式传 `false` 服务端也会再拒一次。
 * 两道闸都是刻意的——「忘了填」和「填了假」都得挡住。
 *
 * `scope`（P2）：缺省 "repo" = 整条目删除（复核未推送/改动）；
 * "artifacts" = 只清构建产物（repos 组内，复核白名单，不动源码与 .git）。
 * 传未知取值 Rust 侧反序列化直接失败——不会退化成整仓删除。
 */
export interface CleanPlan {
  group_id: string
  item_ids: string[]
  confirmed: boolean
  scope?: "repo" | "artifacts"
  /**
   * 产物清理的子集（`scope: "artifacts"` 时有效）：只清这些**相对仓库根**的产物目录。
   * 缺省 / undefined = 仓库内全部；弹窗即使全选也显式传入所选路径。
   * 空数组会被服务端当成「什么都没选中」拒掉，不会退化成清全部。
   *
   * 前端选过不算数：服务端会把每条重新拼回绝对路径，并过与全量清理完全相同的
   * 复核（在仓库内、白名单、不藏 Git），见 `repos::resolve_artifact_target`。
   */
  artifact_targets?: string[]
}

/** Rust `scan::CleanFailure` 的对应结构——单项失败的原因（Text：键或系统错误原文）。 */
export interface CleanFailure {
  id: string
  reason: Text
}

/** Rust `scan::CleanResult` 的对应结构——清理命令的返回。 */
export interface CleanResult {
  group_id: string
  /** 成功移入废纸篓的条目 id */
  succeeded: string[]
  /** 被拒或出错的条目。`id: "*"` 表示整个计划被拒（如未确认） */
  failed: CleanFailure[]
  /** 本次实际腾出的字节数（只计成功的） */
  freed_bytes: number
}

/** Rust `lib.rs::ScanProgress` 的对应结构——扫描进度事件（"scan-progress"）的载荷。 */
export interface ScanProgress {
  /** 已完成的分组数 */
  done: number
  /** 总分组数 */
  total: number
  /**
   * 刚完成的分组名（`Text`，并行扫描下完成顺序不固定）。
   * 首帧（0 进度）没有「刚完成的分组」，后端下发 `null`。
   */
  label: Text | null
  /** 本次扫描的全部分组名（注册序）。首扫空态据此预排行账，完成一行点亮一行 */
  labels: Text[]
}

/** Rust `update.rs::DownloadProgress` 的对应结构——下载进度事件（"update-download-progress"）的载荷。 */
export interface DownloadProgress {
  /** 已写盘的字节数 */
  received: number
  /** 总字节数；null = 服务端没给 Content-Length，此时画不了百分比 */
  total: number | null
}

/**
 * 与 Rust 侧 `scan::MetricValue` 对应。
 *
 * Rust 侧刻意**不用** untagged 序列化——那样 `Int(1700000000)` 和
 * `Bytes(1700000000)` 在 JSON 里无法区分，缓存读回来会把时间戳
 * 当成字节数，「最后提交」列显示成 `1.7 GB`。所以带 `t` 标签分派。
 */
export type MetricValue =
  | { t: "s"; v: string }
  | { t: "b"; v: number }
  | { t: "i"; v: number }
  | { t: "z"; v: boolean }
  /** 界面文案（「未知」「无提交」）——存键，切语言只重渲染。 */
  | { t: "k"; v: string }
  /** 带参数的界面文案（「3 处改动」）——数字由前端按语言格式化（P2b）。 */
  | { t: "f"; v: { k: string; a: Record<string, TextArg> } }

/** Rust `scan::DetailRow`：检查器「明细」的一行。顺序就是数组顺序（P2b 前是
 *  `HashMap` + 键名数字前缀），label 是 `data.detail.*` 键，值三种 Text 形态都有。 */
export interface DetailRow {
  label: Text
  value: Text
}

/** 取裸值（`f` 形态没有裸值，渲染成文案）。 */
export function metricValue(m: MetricValue): string | number | boolean {
  if (m.t === "f") return renderText(m);
  return m.v
}

/**
 * 一条目的体积（字节）。没有 size 指标或类型不对时算 0——WSL1 那种
 * 「没有虚拟磁盘」的条目刻意不报体积，与报 0 不是一回事（0 = 不占空间）。
 */
export function itemSize(it: Item): number {
  const m = it.metrics.size
  return m && (m.t === "b" || m.t === "i") ? m.v : 0
}

/**
 * 一条目的可回收量。分组行读顶层 `reclaimable`；总览行没有顶层字段，
 * 把它放在 `metrics.reclaimable` 里（总览行不是可清理条目，不能标 cleanable）。
 */
export function reclaimableOf(it: Item): number {
  if (it.reclaimable != null) return it.reclaimable
  const m = it.metrics.reclaimable
  return m && (m.t === "b" || m.t === "i") ? m.v : 0
}

/**
 * 一条目的构建产物体积（字节）。repos 组特有（P2），与 `reclaimable` 是
 * 两个口径：前者是「只清产物能腾出多少」（源码保留），后者是「整仓删掉
 * 能腾出多少」。老缓存没有这个指标 → 0，「清理产物」按钮随它一起消失。
 */
export function artifactBytes(it: Item): number {
  const m = it.metrics.artifacts
  return m && m.t === "b" ? m.v : 0
}

/** 本组合计体积。总览分组（id = "overview"）是现算的，算侧栏总数时要剔掉。 */
export function groupSize(g: AssetGroup): number {
  return g.items.reduce((s, it) => s + itemSize(it), 0)
}

/** 本组可回收合计。只累加标了 cleanable 的——与 Rust 侧 `total_reclaimable` 同口径。 */
export function groupReclaimable(g: AssetGroup): number {
  return g.items.reduce((s, it) => (it.cleanable ? s + reclaimableOf(it) : s), 0)
}

/** Rust 的 `MetricValue` 带类型标签，这里按标签分派。 */
export function formatMetric(m: MetricValue, temporal?: boolean): string {
  switch (m.t) {
    case "z":
      return m.v ? t("common.yes") : t("common.no")
    case "i":
      return temporal ? formatAgo(m.v) : formatNumber(m.v)
    case "b":
      return temporal ? formatAgo(m.v) : formatNumber(m.v)
    case "s":
      return m.v
    case "k":
      return translate(m.v)
    case "f":
      return renderText(m)
  }
}

/** 体积列专用：字节要格式化成人看的单位。 */
export function formatMetricBytes(m: MetricValue): string {
  return m.t === "b" ? formatBytes(m.v) : formatMetric(m)
}

export function formatBytes(b: number): string {
  return formatBytesLocalized(b)
}

/**
 * 体积的「值」与「单位」两半，给要把单位排得比数值弱一档的地方用（表格体积列、
 * 状态栏、检查器主角数字）。**刻意复用 `formatBytes` 而不是另写一套换算**——
 * 两处各算一次，单位边界迟早会漂（1024 还是 1000、保留几位小数），那比不拆更糟。
 */
export function formatBytesParts(b: number): { n: string; u: string } {
  const s = formatBytes(b);
  const i = s.lastIndexOf(" ");
  return { n: s.slice(0, i), u: s.slice(i + 1) };
}

/** `nowSecs` 默认墙钟；传时间针（useAssets 的 `now`）可让界面上的「X 前」
 *  跟着钟走，而不是冻在最后一次重渲染的那一刻。相对时间交给 Intl，复数与
 *  语序由各语言自己处理；「刚刚」单列一条（RelativeTimeFormat 的 0 秒是
 *  「0 分钟前」，不是人话）。 */
export function formatAgo(unixSeconds: number, nowSecs: number = Date.now() / 1000): string {
  const diff = nowSecs - unixSeconds
  if (diff < 60) return t("time.justNow")
  const d = Math.floor(diff / 86400)
  if (d === 0) {
    const h = Math.floor(diff / 3600)
    if (h === 0) return relativeTime(-Math.floor(diff / 60), "minute")
    return relativeTime(-h, "hour")
  }
  if (d < 30) return relativeTime(-d, "day")
  const mo = Math.floor(d / 30)
  if (mo < 12) return relativeTime(-mo, "month")
  return relativeTime(-Math.floor(mo / 12), "year")
}

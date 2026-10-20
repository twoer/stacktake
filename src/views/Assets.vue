<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch, type Ref } from "vue";
import { useRouter } from "vue-router";
import {
  AlertTriangle,
  ChevronRight,
  Copy,
  FolderOpen,
  ListFilter,
  LoaderCircle,
  Moon,
  PanelLeft,
  RefreshCw,
  Search,
  Sun,
  Trash2,
  X,
} from "@lucide/vue";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "vue-sonner";
import type { AssetGroup, CleanResult, Item } from "@/lib/types";
import {
  artifactBytes,
  formatAgo,
  formatMetric,
  formatMetricBytes,
  formatBytes,
  groupReclaimable,
  groupSize,
  itemSize,
  reclaimableOf,
} from "@/lib/types";
import { useAssets } from "@/composables/useAssets";
import { useTheme } from "@/composables/useTheme";
import { captureRowTops, playRowFlip } from "@/lib/motion";
import { Button } from "@/components/ui/button";
import ByteValue from "@/components/ByteValue.vue";
import ScanBar from "@/components/ScanBar.vue";

const router = useRouter();
const { groups, current, loading, error, activeId, progress, cachedAt, rescan } =
  useAssets();
const { theme, toggle: toggleTheme } = useTheme();

/** Windows 上 Overlay 标题栏把原生最小化/最大化/关闭浮在右上，工具栏右侧要留位 */
const isWindows = navigator.userAgent.includes("Windows");

/**
 * 工具栏控件共同形态：ghost，无描边，hover 才给底。
 * 一排描边小盒子是「线条感」的最大来源——六个控件六个框，读起来像表单。
 */
const toolbarCtl =
  "flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

// ---- 视图状态（切分组时清掉，否则上个分组的排序和筛选会莫名其妙跟过来）----
const sortKey = ref<string | null>("size");
const sortDesc = ref(true);
const search = ref("");
/** 维度筛选：{ 维度 key: 选中的值 }。空值 = 该维度不筛。 */
const dims = ref<Record<string, string | null>>({});
/** 筛选菜单开合。所有维度共用一个弹出菜单（工具栏只留一个漏斗按钮）。 */
const openFilter = ref(false);
/** 选中的行 id（检查器显示它的明细）。null = 检查器显示分组概览。 */
const selected = ref<string | null>(null);
/** 检查器开关。窗口不够宽时默认收起——侧栏 208 + 检查器 252 会把表格挤死。 */
const inspectorOpen = ref(window.innerWidth >= 1040);
/**
 * 检查器「清理规则」折叠。默认收起：那三条是「本工具怎么判可清理」的说明，
 * 常驻会占掉检查器三分之一屏，天天看——但它对第一次用清理的人确实有用，
 * 所以收成一行而不是删掉。**切分组不重置**：展开过一次说明用户想看。
 */
const rulesOpen = ref(false);
/** 内容滚动容器：切组回顶；FLIP 以它内部的行位移为坐标系。 */
const scrollArea = ref<HTMLElement | null>(null);
const scrollContent = ref<HTMLElement | null>(null);

watch(activeId, () => {
  sortKey.value = "size";
  sortDesc.value = true;
  search.value = "";
  dims.value = {};
  openFilter.value = false;
  selected.value = null;
  // 切分组回到页顶：滚到第 200 行再换组，旧 scrollTop 会把人丢在半页上
  void nextTick(() => scrollArea.value?.scrollTo({ top: 0 }));
});

// ---- 动效状态（U16）----
//
// FLIP 只作用于排序/筛选：行顺序变了才有位移可讲。重扫不重排，走「变了的行
// 闪一下 + 关键数字补间」；两条路不要混在一起，否则一次重扫会整页抖。
const flashIds = ref<Set<string>>(new Set());
let flashTimer = 0;

function withRowFlip(mutate: () => void) {
  const prev = captureRowTops(scrollContent.value);
  mutate();
  void nextTick(() => playRowFlip(scrollContent.value, prev));
}

/** 重扫前记下体积，扫完对比出变动的行 id。 */
function sizeSnapshot(): Map<string, number> {
  const out = new Map<string, number>();
  for (const g of groups.value) {
    if (g.id === "overview") continue;
    for (const it of g.items) out.set(it.id, itemSize(it));
    out.set(`overview:${g.id}`, groupSize(g));
  }
  return out;
}

function flashChanged(before: Map<string, number>) {
  const changed = new Set<string>();
  for (const g of groups.value) {
    if (g.id === "overview") continue;
    for (const it of g.items) {
      const old = before.get(it.id);
      if (old !== undefined && old !== itemSize(it)) changed.add(it.id);
    }
    const oldGroup = before.get(`overview:${g.id}`);
    if (oldGroup !== undefined && oldGroup !== groupSize(g)) changed.add(`overview:${g.id}`);
  }
  window.clearTimeout(flashTimer);
  flashIds.value = changed;
  if (changed.size) {
    flashTimer = window.setTimeout(() => {
      flashIds.value = new Set();
    }, 600);
  }
}

// ---- 行动作：在文件管理器中显示 / 复制路径（U4）----
//
// 只做这两件事，**不做「打开」**——打开一个路径会执行它（`.app` / `.command` /
// 安装包），与「不读文件内容」是同一条理由。这条边界在 Rust 侧（`reveal_path`）
// 与 README「安全边界」里都写着，别只在前端做一半。

/** 右键菜单的位置与目标条目。null = 没打开。 */
const menu = ref<{ x: number; y: number; item: Item } | null>(null);

/** 文案按平台给：macOS 说访达，Windows 说资源管理器。 */
const revealLabel = isWindows ? "在资源管理器中显示" : "在访达中显示";

/**
 * 只有宿主机绝对路径才谈得上「显示位置」或「复制了直接用」。
 * `Item.path` **不都是**宿主机路径——docker 的是 `docker://<镜像名>`，
 * 总览行的是 `overview://<分组id>`，都没有可揭示的位置。
 */
function isLocalPath(p: string): boolean {
  return p.startsWith("/") || /^[A-Za-z]:[\\/]/.test(p);
}

/** 靠右或靠下时往回收，别让菜单跑出窗口（菜单约 176×64）。 */
const menuStyle = computed(() => {
  const m = menu.value;
  if (!m) return {};
  return {
    left: `${Math.max(4, Math.min(m.x, window.innerWidth - 180))}px`,
    top: `${Math.max(4, Math.min(m.y, window.innerHeight - 72))}px`,
  };
});

function openMenu(e: MouseEvent, item: Item) {
  // 不是本机路径就不弹——弹一个两项都点不动的菜单比不弹更糟
  if (!isLocalPath(item.path)) return;
  menu.value = { x: e.clientX, y: e.clientY, item };
}

function closeMenu() {
  menu.value = null;
}

async function onReveal(item: Item) {
  closeMenu();
  try {
    await invoke("reveal_path", { path: item.path });
  } catch (e) {
    // 后端会把「路径已不存在，重扫一次」原样给出来，这里不翻译
    toast.error(String(e));
  }
}

async function onCopyPath(item: Item) {
  closeMenu();
  try {
    await navigator.clipboard.writeText(item.path);
    toast.success("路径已复制");
  } catch {
    toast.error("复制失败，可检查器中手动选中路径");
  }
}

// ---- 清理（S2 / D1 拍板选 A）----
//
// 唯一的 destructive 动作，三道闸：前端确认弹窗（本文件）+ 服务端 `confirmed`
// 校验（Rust 侧）+ 废纸篓兜底（`trash::delete`）。**刻意不进右键菜单**——和
// 「复制路径」并列放会把破坏性操作变成误触目标。入口只在检查器动作条。

/** 清理确认弹窗的目标条目。null = 没打开。 */
const cleanTarget = ref<Item | null>(null);

/**
 * 三个条件都要满足：本组有删除实现（后端下发的 `supports_clean`）、行本身
 * `cleanable`、且是宿主机路径。
 *
 * **为什么不写死分组 id**（S3 起）：S2 之前这里判的是 `id === "repos"`。
 * 加第二个分组（toolchain）就得去改那个字符串，漏改的后果是「界面给按钮、
 * 服务端拒绝」或反过来——后者更坏，用户会以为这个工具没有清理功能。
 * 能力的唯一真相在 Rust 侧（`Scanner::supports_clean` → `AssetGroup` 字段）。
 *
 * 行级 `cleanable` 与分组级是两个粒度：toolchain 组 `supports_clean` 为 true，
 * 但有官方清理命令的那 6 条 `cleanable` 是 false——它们要用人自己的命令清，
 * 而 `pnpm store prune` 只删没被引用的包，比搬走整个目录精确。
 */
function canClean(item: Item): boolean {
  return !!current.value?.supports_clean && !!item.cleanable && isLocalPath(item.path);
}

/**
 * 弹窗里那条「会怎样」的描述。三种性质不同，不能共用一句话：
 * 删仓库丢源码、删缓存只花重下的时间、删 AI 工具数据里的**会话丢历史**。
 */
const cleanKind = computed(() => {
  const id = current.value?.id;
  if (id === "repos") return "repo";
  if (id === "ai_tools") return "ai";
  return "cache";
});

/** AI 工具行的类别标签（后端 `metrics.kind`）；缺失时给中性词，不猜。 */
function aiKindLabel(item: Item | null): string {
  const m = item?.metrics.kind;
  return m && m.t === "s" ? String(m.v) : "数据";
}

/** 工具链缓存删掉的后果：按 detail 的说明逐条显示，不空喊「可恢复」。 */
function cleanNote(item: Item): string {
  return item.detail?.["3. 说明"] ?? "";
}

function askClean(item: Item) {
  if (!canClean(item)) return;
  cleanTarget.value = item;
}

// ---- 清理构建产物（P2）----
//
// 与整仓清理并行的第二个破坏性动作：只清白名单内的产物目录（node_modules /
// target…），源码与 .git 保留。三道闸同构：确认弹窗（本文件）+ 服务端按白名单
// 现找产物目录复核 + 废纸篓。同样**不进右键菜单**。范围用 `scope: "artifacts"`
// 传给后端，产物集合由服务端在点击那一刻重新找出——前端不传产物路径，
// 服务端也不信任它们。

/** 产物清理确认弹窗的目标条目。null = 没打开。 */
const artifactTarget = ref<Item | null>(null);

/**
 * 「清理产物」的判据与 `canClean` 是两个动作：整仓删除看 `cleanable`
 * （复核未推送/改动），产物清理看 `metrics.artifacts`（复核白名单）。
 * 没有 `supports_clean` 的组、非宿主机路径一律不给。
 */
function canCleanArtifacts(item: Item): boolean {
  return !!current.value?.supports_clean && artifactBytes(item) > 0 && isLocalPath(item.path);
}

function askCleanArtifacts(item: Item) {
  if (!canCleanArtifacts(item)) return;
  artifactTarget.value = item;
}

/** 成败 toast + 重扫（缓存里少/变了东西，别让界面停留在旧快照）。 */
async function applyCleanResult(r: CleanResult) {
  if (r.succeeded.length > 0) {
    toast.success(`已移入废纸篓，腾出 ${formatBytes(r.freed_bytes)}`);
  }
  // id = "*" 是整个计划被拒（如未确认），其余是单项失败（复核不通过/废纸篓出错）
  for (const f of r.failed) {
    toast.error(f.id === "*" ? f.reason : `${f.id}：${f.reason}`);
  }
  const before = sizeSnapshot();
  const res = await rescan();
  if (!res.ok) toast.error(res.message);
  else flashChanged(before);
}

// ---- 弹窗焦点管理 ----
//
// 两个清理弹窗都是破坏性确认的必经路径，键盘用户三条都要有：
// ① 打开时焦点进弹窗——落在「取消」上（安全默认，不会误触确认）；
// ② 关闭时把焦点还给打开前那个元素（检查器里的「清理」按钮）；
// ③ Tab 在弹窗内循环，不穿到背后的表格上。
// 缺了这三条，键盘用户打开弹窗后焦点还在背后的行上，Enter/空格会继续操作表格。

const cleanDialog = ref<HTMLElement | null>(null);
const artifactDialog = ref<HTMLElement | null>(null);
/** 弹窗打开前聚焦的元素；关闭时还回去。元素被重渲染掉时 focus() 是无害空操作。 */
let dialogReturnFocus: HTMLElement | null = null;

function watchDialogFocus(target: Ref<Item | null>, dialog: Ref<HTMLElement | null>) {
  watch(target, (t) => {
    if (t) {
      dialogReturnFocus = document.activeElement as HTMLElement | null;
      // watch 默认在 DOM 更新前跑，弹窗是 v-if 出来的，要等下一 tick 才在文档里
      nextTick(() => dialog.value?.querySelector<HTMLElement>("[data-dialog-cancel]")?.focus());
    } else {
      dialogReturnFocus?.focus();
      dialogReturnFocus = null;
    }
  });
}
watchDialogFocus(cleanTarget, cleanDialog);
watchDialogFocus(artifactTarget, artifactDialog);

async function confirmClean() {
  const item = cleanTarget.value;
  if (!item) return;
  const groupId = current.value?.id;
  if (!groupId) return;
  cleanTarget.value = null;
  try {
    const r = await invoke<CleanResult>("clean_items", {
      plan: { group_id: groupId, item_ids: [item.id], confirmed: true, scope: "repo" },
    });
    await applyCleanResult(r);
  } catch (e) {
    toast.error(String(e));
  }
}

async function confirmCleanArtifacts() {
  const item = artifactTarget.value;
  if (!item) return;
  const groupId = current.value?.id;
  if (!groupId) return;
  artifactTarget.value = null;
  try {
    const r = await invoke<CleanResult>("clean_items", {
      plan: { group_id: groupId, item_ids: [item.id], confirmed: true, scope: "artifacts" },
    });
    await applyCleanResult(r);
  } catch (e) {
    toast.error(String(e));
  }
}

/** 菜单收起条件：点到别处、滚动、改窗口大小、按 Esc。 */
function onGlobalPointerDown(e: PointerEvent) {
  const el = e.target as HTMLElement | null;
  if (!el?.closest("[data-row-menu]") && !el?.closest("[data-dim-menu]")) {
    closeMenu();
    openFilter.value = false;
  }
}

function onGlobalKeyDown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    // 弹窗开着时 Esc 只收弹窗——别把后面的选中态一起清了
    if (cleanTarget.value || artifactTarget.value) {
      cleanTarget.value = null;
      artifactTarget.value = null;
    } else if (menu.value || openFilter.value) {
      closeMenu();
      openFilter.value = false;
    } else if (selected.value) {
      selected.value = null;
    }
    return;
  }
  // 弹窗开着时键盘归弹窗：Tab 在弹窗内循环（两个弹窗都只有「取消 / 确认」两个按钮），
  // 其余快捷键一律不响应——否则 "/" 会把焦点搜到背后的表格、↑↓ 会改动幕后的选中行。
  const openDialog = cleanDialog.value ?? artifactDialog.value;
  if (openDialog) {
    if (e.key === "Tab") {
      const nodes = [...openDialog.querySelectorAll<HTMLElement>("button:not([disabled])")];
      if (nodes.length >= 2) {
        const first = nodes[0];
        const last = nodes[nodes.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    }
    return;
  }
  // 「/」聚焦搜索框（输入框里不拦）。搜索框暂时隐藏（v-if="false"）时元素不存在，
  // 不拦这个键，让它正常输入斜杠。
  const tag = (e.target as HTMLElement | null)?.tagName;
  if (e.key === "/" && tag !== "INPUT" && tag !== "TEXTAREA") {
    const input = document.getElementById("row-search");
    if (!input) return;
    e.preventDefault();
    input.focus();
    return;
  }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    const ids = visibleRows.value.map((r) => r.item.id);
    if (!ids.length) return;
    e.preventDefault();
    const cur = ids.indexOf(selected.value ?? "");
    const next = Math.max(
      0,
      Math.min(ids.length - 1, (cur < 0 ? -1 : cur) + (e.key === "ArrowDown" ? 1 : -1)),
    );
    selected.value = ids[next];
  }
}

onMounted(() => {
  // capture 阶段监听：行自己的 click 不该阻止菜单收起。
  // scroll 用 capture 是为了接住 <main> 内部容器的滚动（它不冒泡到 window）。
  window.addEventListener("pointerdown", onGlobalPointerDown, true);
  window.addEventListener("scroll", closeMenu, true);
  window.addEventListener("resize", closeMenu);
  window.addEventListener("keydown", onGlobalKeyDown);
});

onUnmounted(() => {
  window.removeEventListener("pointerdown", onGlobalPointerDown, true);
  window.removeEventListener("scroll", closeMenu, true);
  window.removeEventListener("resize", closeMenu);
  window.removeEventListener("keydown", onGlobalKeyDown);
  window.clearTimeout(flashTimer);
});

/** detail 的键名带 `N. ` 排序前缀（Rust 侧为保证 HashMap 顺序加的），展示时去掉。 */
function detailLabel(key: string): string {
  return key.replace(/^\d+\.\s*/, "");
}

function cell(it: Item, key: string, temporal?: boolean): string {
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

// ---- 列 ----

/**
 * 本组是否需要独立的「可回收」列。默认**不**需要——理由见下。
 *
 * 所有扫描器给可回收量都是「整行口径」：`reclaimable` 要么是 `None`
 * （不可回收），要么恰好等于 `size`（整行可回收，见 Rust 侧各处
 * `reclaimable: if cleanable { Some(size) }`，以及 `tests.rs` 里
 * `reclaimable <= size` 的守备）。独立列因此在分组视图里恒等于体积列——
 * 同一个数一黑一绿印两遍。改成体积列染绿表示「整行可回收」，省下的
 * 列宽给名称。
 *
 * 两种行不合并、仍走独立列：
 * ① 总览行——它的 reclaimable 是分组合计（toolchain 21.5 GB 里 5.8 GB
 *    可回收），与 size 天然不同；且总览自己在 fields 里声明了该列，
 *    走 fields 路径，不经过下面的插列判断；
 * ② 将来出现「部分可回收」的扫描器（可回收 < 体积）。这条是前端对应
 *    Rust 侧 `reclaimable <= size` 守备的另一半：染绿会把部分可回收
 *    虚报成全额可回收——「报超」比「报不满」坏得多（G3 同款教训），
 *    所以宁可保留一列重复的数字。
 */
const splitReclaim = computed(() => {
  const g = current.value;
  if (!g) return false;
  return g.items.some((it) => {
    const r = reclaimableOf(it);
    return r > 0 && r !== itemSize(it);
  });
});

/**
 * 名称列固定第一列，其余取前三个指标（`fields.slice(0, 3)`——第四个字段
 * 不会进表头也不会进列，Rust 侧契约守着这条）。
 * 体积列后补一列「可回收」只发生在 splitReclaim 的组；其余组可回收量
 * 与体积恒等，合并进体积列染绿（见 splitReclaim 与行内渲染）。
 */
const cols = computed(() => {
  const g = current.value;
  if (!g) return [{ key: "name", label: "名称" }];
  const out: { key: string; label: string; temporal?: boolean }[] = [
    { key: "name", label: "名称" },
  ];
  for (const f of g.fields.slice(0, 3)) {
    out.push({ key: f.key, label: f.label, temporal: f.temporal ?? undefined });
    // 总览的「占比」是前端派生列（见 shareOf），紧跟体积：部分与整体并排才读得出比例
    if (f.key === "size" && g.id === "overview") {
      out.push({ key: "share", label: "占比" });
    }
    if (
      f.key === "size" &&
      groupReclaimable(g) > 0 &&
      splitReclaim.value &&
      !g.fields.some((x) => x.key === "reclaimable")
    ) {
      out.push({ key: "reclaimable", label: "可回收" });
    }
  }
  return out;
});

/**
 * 列的角色决定宽度与对齐，不再一律 1fr + 左对齐：
 * - 体积 / 可回收是数字，右对齐才扫得动（`tabular-nums` 早就有，此前缺的是
 *   `text-right`——数字在宽列左边飘着，纵向没法比大小）；
 * - 条目 count 也是数字（总览），此前漏在 NUMERIC_KEYS 外左对齐——短数字贴在
 *   末列左缘，纵向对不齐数不了；
 * - 时间列内容固定很短（"1 个月前"），1fr 是浪费，收窄让给名称；
 * - 分支这类文本列留宽，名称永远吃掉剩下的。
 * mono 只给分支标识符（MONO_KEYS）。U14 曾把数字列也上 mono，U15 退回：
 * 等宽大数字是终端排印，账本的数字是正文字体的 tabular 等宽数字，对齐能力相同。
 */
const NUMERIC_KEYS = new Set(["size", "reclaimable", "share", "count"]);
/** 等宽更合适的列：分支名是标识符不是散文，正文字体下 `codex/xxx-yyy` 读起来散 */
const MONO_KEYS = new Set(["branch"]);
/**
 * 封闭枚举列按内容宽度：「类别」的值是扫描器给的固定几个串（下载缓存 /
 * 已安装 / 会话…），默认 1.2fr 有一半是空的，组内还常常七成同一个值。
 * `minmax(auto, max-content)` 的上限是内容宽、下限是 min-content——空间再
 * 紧也只压到最长一个词的宽度，不会把四个字截成一个字。分支这类开放标识符
 * 不吃这套（一个长分支名能把表撑爆），留在默认 1.2fr。
 */
const CONTENT_KEYS = new Set(["kind"]);

const colMeta = computed(() =>
  cols.value.map((c) => {
    let width = "minmax(0,1.2fr)";
    let align = "text-left";
    if (c.key === "name") {
      width = "minmax(0,2.2fr)";
    } else if (NUMERIC_KEYS.has(c.key)) {
      width = "minmax(0,1fr)";
      align = "text-right";
    } else if (CONTENT_KEYS.has(c.key)) {
      // 类别列改用固定比例而非 max-content：长类别名会撑破容器导致横向滚动
      width = "minmax(0,1fr)";
    } else if (c.temporal) {
      width = "minmax(0,0.9fr)";
      align = "text-right";
    }
    return { ...c, width, align, mono: MONO_KEYS.has(c.key) };
  }),
);

const gridCols = computed(() => colMeta.value.map((c) => c.width).join(" "));

/**
 * 列头 tooltip。体积列的绿色含义是合并可回收列后唯一需要解释的新语义：
 * 「绿 = 可回收」在状态栏、分节小计、检查器三处已有锚点，这里补「是什
 * 么」——绿色数字即该行整体可回收。splitReclaim 的组体积列不染绿，
 * 不给 tooltip。
 */
function colTitle(c: { key: string }): string | undefined {
  if (c.key === "size" && !splitReclaim.value) {
    return "体积；绿色数字表示该行整体可回收（移入废纸篓）";
  }
  return undefined;
}

// ---- 行：基础集合 + 筛选 + 排序 ----

/** 总览视图的行就是各分组（点它进分组）；分组视图的行是 items。 */
const baseRows = computed(() => {
  const g = current.value;
  if (!g) return [];
  return g.items.map((item) => ({ item, rowName: item.name }));
});

const searchHit = (it: Item) => {
  const q = search.value.trim().toLowerCase();
  if (!q) return true;
  // 路径也参与匹配：搜 ~/Documents/Code 下的目录名往往比仓库名更好记
  return it.name.toLowerCase().includes(q) || it.path.toLowerCase().includes(q);
};

const visibleRows = computed(() => {
  let rows = baseRows.value.filter((r) => searchHit(r.item));
  const active = Object.entries(dims.value).filter(([, v]) => v);
  if (active.length) {
    rows = rows.filter((r) => active.every(([k, v]) => matchDim(r.item, k, v!)));
  }
  const key = sortKey.value;
  if (key) {
    const dir = sortDesc.value ? -1 : 1;
    rows = [...rows].sort((a, b) => {
      const va = sortValue(a.item, key);
      const vb = sortValue(b.item, key);
      if (typeof va === "number" && typeof vb === "number") return (va - vb) * dir;
      return String(va).localeCompare(String(vb), "zh-CN") * dir;
    });
  }
  return rows;
});

function sortValue(it: Item, key: string): number | string {
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

function toggleSort(key: string) {
  withRowFlip(() => {
    if (sortKey.value === key) {
      sortDesc.value = !sortDesc.value;
    } else {
      sortKey.value = key;
      // 按名称升序更自然（Z→A 没意义），其余默认降序（体积/时间都是越大越前）
      sortDesc.value = key !== "name";
    }
  });
}

// ---- 筛选维度 ----
//
// 一条 item 的维度取值在不同分组装的是不同的东西：toolchain 的 `group_key` 是
// 「栈」、ai_tools 是「工具名」、java 是「组件」。所以维度由分组声明、名字也由
// 分组给（PREFIX_LABEL）——不是同一根轴的三种叫法，是三种不同的轴。
// 分节与筛选都读 `group_key`（G1 起正式化成数据），不再解析名字里的 ` · `。
// 只有某个维度真有 ≥2 个取值时才做成筛选项：tag 只有一个「可清理」的分组，
// 做成筛选没有意义（选了等于没选），直接不出。

const PREFIX_LABEL: Record<string, string> = {
  toolchain: "栈",
  ai_tools: "工具",
  java: "组件",
  local_models: "来源",
};

interface Dimension {
  key: string;
  label: string;
  options: string[];
}

function dimensionsFor(g: AssetGroup | null): Dimension[] {
  if (!g || !g.items.length) return [];
  const dims: Dimension[] = [];

  const pl = PREFIX_LABEL[g.id];
  if (pl) {
    const vals = new Set<string>();
    for (const it of g.items) if (it.group_key) vals.add(it.group_key);
    if (vals.size >= 2) {
      dims.push({ key: "prefix", label: pl, options: [...vals].sort() });
    }
  }

  const kinds = new Set<string>();
  for (const it of g.items) {
    const m = it.metrics.kind;
    if (m && m.t === "s") kinds.add(m.v);
  }
  if (kinds.size >= 2) {
    dims.push({ key: "kind", label: "类别", options: [...kinds].sort() });
  }

  const tags = new Set<string>();
  for (const it of g.items) for (const t of it.tags) tags.add(t);
  if (tags.size >= 2) {
    dims.push({ key: "tag", label: "标签", options: [...tags].sort() });
  }
  return dims;
}

const dimsOfCurrent = computed(() => dimensionsFor(current.value));

function matchDim(it: Item, key: string, val: string): boolean {
  if (key === "prefix") return it.group_key === val;
  if (key === "kind") {
    const m = it.metrics.kind;
    return !!m && m.t === "s" && m.v === val;
  }
  if (key === "tag") return it.tags.includes(val);
  return true;
}

/**
 * 菜单里每项的计数按「其它维度已经筛过」的口径算——否则数字跟筛选结果对不上。
 */
function optionCount(dimKey: string, value: string | null): number {
  const rest = Object.entries(dims.value).filter(
    ([k, v]) => k !== dimKey && v,
  );
  return baseRows.value.filter(
    (r) =>
      searchHit(r.item) &&
      rest.every(([k, v]) => matchDim(r.item, k, v!)) &&
      (value === null || matchDim(r.item, dimKey, value)),
  ).length;
}

function setDim(key: string, value: string | null) {
  withRowFlip(() => {
    dims.value = { ...dims.value, [key]: value };
    openFilter.value = false;
    selected.value = null;
  });
}
const anyFilter = computed(() => Object.values(dims.value).some(Boolean));
/** 生效中的维度数。漏斗按钮上那个小数字，0 时不显示。 */
const activeDims = computed(() => Object.values(dims.value).filter(Boolean).length);

function clearFilters() {
  withRowFlip(() => {
    dims.value = {};
    openFilter.value = false;
  });
}

// ---- 分节 ----

/**
 * 按 `group_key` 分节渲染：取值做小节标题（带小计），行名只显示后半截。
 * AI 工具分组一个工具就有多个类别行，名称列每行重复「WorkBuddy ·」是纯噪音。
 * 整组都没有 group_key（repos / 鸿蒙 / WSL）时走单组，不分节。
 */
const sections = computed(() => {
  const rows = visibleRows.value;
  const g = current.value;
  if (!g || !rows.length || !rows.every((r) => r.item.group_key)) {
    return [{ key: null as string | null, rows }];
  }
  const map = new Map<string, typeof rows>();
  for (const r of rows) {
    const k = r.item.group_key!;
    if (!map.has(k)) map.set(k, []);
    map.get(k)!.push(r);
  }
  return [...map.entries()].map(([key, rs]) => ({ key, rows: rs }));
});

/** 分节行名去掉 `group_key · ` 前缀（名字仍保留全串，搜索与检查器用全串）。 */
function rowName(it: Item): string {
  const prefix = it.group_key ? `${it.group_key} · ` : "";
  return it.name.startsWith(prefix) ? it.name.slice(prefix.length) : it.name;
}

// ---- 多数派标签抑制 ----
//
// 「大」在体积降序视图里几乎恒真（默认排序就是体积降序），「有产物」在本机仓库里
// 也是十之八九——这类标签每行都挂，信息量为零，还把每行撑成两行。规则是数据驱动的
// 一条：某标签在当前**可见行**里占了绝大多数，这一屏就不渲染它。
// 两个护栏：可见行太少时不抑制（筛到三两行时标签是主要信息）；只影响渲染——
// 筛选维度读的是 `item.tags` 原始数据（`dimensionsFor` / `matchDim`），菜单里照旧可筛。

const MAJORITY_RATIO = 0.7;
const MAJORITY_MIN_ROWS = 6;

const suppressedTags = computed(() => {
  const rows = visibleRows.value;
  const out = new Set<string>();
  if (rows.length < MAJORITY_MIN_ROWS) return out;
  const counts = new Map<string, number>();
  for (const r of rows) for (const t of r.item.tags) counts.set(t, (counts.get(t) ?? 0) + 1);
  for (const [t, n] of counts) if (n / rows.length >= MAJORITY_RATIO) out.add(t);
  return out;
});

function visibleTags(it: Item): string[] {
  return it.tags.filter((t) => {
    if (suppressedTags.value.has(t)) return false;
    // 「可清理」chip 与绿字可回收是同一件事的两次陈述：三个可清理分组的账面上
    // reclaimable>0 ⟺ 有删除入口。绿字已经在体积列里（可回收合并进体积列，
    // 见 splitReclaim），chip 不再挂第二行——它一挂，行就变两行，整列一多半
    // 行都是同一块灰牌子。
    // reclaimable==0 却标了可清理的异常行保留 chip；筛选维度仍读原始 tags。
    if (t === "可清理" && reclaimableOf(it) > 0) return false;
    // 「大」只是体积过阈值的别名，体积列就在同一行右边——行内再写一遍是同一事实
    // 的第二遍陈述（U15 去药丸时顺手收掉；筛选菜单仍读原始 tags，照常可按「大」筛）。
    if (t === "大") return false;
    return true;
  });
}

function sectionSize(rows: { item: Item }[]): number {
  return rows.reduce((s, r) => s + itemSize(r.item), 0);
}

function sectionReclaim(rows: { item: Item }[]): number {
  return rows.reduce((s, r) => s + (r.item.cleanable ? reclaimableOf(r.item) : 0), 0);
}

// ---- 行点击：总览行进分组，分组行选中看明细 ----

const isOverview = computed(() => current.value?.id === "overview");

/** 行 tooltip 给全路径（名称被截断时的兜底）。总览行的 path 是合成串
 * overview://<分组id>，不是宿主机路径，挂出去是实现细节漏进 UI，不给。 */
function rowTitle(it: Item): string | undefined {
  return isOverview.value ? undefined : it.path;
}

function onRowClick(it: Item) {
  closeMenu();
  if (isOverview.value) {
    const gid = it.id.startsWith("overview:") ? it.id.slice("overview:".length) : "";
    if (gid) {
      router.push(`/assets/${gid}`);
      return;
    }
  }
  selected.value = selected.value === it.id ? null : it.id;
}

// ---- 检查器 ----

const selectedRow = computed(() => {
  if (!selected.value) return null;
  return current.value?.items.find((it) => it.id === selected.value) ?? null;
});

/** 检查器内容切换时重放一次 page-in；把分组 id 也编进 key，
 *  否则「未选中行」在各分组间切换时 key 不变，页面没有进入动效。 */
const inspectorKey = computed(
  () => selectedRow.value?.id ?? `group:${current.value?.id ?? ""}`,
);

const reclaimTotal = computed(() =>
  current.value ? groupReclaimable(current.value) : 0,
);

/** 总览首块「本机合计」：总占用 / 可回收 / 分组数 / 近 7 天有提交。 */
const machineStats = computed(() => {
  const real = groups.value.filter((g) => g.id !== "overview");
  const total = real.reduce((s, g) => s + groupSize(g), 0);
  const reclaim = real.reduce((s, g) => s + groupReclaimable(g), 0);
  const repos = real.find((g) => g.id === "repos");
  const week = 7 * 86400;
  const recent = repos
    ? repos.items.filter((it) => {
        const m = it.metrics.last_commit;
        return m && (m.t === "b" || m.t === "i") && Date.now() / 1000 - m.v < week;
      }).length
    : 0;
  return { total, reclaim, count: real.length, recent };
});


/**
 * 总览「占比」列（U15）：前端派生列，不是 Rust 下发的 field——fields 契约
 * （最多三个）不动。份额分母是全部分组的合计（machineStats.total），不是当前
 * 可见行之和：筛掉几行后占比不该跟着变（它说的是「占整机多少」）。
 * 标注档位沿用原占比带：≥10% 取整、≥0.5% 一位小数、再小只写 <0.1%。
 */
function shareOf(it: Item): number {
  const total = machineStats.value.total;
  return total > 0 ? itemSize(it) / total : 0;
}
function shareLabel(it: Item): string {
  const share = shareOf(it);
  if (share <= 0) return "";
  if (share >= 0.1) return `${Math.round(share * 100)}%`;
  if (share >= 0.005) return `${(share * 100).toFixed(1)}%`;
  return "<0.1%";
}

const statusSum = computed(() =>
  visibleRows.value.reduce((s, r) => s + itemSize(r.item), 0),
);
const statusReclaim = computed(() =>
  visibleRows.value.reduce((s, r) => s + (r.item.cleanable ? reclaimableOf(r.item) : 0), 0),
);

const scanPct = computed(() => {
  const p = progress.value;
  return p && p.total > 0 ? Math.round((p.done / p.total) * 100) : 0;
});

const stale = computed(() => cachedAt.value != null && Date.now() / 1000 - cachedAt.value > 86400);

async function onRescan() {
  const before = sizeSnapshot();
  const r = await rescan();
  if (r.ok) {
    flashChanged(before);
    toast.success("扫描完成");
  } else {
    toast.error(`扫描失败：${r.message}`);
  }
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!-- 工具栏：标题 + 维度筛选 + 搜索 + 外观/检查器/重扫。
         空区是窗口拖动区（titleBarStyle: Overlay 要自己给）。
         整条挂 drag-region，控件用 pointer-events: auto 恢复交互。 -->
    <div class="flex h-11 shrink-0 items-center gap-2 border-b border-border/40 bg-background px-3" data-tauri-drag-region>
      <h1 class="min-w-0 truncate text-sm font-semibold">
        {{ current?.label ?? (loading ? "首次扫描…" : "无数据") }}
      </h1>
      <!-- 条目数不挂在这里：状态栏与检查器概览各有一份，同一屏三处「238 项 · 109 GB」
           是纯重复。工具栏留给标题与控件。 -->

      <div class="flex-1" />

      <!-- 筛选：一个漏斗按钮展开所有维度。总览没有维度可筛，按钮不出。
           此前是「每个维度一个描边按钮 + 一个清空按钮」，分组多时工具栏一排小盒子，
           线条感的主场；合并成一个入口也就不必给每个维度留位置。 -->
      <div v-if="dimsOfCurrent.length" class="relative shrink-0" data-dim-menu>
        <button
          :class="[toolbarCtl, anyFilter ? 'text-foreground' : '']"
          :title="anyFilter ? `筛选（已生效 ${activeDims} 项）` : '筛选'"
          aria-label="筛选"
          @click="openFilter = !openFilter"
        >
          <ListFilter class="size-3.5" />
          <span
            v-if="activeDims"
            :key="activeDims"
            class="badge-pop tabular-nums text-[10px]"
          >{{ activeDims }}</span>
        </button>
        <div
          v-if="openFilter"
          class="menu-in absolute right-0 top-full z-30 mt-1 min-w-44 rounded-md border border-border bg-card py-1 shadow-md"
        >
          <template v-for="(d, i) in dimsOfCurrent" :key="d.key">
            <div v-if="i" class="my-1 border-t border-border/60" />
            <div class="px-2 pb-0.5 pt-1 text-[10px] text-muted-foreground">{{ d.label }}</div>
            <button
              class="flex w-full items-center justify-between gap-3 px-2 py-1 text-left text-xs text-foreground hover:bg-muted"
              :class="!dims[d.key] ? 'font-medium' : ''"
              @click="setDim(d.key, null)"
            >
              <span class="flex min-w-0 items-center gap-2">
                <span class="filter-dot" :class="!dims[d.key] ? 'on' : ''" />
                <span>全部</span>
              </span>
              <span class="tabular-nums text-muted-foreground">{{ optionCount(d.key, null) }}</span>
            </button>
            <button
              v-for="v in d.options"
              :key="v"
              class="flex w-full items-center justify-between gap-3 px-2 py-1 text-left text-xs text-foreground hover:bg-muted"
              :class="dims[d.key] === v ? 'font-medium' : ''"
              @click="setDim(d.key, v)"
            >
              <span class="flex min-w-0 items-center gap-2">
                <span class="filter-dot" :class="dims[d.key] === v ? 'on' : ''" />
                <span class="truncate">{{ v }}</span>
              </span>
              <span class="tabular-nums text-muted-foreground">{{ optionCount(d.key, v) }}</span>
            </button>
          </template>
          <div v-if="anyFilter" class="mt-1 border-t border-border/60 pt-1">
            <button
              class="flex w-full items-center px-2 py-1 text-left text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              @click="clearFilters"
            >
              清除筛选条件
            </button>
          </div>
        </div>
      </div>

      <!-- label 必须自己也是 flex：否则 input 作为 inline-block 按 baseline 对齐、底下还挂一段
           strut 下行空隙，整个输入框在 WebKit 下向下溢出 2px，输入框里的文字因此比图标低
           2.75px（固定在盒子中心的图标就显得偏高）。实测：加 flex 后 input 精确落在 0~24。 -->
      <!-- 搜索先隐藏（2026-10-20）。整块 v-if 移出 DOM 而不是 display:none——
           留在 DOM 里 `/` 还能把焦点打进一个看不见的输入框。恢复时删掉 v-if 即可，
           下方的过滤逻辑与 `/` 快捷键守卫都原样保留。 -->
      <label v-if="false" class="group relative flex h-6 shrink-0">
        <span class="sr-only">按名称或路径筛选</span>
        <!-- mt-px：label 改成 flex 后 input 正好占满 24px，但 WebKit 对 input 内部文字的
             放置仍比盒子几何中心低 0.75px（改 line-height 无效，实测 leading-none 也是同一个值）。
             实测下移 1px 后偏差从 +0.75 收到 -0.25。 -->
        <Search
          class="pointer-events-none absolute left-1.5 top-1/2 mt-px size-3 -translate-y-1/2 text-muted-foreground"
        />
        <input
          id="row-search"
          v-model="search"
          type="search"
          placeholder="筛选名称 / 路径"
          class="h-6 w-32 appearance-none rounded-md bg-muted/60 pl-6 pr-6 text-xs text-foreground transition-[width] duration-150 placeholder:text-muted-foreground focus:w-52 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring sm:w-40 sm:focus:w-56"
        />
        <!-- `/` 聚焦快捷键一直存在但没提示过；有输入内容或已聚焦时让位给清除按钮 -->
        <!-- 下面三个挂件与放大镜同一个规则：都落在「input 文字的视觉中线」而不是盒子几何中心上，
             所以一律带 mt-px（否则左右挂件之间会差这 1px）。 -->
        <kbd
          v-if="!search"
          class="pointer-events-none absolute right-1.5 top-1/2 mt-px -translate-y-1/2 rounded border border-border bg-muted px-1 text-[10px] leading-none text-muted-foreground/70 group-focus-within:hidden"
        >
          /
        </kbd>
        <button
          v-if="search"
          class="absolute right-1 top-1/2 mt-px -translate-y-1/2 text-muted-foreground hover:text-foreground"
          aria-label="清除筛选"
          @click="search = ''"
        >
          <X class="size-3" />
        </button>
      </label>

      <div class="flex shrink-0 items-center gap-1" :class="isWindows ? 'pr-[140px]' : ''">
        <!-- 图标语义是「点了去哪个模式」：深色里显示太阳（点它回浅色），反之月亮。
             vue-sonner 自己不读 .dark class，theme 必须显式传。 -->
        <Button
          size="xs"
          variant="ghost"
          class="w-7 px-0"
          :title="theme === 'dark' ? '切换到浅色' : '切换到深色'"
          :aria-label="theme === 'dark' ? '切换到浅色模式' : '切换到深色模式'"
          @click="toggleTheme"
        >
          <Sun v-if="theme === 'dark'" />
          <Moon v-else />
        </Button>
        <!-- 总览没有检查器（数字都在主区首块），开关一并隐藏，点了也是空操作 -->
        <Button
          v-if="!isOverview"
          size="xs"
          variant="ghost"
          class="w-7 px-0"
          :title="inspectorOpen ? '隐藏检查器' : '显示检查器'"
          :aria-label="inspectorOpen ? '隐藏检查器' : '显示检查器'"
          @click="inspectorOpen = !inspectorOpen"
        >
          <PanelLeft :class="inspectorOpen ? '' : 'opacity-50'" />
        </Button>
        <Button
          size="xs"
          variant="ghost"
          :disabled="loading"
          class="min-w-[4.5rem]"
          @click="onRescan"
        >
          <RefreshCw :class="loading ? 'motion-safe:animate-spin' : ''" />
          {{ loading ? (progress ? `扫描中 ${progress.done}/${progress.total}` : "扫描中") : "重扫" }}
        </Button>
      </div>
    </div>

    <!-- 真实进度条：贴在工具栏下沿（U2），替掉按钮里一个干转的 spinner。
         U16 起 1px 淡墨线 + 行首笔尖；进度用 transform 走合成层。 -->
    <Transition name="fade">
      <ScanBar v-if="loading" :pct="scanPct" />
    </Transition>

    <!-- 错误条：仅记录启动/首次加载失败。重扫失败走 toast -->
    <div
      v-if="error"
      class="m-3 flex items-start gap-2 rounded-md border border-destructive/20 bg-destructive/10 px-3 py-2.5"
    >
      <AlertTriangle class="mt-px size-4 shrink-0 text-destructive" />
      <div class="min-w-0 text-xs text-destructive">
        <p class="font-medium">扫描失败</p>
        <p class="mt-0.5 break-all opacity-80">{{ error }}</p>
      </div>
    </div>

    <!-- 主区：列表 + 检查器（主从，原型 v3） -->
    <div class="flex min-h-0 flex-1">
      <div ref="scrollArea" class="min-w-0 flex-1 overflow-y-auto relative">
        <!-- key 按分组重挂内容，page-enter 才有「翻页」的方向感；数据重扫时
             id 不变，行只 patch，不重放整页进入动效。 -->
        <div
          ref="scrollContent"
          :key="current?.id ?? 'empty'"
          class="page-enter min-h-full"
        >
        <!-- 首次扫描：此时明确告知在做什么，否则 26 秒白屏会让人以为卡死了 -->
        <!-- 空态左对齐、贴在表头下方那一行上，不居中：居中 + 图标在上 + 灰字是
             「这界面是生成出来的」的第一指纹，也是唯一一个纯靠位置就能露馅的特征。
             副标题 10px → 11px：它是信息不是装饰，踩到「信息性文字 ≥11px」的纪律。 -->
        <div v-if="!current && loading && !error" class="px-3 py-6">
          <LoaderCircle
            class="size-4 motion-safe:animate-spin motion-reduce:animate-[spin_2.5s_linear_infinite] text-muted-foreground"
          />
          <p class="mt-2 text-xs text-foreground">正在首次扫描本机…</p>
          <p class="mt-1 text-[11px] text-muted-foreground">
            {{ progress ? `已完成 ${progress.done}/${progress.total} 个分组` : "约 26 秒，之后打开直接读缓存" }}
          </p>
        </div>

        <p
          v-else-if="!loading && !error && !current"
          class="px-3 py-6 text-xs text-muted-foreground"
        >
          尚无数据，点顶栏「重扫」开始
        </p>

        <template v-else-if="current">
          <!-- 总览首块（台账式页头）：合计 / 可回收进主区，检查器不再重复。
               一个主角数字，不是一排卡片——卡片形态早先被否过，层级靠字号不靠盒子。
               占比带已删（U15）：带与表格同序同数是同一笔账印两遍，占比改进表格的
               「占比」列；带内印字的分段进度条是云存储仪表盘的构图，不是账本的。 -->
          <div v-if="isOverview && machineStats.total > 0" class="px-3 pt-3">
            <div class="flex flex-wrap items-end justify-between gap-x-6 gap-y-1.5">
              <div>
                <div class="text-[11px] text-muted-foreground">本机合计</div>
                <!-- 30px 正文字体主角位（U15 退回正文）：等宽大数字是终端排印，账本的数字
                     是正文字体的 tabular 等宽数字——对齐能力相同，气质是书的黑不是终端的绿。
                     走 ByteValue：值 500 字重、单位 11px 同色，与全表同一套排印。 -->
                <p class="hero-figure mt-1 text-[30px] font-semibold leading-none tracking-tight">
                  <ByteValue :bytes="machineStats.total" animate />
                </p>
              </div>
              <p class="pb-0.5 text-xs text-muted-foreground">
                可回收
                <ByteValue class="text-reclaim" :bytes="machineStats.reclaim" animate />
                · {{ machineStats.count }} 个分组 · 近 7 天 {{ machineStats.recent }} 个仓库有提交
              </p>
            </div>
            <!-- 账本头线：页头与账目之间一条墨线，全页唯一的重线。合计属于页眉，
                 线以下才是条目；分组视图没有页头也就没有这条线（一页最多一条重线，
                 多了就退回网格纸）。进入时从左侧画到右侧，像落笔划线。 -->
            <div class="head-rule mt-3" />
          </div>
          <!-- 仓库那组两百多行滚下去表头就没了，不知道每列是什么，所以吸顶 -->
          <div
            v-if="visibleRows.length"
            class="sticky top-0 z-10 grid gap-2 border-b border-border/40 bg-background px-3 pb-1.5 pt-1.5 text-xs text-muted-foreground"
            :style="{ gridTemplateColumns: gridCols }"
          >
            <button
              class="min-w-0 text-left transition-colors hover:text-foreground"
              :aria-label="`按名称排序`"
              :aria-sort="sortKey === 'name' ? (sortDesc ? 'descending' : 'ascending') : 'none'"
              @click="toggleSort('name')"
            >
              名称<span v-if="sortKey === 'name'">{{ sortDesc ? " ↓" : " ↑" }}</span>
            </button>
            <button
              v-for="c in colMeta.slice(1)"
              :key="c.key"
              class="min-w-0 truncate transition-colors hover:text-foreground"
              :class="c.align"
              :aria-label="`按${c.label}排序`"
              :aria-sort="sortKey === c.key ? (sortDesc ? 'descending' : 'ascending') : 'none'"
              :title="colTitle(c)"
              @click="toggleSort(c.key)"
            >
              {{ c.label }}<span v-if="sortKey === c.key">{{ sortDesc ? " ↓" : " ↑" }}</span>
            </button>
          </div>

          <!-- 行界 = 账本行线（U15 推翻「行间不画线」）：此前怕两百条线读成网格纸，
               但那时行里还有占比条与药丸两种灰块在争抢——三种灰叠在一起才是网格纸。
               行条与药丸都删掉后，26px 节距上的 1px 暖 hairline 正是「划好线的账页」，
               这是纸账本身份的主要来源之一，不是装饰。节与节之间同一档线。
               分节标题降成 11px 弱标签——它不是一行数据，不该跟行名抢字重。 -->
          <div
            v-for="(sec, si) in sections"
            :key="sec.key ?? '_'"
            :class="si > 0 ? 'border-t border-border/40' : ''"
          >
            <div
              v-if="sec.key"
              class="flex items-baseline justify-between px-3 pb-1 pt-4"
            >
              <span class="text-[11px] font-semibold text-muted-foreground">{{ sec.key }}</span>
              <span class="text-[11px] tabular-nums text-muted-foreground">
                <ByteValue :bytes="sectionSize(sec.rows)" />
                <!-- 绿色语义收敛在 --reclaim token（U14）：浅 6.51:1 / 深 8.91:1，
                     11px 数字也过 4.5:1；调色只动 token，不在这里挑 Tailwind 色号。 -->
                <template v-if="sectionReclaim(sec.rows) > 0">
                  · 可回收
                  <ByteValue
                    class="text-reclaim"
                    :bytes="sectionReclaim(sec.rows)"
                  />
                </template>
              </span>
            </div>

            <ul>
              <li
                v-for="row in sec.rows"
                :key="row.item.id"
                :data-row-id="row.item.id"
                class="ledger-row border-t border-border/40 first:border-t-0"
                :class="[
                  selected === row.item.id ? 'selected' : '',
                  flashIds.has(row.item.id) ? 'flash' : '',
                  isOverview ? 'is-overview' : '',
                ]"
              >
                <!-- 墨洗与行首刻线垫在按钮下面（按钮由 .ledger-row 抬到 z-1），
                     不参与命中区域；选中 / hover 的形态集中在 .ledger-row 一层，
                     不再用 bg-muted 整行硬切。 -->
                <span class="row-wash" aria-hidden="true" />
                <span class="row-tick" aria-hidden="true" />
                <!-- 总览行是分组行，只有 6 行，行距放大一档；分组视图回到 26px 紧凑档。 -->
                <button
                  class="grid w-full min-w-0 items-baseline gap-2 px-3 text-left leading-[18px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  :style="{ gridTemplateColumns: gridCols }"
                  :class="isOverview ? 'py-2.5' : 'py-1'"
                  :title="rowTitle(row.item)"
                  @click="onRowClick(row.item)"
                  @contextmenu.prevent="openMenu($event, row.item)"
                >
                  <!-- 行背景占比条已删（U15）：238 行按体积归一的灰条连成一道灰阶梯，
                       把表格读成仪表盘热力图；账本的比例语义由数字列与总览「占比」列承担。
                       名称与状态同一行：状态是 11px 灰字不是药丸——药丸把单行撑成两行、
                       全列铺满小灰块纹理，是生成式列表 UI 的第一指纹。两段都可截断，
                       按各自宽度比例分摊挤压，窄窗口下谁都不会把谁顶出去。 -->
                  <span class="flex min-w-0 items-baseline gap-2">
                    <span
                      class="min-w-0 truncate"
                      :class="isOverview ? 'text-[14px] font-medium' : 'text-[13px]'"
                    >{{ rowName(row.item) }}</span>
                    <span
                      v-if="visibleTags(row.item).length"
                      class="min-w-0 truncate text-[11px] text-muted-foreground"
                    >{{ visibleTags(row.item).join(" · ") }}</span>
                  </span>

                  <span
                    v-for="c in colMeta.slice(1)"
                    :key="c.key"
                    class="min-w-0 truncate tabular-nums"
                    :class="[
                      c.align,
                      // 值近黑、时间列才灰：此前除 size 外一律 muted，一屏四列里三列
                      // 是同一个灰，整表没有黑锚点——这是「洗白模板」观感的来源。
                      // 判据用 c.temporal 而不是列 key：维度列（分支 / 类别）显示的
                      // 也是值，跟时间这种元数据不是一回事。
                      c.temporal ? 'text-muted-foreground' : 'text-foreground',
                      c.mono ? 'font-mono text-[11px]' : '',
                      isOverview ? 'text-[13px]' : 'text-xs',
                    ]"
                  >
                    <template v-if="c.key === 'reclaimable'">
                      <!-- 没有回收量的行留空而不是打「—」：一屏十几个破折号读起来像这列坏了。
                           「账面为 0」与「无数据」在这列没有区分价值，列头与绿色数字已经说清了。
                           只有 splitReclaim 的组（当前仅总览）才有这一列。 -->
                      <ByteValue
                        v-if="reclaimableOf(row.item) > 0"
                        class="text-reclaim"
                        :bytes="reclaimableOf(row.item)"
                        :animate="isOverview"
                      />
                    </template>
                    <template v-else-if="c.key === 'size' && !splitReclaim && reclaimableOf(row.item) > 0">
                      <!-- 合并口径（见 splitReclaim）：可回收量与体积相同的行，体积数字
                           直接染绿。显示的是体积（列头是「体积」），两者当前恒等；
                           真出现不一致时 splitReclaim 把该组切回独立列，不会走到这。 -->
                      <ByteValue
                        class="text-reclaim"
                        :bytes="itemSize(row.item)"
                        :animate="isOverview"
                      />
                    </template>
                    <!-- 体积列无论染不染绿都走组件：它是这张表最该有排印层次的一列
                         （值比单位重一档）。此前走 cell() 拿到的是一整串字符串，拆不开。 -->
                    <ByteValue v-else-if="c.key === 'size'" :bytes="itemSize(row.item)" :animate="isOverview" />
                    <template v-else-if="c.key === 'share'">{{ shareLabel(row.item) }}</template>
                    <template v-else>{{ cell(row.item, c.key, c.temporal) }}</template>
                  </span>
                </button>
              </li>
            </ul>
          </div>

          <div v-if="!visibleRows.length" class="px-3 py-6">
            <p class="text-xs text-foreground">无匹配项</p>
            <button
              v-if="search || anyFilter"
              class="mt-1 text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground"
              @click="((search = ''), clearFilters())"
            >
              清除筛选条件
            </button>
          </div>
        </template>
        </div>
      </div>

      <!-- 检查器：原来四张卡片搬到这里（D5 降级后保留外观与交互部分）。
           收起时不卸载：宽度 252 → 0 让主区跟着收放，内容用 inert 挡键盘，
           避免「瞬间消失」把主区宽度一步跳过去。 -->
      <aside
        v-if="current && !isOverview"
        class="inspector flex shrink-0 flex-col border-l border-border/30"
        :class="{ closed: !inspectorOpen }"
        :inert="!inspectorOpen"
        :aria-hidden="!inspectorOpen"
      >
        <!-- 总览不进检查器（台账式首块把合计 / 可回收 / 占比带搬进了主区），
             这里的数字只会与主区重复；工具栏的检查器开关在总览一并隐藏。 -->
        <div :key="inspectorKey" class="page-in min-h-0">
        <template v-if="selectedRow">
          <p class="min-w-0 break-words text-[13px] font-medium">{{ selectedRow.name }}</p>
          <p v-if="isLocalPath(selectedRow.path)" class="mt-0.5 break-words font-mono text-[10px] text-muted-foreground">
            {{ selectedRow.path }}
          </p>

          <!-- 路径动作条：只对宿主机绝对路径出现。flex-wrap：清理按钮进来后
               多个 xs 按钮在 228px 内容区里会超宽（S2）。复制路径不在这条上——
               2026-10-19 按用户要求移除，入口收进行右键菜单一处。 -->
          <div v-if="isLocalPath(selectedRow.path)" class="mt-2 flex flex-wrap items-center gap-1.5">
            <Button size="xs" variant="outline" @click="onReveal(selectedRow)">
              <FolderOpen />
              {{ revealLabel }}
            </Button>
            <Button
              v-if="canClean(selectedRow)"
              size="xs"
              variant="outline"
              class="text-destructive"
              @click="askClean(selectedRow)"
            >
              <Trash2 />
              清理
            </Button>
            <Button
              v-if="canCleanArtifacts(selectedRow)"
              size="xs"
              variant="outline"
              class="text-destructive"
              @click="askCleanArtifacts(selectedRow)"
            >
              <Trash2 />
              清理产物
            </Button>
          </div>

          <!-- 标签与表格行同一形态：灰字不是药丸（U15），检查器里也不留一处例外 -->
          <p v-if="selectedRow.tags.length" class="mt-2 text-[11px] text-muted-foreground">
            {{ selectedRow.tags.join(" · ") }}
          </p>

          <div class="mt-3 text-[11px] font-semibold text-muted-foreground">指标</div>
          <dl class="mt-1 space-y-1">
            <template v-for="f in current.fields.slice(0, 3)" :key="f.key">
              <div class="flex items-baseline justify-between gap-2">
                <dt class="text-xs text-muted-foreground">{{ f.label }}</dt>
                <!-- 与表格同一口径：可回收量与体积恒等的行（splitReclaim 为 false），
                     体积在指标区也染绿，不再单列一行「可回收」——那是同一个数印第二遍。
                     splitReclaim 的组保留独立的可回收行。 -->
                <dd
                  class="text-xs tabular-nums"
                  :class="f.key === 'size' && !splitReclaim && reclaimableOf(selectedRow) > 0 ? 'text-reclaim' : ''"
                >
                  {{ cell(selectedRow, f.key, f.temporal) }}
                </dd>
              </div>
            </template>
            <div v-if="splitReclaim && reclaimableOf(selectedRow) > 0" class="flex items-baseline justify-between gap-2">
              <dt class="text-xs text-muted-foreground">可回收</dt>
              <dd class="text-xs text-reclaim">
                <ByteValue :bytes="reclaimableOf(selectedRow)" />
              </dd>
            </div>
            <div v-if="artifactBytes(selectedRow) > 0" class="flex items-baseline justify-between gap-2">
              <dt class="text-xs text-muted-foreground">构建产物</dt>
              <dd class="text-xs"><ByteValue :bytes="artifactBytes(selectedRow)" /></dd>
            </div>
          </dl>

          <template v-if="selectedRow.detail">
            <div class="mt-4 text-[11px] font-semibold text-muted-foreground">明细</div>
            <dl class="mt-1 space-y-1">
              <!-- Rust 侧给 detail 的键加了 `N. ` 前缀是为了压住 HashMap 的不确定顺序，
                   这个编号是给序列化用的，不该出现在界面上 -->
              <div v-for="(v, k) in selectedRow.detail" :key="k" class="flex gap-2">
                <dt class="w-16 shrink-0 truncate text-xs text-muted-foreground" :title="detailLabel(k)">
                  {{ detailLabel(k) }}
                </dt>
                <!-- break-words 而不是 break-all：后者会在单词中间断行
                     （"dist, node_modules, target" 断成 "targe t"），前者优先在空格断，
                     无空格的长路径 / URL 仍能在容器边缘换行。
                     whitespace-pre-line 给「产物明细」的多行值用（repos 的产物
                     逐目录一行收在单键里，detail 是 HashMap、序列化序随机） -->
                <dd class="min-w-0 whitespace-pre-line break-words font-mono text-[11px] text-foreground">{{ v }}</dd>
              </div>
            </dl>
          </template>
        </template>

        <template v-else>
          <div class="text-[11px] font-semibold text-muted-foreground">{{ current.label }}</div>
          <!-- Rust 侧 summary 本身就是「N 项 · 合计 X · N 项可回收 · 共 Y」，
               与此前它上面的 条目/合计/可回收 三行 dl 逐字重复——留一行文案，删三行 dl。
               整组数字的另一份在状态栏；筛选生效时状态栏是筛后口径，这里是全组口径。 -->
          <p v-if="current.summary" class="mt-1 text-[11px] leading-relaxed text-muted-foreground">
            {{ current.summary }}
          </p>
          <!-- notes 是口径事实（如「嵌套仓库体积已排除」），不是说明书：不配「说明」标题、
               不配 ⓘ 图标，就是几行灰字（U15）。「选中一行看明细」这类占位指令删掉——
               原生 app 不会告诉你怎么用它自己。 -->
          <p
            v-for="(n, i) in current.notes ?? []"
            :key="i"
            class="mt-2 text-[11px] leading-relaxed text-muted-foreground"
          >
            {{ n }}
          </p>
          <!-- 清理规则默认收起。这三条讲的是「本工具怎么判可清理」，常驻等于把 README 塞进
               界面：占掉检查器三分之一屏，且每次打开都读一遍。**一字未改，只从常驻改成折叠**
               ——第一次真要清理的人会点开，天天用的人不必再看。措辞改动是另一把刀的事。 -->
          <template v-if="current?.supports_clean">
            <button
              class="mt-1.5 flex items-center gap-1 rounded text-[11px] text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              :aria-expanded="rulesOpen"
              @click="rulesOpen = !rulesOpen"
            >
              <ChevronRight
                class="size-3 shrink-0 transition-transform"
                :class="rulesOpen ? 'rotate-90' : ''"
              />
              清理规则
            </button>
            <div class="rules" :class="{ open: rulesOpen }" :aria-hidden="!rulesOpen">
              <div>
                <ul class="mt-1 space-y-1.5 text-[11px] leading-relaxed text-muted-foreground">
                  <li>
                    「可清理」行可在检查器里发起清理（移入废纸篓）；带构建产物的仓库可以只清产物（node_modules
                    / target 等，源码保留）。
                  </li>
                  <li>
                    工具链缓存里那些有官方清理命令的行不标「可清理」——命令写在明细里，请自己运行，它比搬走整个目录精确。
                  </li>
                  <li>
                    编辑器与 AI
                    工具按「工具 × 类别」清日志 / 会话 / 缓存；会话含完整对话记录，删除后不会自动重建。
                  </li>
                </ul>
              </div>
            </div>
          </template>
          <p v-else-if="reclaimTotal > 0" class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            本组只读：账面可回收不等于有删除入口。
          </p>
        </template>
        </div>
      </aside>
    </div>

    <!-- 状态栏：当前视图的条目数 / 合计 / 可回收 -->
    <div
      v-if="current"
      class="flex h-7 shrink-0 items-center justify-between gap-2 border-t border-border/40 px-3 text-[11px] text-muted-foreground"
    >
      <!-- 合计是本屏最该看清的两个数之一（另一个在检查器主角位），近黑；
           整条状态栏底色是 muted，这里不显式抬到 foreground 的话会跟着一起灰 -->
      <span class="tabular-nums text-foreground">
        {{ visibleRows.length }} 项 · <ByteValue :bytes="statusSum" />
      </span>
      <div class="flex items-center gap-3">
        <!-- 数据新鲜度挪到状态栏常驻：此前只挂在「重扫」按钮的 title 上，
             不 hover 就永远不知道眼前这份快照是什么时候的 -->
        <span
          v-if="cachedAt"
          :key="cachedAt"
          class="updated tabular-nums"
          :class="stale ? 'text-foreground' : ''"
          :title="stale ? '已超过一天，建议重扫' : '缓存快照时间'"
        >
          数据更新于 {{ formatAgo(cachedAt) }}
        </span>
        <span v-if="statusReclaim > 0" class="tabular-nums">
          可回收
          <ByteValue class="text-reclaim" :bytes="statusReclaim" />
        </span>
        <!-- 「设置」入口 2026-10-19 挪回侧栏（App.vue 底部，与分组行同一套选中规则）。
             此前注释说它「不是资产分组、不该挤在侧栏里」——这条考虑到头了分组列表的一侧，
             漏了「应用级入口应该恒定可达」；状态栏属于当前视图，换个分组就换一回，
             不再是它的位置。再想把它挪回来，先把这条理由推翻，别在两个地方各挂一个。 -->
      </div>
    </div>

    <!-- 行右键菜单。只有两项，自己画一个比拉一个菜单库划算。
         位置钳制在视口内；点别处 / 滚动 / Esc / 改窗口大小都收（见 script 的监听）。 -->
    <div
      v-if="menu"
      data-row-menu
      role="menu"
      class="ctx-menu menu-in fixed z-50 min-w-44 rounded-md border border-border bg-card py-1 shadow-md"
      :style="menuStyle"
    >
      <button
        role="menuitem"
        class="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs hover:bg-muted"
        @click="onReveal(menu.item)"
      >
        <FolderOpen class="size-3.5 shrink-0 text-muted-foreground" />
        {{ revealLabel }}
      </button>
      <button
        role="menuitem"
        class="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs hover:bg-muted"
        @click="onCopyPath(menu.item)"
      >
        <Copy class="size-3.5 shrink-0 text-muted-foreground" />
        复制路径
      </button>
    </div>

    <!-- 清理确认弹窗（S2 / D1 拍板选 A）。破坏性操作必须显式确认，且把三件事
         写在明处：删什么（整个仓库，含源码与 .git）、删多少（体积）、去哪
         （废纸篓，可恢复）。Esc / 点遮罩 / 取消都收；确认才发起 invoke。 -->
    <div v-if="cleanTarget" class="fixed inset-0 z-50 flex items-center justify-center">
      <div class="absolute inset-0 bg-black/40" @click="cleanTarget = null" />
      <div
        ref="cleanDialog"
        role="dialog"
        aria-modal="true"
        class="relative w-[420px] rounded-lg border border-border bg-card p-4 shadow-lg"
      >
        <p class="text-[13px] font-medium">移入废纸篓</p>
        <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
          <!-- 三组性质不同，不能共用一句话（S3 / B1）：删仓库丢源码、删缓存花重下的时间、
               删 AI 工具数据里的会话丢历史 -->
          <template v-if="cleanKind === 'repo'">
            将把整个仓库目录——<span class="text-foreground">连同源码与 .git 历史</span
            >——移入废纸篓，之后可以从废纸篓恢复。
          </template>
          <template v-else-if="cleanKind === 'ai'">
            将把「{{ cleanTarget.name }}」这一行下的
            <span class="text-foreground">{{ aiKindLabel(cleanTarget) }}</span>
            条目移入废纸篓，之后可以从废纸篓恢复。
            <template v-if="aiKindLabel(cleanTarget) === '会话'">
              <span class="text-foreground">会话含完整对话记录与代码片段，删除后不会自动重建。</span>
              日志与缓存会在下次运行时重新生成。
            </template>
            <template v-else>日志与缓存会在下次运行时重新生成，功能不受影响。</template>
          </template>
          <template v-else>
            将把整个缓存目录移入废纸篓，之后可以从废纸篓恢复。
            <span class="text-foreground">下次用到时要重新下载或重建</span>，功能不受影响。
          </template>
        </p>
        <dl class="mt-3 space-y-1 rounded-md bg-muted/50 p-2 text-xs">
          <div class="flex items-baseline justify-between gap-2">
            <dt class="shrink-0 text-muted-foreground">
              {{ cleanKind === "repo" ? "仓库" : cleanKind === "ai" ? "工具" : "缓存" }}
            </dt>
            <dd class="min-w-0 truncate font-medium">{{ cleanTarget.name }}</dd>
          </div>
          <div v-if="cleanKind === 'ai'" class="flex items-baseline justify-between gap-2">
            <dt class="shrink-0 text-muted-foreground">类别</dt>
            <dd class="min-w-0 truncate font-medium">{{ aiKindLabel(cleanTarget) }}</dd>
          </div>
          <div class="flex items-baseline justify-between gap-2">
            <dt class="shrink-0 text-muted-foreground">路径</dt>
            <dd class="min-w-0 truncate font-mono text-[10px]" :title="cleanTarget.path">
              {{ cleanTarget.path }}
            </dd>
          </div>
          <div class="flex items-baseline justify-between gap-2">
            <dt class="shrink-0 text-muted-foreground">体积</dt>
            <dd><ByteValue :bytes="reclaimableOf(cleanTarget)" /></dd>
          </div>
        </dl>
        <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
          <template v-if="cleanKind === 'repo'">
            有未推送提交或未提交改动的仓库会被拒绝——点击与扫描之间状态可能变了，服务端会逐项复核。
          </template>
          <template v-else-if="cleanKind === 'ai'">
            服务端会核对工具仍在扫描白名单内，并按点击那一刻的文件名重新分类；只有仍属于该类别的一级条目会被清理。会话删除后只能从废纸篓找回。
          </template>
          <template v-else>
            <template v-if="cleanNote(cleanTarget)">{{ cleanNote(cleanTarget) }}</template>
            <template v-else>
              正在进行的构建或下载建议等完成后再清理。
            </template>
            服务端会复核这条仍在可清理白名单内。
          </template>
        </p>
        <div class="mt-4 flex justify-end gap-2">
          <Button data-dialog-cancel size="xs" variant="outline" @click="cleanTarget = null">取消</Button>
          <Button size="xs" variant="destructive" @click="confirmClean">确认移入废纸篓</Button>
        </div>
      </div>
    </div>

    <!-- 清理构建产物弹窗（P2）。与整仓删除的弹窗刻意分开：范围、复核方式、
         后果都不同。要点写在明处：删什么（白名单产物目录）、保什么（源码
         与 .git）、复核是什么（服务端点击那一刻按白名单现找，不信任前端路径）。 -->
    <div v-if="artifactTarget" class="fixed inset-0 z-50 flex items-center justify-center">
      <div class="absolute inset-0 bg-black/40" @click="artifactTarget = null" />
      <div
        ref="artifactDialog"
        role="dialog"
        aria-modal="true"
        class="relative w-[420px] rounded-lg border border-border bg-card p-4 shadow-lg"
      >
        <p class="text-[13px] font-medium">清理构建产物</p>
        <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
          将把该仓库内的构建产物目录（node_modules、target 等）移入废纸篓。
          <span class="text-foreground">源码与 .git 不受影响</span
          >，重新构建或安装依赖时会再次生成。
        </p>
        <dl class="mt-3 space-y-1 rounded-md bg-muted/50 p-2 text-xs">
          <div class="flex items-baseline justify-between gap-2">
            <dt class="shrink-0 text-muted-foreground">仓库</dt>
            <dd class="min-w-0 truncate font-medium">{{ artifactTarget.name }}</dd>
          </div>
          <div class="flex items-baseline justify-between gap-2">
            <dt class="shrink-0 text-muted-foreground">路径</dt>
            <dd class="min-w-0 truncate font-mono text-[10px]" :title="artifactTarget.path">
              {{ artifactTarget.path }}
            </dd>
          </div>
          <div class="flex items-baseline justify-between gap-2">
            <dt class="shrink-0 text-muted-foreground">可腾出</dt>
            <dd><ByteValue :bytes="artifactBytes(artifactTarget)" /></dd>
          </div>
          <div v-if="artifactTarget.detail?.['构建产物']" class="flex items-baseline justify-between gap-2">
            <dt class="shrink-0 text-muted-foreground">明细</dt>
            <dd class="min-w-0 truncate" :title="artifactTarget.detail['构建产物']">
              {{ artifactTarget.detail["构建产物"] }}
            </dd>
          </div>
        </dl>
        <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
          不检查未推送提交或未提交改动——清产物不碰源码。服务端会按白名单重新找出产物目录，以点击那一刻为准；藏有 Git 仓库的产物目录会被拒绝。
        </p>
        <div class="mt-4 flex justify-end gap-2">
          <Button data-dialog-cancel size="xs" variant="outline" @click="artifactTarget = null">取消</Button>
          <Button size="xs" variant="destructive" @click="confirmCleanArtifacts">确认移入废纸篓</Button>
        </div>
      </div>
    </div>
  </div>
</template>

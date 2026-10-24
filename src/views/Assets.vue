<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  FolderPlus,
  LoaderCircle,
  RefreshCw,
} from "@lucide/vue";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "vue-sonner";
import type { AssetGroup, CleanResult, Item } from "@/lib/types";
import {
  formatAgo,
  formatBytes,
  groupReclaimable,
  groupSize,
  itemSize,
  reclaimableOf,
} from "@/lib/types";
import { useAssets } from "@/composables/useAssets";
import { useRepoRoots } from "@/composables/useRepoRoots";
import { captureRowTops, playRowFlip } from "@/lib/motion";
import { canCleanArtifactsItem, canCleanItem, cell, tagHint } from "@/lib/assets";
import type { Dimension } from "@/lib/assets";
import { isLocalPath } from "@/lib/path";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import ByteValue from "@/components/ByteValue.vue";
import Hint from "@/components/Hint.vue";
import ScanBar from "@/components/ScanBar.vue";
import CleanDialog from "@/components/assets/CleanDialog.vue";
import ArtifactCleanDialog from "@/components/assets/ArtifactCleanDialog.vue";
import RowContextMenu from "@/components/assets/RowContextMenu.vue";
import InspectorPanel from "@/components/assets/InspectorPanel.vue";
import FilterMenu from "@/components/assets/FilterMenu.vue";
import AssetsToolbar from "@/components/assets/AssetsToolbar.vue";

const router = useRouter();
const {
  groups,
  current,
  loading,
  error,
  activeId,
  progress,
  cachedAt,
  groupSavedAt,
  rescan,
  rescanGroup,
} = useAssets();
// 主题切给 AssetsToolbar 自己管（useTheme 单例）；本文件不再读它。
const {
  roots: repoRoots,
  pending: repoRootsPending,
  error: repoRootsError,
  busy: repoRootsBusy,
  loadRoots,
  addRoot,
} = useRepoRoots();
// 只在真实扫描为空时读取配置，搜索/筛选无匹配不应引导用户改变扫描范围。
const reposEmpty = computed(() => current.value?.id === "repos" && current.value.items.length === 0);
watch(reposEmpty, (empty) => {
  if (empty) void loadRoots();
}, { immediate: true });

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
const scrollArea = ref<InstanceType<typeof ScrollArea> | null>(null);
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

function openMenu(e: MouseEvent, item: Item) {
  // 不是本机路径就不弹——弹一个两项都点不动的菜单比不弹更糟
  if (!isLocalPath(item.path)) return;
  menu.value = { x: e.clientX, y: e.clientY, item };
}

function closeMenu() {
  menu.value = null;
}

/**
 * 定位一个路径。**只 reveal、不 open**（见文件头「行动作」那段与 Rust 侧
 * `reveal_path`）：产物明细是目录，看着「打开」更自然，但 open 会执行路径
 * 指向的东西，那条边界不能因为这里看着安全就破。
 */
async function revealAt(path: string) {
  try {
    await invoke("reveal_path", { path });
  } catch (e) {
    // 后端会把「路径已不存在，重扫一次」原样给出来，这里不翻译
    toast.error(String(e));
  }
}

async function onReveal(item: Item) {
  closeMenu();
  await revealAt(item.path);
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

function askClean(item: Item) {
  if (!canCleanItem(current.value, item)) return;
  cleanTarget.value = item;
}

// ---- 清理构建产物（P2）----
//
// 与整仓清理并行的第二个破坏性动作：只清白名单内的产物目录（node_modules /
// target…），源码与 .git 保留。三道闸同构：确认弹窗（本文件）+ 服务端按白名单
// 现找产物目录复核 + 废纸篓。同样**不进右键菜单**。范围用 `scope: "artifacts"`
// 传给后端；前端仅传勾选的相对路径，服务端逐条重新复核。

/** 产物清理确认弹窗的目标条目。null = 没打开。 */
const artifactTarget = ref<Item | null>(null);

/**
 * 「清理产物」的判据与整仓清理是两个动作（判据在 lib/assets：canCleanArtifactsItem）。
 */
function askCleanArtifacts(item: Item) {
  if (!canCleanArtifactsItem(current.value, item)) return;
  // 打开即全选的默认在弹窗组件内初始化（watch target）
  artifactTarget.value = item;
}

/** 成败 toast + 重扫（缓存里少/变了东西，别让界面停留在旧快照）。
 *  只重扫被清理的那个分组（`CleanResult.group_id`）：清理只动一组的数据，
 *  全量重扫要几十秒，其余分组没变，犯不着陪跑。全失败 = 什么都没删成，
 *  数据没变，不扫。 */
async function applyCleanResult(r: CleanResult) {
  if (r.succeeded.length > 0) {
    toast.success(`已移入废纸篓，腾出 ${formatBytes(r.freed_bytes)}`);
  }
  // id = "*" 是整个计划被拒（如未确认），其余是单项失败（复核不通过/废纸篓出错）
  for (const f of r.failed) {
    toast.error(f.id === "*" ? f.reason : `${f.id}：${f.reason}`);
  }
  if (r.succeeded.length === 0) return;
  const before = sizeSnapshot();
  const res = await rescanGroup(r.group_id);
  if (!res.ok) toast.error(res.message);
  else flashChanged(before);
}

// ---- 弹窗焦点管理 ----
//
// 焦点的「进（落取消）/ 还（回检查器按钮）」在两个弹窗组件内部
// （`useModalFocus`）；这里只留 Tab 圈闭需要的弹窗根元素——
// 组件经 defineExpose 暴露的 rootEl。开合真相（cleanTarget /
// artifactTarget）在父组件：全局键盘的 Escape 优先级要读它。

const cleanDialog = ref<InstanceType<typeof CleanDialog> | null>(null);
const artifactDialog = ref<InstanceType<typeof ArtifactCleanDialog> | null>(null);

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

async function confirmCleanArtifacts(targets: string[], item: Item) {
  const groupId = current.value?.id;
  if (!groupId) return;
  // 空勾选不发请求：服务端会把空数组当「什么都没选中」拒掉，前端先挡一道
  if (targets.length === 0) return;
  artifactTarget.value = null;
  try {
    const r = await invoke<CleanResult>("clean_items", {
      plan: {
        group_id: groupId,
        item_ids: [item.id],
        confirmed: true,
        scope: "artifacts",
        artifact_targets: targets,
      },
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
  const openDialog = cleanDialog.value?.rootEl ?? artifactDialog.value?.rootEl;
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

// ---- 标签释义 ----
//
// 标签是筛选维度的取值（Rust 侧定死，`scan/tests.rs` 断言钉着），所以不能靠
// 改名把它说清楚——只能在 hover 上补一句。补的这句只解释「这条是怎么判出来的」，
// 不重复行里已经有的数字。
//
// 两个数字（200 MB / 180 天）与 Rust 侧 `LARGE_REPO_BYTES` / `STALE_DAYS`
// （`src-tauri/src/scan/repos.rs`）对齐。改那边要同步改这里，否则提示会撒谎。
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
// 内容在 InspectorPanel（拆分计划阶段 3）；这里只保留选中行的查找与传参。
// rulesOpen 仍在本组件：检查器在总览视图整体卸载，「切分组不重置」的
// 折叠语义靠这里持值经 v-model 传下去才保得住。

const selectedRow = computed(() => {
  if (!selected.value) return null;
  return current.value?.items.find((it) => it.id === selected.value) ?? null;
});

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

/** 单组扫描只有一个 0→1 事件，比例条原地 0% 像坏了：改走来回扫的墨线。 */
const scanIndeterminate = computed(() => progress.value?.total === 1);

/** 当前视图的「数据更新于」基准：组视图取该组自己的扫描时间，总览取各组
 *  最早值（总览聚合全体，它的新鲜度由最旧的组决定）。单组重扫之后各组
 *  时间不同，不能拿缓存写入时间冒充全体数据的新鲜度。 */
const viewSavedAt = computed<number | null>(() => {
  const id = current.value?.id;
  if (!id) return cachedAt.value;
  if (id === "overview") {
    const times = Object.values(groupSavedAt.value);
    return times.length ? Math.min(...times) : cachedAt.value;
  }
  return groupSavedAt.value[id] ?? cachedAt.value;
});

const stale = computed(
  () => viewSavedAt.value != null && Date.now() / 1000 - viewSavedAt.value > 86400,
);

/** 按钮文案随上下文：组视图 = 重扫本组（秒级）；总览与无数据态 = 全量重扫。
 *  扫描中区分单组/全量：单组没有多组比例可报（total=1），报正在扫谁。 */
const rescanLabel = computed(() => {
  if (!loading.value) return isOverview.value || !current.value ? "重扫" : "重扫本组";
  const p = progress.value;
  if (!p) return "扫描中";
  return p.total === 1
    ? p.label
      ? `正在扫描 ${p.label}`
      : "扫描中"
    : `扫描中 ${p.done}/${p.total}`;
});

/** 与 rescanLabel 同一个判据（总览或无数据 = 全量），别让 hover 提示跟实际行为打架。 */
const rescanTitle = computed(() =>
  isOverview.value || !current.value ? "重扫全部分组" : "只重扫当前分组，几秒完成",
);

async function onRescan() {
  const before = sizeSnapshot();
  const gid = current.value?.id;
  const r = isOverview.value || !gid ? await rescan() : await rescanGroup(gid);
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
    <!-- 工具栏（AssetsToolbar）：标题与重扫文案父算好传入，中段筛选走插槽。 -->
    <AssetsToolbar
      :title="current?.label ?? (loading ? '首次扫描…' : '无数据')"
      :loading="loading"
      :is-overview="isOverview"
      v-model:inspector-open="inspectorOpen"
      v-model:search="search"
      :rescan-label="rescanLabel"
      :rescan-title="rescanTitle"
      @rescan="onRescan"
    >
      <FilterMenu
        v-if="dimsOfCurrent.length"
        :dims="dims"
        :dimensions="dimsOfCurrent"
        :open="openFilter"
        :count-for="optionCount"
        @set="setDim"
        @clear="clearFilters"
        @toggle="openFilter = !openFilter"
      />
    </AssetsToolbar>


    <!-- 真实进度条：贴在工具栏下沿（U2），替掉按钮里一个干转的 spinner。
         U16 起 1px 淡墨线 + 行首笔尖；进度用 transform 走合成层。
         单组扫描没有中间比例，走 indeterminate 的来回墨线。 -->
    <Transition name="fade">
      <ScanBar v-if="loading" :pct="scanPct" :indeterminate="scanIndeterminate" />
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
      <ScrollArea ref="scrollArea" class="min-w-0 flex-1">
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
              <div class="flex flex-wrap items-baseline gap-x-4 gap-y-1 pb-0.5 text-[11px] text-muted-foreground">
                <span class="whitespace-nowrap">
                  可回收
                  <ByteValue class="text-reclaim text-xs" :bytes="machineStats.reclaim" animate />
                </span>
                <span class="whitespace-nowrap">{{ machineStats.count }} 个分组</span>
                <span class="whitespace-nowrap">近 7 天 {{ machineStats.recent }} 个仓库有提交</span>
              </div>
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
              class="flex min-w-0 items-center gap-1 text-left transition-colors hover:text-foreground"
              :class="sortKey === 'name' ? 'text-foreground' : ''"
              :aria-label="`按名称排序${sortKey === 'name' ? (sortDesc ? '，当前降序' : '，当前升序') : ''}`"
              @click="toggleSort('name')"
            >
              名称
              <component v-if="sortKey === 'name'" :is="sortDesc ? ArrowDown : ArrowUp" class="size-3 shrink-0" aria-hidden="true" />
            </button>
            <Hint
              v-for="c in colMeta.slice(1)"
              :key="c.key"
              :text="colTitle(c)"
            >
              <button
                class="flex min-w-0 items-center gap-1 transition-colors hover:text-foreground"
                :class="[c.align === 'text-right' ? 'justify-end' : '', sortKey === c.key ? 'text-foreground' : '']"
                :aria-label="`按${c.label}排序${sortKey === c.key ? (sortDesc ? '，当前降序' : '，当前升序') : ''}`"
                @click="toggleSort(c.key)"
              >
                <span class="truncate">{{ c.label }}</span>
                <component v-if="sortKey === c.key" :is="sortDesc ? ArrowDown : ArrowUp" class="size-3 shrink-0" aria-hidden="true" />
              </button>
            </Hint>
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
                  @click="onRowClick(row.item)"
                  @contextmenu.prevent="openMenu($event, row.item)"
                >
                  <!-- 行背景占比条已删（U15）：238 行按体积归一的灰条连成一道灰阶梯，
                       把表格读成仪表盘热力图；账本的比例语义由数字列与总览「占比」列承担。
                       名称与状态同一行：状态是 11px 灰字不是药丸——药丸把单行撑成两行、
                       全列铺满小灰块纹理，是生成式列表 UI 的第一指纹。两段都可截断，
                       按各自宽度比例分摊挤压，窄窗口下谁都不会把谁顶出去。
                       两段各自的 tooltip 挂在各自那段上，**不挂整行**：行的 tooltip
                       会与段内的 tooltip 嵌套，pointermove 冒泡上去两条一起排队开，
                       谁先弹取决于计时器，扫过时就是一次闪烁。 -->
                  <span class="flex min-w-0 items-baseline gap-2">
                    <Hint :text="rowTitle(row.item)">
                      <span
                        class="min-w-0 truncate"
                        :class="isOverview ? 'text-[14px] font-medium' : 'text-[13px]'"
                      >{{ rowName(row.item) }}</span>
                    </Hint>
                    <!-- 标签逐个挂提示：hover 哪个解释哪个，比整段一份说明精确。
                         分隔点不在提示范围内（它是标点，不是信息）。 -->
                    <span
                      v-if="visibleTags(row.item).length"
                      class="flex max-w-[40%] shrink-0 items-baseline gap-1 overflow-hidden"
                    >
                      <template v-for="(t, ti) in visibleTags(row.item)" :key="t">
                        <span
                          v-if="ti"
                          class="shrink-0 text-[11px] text-muted-foreground"
                          aria-hidden="true"
                        >·</span>
                        <Hint :text="tagHint(t)">
                          <span class="shrink-0 whitespace-nowrap text-[11px] text-muted-foreground">{{ t }}</span>
                        </Hint>
                      </template>
                    </span>
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
            <div v-if="reposEmpty" class="max-w-[600px]" :aria-busy="!!repoRootsPending">
              <h2 class="text-xs font-medium text-foreground">{{ current.error ? "未能完成仓库扫描" : "未发现 Git 仓库" }}</h2>
              <p v-if="current.error" class="mt-1 text-[11px] text-destructive [overflow-wrap:anywhere]">{{ current.error }}</p>
              <h3 class="mt-3 text-[11px] text-muted-foreground">{{ current.error ? "当前扫描范围" : "已扫描范围" }}</h3>
              <ul v-if="current.notes?.length" class="mt-1 space-y-1">
                <li v-for="note in current.notes" :key="note" class="select-text text-[11px] leading-relaxed text-muted-foreground [overflow-wrap:anywhere]">{{ note }}</li>
              </ul>
              <p v-else class="mt-1 text-[11px] text-muted-foreground">当前快照未记录扫描范围，请重扫本组。</p>
              <p v-if="repoRootsPending === 'loading'" role="status" class="mt-3 text-[11px] text-muted-foreground">正在读取扫描目录…</p>
              <div v-if="repoRootsError" class="mt-3 text-[11px]">
                <p role="alert" class="whitespace-pre-line text-destructive [overflow-wrap:anywhere]">{{ repoRootsError }}</p>
                <Button variant="ghost" size="xs" class="mt-1 text-muted-foreground" :disabled="repoRootsBusy" @click="loadRoots">
                  <RefreshCw aria-hidden="true" />
                  重新读取目录
                </Button>
              </div>
              <div class="mt-3 flex flex-wrap items-center gap-2">
                <Button variant="outline" size="xs" :disabled="repoRootsBusy || !repoRoots" @click="addRoot">
                  <FolderPlus aria-hidden="true" />
                  补充目录
                </Button>
                <Button variant="ghost" size="xs" class="text-muted-foreground" @click="router.push('/settings')">管理扫描目录</Button>
                <span v-if="repoRootsPending === 'adding' || repoRootsPending === 'removing'" role="status" class="text-[11px] text-muted-foreground">正在更新目录…</span>
              </div>
            </div>
            <template v-else>
              <p class="text-xs text-foreground">无匹配项</p>
              <button
                v-if="search || anyFilter"
                class="mt-1 text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground"
                @click="((search = ''), clearFilters())"
              >
                清除筛选条件
              </button>
            </template>
          </div>
        </template>
        </div>
      </ScrollArea>

      <!-- 检查器：原来四张卡片搬到这里（D5 降级后保留外观与交互部分）。
           收起时不卸载：宽度 252 → 0 让主区跟着收放，内容用 inert 挡键盘，
           避免「瞬间消失」把主区宽度一步跳过去。 -->
      <ScrollArea
        v-if="current && !isOverview"
        as="aside"
        id="asset-inspector"
        aria-label="检查器"
        class="inspector flex shrink-0 flex-col border-l border-border/60"
        :class="{ closed: !inspectorOpen }"
        :inert="!inspectorOpen"
        :aria-hidden="!inspectorOpen"
      >
        <InspectorPanel
          v-model:rules-open="rulesOpen"
          :group="current"
          :selected-row="selectedRow"
          :split-reclaim="splitReclaim"
          @reveal="onReveal"
          @reveal-at="revealAt"
          @ask-clean="askClean"
          @ask-clean-artifacts="askCleanArtifacts"
        />
      </ScrollArea>
    </div>

    <!-- 状态栏：当前视图的条目数 / 合计 / 可回收 -->
    <div
      v-if="current"
      class="flex h-10 shrink-0 items-center justify-between gap-2 border-t border-border/40 px-3 text-[11px] text-muted-foreground"
    >
      <!-- 合计是本屏最该看清的两个数之一（另一个在检查器主角位），近黑；
           整条状态栏底色是 muted，这里不显式抬到 foreground 的话会跟着一起灰 -->
      <span class="tabular-nums text-foreground">
        {{ visibleRows.length }} 项 · <ByteValue :bytes="statusSum" />
      </span>
      <div class="flex items-center gap-3">
        <!-- 数据新鲜度挪到状态栏常驻：此前只挂在「重扫」按钮的 title 上，
             不 hover 就永远不知道眼前这份快照是什么时候的。单组重扫后各分组
             时间不同，这里按当前视图取对应的扫描时间（总览取最早的组）。 -->
        <!-- 状态栏在窗口底缘，朝上展开的 tooltip 不会跑出去。
             v-if 挂在 Hint 上而不是里面的 span：asChild 的触发器要求插槽里真有
             一个元素，条件为假时插槽是空的。 -->
        <Hint
          v-if="viewSavedAt"
          :text="stale ? '已超过一天，建议重扫' : '本视图数据的扫描时间'"
        >
          <span
            :key="viewSavedAt"
            class="updated tabular-nums"
            :class="stale ? 'text-foreground' : ''"
          >
            数据更新于 {{ formatAgo(viewSavedAt) }}
          </span>
        </Hint>
        <Hint
          v-if="statusReclaim > 0"
          text="可回收为估算值：含共享硬链接的目录（如 Rust 的 target/）可能偏高"
        >
          <span class="tabular-nums">
            可回收
            <ByteValue class="text-reclaim" :bytes="statusReclaim" />
          </span>
        </Hint>
        <!-- 「设置」入口 2026-10-19 挪回侧栏（App.vue 底部，与分组行同一套选中规则）。
             此前注释说它「不是资产分组、不该挤在侧栏里」——这条考虑到头了分组列表的一侧，
             漏了「应用级入口应该恒定可达」；状态栏属于当前视图，换个分组就换一回，
             不再是它的位置。再想把它挪回来，先把这条理由推翻，别在两个地方各挂一个。 -->
      </div>
    </div>

    <!-- 行右键菜单（RowContextMenu）：位置钳制在组件内；点别处 / 滚动 / Esc /
         改窗口大小都收（本文件的 window 监听）。 -->
    <RowContextMenu :menu="menu" @reveal="onReveal" @copy="onCopyPath" />

    <!-- 清理确认弹窗（S2 / D1 拍板选 A）：文案与复核提示都在 CleanDialog 里。
         Esc / 点遮罩 / 取消都收（emit cancel）；确认 emit confirm——invoke 与
         applyCleanResult 在本组件，与产物弹窗共用。 -->
    <CleanDialog
      ref="cleanDialog"
      :target="cleanTarget"
      :group-id="current?.id ?? null"
      @confirm="confirmClean"
      @cancel="cleanTarget = null"
    />

    <!-- 清理构建产物弹窗（P2）：勾选集合是弹窗组件的内部状态（打开即全选），
         确认时把勾选的相对路径随 emit 展开交上来。 -->
    <ArtifactCleanDialog
      ref="artifactDialog"
      :target="artifactTarget"
      @confirm="confirmCleanArtifacts"
      @cancel="artifactTarget = null"
    />
  </div>
</template>

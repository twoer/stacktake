import { computed, ref } from "vue";
import { useRoute } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "vue-sonner";
import { autoScanDue } from "@/lib/autoScan";
import type { AssetGroup, BootPayload, ScanProgress, ScanResult } from "@/lib/types";
import { useAutoScan } from "@/composables/useAutoScan";

/**
 * 扫描状态。模块级 ref 天然单例——App 外壳（侧栏、顶栏）和 Assets 视图共享同一份，
 * 不需要 pinia。等真出现「A 页面写入、B 页面响应」的跨页状态再引入。
 */
const groups = ref<AssetGroup[]>([]);
/** 是否有任何扫描在进行。首次启动无缓存、或缓存过期且设置开启时会自动扫描。 */
const loading = ref(true);
/** 仅记录启动/首次加载失败。重扫失败走 toast，不占内容区。 */
const error = ref<string | null>(null);
/** 缓存写入时间。手动重扫成功后更新。 */
const cachedAt = ref<number | null>(null);
/**
 * 各分组各自的写入时间。单组重扫后不同分组的新鲜度不同，「数据更新于」
 * 要按当前视图取对应组的时间，不能拿全体写入时间冒充。全量扫描时所有
 * 值与 cachedAt 相同。
 */
const groupSavedAt = ref<Record<string, number>>({});
/**
 * 扫描进度。Rust 侧每完成一个分组 emit 一次（事件名 "scan-progress"，
 * 与 lib.rs::SCAN_PROGRESS_EVENT 一致），顶栏据此画真实进度条。
 * 没有扫描时为 null。
 */
const progress = ref<ScanProgress | null>(null);
/**
 * 本次扫描已完成的分组名（到达序，扫描器并行所以不固定）。事件是瞬时的，
 * 「哪些组扫完了」要靠这里累计；首扫空态的行账拿它点亮对应的行。
 * 与 progress 同生共死：扫描开始清空，结束一并归 null。
 */
const scanDoneLabels = ref<string[]>([]);

/**
 * 时间针：每分钟跳一次的「现在」（秒，与缓存时间戳同单位）。formatAgo、
 * stale 这类读数依赖墙钟，而 Date.now() 不是响应式的——不接这根针，界面
 * 开着过夜，「数据更新于 X 前」整夜不变，跨过 24 小时的「建议重扫」高亮
 * 也要等下一次别的状态变化才出现。粒度对齐显示粒度（最细「X 分钟前」），
 * 60 秒一跳足够；后台被节流没关系，回前台的下一次跳变会追上。
 */
const now = ref(Math.floor(Date.now() / 1000));
window.setInterval(() => {
  now.value = Math.floor(Date.now() / 1000);
}, 60_000);

// 模块级注册、随应用生命周期存活：进度事件只在扫描期间来，早注册不会漏掉
// 开扫那一刻的 done: 0。非 Tauri 环境（浏览器里跑）invoke 会拒，吞掉即可。
void listen<ScanProgress>("scan-progress", (e) => {
  progress.value = e.payload;
  if (e.payload.label && !scanDoneLabels.value.includes(e.payload.label)) {
    scanDoneLabels.value.push(e.payload.label);
  }
}).catch(() => {});

/** 所有返回完整快照的命令都更新这同一份数据与分组时间。 */
function applySnapshot(next: ScanResult) {
  groups.value = next.groups;
  cachedAt.value = next.saved_at;
  groupSavedAt.value = next.group_saved_at;
}

export function useAssets() {
  const route = useRoute();

  const activeId = computed(() => (route.params.groupId as string) ?? null);

  /** URL 里的分组不存在时退回第一个，避免手输错 id 就白屏 */
  const current = computed(
    () =>
      groups.value.find((g) => g.id === activeId.value) ?? groups.value[0] ?? null,
  );

  /**
   * 启动：有缓存直接用，没缓存才扫一次；缓存里最旧的分组超过 24 小时时，
   * 再自动重扫一次（设置页可关，默认开，判据见 lib/autoScan）。
   *
   * 仍然先出缓存再开扫——首帧不该白等 26 秒。除此之外不做任何启动期扫描：
   * 数据变化很慢，刷新频率不该跟着打开次数走，24 小时已经够新鲜。
   */
  async function boot() {
    loading.value = true;
    error.value = null;
    let autoScan = false;

    try {
      const payload = await invoke<BootPayload>("bootstrap");
      applySnapshot(payload);
      const { enabled } = useAutoScan();
      autoScan = autoScanDue(
        enabled.value,
        payload.scanned,
        payload.group_saved_at,
        payload.saved_at,
        Math.floor(Date.now() / 1000),
      );
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
      progress.value = null;
      scanDoneLabels.value = [];
    }

    if (autoScan) void autoScanAfterBoot();
  }

  /**
   * 启动后的自动重扫。先让缓存上屏再开扫；失败保留缓存，只 toast。
   * 一次启动只由 boot 调一次，不需要额外的「已尝试」标记。
   *
   * 用 setTimeout 而不是 rAF：窗口被遮挡 / 最小化时 rAF 会被挂起，扫描要等
   * 用户回来才开始；定时器在后台仍会走（可能被节流到 1 秒，可接受）。
   */
  async function autoScanAfterBoot() {
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    if (loading.value) return; // 用户已经发起扫描了，别叠第二次
    toast.info("缓存已超过 24 小时，正在自动重扫");
    const r = await rescan();
    if (!r.ok) toast.error(`自动重扫失败：${r.message}`);
  }

  /** 手动重扫：等扫完再返回。成败交回调用方去 toast，这里不碰 UI。 */
  async function rescan(): Promise<{ ok: true } | { ok: false; message: string }> {
    if (loading.value) return { ok: false, message: "正在扫描或更新目录，请稍后再试" };
    loading.value = true;
    progress.value = null;
    scanDoneLabels.value = [];
    try {
      // saved_at 用后端写进缓存的那个值，不自己算 Date.now()——扫描本身要几十秒，
      // 那样「数据更新于 X」会凭空多算一个扫描时长，重启后又会跳回去
      const next = await invoke<ScanResult>("rescan_now");
      applySnapshot(next);
      return { ok: true };
    } catch (e) {
      return { ok: false, message: String(e) };
    } finally {
      loading.value = false;
      progress.value = null;
      scanDoneLabels.value = [];
    }
  }

  /**
   * 单组重扫：只扫一个分组，其余沿用缓存旧快照（合并在后端做，这里整表替换）。
   * 与 rescan 共用同一套 loading/progress 状态机；进度事件 total=1。
   */
  async function rescanGroup(
    groupId: string,
  ): Promise<{ ok: true } | { ok: false; message: string }> {
    if (loading.value) return { ok: false, message: "正在扫描或更新目录，请稍后再试" };
    loading.value = true;
    progress.value = null;
    scanDoneLabels.value = [];
    try {
      const next = await invoke<ScanResult>("rescan_group", { groupId });
      applySnapshot(next);
      return { ok: true };
    } catch (e) {
      return { ok: false, message: String(e) };
    } finally {
      loading.value = false;
      progress.value = null;
      scanDoneLabels.value = [];
    }
  }

  return {
    groups,
    loading,
    error,
    cachedAt,
    groupSavedAt,
    progress,
    scanDoneLabels,
    now,
    activeId,
    current,
    applySnapshot,
    boot,
    rescan,
    rescanGroup,
  };
}

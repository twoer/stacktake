import { computed, ref } from "vue";
import { useRoute } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AssetGroup, BootPayload, ScanProgress, ScanResult } from "@/lib/types";

/**
 * 扫描状态。模块级 ref 天然单例——App 外壳（侧栏、顶栏）和 Assets 视图共享同一份，
 * 不需要 pinia。等真出现「A 页面写入、B 页面响应」的跨页状态再引入。
 */
const groups = ref<AssetGroup[]>([]);
/** 是否有任何扫描在进行。首次启动无缓存时会真扫（唯一会自动扫描的场景）。 */
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
   * 启动：有缓存直接用，没缓存才扫一次。
   *
   * 刻意不在启动时后台自动重扫。实测全量扫描约 26 秒（debug 构建），
   * 而数据变化很慢——两百多个仓库里近 7 天有提交的只有个位数。
   * 自动刷新换不来新鲜度（新扫的结果和缓存几乎一样），却让每次打开都啃 20 秒磁盘。
   * 之后想更新自己点顶栏的「重扫」。
   */
  async function boot() {
    loading.value = true;
    error.value = null;

    try {
      const payload = await invoke<BootPayload>("bootstrap");
      applySnapshot(payload);
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
      progress.value = null;
      scanDoneLabels.value = [];
    }
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

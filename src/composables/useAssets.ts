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
 * 扫描进度。Rust 侧每完成一个分组 emit 一次（事件名 "scan-progress"，
 * 与 lib.rs::SCAN_PROGRESS_EVENT 一致），顶栏据此画真实进度条。
 * 没有扫描时为 null。
 */
const progress = ref<ScanProgress | null>(null);

// 模块级注册、随应用生命周期存活：进度事件只在扫描期间来，早注册不会漏掉
// 开扫那一刻的 done: 0。非 Tauri 环境（浏览器里跑）invoke 会拒，吞掉即可。
void listen<ScanProgress>("scan-progress", (e) => {
  progress.value = e.payload;
}).catch(() => {});

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
      groups.value = payload.groups;
      cachedAt.value = payload.saved_at;
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
      progress.value = null;
    }
  }

  /** 手动重扫：等扫完再返回。成败交回调用方去 toast，这里不碰 UI。 */
  async function rescan(): Promise<{ ok: true } | { ok: false; message: string }> {
    loading.value = true;
    progress.value = null;
    try {
      // saved_at 用后端写进缓存的那个值，不自己算 Date.now()——扫描本身要几十秒，
      // 那样「数据更新于 X」会凭空多算一个扫描时长，重启后又会跳回去
      const next = await invoke<ScanResult>("rescan_now");
      groups.value = next.groups;
      cachedAt.value = next.saved_at;
      return { ok: true };
    } catch (e) {
      return { ok: false, message: String(e) };
    } finally {
      loading.value = false;
      progress.value = null;
    }
  }

  return { groups, loading, error, cachedAt, progress, activeId, current, boot, rescan };
}

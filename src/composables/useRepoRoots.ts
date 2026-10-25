import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "vue-sonner";
import type { RepoRoots, RepoRootsError, ScanResult } from "@/lib/types";
import { useAssets } from "@/composables/useAssets";

// 设置页与仓库空态共用配置、错误和互斥状态，切页不会丢失正在进行的操作。
const roots = ref<RepoRoots | null>(null);
const pending = ref<"loading" | "adding" | "removing" | null>(null);
const error = ref<string | null>(null);

async function readRoots() {
  try {
    roots.value = await invoke<RepoRoots>("get_repo_roots");
  } catch (e) {
    // 读失败不能伪装成「没有补充目录」，也不能覆盖刚才的操作错误。
    const message = `目录配置读取失败${roots.value ? "（保留上次读取结果）" : ""}：${String(e)}`;
    error.value = error.value ? `${error.value}\n${message}` : message;
  }
}

export function useRepoRoots() {
  const { loading, progress, applySnapshot } = useAssets();
  const busy = computed(() => loading.value || pending.value !== null);

  async function loadRoots() {
    if (pending.value) return;
    pending.value = "loading";
    error.value = null;
    try {
      await readRoots();
    } finally {
      pending.value = null;
    }
  }

  async function updateRoots(action: "adding" | "removing", path?: string) {
    if (busy.value) return;
    pending.value = action;
    // 从原生选择器弹出起占用同一把扫描锁，直到配置重读结束才释放。
    loading.value = true;
    progress.value = null;
    error.value = null;
    let next: ScanResult | null = null;
    /** 操作本身的失败。finally 里那次重读若失败，只会记在 error.value，不混进这条。 */
    let failure: string | null = null;
    try {
      next = action === "adding"
        ? await invoke<ScanResult | null>("add_repo_root")
        : await invoke<ScanResult>("remove_repo_root", { path });
      if (next) applySnapshot(next);
    } catch (e) {
      if (e && typeof e === "object" && "message" in e && "snapshot" in e) {
        const asError = e as RepoRootsError;
        if (asError.snapshot) applySnapshot(asError.snapshot);
        failure = asError.message;
      } else {
        failure = String(e);
      }
      error.value = failure;
    } finally {
      await readRoots();
      loading.value = false;
      progress.value = null;
      pending.value = null;
    }

    // 成败只看操作本身。成功路径上 finally 的重读若失败，error.value 会被
    // readRoots 填上——配置与快照其实都已生效，不能把它盖成「操作失败」，
    // 但也不能吞掉：成功 toast 之后补一条单独的错误 toast，列表刷新靠
    // 错误块里的「重新读取目录」。
    if (failure) {
      toast.error(failure);
    } else if (next) {
      toast.success(action === "adding"
        ? "已补充目录 · Git 仓库已刷新"
        : "已取消扫描该目录 · 未删除文件 · Git 仓库已刷新");
      if (error.value) toast.error(error.value);
    }
    // null 只表示取消原生选择器，不扫描，也不提示成功。
  }

  return {
    roots,
    pending,
    error,
    busy,
    loadRoots,
    addRoot: () => updateRoots("adding"),
    removeRoot: (path: string) => updateRoots("removing", path),
  };
}

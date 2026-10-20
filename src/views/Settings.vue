<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { Download, RefreshCw, Trash2 } from "@lucide/vue";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "vue-sonner";
import { formatAgo } from "@/lib/types";
import { useAssets } from "@/composables/useAssets";
import { useUpdate } from "@/composables/useUpdate";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import ScanBar from "@/components/ScanBar.vue";

const router = useRouter();
const { groups, cachedAt, loading, progress, rescan } = useAssets();
// 与侧栏共享同一份状态：App.vue 启动那次静默检查的结果这里直接读到，
// 不重复发请求。手动点「检查更新」才再查一次。
const {
  version: appVersion,
  state: updateState,
  latest,
  message: updateError,
  checkNow,
  openDownload,
} = useUpdate();

async function clearCache() {
  try {
    await invoke("clear_cache");
    // 清空内存里的数据，让侧栏和主区立刻反映"无缓存"状态
    groups.value = [];
    cachedAt.value = null;
    toast.success("缓存已清空 · 请点下方「立即重扫」刷新数据");
  } catch (e) {
    toast.error(String(e));
  }
}

/** 立即重扫：等扫完再跳回资产页——26 秒的等待在设置页做完，回去就是新数据。 */
async function rescanNow() {
  const r = await rescan();
  if (r.ok) {
    toast.success("重扫完成");
    router.push("/assets");
  } else {
    toast.error(r.message);
  }
}

const scanning = computed(() => loading.value);
const scanPct = computed(() => {
  const p = progress.value;
  return p && p.total > 0 ? Math.round((p.done / p.total) * 100) : 0;
});
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!-- 顶栏必须与 Assets.vue 工具栏逐值对齐：
         高度 h-11 / px-3 / hairline border-border/40 / 14px semibold 标题 / 拖动区。
         此前是 h-10 + px-4 + border-border + 13px medium，切页时标题基线跳 4px、
         正文左缘错 4px、底边线深一档。 -->
    <div class="flex h-11 shrink-0 items-center border-b border-border/40 bg-background px-3" data-tauri-drag-region>
      <h1 class="text-sm font-semibold">设置</h1>
      <div class="flex-1" />
    </div>
    <!-- 重扫进度条：useAssets 是模块级单例，进度事件照样来，这里也画一份 -->
    <Transition name="fade">
      <ScanBar v-if="scanning" :pct="scanPct" />
    </Transition>

    <ScrollArea class="min-h-0 flex-1">
      <div class="mx-auto max-w-[600px] space-y-8 px-4 py-5 text-[12px]">
        <!-- ============ 缓存 ============ -->
        <section>
          <h2 class="text-[13px] font-medium">缓存</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            扫描结果存在本机缓存里，启动直接读（毫秒级），只有首次运行或缓存被清才真扫。
            当前数据更新于 <span class="tabular-nums">{{ cachedAt ? formatAgo(cachedAt) : "—" }}</span>。
          </p>
          <!-- 缓存被清空后的引导态：groups 为空且 cachedAt 为 null 时显示 -->
          <div v-if="!cachedAt && groups.length === 0" class="mt-3 rounded-md border border-border/40 bg-muted/40 px-3 py-2.5">
            <p class="text-xs font-medium text-foreground">缓存已清空</p>
            <p class="mt-1 text-[11px] leading-relaxed text-muted-foreground">
              点下方「立即重扫」开始扫描，或重启应用自动扫描。
            </p>
          </div>
          <div class="mt-3 flex items-center gap-1.5">
            <Button variant="outline" size="xs" :disabled="scanning" @click="rescanNow">
              <RefreshCw />
              立即重扫
            </Button>
            <Button variant="ghost" size="xs" class="text-muted-foreground" @click="clearCache">
              <Trash2 />
              清空缓存
            </Button>
          </div>
          <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            全量扫描约 26 秒（实测值出处见 README「扫描策略」）。清空缓存不删任何资产数据，
            只是下次启动重新扫一遍。
          </p>
        </section>

        <!-- ============ 更新 ============ -->
        <section class="border-t border-border pt-6">
          <h2 class="text-[13px] font-medium">更新</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            当前版本 <span class="tabular-nums">v{{ appVersion || "—" }}</span>。
            <strong class="font-medium text-foreground">不做应用内自动更新</strong>——
            查出新版本只在侧栏给个标记，装哪个包、什么时候装由你自己决定。
          </p>

          <!-- 状态行：idle 不写任何东西。启动那次检查如果失败，这里就是空的，
               因为失败被刻意静默了；想看具体原因就点一次「检查更新」。 -->
          <p class="mt-2 leading-relaxed">
            <template v-if="updateState === 'checking'">
              <span class="text-muted-foreground">正在检查…</span>
            </template>
            <template v-else-if="updateState === 'available'">
              <span class="text-primary">有新版本 {{ latest }}</span>
              <span class="text-muted-foreground">—— 去下载页挑对应的安装包。</span>
            </template>
            <template v-else-if="updateState === 'up-to-date'">
              <span class="text-muted-foreground">已是最新。</span>
            </template>
            <template v-else-if="updateState === 'error'">
              <span class="text-destructive">检查失败：{{ updateError }}</span>
            </template>
          </p>

          <div class="mt-3 flex items-center gap-1.5">
            <Button
              variant="outline"
              size="xs"
              :disabled="updateState === 'checking'"
              @click="checkNow"
            >
              <RefreshCw />
              检查更新
            </Button>
            <Button v-if="updateState === 'available'" size="xs" @click="openDownload">
              <Download />
              前往下载页
            </Button>
          </div>
          <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            检查只请求一次 api.github.com 拿最新 tag，不带本机任何数据。
            macOS 的安装包没有 Apple 开发者签名，双击后若被系统拦下，右键→打开即可。
          </p>
        </section>
      </div>
    </ScrollArea>
  </div>
</template>

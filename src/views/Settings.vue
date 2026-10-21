<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { Code, Download, MessageSquare, RefreshCw, Scale, Trash2 } from "@lucide/vue";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "vue-sonner";
import { formatAgo } from "@/lib/types";
import { ISSUES_URL, LICENSE_URL, REPO_URL } from "@/lib/release";
import { useAssets } from "@/composables/useAssets";
import { useUpdate } from "@/composables/useUpdate";
import { Button } from "@/components/ui/button";
import ByteValue from "@/components/ByteValue.vue";
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
  downloadUpdate,
  openDownload,
  dlState,
  dlProgress,
  dlPath,
  dlMessage,
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

/** 静态外链交系统浏览器打开。走 opener 的 openUrl 而非 openPath——目标是
 *  https URL 不是本机路径（useUpdate.openDownload 同款，别改成 openPath）。
 *  失败给 toast，与 clearCache 一个路子；按钮不置 disabled，点没打开也有反馈。 */
async function openLink(url: string) {
  try {
    await openUrl(url);
  } catch (e) {
    toast.error(String(e));
  }
}

const scanning = computed(() => loading.value);
const scanPct = computed(() => {
  const p = progress.value;
  return p && p.total > 0 ? Math.round((p.done / p.total) * 100) : 0;
});
/** 下载百分比。total 拿不到（服务端没给 Content-Length）时显示 0，由文案兜底。 */
const dlPct = computed(() => {
  const p = dlProgress.value;
  return p && p.total ? Math.round((p.received / p.total) * 100) : 0;
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
    <!-- 顶部进度条：扫描与下载共用这一条（两个模块级单例各发各的事件，同一时刻
         只可能有一个在跑）。下载也在这里画一份——点完「下载」用户可能已经滚到别处，
         顶栏这条不随滚动走；与扫扫的进度是同一支笔，不用学第二套。 -->
    <Transition name="fade">
      <ScanBar v-if="scanning" :pct="scanPct" />
      <ScanBar v-else-if="dlState === 'downloading'" :pct="dlPct" label="下载进度" />
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
            查出新版本后点「下载」直接获取本平台的安装包——仍在应用内下载、
            不静默安装：下完自动打开安装包，装不装、什么时候装由你最后一步决定。
          </p>

          <!-- 状态行：idle 不写任何东西。启动那次检查如果失败，这里就是空的，
               因为失败被刻意静默了；想看具体原因就点一次「检查更新」。 -->
          <p class="mt-2 leading-relaxed">
            <template v-if="updateState === 'checking'">
              <span class="text-muted-foreground">正在检查…</span>
            </template>
            <template v-else-if="updateState === 'available'">
              <span class="text-primary">有新版本 {{ latest }}</span>
              <span class="text-muted-foreground">—— 点下方「下载」直接获取。</span>
            </template>
            <template v-else-if="updateState === 'up-to-date'">
              <span class="text-muted-foreground">已是最新。</span>
            </template>
            <template v-else-if="updateState === 'error'">
              <span class="text-destructive">检查失败：{{ updateError }}</span>
            </template>
          </p>

          <!-- 下载进度：节内一条 ScanBar + 12px 正文字号的百分比。
               第一版只有一行 11px 灰字，点完下载按钮消失后附近没有任何显眼信号
               （2026-10-21 用户反馈「不明显」）——现在与扫描进度同一支笔、
               同一套位置逻辑（顶部一份、节内一份）。总量拿不到时退化成「已接收 X」。 -->
          <div v-if="dlState === 'downloading'" class="mt-3">
            <ScanBar :pct="dlPct" label="下载进度" />
            <p class="mt-1.5 text-[12px] leading-relaxed text-foreground">
              下载中…
              <span class="font-medium tabular-nums">
                {{ dlProgress?.total ? `${dlPct}%` : "已接收" }}
              </span>
              <span class="text-muted-foreground">
                （<ByteValue class="tabular-nums" :bytes="dlProgress?.received ?? 0" /><template
                  v-if="dlProgress?.total"
                >
                  / <ByteValue class="tabular-nums" :bytes="dlProgress.total" /></template
                >）
              </span>
            </p>
          </div>
          <p v-else-if="dlState === 'done' && dlPath" class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            已下载并打开：<span class="tabular-nums">{{ dlPath }}</span>
          </p>
          <p v-else-if="dlMessage" class="mt-2 text-[11px] leading-relaxed text-destructive">
            下载失败：{{ dlMessage }}
          </p>

          <div class="mt-3 flex items-center gap-1.5">
            <Button
              variant="outline"
              size="xs"
              :disabled="updateState === 'checking' || dlState === 'downloading'"
              @click="checkNow"
            >
              <RefreshCw />
              检查更新
            </Button>
            <Button
              v-if="updateState === 'available' && dlState !== 'downloading'"
              size="xs"
              @click="downloadUpdate"
            >
              <Download />
              下载 {{ latest }}
            </Button>
          </div>
          <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            安装包下载到「下载」文件夹并自动打开。检查与下载都不经浏览器：检查只请求
            api.github.com，安装包从 GitHub 的发布资产直取，都不带本机任何数据。
            macOS 的安装包没有 Apple 开发者签名，双击后若被系统拦下，右键→打开即可；
            自动打开失败或没有本平台的包时，可<button
              class="rounded text-primary hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              @click="openDownload"
            >前往下载页</button>手动获取。
          </p>
        </section>

        <!-- ============ 项目 ============ -->
        <section class="border-t border-border pt-6">
          <h2 class="text-[13px] font-medium">项目</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            Stacktake 以 MIT 协议开源，源码、issue 与协议文本都在 GitHub 上。
            下面几个链接交给系统浏览器打开，应用不为此多发任何请求。
          </p>
          <div class="mt-3 flex items-center gap-1.5">
            <Button variant="outline" size="xs" @click="openLink(REPO_URL)">
              <Code />
              仓库主页
            </Button>
            <Button variant="ghost" size="xs" class="text-muted-foreground" @click="openLink(ISSUES_URL)">
              <MessageSquare />
              反馈问题
            </Button>
            <Button variant="ghost" size="xs" class="text-muted-foreground" @click="openLink(LICENSE_URL)">
              <Scale />
              开源协议
            </Button>
          </div>
        </section>
      </div>
    </ScrollArea>
  </div>
</template>

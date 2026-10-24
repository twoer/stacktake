<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import { Code, Download, FolderPlus, MessageSquare, RefreshCw, Scale, Trash2 } from "@lucide/vue";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "vue-sonner";
import { formatAgo } from "@/lib/types";
import { ISSUES_URL, LICENSE_URL, REPO_URL } from "@/lib/release";
import { useAssets } from "@/composables/useAssets";
import { useRepoRoots } from "@/composables/useRepoRoots";
import { useUpdate } from "@/composables/useUpdate";
import { Button } from "@/components/ui/button";
import ByteValue from "@/components/ByteValue.vue";
import Hint from "@/components/Hint.vue";
import { ScrollArea } from "@/components/ui/scroll-area";
import ScanBar from "@/components/ScanBar.vue";

const router = useRouter();
const { groups, cachedAt, loading, progress, rescan } = useAssets();
const {
  roots: repoRoots,
  pending: repoRootsPending,
  error: repoRootsError,
  busy: repoRootsBusy,
  loadRoots,
  addRoot,
  removeRoot,
} = useRepoRoots();
onMounted(() => { void loadRoots(); });

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
  if (loading.value) return;
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
    <!-- 顶部进度条：只画扫描。这里一度让下载也挤进来共用一条（扫描完就原地改写
         成下载百分比），观感是「下载污染了扫描进度条」——两支进度共用一支笔，
         人分不清刚才动的是谁。下载进度回归「更新」节内，各画各的。
         顺带：两个分支都是 ScanBar 且当时没写 key，Vue 视为同一元素就地 patch，
         fade 过渡也没触发（说到这一条是为了别再把它加回来：共用必出问题）。 -->
    <Transition name="fade">
      <ScanBar v-if="scanning" :pct="scanPct" />
    </Transition>

    <ScrollArea class="min-h-0 flex-1">
      <div class="mx-auto max-w-[600px] space-y-8 px-4 py-5 text-[12px]">
        <section aria-labelledby="repo-roots-heading" :aria-busy="!!repoRootsPending">
          <h2 id="repo-roots-heading" class="text-[13px] font-medium">Git 仓库扫描目录</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            补充仓库所在的父目录，添加或移除后只刷新 Git 仓库。
          </p>
          <template v-if="repoRoots">
            <h3 class="mt-3 text-[11px] text-muted-foreground">默认目录 · 只读</h3>
            <ul class="mt-1">
              <li v-for="path in repoRoots.default_roots" :key="path" class="border-b border-border/40 py-1.5">
                <span class="select-text font-mono text-[11px] [overflow-wrap:anywhere]">{{ path }}</span>
              </li>
            </ul>
            <p v-if="!repoRoots.default_roots.length" class="mt-1 text-[11px] text-muted-foreground">未找到默认目录</p>
            <h3 class="mt-3 text-[11px] text-muted-foreground">补充目录</h3>
            <ul class="mt-1">
              <li v-for="root in repoRoots.extra_roots" :key="root.path" class="flex items-start gap-2 border-b border-border/40 py-1.5">
                <div class="min-w-0 flex-1">
                  <p class="select-text font-mono text-[11px] leading-6 [overflow-wrap:anywhere]">{{ root.path }}</p>
                  <p v-if="!root.available" class="text-[11px] text-muted-foreground">目录不可用</p>
                </div>
                <Hint text="仅取消扫描，不删除文件">
                  <Button
                    variant="ghost"
                    size="xs"
                    class="shrink-0 text-muted-foreground"
                    :disabled="repoRootsBusy || dlState === 'downloading'"
                    :aria-label="`取消扫描 ${root.path}，不删除文件`"
                    @click="removeRoot(root.path)"
                  >移除</Button>
                </Hint>
              </li>
            </ul>
            <p v-if="!repoRoots.extra_roots.length" class="mt-1 text-[11px] text-muted-foreground">尚未补充目录</p>
          </template>
          <p v-if="repoRootsPending === 'loading'" role="status" class="mt-3 text-[11px] text-muted-foreground">正在读取扫描目录…</p>
          <div v-if="repoRootsError" class="mt-3 text-[11px]">
            <p role="alert" class="whitespace-pre-line text-destructive [overflow-wrap:anywhere]">{{ repoRootsError }}</p>
            <Button variant="ghost" size="xs" class="mt-1 text-muted-foreground" :disabled="repoRootsBusy" @click="loadRoots">
              <RefreshCw aria-hidden="true" />
              重新读取目录
            </Button>
          </div>
          <div class="mt-3 flex flex-wrap items-center gap-2">
            <Button variant="outline" size="xs" :disabled="repoRootsBusy || !repoRoots || dlState === 'downloading'" @click="addRoot">
              <FolderPlus aria-hidden="true" />
              补充目录
            </Button>
            <span v-if="repoRootsPending === 'adding' || repoRootsPending === 'removing'" role="status" class="text-[11px] text-muted-foreground">正在更新目录…</span>
          </div>
          <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">移除仅取消扫描，不会删除目录或文件。</p>
        </section>

        <!-- ============ 缓存 ============ -->
        <section class="border-t border-border pt-6">
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
            <!-- 与「下载」互相排斥（另一边同样 disabled）：扫描条在顶部、下载条在更新节，
                 同色同粗细的两条墨线同时在屏上会让人分不清谁是谁。 -->
            <Button variant="outline" size="xs" :disabled="scanning || dlState === 'downloading'" @click="rescanNow">
              <RefreshCw />
              立即重扫
            </Button>
            <Button variant="ghost" size="xs" class="text-muted-foreground" :disabled="scanning" @click="clearCache">
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
               同一支笔。总量拿不到时退化成「已接收 X」。
               ⚠️ 只在节内这一份，别再往顶部搬：顶部那条归扫描，共用会串成一根线
               （原因见上方顶部进度条的注释）。滚出视野的代价已经用「下载按钮期间互斥」
               的方式降掉了——同一时刻页面上只可能有这一进度在进行。 -->
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
              :disabled="scanning"
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

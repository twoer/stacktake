<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import { Code, Download, FolderPlus, MessageSquare, RefreshCw, Scale, Trash2 } from "@lucide/vue";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "vue-sonner";
import { formatAgo } from "@/lib/types";
import { LOCALE_OPTIONS, type AppLocale } from "@/lib/locale";
import { t } from "@/lib/i18n";
import { ISSUES_URL, LICENSE_URL, REPO_URL } from "@/lib/release";
import { useAssets } from "@/composables/useAssets";
import { useAutoScan } from "@/composables/useAutoScan";
import { useLocale } from "@/composables/useLocale";
import { useRepoRoots } from "@/composables/useRepoRoots";
import { useTheme } from "@/composables/useTheme";
import { useUpdate } from "@/composables/useUpdate";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import ByteValue from "@/components/ByteValue.vue";
import Hint from "@/components/Hint.vue";
import { ScrollArea } from "@/components/ui/scroll-area";
import ScanBar from "@/components/ScanBar.vue";

const router = useRouter();
const { groups, cachedAt, loading, progress, rescan, now } = useAssets();
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

const { enabled: autoScanEnabled, setEnabled: setAutoScanEnabled } = useAutoScan();
/** reka 的 Checkbox 会额外发 "indeterminate"，这里只有开/关两态。 */
function onAutoScanChange(value: boolean | "indeterminate") {
  setAutoScanEnabled(value === true);
}

const { preference: themePreference, setPreference: setThemePreference } = useTheme();
const THEME_OPTIONS = computed(() => [
  { value: "system" as const, label: t("settings.appearance.auto") },
  { value: "light" as const, label: t("settings.appearance.light") },
  { value: "dark" as const, label: t("settings.appearance.dark") },
]);

// 语言名本身不翻译（English / Español / 日本語 …），只有「跟随系统」那句随界面语言走。
const { preference: localePreference, resolved: localeResolved, setPreference: setLocalePreference } =
  useLocale();
const endonym = (value: AppLocale) =>
  LOCALE_OPTIONS.find((opt) => opt.value === value)?.label ?? value;
const LANGUAGE_OPTIONS = computed(() => [
  {
    value: "system" as const,
    label: t("settings.language.systemWith", { language: endonym(localeResolved.value) }),
  },
  ...LOCALE_OPTIONS,
]);

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
    toast.success(t("settings.cache.clearedToast"));
  } catch (e) {
    toast.error(String(e));
  }
}

/** 立即重扫：等扫完再跳回资产页——26 秒的等待在设置页做完，回去就是新数据。 */
async function rescanNow() {
  const r = await rescan();
  if (r.ok) {
    toast.success(t("settings.cache.rescanDone"));
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
/** 「当前数据更新于」与 Assets 状态栏接同一根时间针：开着过夜也要往前走。 */
const cachedAtLabel = computed(() =>
  cachedAt.value ? formatAgo(cachedAt.value, now.value) : "—",
);
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
      <h1 class="text-sm font-semibold">{{ t("settings.title") }}</h1>
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
        <section aria-labelledby="language-heading">
          <h2 id="language-heading" class="text-[13px] font-medium">{{ t("settings.language.heading") }}</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            {{ t("settings.language.hint") }}
          </p>
          <fieldset class="mt-3">
            <legend class="sr-only">{{ t("settings.language.legend") }}</legend>
            <!-- 单行自适应宽度、放不下才折行：等宽网格把六项摊成两排大块，
                 高度是外观那条轨道的一倍，视觉上压过了它。 -->
            <div
              class="inline-flex max-w-full flex-wrap gap-0.5 rounded-md border border-border/60 bg-muted/60 p-0.5"
            >
              <label
                v-for="opt in LANGUAGE_OPTIONS"
                :key="opt.value"
                class="inline-flex h-6 cursor-pointer select-none items-center justify-center rounded-sm px-2.5 text-[11px] transition-colors has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring"
                :class="
                  localePreference === opt.value
                    ? 'bg-card text-foreground shadow-sm ring-1 ring-border/60'
                    : 'text-muted-foreground hover:text-foreground'
                "
              >
                <input
                  type="radio"
                  name="language"
                  class="sr-only"
                  :value="opt.value"
                  :checked="localePreference === opt.value"
                  @change="setLocalePreference(opt.value)"
                />
                {{ opt.label }}
              </label>
            </div>
          </fieldset>
        </section>

        <section aria-labelledby="appearance-heading" class="border-t border-border pt-6">
          <h2 id="appearance-heading" class="text-[13px] font-medium">{{ t("settings.appearance.heading") }}</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            {{ t("settings.appearance.hint") }}
          </p>
          <!-- 原生 radio 保留键盘方向键与读屏语义，视觉收成一条等宽轨道。
               标签不折行：24px 的段内放不下第二行，折出来的字会溢到轨道外
               （es 的「Automático (seguir al sistema)」踩过）。真放不下时
               宁可让外层换行，也别把一句话劈成两行塞进 24px。 -->
          <fieldset class="mt-3">
            <legend class="sr-only">{{ t("settings.appearance.legend") }}</legend>
            <div
              class="inline-grid max-w-full grid-flow-col auto-cols-fr rounded-md border border-border/60 bg-muted/60 p-0.5"
            >
              <label
                v-for="opt in THEME_OPTIONS"
                :key="opt.value"
                class="inline-flex h-6 cursor-pointer select-none items-center justify-center whitespace-nowrap rounded-sm px-3 text-xs transition-colors has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring"
                :class="
                  themePreference === opt.value
                    ? 'bg-card text-foreground shadow-sm ring-1 ring-border/60'
                    : 'text-muted-foreground hover:text-foreground'
                "
              >
                <input
                  type="radio"
                  name="appearance"
                  class="sr-only"
                  :value="opt.value"
                  :checked="themePreference === opt.value"
                  @change="setThemePreference(opt.value)"
                />
                {{ opt.label }}
              </label>
            </div>
          </fieldset>
        </section>

        <section aria-labelledby="repo-roots-heading" class="border-t border-border pt-6" :aria-busy="!!repoRootsPending">
          <h2 id="repo-roots-heading" class="text-[13px] font-medium">{{ t("settings.repoRoots.heading") }}</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            {{ t("settings.repoRoots.hint") }}
          </p>
          <template v-if="repoRoots">
            <h3 class="mt-3 text-[11px] text-muted-foreground">{{ t("settings.repoRoots.defaultRoots") }}</h3>
            <ul class="mt-1">
              <li v-for="path in repoRoots.default_roots" :key="path" class="border-b border-border/40 py-1.5">
                <span class="select-text font-mono text-[11px] [overflow-wrap:anywhere]">{{ path }}</span>
              </li>
            </ul>
            <p v-if="!repoRoots.default_roots.length" class="mt-1 text-[11px] text-muted-foreground">{{ t("settings.repoRoots.notFound") }}</p>
            <h3 class="mt-3 text-[11px] text-muted-foreground">{{ t("settings.repoRoots.extraRoots") }}</h3>
            <ul class="mt-1">
              <li v-for="root in repoRoots.extra_roots" :key="root.path" class="flex items-start gap-2 border-b border-border/40 py-1.5">
                <div class="min-w-0 flex-1">
                  <p class="select-text font-mono text-[11px] leading-6 [overflow-wrap:anywhere]">{{ root.path }}</p>
                  <p v-if="!root.available" class="text-[11px] text-muted-foreground">{{ t("settings.repoRoots.unavailable") }}</p>
                </div>
                <Hint :text="t('settings.repoRoots.removeHint')">
                  <Button
                    variant="ghost"
                    size="xs"
                    class="shrink-0 text-muted-foreground"
                    :disabled="repoRootsBusy || dlState === 'downloading'"
                    :aria-label="t('settings.repoRoots.removeAria', { path: root.path })"
                    @click="removeRoot(root.path)"
                  >{{ t("settings.repoRoots.remove") }}</Button>
                </Hint>
              </li>
            </ul>
            <p v-if="!repoRoots.extra_roots.length" class="mt-1 text-[11px] text-muted-foreground">{{ t("settings.repoRoots.noExtra") }}</p>
          </template>
          <p v-if="repoRootsPending === 'loading'" role="status" class="mt-3 text-[11px] text-muted-foreground">{{ t("settings.repoRoots.reading") }}</p>
          <div v-if="repoRootsError" class="mt-3 text-[11px]">
            <p role="alert" class="whitespace-pre-line text-destructive [overflow-wrap:anywhere]">{{ repoRootsError }}</p>
            <Button variant="ghost" size="xs" class="mt-1 text-muted-foreground" :disabled="repoRootsBusy" @click="loadRoots">
              <RefreshCw aria-hidden="true" />
              {{ t("settings.repoRoots.reread") }}
            </Button>
          </div>
          <div class="mt-3 flex flex-wrap items-center gap-2">
            <Button variant="outline" size="xs" :disabled="repoRootsBusy || !repoRoots || dlState === 'downloading'" @click="addRoot">
              <FolderPlus aria-hidden="true" />
              {{ t("settings.repoRoots.add") }}
            </Button>
            <span v-if="repoRootsPending === 'adding' || repoRootsPending === 'removing'" role="status" class="text-[11px] text-muted-foreground">{{ t("settings.repoRoots.updating") }}</span>
          </div>
          <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">{{ t("settings.repoRoots.footnote") }}</p>
        </section>

        <!-- ============ 缓存 ============ -->
        <section class="border-t border-border pt-6">
          <h2 class="text-[13px] font-medium">{{ t("settings.cache.heading") }}</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            {{ t("settings.cache.hint") }}
          </p>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            {{ t("settings.cache.updated", { ago: cachedAtLabel }) }}
          </p>
          <!-- 勾选框与文案整行可点：for 指向 Checkbox 的 button，label 的激活行为
               会转成一次 click。别把 click 也挂到外层，否则一次点击切两下。 -->
          <div class="mt-3 flex items-start gap-2">
            <Checkbox
              id="auto-scan"
              aria-labelledby="auto-scan-label"
              aria-describedby="auto-scan-hint"
              :model-value="autoScanEnabled"
              class="mt-0.5"
              @update:model-value="onAutoScanChange"
            />
            <label for="auto-scan" class="min-w-0 cursor-pointer select-none">
              <span id="auto-scan-label" class="block text-xs">{{ t("settings.cache.autoLabel") }}</span>
              <span id="auto-scan-hint" class="mt-0.5 block text-[11px] leading-relaxed text-muted-foreground">
                {{ t("settings.cache.autoHint") }}
              </span>
            </label>
          </div>
          <!-- 缓存被清空后的引导态：groups 为空且 cachedAt 为 null 时显示 -->
          <div v-if="!cachedAt && groups.length === 0" class="mt-3 rounded-md border border-border/40 bg-muted/40 px-3 py-2.5">
            <p class="text-xs font-medium text-foreground">{{ t("settings.cache.clearedTitle") }}</p>
            <p class="mt-1 text-[11px] leading-relaxed text-muted-foreground">
              {{ t("settings.cache.clearedHint") }}
            </p>
          </div>
          <div class="mt-3 flex items-center gap-1.5">
            <!-- 与「下载」互相排斥（另一边同样 disabled）：扫描条在顶部、下载条在更新节，
                 同色同粗细的两条墨线同时在屏上会让人分不清谁是谁。 -->
            <Button variant="outline" size="xs" :disabled="scanning || dlState === 'downloading'" @click="rescanNow">
              <RefreshCw />
              {{ t("settings.cache.rescanNow") }}
            </Button>
            <Button variant="ghost" size="xs" class="text-muted-foreground" :disabled="scanning" @click="clearCache">
              <Trash2 />
              {{ t("settings.cache.clear") }}
            </Button>
          </div>
          <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            {{ t("settings.cache.footnote") }}
          </p>
        </section>

        <!-- ============ 更新 ============ -->
        <section class="border-t border-border pt-6">
          <h2 class="text-[13px] font-medium">{{ t("settings.update.heading") }}</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            {{ t("settings.update.hint", { version: appVersion || "—" }) }}
          </p>

          <!-- 下载进度：节内一条 ScanBar + 12px 正文字号的百分比。
               第一版只有一行 11px 灰字，点完下载按钮消失后附近没有任何显眼信号
               （2026-10-21 用户反馈「不明显」）——现在与扫描进度同一支笔、
               同一支笔。总量拿不到时退化成「已接收 X」。
               ⚠️ 只在节内这一份，别再往顶部搬：顶部那条归扫描，共用会串成一根线
               （原因见上方顶部进度条的注释）。滚出视野的代价已经用「下载按钮期间互斥」
               的方式降掉了——同一时刻页面上只可能有这一进度在进行。 -->
          <div v-if="dlState === 'downloading'" class="mt-3">
            <ScanBar :pct="dlPct" :label="t('settings.update.downloadProgress')" />
            <p class="mt-1.5 text-[12px] leading-relaxed text-foreground">
              {{ t("settings.update.downloading") }}
              <span class="font-medium tabular-nums">
                {{ dlProgress?.total ? `${dlPct}%` : t("settings.update.received") }}
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
            {{ t("settings.update.downloaded", { path: dlPath }) }}
          </p>
          <p v-else-if="dlMessage" class="mt-2 text-[11px] leading-relaxed text-destructive">
            {{ t("settings.update.failed", { message: dlMessage }) }}
          </p>

          <!-- 检查状态跟在按钮后面（与「补充目录」那行同一形态）：idle 不写任何东西。
               启动那次检查如果失败，这里就是空的，因为失败被刻意静默了；
               想看具体原因就点一次「检查更新」。 -->
          <div class="mt-3 flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="xs"
              :disabled="updateState === 'checking' || dlState === 'downloading'"
              @click="checkNow"
            >
              <RefreshCw />
              {{ t("settings.update.check") }}
            </Button>
            <Button
              v-if="updateState === 'available' && dlState !== 'downloading'"
              size="xs"
              :disabled="scanning"
              @click="downloadUpdate"
            >
              <Download />
              {{ t("settings.update.download", { version: latest }) }}
            </Button>
            <span
              v-if="updateState !== 'idle'"
              class="min-w-0 text-[11px] leading-relaxed [overflow-wrap:anywhere]"
            >
              <template v-if="updateState === 'checking'">
                <span class="text-muted-foreground">{{ t("settings.update.checking") }}</span>
              </template>
              <template v-else-if="updateState === 'available'">
                <span class="text-primary">{{ t("settings.update.available", { version: latest }) }}</span>
                <span class="text-muted-foreground">{{ t("settings.update.availableHint") }}</span>
              </template>
              <template v-else-if="updateState === 'up-to-date'">
                <span class="text-muted-foreground">{{ t("settings.update.upToDate") }}</span>
              </template>
              <template v-else-if="updateState === 'error'">
                <span class="text-destructive">{{ t("settings.update.checkFailed", { message: updateError }) }}</span>
              </template>
            </span>
          </div>
          <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            {{ t("settings.update.footnoteBefore")
            }}<button
              class="rounded text-primary hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              @click="openDownload"
            >{{ t("settings.update.footnoteLink") }}</button>{{ t("settings.update.footnoteAfter") }}
          </p>
        </section>

        <!-- ============ 项目 ============ -->
        <section class="border-t border-border pt-6">
          <h2 class="text-[13px] font-medium">{{ t("settings.project.heading") }}</h2>
          <p class="mt-1 leading-relaxed text-muted-foreground">
            {{ t("settings.project.hint") }}
          </p>
          <div class="mt-3 flex items-center gap-1.5">
            <Button variant="outline" size="xs" @click="openLink(REPO_URL)">
              <Code />
              {{ t("settings.project.repo") }}
            </Button>
            <Button variant="ghost" size="xs" class="text-muted-foreground" @click="openLink(ISSUES_URL)">
              <MessageSquare />
              {{ t("settings.project.issues") }}
            </Button>
            <Button variant="ghost" size="xs" class="text-muted-foreground" @click="openLink(LICENSE_URL)">
              <Scale />
              {{ t("settings.project.license") }}
            </Button>
          </div>
        </section>
      </div>
    </ScrollArea>
  </div>
</template>

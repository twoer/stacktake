<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
// 侧栏图标（2026-10-21 用户拍板重开 U15）。U15 原判「侧栏不放图标」——通用线性图标
// 铺满侧栏是生成式界面的指纹，且当时 IconPark 分组图标 + Lucide 控件图标两套并排，
// 谁都不像立场。重开后只守两条：① 单色 currentColor，跟着行文字走（选中是墨色、
// 未选中退一档），不引彩色——配色仍只出自 style.css 的纸墨 token；② 只此一套 Lucide。
// 鸿蒙走自建的 HarmonyOsIcon（Lucide 无鸿蒙、品牌图标 v1 已删）。
import {
  Apple,
  Boxes,
  BrainCircuit,
  Coffee,
  FolderGit2,
  LayoutDashboard,
  Package,
  Settings,
  Smartphone,
  Sparkles,
  Terminal,
} from "@lucide/vue";
import type { Component } from "vue";
import HarmonyOsIcon from "@/components/icons/HarmonyOsIcon.vue";
import { groupSize } from "@/lib/types";
import { EASE_OUT, reducedMotion } from "@/lib/motion";
import { useAssets } from "@/composables/useAssets";
import { useUpdate } from "@/composables/useUpdate";
import { useTheme } from "@/composables/useTheme";
import ByteValue from "@/components/ByteValue.vue";
import Hint from "@/components/Hint.vue";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";

// titleBarStyle: Overlay 只对 macOS 生效；Windows 走系统标题栏，webview 从标题栏
// 下方才开始，侧栏的红绿灯空带在那边是纯空白。口径与 index.html 打 .macos 同帧
// 标记一致，两边别分家。
const isMac = /Mac/.test(navigator.userAgent);

const route = useRoute();
const router = useRouter();
const { groups, current, activeId, boot } = useAssets();
const { theme } = useTheme();
// 版本号与更新状态都由 useUpdate 提供：它启动时顺手拿一次 getVersion() 用来和
// 远端 tag 比对，侧栏与设置页共用同一份，不各调一次。
const { state: updateState, version, latest, checkOnBoot } = useUpdate();

/** 在设置页时侧栏不点亮任何分组，URL 同步 watch 也不纠偏（否则一进设置页
 *  就被 replace 回 /assets/overview——activeId 为 null 时 current 回落第一组）。 */
const onSettings = computed(() => route.path.startsWith("/settings"));

/**
 * 侧栏要显示的分组：**剔掉总览**（它是现算的合成行，不是一个真实分组），
 * 顺序沿用后端 `GROUP_ORDER` 的固定语义序（D13 拍板：源码 → 开发环境 →
 * 依赖缓存 → 工具数据 → 模型 → 虚拟机）。前端不再排序。
 */
const sidebarGroups = computed(() => groups.value.filter((g) => g.id !== "overview"));

/**
 * 侧栏图标表：分组 id → 图标构件。
 *
 * **未知 id 回落 Boxes，不是报错也不是空白**——AGENTS 架构段要求「新增资产类型只需
 * 实现 Scanner 并注册进 scanners()，前端不用改」，所以新分组必须能在不动前端的前提下
 * 出现；缺一颗图标不该让它整行缩进塌掉（有的行有图标有的行没有，比都有一颗通用图标丑）。
 * 表本身覆盖全部 9 个 id（含 H14 隐藏的 java / harmony 与仅 Windows 注册的 wsl），
 * 撤隐藏 / 换平台都不用回来补。
 *
 * 总览与设置这两个非分组入口也走这张表（它们和分组行同一套选中规则 + 同一条墨标，
 * 图标口径不该分家）。
 */
const SIDE_ICONS: Record<string, Component> = {
  overview: LayoutDashboard,
  repos: FolderGit2,
  java: Coffee,
  ios: Apple,
  android: Smartphone,
  harmony: HarmonyOsIcon,
  toolchain: Package,
  ai_tools: Sparkles,
  local_models: BrainCircuit,
  wsl: Terminal,
  settings: Settings,
};

const sideIcon = (id: string): Component => SIDE_ICONS[id] ?? Boxes;

/** 全机合计 = 各分组体积之和（总览行的体积就是这个数，但它是现算的，不重复累它）。 */
const totalSize = computed(() =>
  sidebarGroups.value.reduce((s, g) => s + groupSize(g), 0),
);

onMounted(async () => {
  // 先等初始导航完成再 boot，否则 URL 带 /assets/git 时，
  // boot 早于路由就绪会令 watch 把默认分组 replace 上去，覆盖 URL 里的分组
  await router.isReady();
  await boot();
  // 版本检查放在 boot 之后、不 await：本工具唯一一次对外请求（api.github.com），
  // 6 秒超时，失败不留痕。别让它挡住首屏。
  void checkOnBoot();
});

// 让 URL 始终等于「实际显示的那个分组」：
// - URL 没写分组（/assets）→ 补上默认分组（总览，groups[0]），否则侧栏选中态对不上；
// - URL 写的分组不存在（手输错 id、或重扫后该分组消失）→ 换成实际显示的那个。
//   旧行为是静默回退到第一组却不纠正 URL，地址栏会一直停在那个不存在的 id 上。
// replace 之后 id === target，watch 不会自激。
watch(
  [groups, activeId],
  ([g, id]) => {
    // 设置页不参与分组 URL 同步：它没有 groupId，不拦就会被 replace 回总览
    if (!g.length || onSettings.value) return;
    const target = current.value?.id;
    if (target && id !== target) router.replace(`/assets/${target}`);
  },
  { immediate: true },
);

function pick(id: string) {
  router.push(`/assets/${id}`);
}

// ---- 侧栏墨标（U16）----
//
// 当前项的左侧一道 2px 墨线。它是「书签」不是图标：不参与布局、不抢选中 pill
// 的对比，只在切换分组时滑过去，让侧栏的选中状态有一个方向感。
// 设置与分组共用同一条墨标——设置也是「当前位置」的一部分。

const sidebarEl = ref<HTMLElement | null>(null);
const markerEl = ref<HTMLElement | null>(null);
let markerY = 0;
let markerReady = false;

const activeSideKey = computed(() => {
  if (onSettings.value) return "settings";
  return current.value?.id ?? null;
});

function placeMarker(animate: boolean) {
  const side = sidebarEl.value;
  const mark = markerEl.value;
  const key = activeSideKey.value;
  if (!side || !mark || !key) {
    if (mark) mark.style.opacity = "0";
    return;
  }
  const item = side.querySelector<HTMLElement>(`[data-side-key="${key}"]`);
  if (!item) {
    mark.style.opacity = "0";
    return;
  }
  // 连续快速切换时，上一段滑动还挂在元素上；取它此刻的位移当起点再收掉，
  // 否则新动画会从旧目标值起跳、与残留动画叠着算
  let from = markerY;
  const running = mark.getAnimations();
  if (running.length) {
    const now = getComputedStyle(mark).transform;
    if (now && now !== "none") from = new DOMMatrixReadOnly(now).m42;
    for (const anim of running) anim.cancel();
  }
  const sr = side.getBoundingClientRect();
  const r = item.getBoundingClientRect();
  const to = r.top - sr.top + r.height / 2 - 9;
  // 分组多到滚出视口时，墨标跟着藏起来，别浮到红绿灯上
  mark.style.opacity = r.bottom > sr.top && r.top < sr.bottom ? "1" : "0";
  if (!animate || !markerReady || reducedMotion()) {
    mark.style.transform = `translateY(${to}px)`;
  } else if (Math.abs(to - from) > 0.5) {
    mark.animate(
      [
        { transform: `translateY(${from}px)` },
        { transform: `translateY(${(from + to) / 2}px) scaleY(1.35)`, offset: 0.55 },
        { transform: `translateY(${to}px)` },
      ],
      { duration: 300, easing: EASE_OUT },
    );
    mark.style.transform = `translateY(${to}px)`;
  }
  markerY = to;
  markerReady = true;
}

watch([activeSideKey, () => groups.value.length], () => {
  void nextTick(() => placeMarker(true));
});

function onSidebarScroll(e: Event) {
  const target = e.target as Node | null;
  if (target && sidebarEl.value?.contains(target)) placeMarker(false);
}
const onResize = () => placeMarker(false);

onMounted(() => {
  // 侧栏内部滚动不冒泡到 window，capture 才接得住（与 Assets 的菜单同一手法）
  window.addEventListener("scroll", onSidebarScroll, true);
  window.addEventListener("resize", onResize);
  void nextTick(() => requestAnimationFrame(() => placeMarker(false)));
});

onUnmounted(() => {
  window.removeEventListener("scroll", onSidebarScroll, true);
  window.removeEventListener("resize", onResize);
});
</script>

<template>
  <!-- TooltipProvider 只提供上下文、不渲染 DOM（reka 的 Provider 就是个 slot），
       所以套在根容器外面不改变任何布局。

       delay-duration 500：比原生的约 1 秒短一半。账本行密，扫过就想看到说明，
       但要长到「手腕划过整列」不会沿途连弹。

       skip-delay-duration 0 是刻意的：reka 默认 300，含义是「关掉一个 tooltip 后
       300ms 内再 hover 别的触发器就立刻弹」。账本行的标签是紧挨着的，默认行为会
       让相邻标签秒开——那就不是「每个 hover 都等 500ms」了。归零之后每次悬停都
       实打实等满，代价是在同一行里挨个看标签要每个都停半秒。 -->
  <TooltipProvider :delay-duration="500" :skip-delay-duration="0">
    <!-- 根容器不挂 bg-background：macOS 下窗口透明，这里染色会把侧栏的 vibrancy 盖掉。
         不透明底交给主区那条 div；Windows 下由 body 的实色底兜住，观感与之前一致。 -->
    <div class="flex h-screen text-foreground">
      <!-- 侧栏：Finder/Mail 式通高。顶部留一条标题栏高度的空带给系统红绿灯
           （titleBarStyle: Overlay，真红绿灯浮在内容上，这里不画假的）-->
      <aside
        ref="sidebarEl"
        class="relative flex w-[208px] shrink-0 flex-col border-r border-border/50 bg-muted/40"
      >
        <!-- 标题栏空带留给系统红绿灯（titleBarStyle: Overlay 下真红绿灯浮在内容上，
             这里不画假的，也不再放产品名——顶栏已有分组名，侧栏不必再标一次）。
             仅 macOS 渲染：Windows 忽略 titleBarStyle、系统标题栏在 webview 外面，
             这条空带只是 28px 死空白（真机上整个侧栏内容跟着下移）。 -->
        <div v-if="isMac" class="h-7 shrink-0" data-tauri-drag-region />

        <ScrollArea class="min-h-0 flex-1">
          <!-- pt-4 不是随便填的：首行上缘实测 32px 时距红绿灯只剩 13px，白色选中 pill
               一贴上去就显得顶到红绿灯了；44px（pt-4）是 Finder/Mail 的口径。 -->
          <nav class="space-y-px px-1.5 pb-2 pt-4">
            <button
              data-side-key="overview"
              class="flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              :class="
                !onSettings && current?.id === 'overview'
                  ? // 深色下 bg-background 比侧栏底和 hover 都暗，选中行会变成最不突出的一行，
                    // 所以深色改挂 bg-accent，保持「选中 > hover > 侧栏底」的明序（U1 调过的）
                    'bg-background font-medium text-foreground dark:bg-accent'
                  : 'text-muted-foreground hover:bg-muted/60'
              "
              :aria-current="!onSettings && current?.id === 'overview' ? 'page' : undefined"
              @click="pick('overview')"
            >
              <component :is="sideIcon('overview')" class="size-3.5 shrink-0" />
              <span class="min-w-0 flex-1 truncate text-[13px]">总览</span>
              <span class="shrink-0 text-[11px] text-muted-foreground">
                <ByteValue :bytes="totalSize" animate />
              </span>
            </button>

            <div class="px-2 pb-1 pt-3 text-[11px] font-semibold text-muted-foreground">
              分组
            </div>

            <!-- 分组名被侧栏宽度截断时，tooltip 给全名。side=right：向侧栏内侧展开，
                 朝上会压住红绿灯那一条。 -->
            <Hint
              v-for="g in sidebarGroups"
              :key="g.id"
              :text="g.label"
              side="right"
            >
              <button
                :data-side-key="g.id"
                class="flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                :class="
                  !onSettings && current?.id === g.id
                    ? 'bg-background font-medium text-foreground dark:bg-accent'
                    : 'text-muted-foreground hover:bg-muted/60'
                "
                :aria-current="!onSettings && current?.id === g.id ? 'page' : undefined"
                @click="pick(g.id)"
              >
                <component :is="sideIcon(g.id)" class="size-3.5 shrink-0" />
                <span class="min-w-0 flex-1 truncate text-[13px]">{{ g.label }}</span>
                <span class="shrink-0 text-[11px] text-muted-foreground">
                  <ByteValue v-if="groupSize(g) > 0" :bytes="groupSize(g)" animate />
                  <template v-else>—</template>
                </span>
              </button>
            </Hint>
          </nav>
        </ScrollArea>

        <!-- 设置：钉在侧栏底部、不进 ScrollArea——分组多到能滚时也不能跟着滚走。
             早年挂在状态栏右侧，2026-10-19 按用户要求挪回侧栏：它是「整个应用」的入口，
             和「数据更新于」这种当前视图的状态同栏，等于每换一个分组就要重新找一遍。
             选中态与分组行同一套规则（pill + 加粗），切换过来一眼看得出在哪。
             版本号并入按钮（2026-10-21 用户拍板）：撤掉独立的「有更新」行，
             有更新时把显示的版本号换成新版本 + 小点，点击仍进设置页——侧栏只做
             状态展示，下载动作在设置页的「更新」节（一次点击一个职责）。
             启动那次检查是静默的：断网、没有 release、读不到版本号，这里什么都不显。
             上方分隔线 2026-10-21 用户拍板去掉（border-t）：设置行与分组之间只靠留白分隔。 -->
        <div class="px-1.5 py-1.5">
          <!-- 版本提示挂这里：按钮右侧已经印着 v0.1.7，再补一句「有新版本」只能靠
               hover。没读到版本号时 Hint 不出实例，按钮也不会变成空 tooltip 的靶子。 -->
          <Hint
            :text="
              updateState === 'available'
                ? `有新版本 ${latest}，前往设置页下载`
                : version
                  ? `当前版本 v${version}`
                  : undefined
            "
            side="right"
          >
            <button
              data-side-key="settings"
              class="flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              :class="
                onSettings
                  ? 'bg-background font-medium text-foreground dark:bg-accent'
                  : 'text-muted-foreground hover:bg-muted/60'
              "
              :aria-current="onSettings ? 'page' : undefined"
              @click="router.push('/settings')"
            >
              <component :is="sideIcon('settings')" class="size-3.5 shrink-0" />
              <span class="min-w-0 flex-1 truncate text-[13px]">设置</span>
              <template v-if="updateState === 'available'">
                <span class="size-1.5 shrink-0 rounded-full bg-primary" />
                <span class="shrink-0 text-[11px] tabular-nums text-primary">{{ latest }}</span>
              </template>
              <span v-else-if="version" class="shrink-0 text-[11px] tabular-nums text-muted-foreground">
                v{{ version }}
              </span>
            </button>
          </Hint>
        </div>
        <span ref="markerEl" class="side-marker" style="opacity: 0" aria-hidden="true" />
      </aside>

      <!-- 主区自己扛不透明底：macOS 透明窗口下，只有侧栏该透出 vibrancy -->
      <div class="flex min-w-0 flex-1 flex-col bg-background">
        <main class="min-h-0 flex-1">
          <router-view />
        </main>
      </div>

      <!-- toast 宿主。G1（4a924d7）重写 App.vue 时把这个挂载点丢了，此后所有
           toast.* 静默 no-op——清理结果、复制路径、重扫、设置四动作全无反馈。
           theme 必须显式传：vue-sonner 自己不读 .dark class（见 Assets.vue 同款注释）。
           挂载点由 App.vue 提供，调用方在 views 里，别再把这里删掉。 -->
      <Toaster :theme="theme === 'dark' ? 'dark' : 'light'" position="top-center" :duration="4000" />
    </div>
  </TooltipProvider>
</template>

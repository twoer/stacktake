<script setup lang="ts">
import { PanelRight, RefreshCw, Search, X } from "@lucide/vue";
import { t } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import Hint from "@/components/Hint.vue";

/**
 * 资产页工具栏（2026-10-24 拆分计划阶段 4 从 Assets.vue 搬出，模板逐字保留）：
 * 标题 + 中段插槽（维度筛选 / 搜索）+ 右侧检查器 / 重扫控件。
 *
 * 职责边界：标题与重扫文案由父组件算好传入（判据依赖 loading / progress /
 * isOverview，收进来反而要把这圈状态都搬过来）；检查器开合与搜索词是真值
 * 在父的 defineModel；外观三档在设置页，不占这里。空区是窗口拖动区
 * （Overlay 标题栏要自己给），整条挂 drag-region，控件靠自身交互恢复事件。
 */
defineProps<{
  title: string;
  loading: boolean;
  /** 总览没有检查器（数字都在主区首块），开关一并隐藏，点了也是空操作。 */
  isOverview: boolean;
  rescanLabel: string;
  rescanTitle: string;
}>();

const inspectorOpen = defineModel<boolean>("inspectorOpen", { default: true });
const search = defineModel<string>("search", { default: "" });

const emit = defineEmits<{
  rescan: [];
}>();
</script>

<template>
  <!-- 工具栏：标题 + 维度筛选 + 搜索 + 检查器/重扫。
       空区是窗口拖动区（titleBarStyle: Overlay 要自己给）。
       整条挂 drag-region，控件用 pointer-events: auto 恢复交互。 -->
  <div class="flex h-11 shrink-0 items-center gap-2 border-b border-border/40 bg-background px-3" data-tauri-drag-region>
    <h1 class="min-w-0 truncate text-sm font-semibold">
      {{ title }}
    </h1>
    <!-- 条目数不挂在这里：状态栏与检查器概览各有一份，同一屏三处「238 项 · 109 GB」
         是纯重复。工具栏留给标题与控件。 -->

    <div class="flex-1" />

    <slot />

    <!-- label 必须自己也是 flex：否则 input 作为 inline-block 按 baseline 对齐、底下还挂一段
         strut 下行空隙，整个输入框在 WebKit 下向下溢出 2px，输入框里的文字因此比图标低
         2.75px（固定在盒子中心的图标就显得偏高）。实测：加 flex 后 input 精确落在 0~24。 -->
    <!-- 搜索先隐藏（2026-10-20）。整块 v-if 移出 DOM 而不是 display:none——
         留在 DOM 里 `/` 还能把焦点打进一个看不见的输入框。恢复时删掉 v-if 即可，
         下方的过滤逻辑与 `/` 快捷键守卫都原样保留。 -->
    <label v-if="false" class="group relative flex h-6 shrink-0">
      <span class="sr-only">{{ t("toolbar.searchAria") }}</span>
      <!-- mt-px：label 改成 flex 后 input 正好占满 24px，但 WebKit 对 input 内部文字的
           放置仍比盒子几何中心低 0.75px（改 line-height 无效，实测 leading-none 也是同一个值）。
           实测下移 1px 后偏差从 +0.75 收到 -0.25。 -->
      <Search
        class="pointer-events-none absolute left-1.5 top-1/2 mt-px size-3 -translate-y-1/2 text-muted-foreground"
      />
      <input
        id="row-search"
        v-model="search"
        type="search"
        :placeholder="t('toolbar.searchPlaceholder')"
        class="h-6 w-32 appearance-none rounded-md bg-muted/60 pl-6 pr-6 text-xs text-foreground transition-[width] duration-150 placeholder:text-muted-foreground focus:w-52 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring sm:w-40 sm:focus:w-56"
      />
      <!-- `/` 聚焦快捷键一直存在但没提示过；有输入内容或已聚焦时让位给清除按钮 -->
      <!-- 下面三个挂件与放大镜同一个规则：都落在「input 文字的视觉中线」而不是盒子几何中心上，
           所以一律带 mt-px（否则左右挂件之间会差这 1px）。 -->
      <kbd
        v-if="!search"
        class="pointer-events-none absolute right-1.5 top-1/2 mt-px -translate-y-1/2 rounded border border-border bg-muted px-1 text-[10px] leading-none text-muted-foreground/70 group-focus-within:hidden"
      >
        /
      </kbd>
      <button
        v-if="search"
        class="absolute right-1 top-1/2 mt-px -translate-y-1/2 text-muted-foreground hover:text-foreground"
        :aria-label="t('toolbar.clearSearch')"
        @click="search = ''"
      >
        <X class="size-3" />
      </button>
    </label>

    <!-- 别给 Windows 加 caption 按钮避让：它有系统标题栏，窗口按钮不浮在 webview 上，
         避让只会把重扫推离右缘（pr-[140px] 在 Windows 真机上就是这么露馅的）。 -->
    <div class="flex shrink-0 items-center gap-1">
      <!-- 总览没有检查器（数字都在主区首块），开关一并隐藏，点了也是空操作 -->
      <Hint v-if="!isOverview" :text="inspectorOpen ? t('toolbar.hideInspector') : t('toolbar.showInspector')">
        <Button
          size="xs"
          variant="ghost"
          class="h-6 w-6 px-0 text-muted-foreground [&_svg]:size-3.5"
          :class="inspectorOpen ? 'bg-muted text-foreground' : ''"
          :aria-label="inspectorOpen ? t('toolbar.hideInspector') : t('toolbar.showInspector')"
          :aria-pressed="inspectorOpen"
          aria-controls="asset-inspector"
          @click="inspectorOpen = !inspectorOpen"
        >
          <PanelRight aria-hidden="true" />
        </Button>
      </Hint>
      <!-- 扫描中按钮是 disabled 的：disabled 元素不再派发指针事件，这条提示在
           那几秒里不会出现。扫描范围本来就写在按钮文案里（「重扫」「重扫本组」），
           缺的只是 hover 那句，不是信息。 -->
      <Hint :text="rescanTitle">
        <Button
          size="xs"
          variant="ghost"
          :disabled="loading"
          class="h-6 min-w-[4.5rem] gap-1.5 text-[11px] text-muted-foreground [&_svg]:size-3.5"
          @click="emit('rescan')"
        >
          <RefreshCw :class="loading ? 'motion-safe:animate-spin' : ''" aria-hidden="true" />
          {{ rescanLabel }}
        </Button>
      </Hint>
    </div>
  </div>
</template>

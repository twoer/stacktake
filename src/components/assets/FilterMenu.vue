<script setup lang="ts">
import { computed } from "vue";
import { ListFilter } from "@lucide/vue";
import type { Dimension } from "@/lib/assets";
import { t } from "@/lib/i18n";
import Hint from "@/components/Hint.vue";

/**
 * 维度筛选菜单（2026-10-24 拆分计划阶段 4 从 Assets.vue 搬出，模板逐字保留）：
 * 工具栏一个漏斗按钮展开所有维度，选项带「其它维度已筛过」口径的计数。
 *
 * 职责边界：维度声明（dimensions）与计数（countFor 回调——依赖父级的行集合
 * 与搜索口径，不把行数据灌进来）由父组件给；选中值（dims）与开合（open）
 * 的真值也在父组件——Escape 优先级与「切分组重置」都挂在那边。选中即
 * `emit('set')`，关菜单的时序由父级的 setDim 决定（沿用拆出前的行为）。
 */
const props = defineProps<{
  dims: Record<string, string | null>;
  dimensions: Dimension[];
  open: boolean;
  countFor: (dimKey: string, value: string | null) => number;
}>();

const emit = defineEmits<{
  set: [key: string, value: string | null];
  clear: [];
  toggle: [];
}>();

const anyFilter = computed(() => Object.values(props.dims).some(Boolean));
/** 生效中的维度数。漏斗按钮上那个小数字，0 时不显示。 */
const activeDims = computed(() => Object.values(props.dims).filter(Boolean).length);

/**
 * 工具栏控件共同形态：ghost，无描边，hover 才给底。
 * 一排描边小盒子是「线条感」的最大来源——六个控件六个框，读起来像表单。
 */
const toolbarCtl =
  "flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";
</script>

<template>
  <!-- 筛选：一个漏斗按钮展开所有维度。总览没有维度可筛，按钮不出。
       此前是「每个维度一个描边按钮 + 一个清空按钮」，分组多时工具栏一排小盒子，
       线条感的主场；合并成一个入口也就不必给每个维度留位置。 -->
  <div class="relative shrink-0" data-dim-menu>
    <Hint :text="anyFilter ? t('filter.active', { count: activeDims }) : t('filter.label')">
      <button
        :class="[toolbarCtl, anyFilter || open ? 'bg-muted text-foreground' : '']"
        :aria-label="t('filter.label')"
        :aria-expanded="open"
        aria-controls="asset-filters"
        @click="emit('toggle')"
      >
        <ListFilter class="size-3.5" aria-hidden="true" />
        <span
          v-if="activeDims"
          :key="activeDims"
          class="badge-pop tabular-nums text-[10px]"
        >{{ activeDims }}</span>
      </button>
    </Hint>
    <div
      v-if="open"
      id="asset-filters"
      class="menu-in absolute right-0 top-full z-30 mt-1 min-w-44 rounded-md border border-border bg-card py-1 shadow-md"
    >
      <template v-for="(d, i) in dimensions" :key="d.key">
        <div v-if="i" class="my-1 border-t border-border/60" />
        <div class="px-2 pb-0.5 pt-1 text-[10px] text-muted-foreground">{{ d.label }}</div>
        <button
          class="flex w-full items-center justify-between gap-3 px-2 py-1 text-left text-xs text-foreground hover:bg-muted"
          :class="!dims[d.key] ? 'font-medium' : ''"
          @click="emit('set', d.key, null)"
        >
          <span class="flex min-w-0 items-center gap-2">
            <span class="filter-dot" :class="!dims[d.key] ? 'on' : ''" />
            <span>{{ t("common.all") }}</span>
          </span>
          <span class="tabular-nums text-muted-foreground">{{ countFor(d.key, null) }}</span>
        </button>
        <!-- 选项两半：`value` 是后端词表里的取值（筛选比较与计数口径都用它），
             `label` 是给人看的展示名。切语言只换 label，选中状态不会掉。 -->
        <button
          v-for="o in d.options"
          :key="o.value"
          class="flex w-full items-center justify-between gap-3 px-2 py-1 text-left text-xs text-foreground hover:bg-muted"
          :class="dims[d.key] === o.value ? 'font-medium' : ''"
          @click="emit('set', d.key, o.value)"
        >
          <span class="flex min-w-0 items-center gap-2">
            <span class="filter-dot" :class="dims[d.key] === o.value ? 'on' : ''" />
            <span class="truncate">{{ o.label }}</span>
          </span>
          <span class="tabular-nums text-muted-foreground">{{ countFor(d.key, o.value) }}</span>
        </button>
      </template>
      <div v-if="anyFilter" class="mt-1 border-t border-border/60 pt-1">
        <button
          class="flex w-full items-center px-2 py-1 text-left text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          @click="emit('clear')"
        >
          {{ t("filter.clear") }}
        </button>
      </div>
    </div>
  </div>
</template>

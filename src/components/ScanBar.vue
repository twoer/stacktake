<script setup lang="ts">
import { computed } from "vue";
import { t } from "@/lib/i18n";

/** `label` 只为无障碍：扫描与下载共用这一条，读屏软件要说对正在发生什么。
 *  缺省文案在模板里取 `t()`——属性默认值在模块加载时求值，会把语言锁死。
 *  `indeterminate`：单组重扫只有 0→1 两个事件，没有中间比例可画，改走来回扫的
 *  墨线（样式见 style.css 的 scanbar--indet）。 */
const props = withDefaults(
  defineProps<{ pct: number; label?: string; indeterminate?: boolean }>(),
  { label: undefined, indeterminate: false },
);
const clamped = computed(() => Math.max(0, Math.min(100, props.pct)));
</script>

<template>
  <!-- 真实进度条：墨色填充 + 行首笔尖。宽度用 transform 而不是 width，
       每来一个分组完成事件只重绘合成层，不触发布局。 -->
  <div
    class="scanbar"
    :class="indeterminate ? 'scanbar--indet' : ''"
    role="progressbar"
    :aria-label="props.label ?? t('assets.scanProgress')"
    :aria-valuenow="indeterminate ? undefined : Math.round(clamped)"
    aria-valuemin="0"
    aria-valuemax="100"
  >
    <div
      class="scanfill"
      :style="indeterminate ? undefined : { transform: `scaleX(${clamped / 100})` }"
    />
    <span v-if="!indeterminate" class="scannib" :style="{ left: `${clamped}%` }" />
  </div>
</template>

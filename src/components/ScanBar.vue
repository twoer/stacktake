<script setup lang="ts">
import { computed } from "vue";

/** `label` 只为无障碍：扫描与下载共用这一条，读屏软件要说对正在发生什么。
 *  `indeterminate`：单组重扫只有 0→1 两个事件，没有中间比例可画，改走来回扫的
 *  墨线（样式见 style.css 的 scanbar--indet）。 */
const props = withDefaults(
  defineProps<{ pct: number; label?: string; indeterminate?: boolean }>(),
  { label: "扫描进度", indeterminate: false },
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
    :aria-label="props.label"
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

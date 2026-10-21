<script setup lang="ts">
import { computed } from "vue";

/** `label` 只为无障碍：扫描与下载共用这一条，读屏软件要说对正在发生什么。 */
const props = withDefaults(defineProps<{ pct: number; label?: string }>(), {
  label: "扫描进度",
});
const clamped = computed(() => Math.max(0, Math.min(100, props.pct)));
</script>

<template>
  <!-- 真实进度条：墨色填充 + 行首笔尖。宽度用 transform 而不是 width，
       每来一个分组完成事件只重绘合成层，不触发布局。 -->
  <div
    class="scanbar"
    role="progressbar"
    :aria-label="props.label"
    :aria-valuenow="Math.round(clamped)"
    aria-valuemin="0"
    aria-valuemax="100"
  >
    <div class="scanfill" :style="{ transform: `scaleX(${clamped / 100})` }" />
    <span class="scannib" :style="{ left: `${clamped}%` }" />
  </div>
</template>

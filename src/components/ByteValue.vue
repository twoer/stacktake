<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { formatBytesParts } from "@/lib/types";
import { tweenNumber } from "@/lib/motion";

/**
 * `animate` 只给少数位置开（侧栏合计、总览首块、总览分组行）：重扫后这些数字
 * 是页面上最该被看见的变化；给几百行同时开 rAF 补间只会拖慢重绘。
 */
const props = withDefaults(defineProps<{ bytes: number; animate?: boolean }>(), {
  animate: false,
});

const display = ref(props.bytes);
let stopTween = () => {};

watch(
  () => props.bytes,
  (to) => {
    stopTween();
    if (!props.animate) {
      display.value = to;
      return;
    }
    stopTween = tweenNumber(display.value, to, 480, (v) => {
      display.value = v;
    });
  },
);

watch(
  () => props.animate,
  (on) => {
    if (!on) {
      stopTween();
      display.value = props.bytes;
    }
  },
);

onUnmounted(() => stopTween());

const parts = computed(() => formatBytesParts(display.value));
</script>

<template>
  <!-- 值与单位分字重：单位是同一串里最不重要的部分，降字重 + 降字号，
       **不降颜色也不加透明度**——可回收绿的实测对比度是 6.51:1（--reclaim，
       U14 起 green-700 退场），再乘透明度就掉到 4.5:1 以下，等于刚修好的
       对比度被排印改坏。
       值部不用 mono（U14 上过、U15 退回）：tabular-nums 已让数字列纵向严格对位，
       等宽字体只多带来「终端」气质——账本的数字是正文字体的黑，不是终端的绿。
       单位恒 11px：值越小单位越不能再小，否则踩到「信息性文字 ≥11px」的密度纪律。
       nowrap 是必须的：值 12px + 单位 11px 在窄列里会被折成两行，单位孤零零掉下去。 -->
  <span class="whitespace-nowrap"
    ><span class="font-medium tabular-nums">{{ parts.n }}</span
    ><span class="ml-0.5 text-[11px] font-normal">{{ parts.u }}</span></span
  >
</template>

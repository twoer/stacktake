<script setup lang="ts">
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/**
 * 「Tooltip + asChild 触发器 + 内容」三件套压成一个标签，调用点只写一行。
 *
 * 两处刻意的取舍：
 *
 * - 没文案（空串 / undefined）就原样吐出插槽，一个组件实例都不挂。仓库那组
 *   两百多行、每行最多再挂三个标签提示，没得说的行不该为它付实例的代价。
 * - 触发器走 asChild，不额外插 DOM。侧栏墨标按 `[data-side-key]` 量位置、
 *   账本行的 grid 单元格靠「父节点的直接子元素」成立，多一层 wrapper 就散了。
 */
withDefaults(
  defineProps<{
    text?: string;
    side?: "top" | "right" | "bottom" | "left";
    contentClass?: string;
  }>(),
  { side: "top" },
);
</script>

<template>
  <Tooltip v-if="text">
    <TooltipTrigger as-child><slot /></TooltipTrigger>
    <!-- 长路径会撑出屏幕：限宽 + 任意处断行（路径里没有空格，break-words 拦不住） -->
    <TooltipContent
      :side="side"
      :class="cn('max-w-[320px] [overflow-wrap:anywhere]', contentClass)"
    >{{ text }}</TooltipContent>
  </Tooltip>
  <slot v-else />
</template>

<script setup lang="ts">
import { computed } from "vue";
import { Copy, FolderOpen } from "@lucide/vue";
import type { Item } from "@/lib/types";
import { t } from "@/lib/i18n";
import { revealLabel } from "@/lib/path";

/**
 * 行右键菜单（2026-10-24 拆分计划阶段 2 从 Assets.vue 搬出，模板逐字保留）。
 *
 * 只有两项（揭示位置 / 复制路径），自己画比拉一个菜单库划算。动作经 emit
 * 交给父组件（`revealAt` 的 invoke 与剪贴板都在那边）；开合真相（menu ref）
 * 也在父组件——收起条件（点别处 / 滚动 / Esc / 改窗口大小）挂在 window
 * 监听上，Escape 优先级（弹窗 > 菜单 > 选中态）要读它。
 */
const props = defineProps<{
  menu: { x: number; y: number; item: Item } | null;
}>();

const emit = defineEmits<{
  reveal: [item: Item];
  copy: [item: Item];
}>();

/**
 * 靠右或靠下时往回收，别让菜单跑出窗口（菜单约 176×64）。
 * 注意：computed 里读 window.innerWidth **不是响应式的**——窗口 resize 时
 * 菜单不会重新钳位。这没问题：父组件把 resize 注册成了收起条件，菜单在
 * 重新布局前就已经关掉。两层是刻意配合，别拆散。
 */
const menuStyle = computed(() => {
  const m = props.menu;
  if (!m) return {};
  return {
    left: `${Math.max(4, Math.min(m.x, window.innerWidth - 180))}px`,
    top: `${Math.max(4, Math.min(m.y, window.innerHeight - 72))}px`,
  };
});
</script>

<template>
  <!-- 行右键菜单。只有两项，自己画一个比拉一个菜单库划算。
       位置钳制在视口内；点别处 / 滚动 / Esc / 改窗口大小都收（父组件的监听）。 -->
  <div
    v-if="menu"
    data-row-menu
    role="menu"
    class="ctx-menu menu-in fixed z-50 min-w-44 rounded-md border border-border bg-card py-1 shadow-md"
    :style="menuStyle"
  >
    <button
      role="menuitem"
      class="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs hover:bg-muted"
      @click="emit('reveal', menu.item)"
    >
      <FolderOpen class="size-3.5 shrink-0 text-muted-foreground" />
      {{ revealLabel() }}
    </button>
    <button
      role="menuitem"
      class="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs hover:bg-muted"
      @click="emit('copy', menu.item)"
    >
      <Copy class="size-3.5 shrink-0 text-muted-foreground" />
      {{ t("actions.copyPath") }}
    </button>
  </div>
</template>

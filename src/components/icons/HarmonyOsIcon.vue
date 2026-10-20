<script setup lang="ts">
// 鸿蒙的分组图标不走 IconPark——全库没有鸿蒙品牌图标。这里内联的是华为
// 2021 年（Mate X2 发布会）启用的 HarmonyOS 标志（圆环+底座），path 取自
// Ant Design 图标集的 harmony-o-s-outlined（Apache-2.0，与 IconPark 同协议）。
// fill 用 currentColor，深浅色自适应与侧栏其它图标一致。
</script>

<template>
  <svg viewBox="0 0 1024 1024" fill="currentColor" aria-hidden="true">
    <path
      fill-rule="evenodd"
      d="M511.5 65C719.987 65 889 234.013 889 442.5S719.987 820 511.5 820S134 650.987 134 442.5S303.013 65 511.5 65m0 64C338.359 129 198 269.359 198 442.5S338.359 756 511.5 756S825 615.641 825 442.5S684.641 129 511.5 129M745 889v72H278v-72z"
    />
  </svg>
</template>

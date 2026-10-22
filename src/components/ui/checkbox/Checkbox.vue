<script setup lang="ts">
import { reactiveOmit } from "@vueuse/core"
import type { CheckboxRootEmits, CheckboxRootProps } from "reka-ui"
import type { HTMLAttributes } from "vue"
import { CheckboxIndicator, CheckboxRoot, useForwardPropsEmits } from "reka-ui"
import { cn } from "@/lib/utils"

interface Props extends CheckboxRootProps {
  class?: HTMLAttributes["class"]
}

const props = defineProps<Props>()
const emits = defineEmits<CheckboxRootEmits>()
const forwarded = useForwardPropsEmits(reactiveOmit(props, "class"), emits)
</script>

<template>
  <CheckboxRoot v-bind="forwarded" :class="cn('cb', props.class)">
    <CheckboxIndicator class="cb-mark" />
  </CheckboxRoot>
</template>

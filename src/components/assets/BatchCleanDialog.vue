<script setup lang="ts">
import { computed, ref } from "vue";
import { reclaimableOf } from "@/lib/types";
import { aiKindLabel, cleanKindOf, isSessionKind, type BatchCleanTarget } from "@/lib/assets";
import { rowFullName } from "@/lib/rows";
import { t } from "@/lib/i18n";
import { useModalFocus } from "@/composables/useModalFocus";
import { Button } from "@/components/ui/button";
import ByteValue from "@/components/ByteValue.vue";

/**
 * 批量清理确认弹窗（2026-10-25）。与单条 CleanDialog 分两个组件：单条的口径是
 * 「这一条会发生什么」，批量要列全待删项并给出合计——把两者塞进一个组件会让
 * `target` 变成联合类型，文案与清单都要分岔两次。
 *
 * 快照在打开那一刻由父组件锁进 `target`（与 cleanTarget 同一先例）：弹窗到确认
 * 之间不读 current，筛选/重扫改变背后的行不影响这次确认的对象。
 */
const props = defineProps<{
  target: BatchCleanTarget | null;
}>();

const emit = defineEmits<{
  confirm: [];
  cancel: [];
}>();

const rootEl = ref<HTMLElement | null>(null);
defineExpose({ rootEl });

useModalFocus(
  computed(() => props.target),
  rootEl,
);

const kind = computed(() => cleanKindOf(props.target?.groupId ?? null));
const items = computed(() => props.target?.items ?? []);
const total = computed(() => items.value.reduce((sum, it) => sum + reclaimableOf(it), 0));
/** 会话删了不会再生，必须在批量弹窗里单独说——不能套缓存的文案。 */
const hasSession = computed(
  () => kind.value === "ai" && items.value.some(isSessionKind),
);
</script>

<template>
  <div v-if="target" class="fixed inset-0 z-50 flex items-center justify-center">
    <div class="absolute inset-0 bg-black/40" @click="emit('cancel')" />
    <div
      ref="rootEl"
      role="dialog"
      aria-modal="true"
      class="relative w-[480px] rounded-lg border border-border bg-card p-4 shadow-lg"
    >
      <p class="text-[13px] font-medium">{{ t("clean.title") }}</p>
      <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
        <template v-if="kind === 'repo'">
          {{ t("clean.batchRepoBody", { count: items.length }) }}
        </template>
        <template v-else-if="kind === 'ai'">
          {{ t("clean.batchAiBody", { count: items.length }) }}
          <template v-if="hasSession">
            <span class="text-foreground">{{ t("clean.sessionStrong") }}</span>
            {{ t("clean.sessionAfter") }}
          </template>
          <template v-else>{{ t("clean.otherAfter") }}</template>
        </template>
        <template v-else>{{ t("clean.batchCacheBody", { count: items.length }) }}</template>
      </p>

      <div class="mt-3 flex items-baseline justify-between text-[11px] text-muted-foreground">
        <span>{{ t("clean.totalItems", { count: items.length }) }}</span>
        <span>{{ t("clean.totalSize") }} <ByteValue :bytes="total" /></span>
      </div>
      <ul class="mt-1 max-h-[220px] overflow-y-auto rounded-md border border-border/60 bg-muted/40">
        <li
          v-for="it in items"
          :key="it.id"
          class="border-t border-border/40 px-2 py-1.5 first:border-t-0"
        >
          <div class="flex items-baseline justify-between gap-2 text-xs">
            <span class="min-w-0 truncate font-medium">{{ rowFullName(it) }}</span>
            <ByteValue class="shrink-0 text-muted-foreground" :bytes="reclaimableOf(it)" />
          </div>
          <!-- 路径完整显示（换行不截断）：确认弹窗的意义就是让人核对删的是哪一处 -->
          <p class="mt-0.5 select-text font-mono text-[10px] leading-relaxed text-muted-foreground [overflow-wrap:anywhere]">
            {{ it.path }}
          </p>
          <p v-if="kind === 'ai'" class="text-[10px] text-muted-foreground">
            {{ t("clean.categoryLine", { kind: aiKindLabel(it) }) }}
          </p>
        </li>
      </ul>

      <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
        <template v-if="kind === 'repo'">{{ t("clean.repoReviewBatch") }}</template>
        <template v-else-if="kind === 'ai'">
          {{ t("clean.aiReviewBatch") }}
          <template v-if="hasSession">{{ t("clean.sessionTrash") }}</template>
        </template>
        <template v-else>{{ t("clean.cacheReviewBatch") }}</template>
        <br />
        <span class="opacity-70">{{ t("clean.estimate") }}</span>
      </p>

      <div class="mt-4 flex justify-end gap-2">
        <Button data-dialog-cancel size="xs" variant="outline" @click="emit('cancel')">{{ t("common.cancel") }}</Button>
        <Button size="xs" variant="destructive" @click="emit('confirm')">
          {{ t("clean.confirmCount", { count: items.length }) }}
        </Button>
      </div>
    </div>
  </div>
</template>

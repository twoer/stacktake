<script setup lang="ts">
import { computed, ref } from "vue";
import type { Item } from "@/lib/types";
import { detailValueOf, reclaimableOf } from "@/lib/types";
import { aiKindLabel, cleanKindOf, isSessionKind } from "@/lib/assets";
import { rowFullName } from "@/lib/rows";
import { renderText, t } from "@/lib/i18n";
import { useModalFocus } from "@/composables/useModalFocus";
import { Button } from "@/components/ui/button";
import ByteValue from "@/components/ByteValue.vue";
import Hint from "@/components/Hint.vue";

/**
 * 整仓 / 整条目清理的确认弹窗（S2 / D1 拍板选 A；2026-10-24 拆分计划阶段 1
 * 从 Assets.vue 搬出，模板与文案逐字保留）。
 *
 * 破坏性操作必须显式确认，且把三件事写在明处：删什么、删多少（体积）、去哪
 * （废纸篓，可恢复）。Esc / 点遮罩 / 取消都收（emit cancel）；确认只 emit——
 * `invoke("clean_items")` 与 `applyCleanResult` 留在 Assets.vue，两个弹窗共用。
 * 弹窗开合的真相（cleanTarget ref）也在父组件：全局键盘的 Escape 优先级
 * （弹窗 > 菜单 > 选中态）要读它。
 */
const props = defineProps<{
  target: Item | null;
  /** 当前分组 id——「会怎样」的文案按分组性质分岔（仓库 / AI 工具 / 缓存）。 */
  groupId: string | null;
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

/**
 * 弹窗里那条「会怎样」的描述。三种性质不同，不能共用一句话：
 * 删仓库丢源码、删缓存只花重下的时间、删 AI 工具数据里的**会话丢历史**。
 * 分岔表在 lib/assets（cleanKindOf）——这里只消费性质，不认得具体分组 id。
 */
const cleanKind = computed(() => cleanKindOf(props.groupId));

/** 工具链缓存删掉的后果：按 detail 的说明行（`data.detail.note`）显示，不空喊「可恢复」。 */
function cleanNote(item: Item): string {
  const v = detailValueOf(item, "data.detail.note");
  return v ? renderText(v) : "";
}
</script>

<template>
  <!-- 清理确认弹窗（S2 / D1 拍板选 A）。破坏性操作必须显式确认，且把三件事
       写在明处：删什么（整个仓库，含源码与 .git）、删多少（体积）、去哪
       （废纸篓，可恢复）。Esc / 点遮罩 / 取消都收；确认才发起 invoke。 -->
  <div v-if="target" class="fixed inset-0 z-50 flex items-center justify-center">
    <div class="absolute inset-0 bg-black/40" @click="emit('cancel')" />
    <div
      ref="rootEl"
      role="dialog"
      aria-modal="true"
      class="relative w-[420px] rounded-lg border border-border bg-card p-4 shadow-lg"
    >
      <p class="text-[13px] font-medium">{{ t("clean.title") }}</p>
      <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
        <!-- 三组性质不同，不能共用一句话（S3 / B1）：删仓库丢源码、删缓存花重下的时间、
             删 AI 工具数据里的会话丢历史。强调句单独一个键，不把 <span> 塞进译文。 -->
        <template v-if="cleanKind === 'repo'">{{ t("clean.repoBody") }}</template>
        <template v-else-if="cleanKind === 'ai'">
          {{ t("clean.aiBody", { name: rowFullName(target), kind: aiKindLabel(target) }) }}
          <template v-if="isSessionKind(target)">
            <span class="text-foreground">{{ t("clean.sessionStrong") }}</span>
            {{ t("clean.sessionAfter") }}
          </template>
          <template v-else>{{ t("clean.otherAfter") }}</template>
        </template>
        <template v-else>{{ t("clean.cacheBody") }}</template>
      </p>
      <dl class="mt-3 space-y-1 rounded-md bg-muted/50 p-2 text-xs">
        <div class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">
            {{
              cleanKind === "repo"
                ? t("clean.fieldRepo")
                : cleanKind === "ai"
                  ? t("clean.fieldTool")
                  : t("clean.fieldCache")
            }}
          </dt>
          <dd class="min-w-0 truncate font-medium">{{ rowFullName(target) }}</dd>
        </div>
        <div v-if="cleanKind === 'ai'" class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">{{ t("common.category") }}</dt>
          <dd class="min-w-0 truncate font-medium">{{ aiKindLabel(target) }}</dd>
        </div>
        <div class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">{{ t("common.path") }}</dt>
          <Hint :text="target.path">
            <dd class="min-w-0 truncate font-mono">{{ target.path }}</dd>
          </Hint>
        </div>
        <div class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">{{ t("common.size") }}</dt>
          <dd><ByteValue :bytes="reclaimableOf(target)" /></dd>
        </div>
      </dl>
      <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
        <template v-if="cleanKind === 'repo'">{{ t("clean.repoReview") }}</template>
        <template v-else-if="cleanKind === 'ai'">{{ t("clean.aiReview") }}</template>
        <template v-else>
          <template v-if="cleanNote(target)">{{ cleanNote(target) }}</template>
          <template v-else>{{ t("clean.cacheReview") }}</template>
          {{ t("clean.recheckWhitelist") }}
        </template>
        <br />
        <span class="opacity-70">{{ t("clean.estimate") }}</span>
      </p>
      <div class="mt-4 flex justify-end gap-2">
        <Button data-dialog-cancel size="xs" variant="outline" @click="emit('cancel')">{{ t("common.cancel") }}</Button>
        <Button size="xs" variant="destructive" @click="emit('confirm')">{{ t("clean.confirm") }}</Button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { reclaimableOf } from "@/lib/types";
import { aiKindLabel, cleanKindOf, type BatchCleanTarget } from "@/lib/assets";
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
  () => kind.value === "ai" && items.value.some((it) => aiKindLabel(it) === "会话"),
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
      <p class="text-[13px] font-medium">移入废纸篓</p>
      <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
        <template v-if="kind === 'repo'">
          将把以下 {{ items.length }} 个仓库——<span class="text-foreground">连同源码与 .git 历史</span
          >——移入废纸篓，之后可以从废纸篓恢复。
        </template>
        <template v-else-if="kind === 'ai'">
          将把以下 {{ items.length }} 条「工具 × 类别」数据移入废纸篓，之后可以从废纸篓恢复。
          <template v-if="hasSession">
            <span class="text-foreground">会话含完整对话记录与代码片段，删除后不会自动重建。</span>
            日志与缓存会在下次运行时重新生成。
          </template>
          <template v-else>日志与缓存会在下次运行时重新生成，功能不受影响。</template>
        </template>
        <template v-else>
          将把以下 {{ items.length }} 个缓存目录移入废纸篓，之后可以从废纸篓恢复。
          <span class="text-foreground">下次用到时要重新下载或重建</span>，功能不受影响。
        </template>
      </p>

      <div class="mt-3 flex items-baseline justify-between text-[11px] text-muted-foreground">
        <span>共 {{ items.length }} 项</span>
        <span>合计 <ByteValue :bytes="total" /></span>
      </div>
      <ul class="mt-1 max-h-[220px] overflow-y-auto rounded-md border border-border/60 bg-muted/40">
        <li
          v-for="it in items"
          :key="it.id"
          class="border-t border-border/40 px-2 py-1.5 first:border-t-0"
        >
          <div class="flex items-baseline justify-between gap-2 text-xs">
            <span class="min-w-0 truncate font-medium">{{ it.name }}</span>
            <ByteValue class="shrink-0 text-muted-foreground" :bytes="reclaimableOf(it)" />
          </div>
          <!-- 路径完整显示（换行不截断）：确认弹窗的意义就是让人核对删的是哪一处 -->
          <p class="mt-0.5 select-text font-mono text-[10px] leading-relaxed text-muted-foreground [overflow-wrap:anywhere]">
            {{ it.path }}
          </p>
          <p v-if="kind === 'ai'" class="text-[10px] text-muted-foreground">
            类别：{{ aiKindLabel(it) }}
          </p>
        </li>
      </ul>

      <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
        <template v-if="kind === 'repo'">
          有未推送提交或未提交改动的仓库会被拒绝；服务端逐项复核，其余照常移入废纸篓。
        </template>
        <template v-else-if="kind === 'ai'">
          服务端会核对工具仍在扫描白名单内，并按点击那一刻的文件名重新分类；未通过的项目单独列出，其余照常移入废纸篓。
          <template v-if="hasSession">会话删除后只能从废纸篓找回。</template>
        </template>
        <template v-else>
          正在进行的构建或下载建议等完成后再清理。服务端会复核每条仍在可清理白名单内；未通过的项目单独列出，其余照常移入废纸篓。
        </template>
        <br />
        <span class="opacity-70">可回收为估算值：含共享硬链接的目录（如 Rust 的 target/）可能偏高。</span>
      </p>

      <div class="mt-4 flex justify-end gap-2">
        <Button data-dialog-cancel size="xs" variant="outline" @click="emit('cancel')">取消</Button>
        <Button size="xs" variant="destructive" @click="emit('confirm')">
          确认移入废纸篓（{{ items.length }} 项）
        </Button>
      </div>
    </div>
  </div>
</template>

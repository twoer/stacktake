<script setup lang="ts">
import { computed, ref } from "vue";
import type { Item } from "@/lib/types";
import { reclaimableOf } from "@/lib/types";
import { aiKindLabel, cleanKindOf } from "@/lib/assets";
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

/** 工具链缓存删掉的后果：按 detail 的说明逐条显示，不空喊「可恢复」。 */
function cleanNote(item: Item): string {
  return item.detail?.["3. 说明"] ?? "";
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
      <p class="text-[13px] font-medium">移入废纸篓</p>
      <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
        <!-- 三组性质不同，不能共用一句话（S3 / B1）：删仓库丢源码、删缓存花重下的时间、
             删 AI 工具数据里的会话丢历史 -->
        <template v-if="cleanKind === 'repo'">
          将把整个仓库目录——<span class="text-foreground">连同源码与 .git 历史</span
          >——移入废纸篓，之后可以从废纸篓恢复。
        </template>
        <template v-else-if="cleanKind === 'ai'">
          将把「{{ target.name }}」这一行下的
          <span class="text-foreground">{{ aiKindLabel(target) }}</span>
          条目移入废纸篓，之后可以从废纸篓恢复。
          <template v-if="aiKindLabel(target) === '会话'">
            <span class="text-foreground">会话含完整对话记录与代码片段，删除后不会自动重建。</span>
            日志与缓存会在下次运行时重新生成。
          </template>
          <template v-else>日志与缓存会在下次运行时重新生成，功能不受影响。</template>
        </template>
        <template v-else>
          将把整个缓存目录移入废纸篓，之后可以从废纸篓恢复。
          <span class="text-foreground">下次用到时要重新下载或重建</span>，功能不受影响。
        </template>
      </p>
      <dl class="mt-3 space-y-1 rounded-md bg-muted/50 p-2 text-xs">
        <div class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">
            {{ cleanKind === "repo" ? "仓库" : cleanKind === "ai" ? "工具" : "缓存" }}
          </dt>
          <dd class="min-w-0 truncate font-medium">{{ target.name }}</dd>
        </div>
        <div v-if="cleanKind === 'ai'" class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">类别</dt>
          <dd class="min-w-0 truncate font-medium">{{ aiKindLabel(target) }}</dd>
        </div>
        <div class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">路径</dt>
          <Hint :text="target.path">
            <dd class="min-w-0 truncate font-mono">{{ target.path }}</dd>
          </Hint>
        </div>
        <div class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">体积</dt>
          <dd><ByteValue :bytes="reclaimableOf(target)" /></dd>
        </div>
      </dl>
      <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
        <template v-if="cleanKind === 'repo'">
          有未推送提交或未提交改动的仓库会被拒绝——点击与扫描之间状态可能变了，服务端会逐项复核。
        </template>
        <template v-else-if="cleanKind === 'ai'">
          服务端会核对工具仍在扫描白名单内，并按点击那一刻的文件名重新分类；只有仍属于该类别的一级条目会被清理。会话删除后只能从废纸篓找回。
        </template>
        <template v-else>
          <template v-if="cleanNote(target)">{{ cleanNote(target) }}</template>
          <template v-else>
            正在进行的构建或下载建议等完成后再清理。
          </template>
          服务端会复核这条仍在可清理白名单内。
        </template>
        <br />
        <span class="opacity-70">可回收为估算值：含共享硬链接的目录（如 Rust 的 target/）可能偏高。</span>
      </p>
      <div class="mt-4 flex justify-end gap-2">
        <Button data-dialog-cancel size="xs" variant="outline" @click="emit('cancel')">取消</Button>
        <Button size="xs" variant="destructive" @click="emit('confirm')">确认移入废纸篓</Button>
      </div>
    </div>
  </div>
</template>

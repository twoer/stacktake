<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { Item } from "@/lib/types";
import { artifactRowsOf } from "@/lib/assets";
import { rowFullName } from "@/lib/rows";
import { t } from "@/lib/i18n";
import { useModalFocus } from "@/composables/useModalFocus";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ScrollArea } from "@/components/ui/scroll-area";
import ByteValue from "@/components/ByteValue.vue";
import Hint from "@/components/Hint.vue";

/**
 * 清理构建产物的确认弹窗（P2；2026-10-24 拆分计划阶段 1 从 Assets.vue 搬出，
 * 模板与文案逐字保留）。与整仓删除的弹窗刻意分开：范围、复核方式、后果都不同。
 * 要点写在明处：删什么（白名单产物目录）、保什么（源码与 .git）、复核是什么
 * （服务端点击那一刻按白名单现找，不信任前端路径）。
 *
 * 勾选集合（唯一真相）是本组件的内部状态：打开时全选（沿用拆出前的默认），
 * 确认时经 emit 把勾选的相对路径展开给父组件——`invoke` 与 `applyCleanResult`
 * 留在 Assets.vue，与整仓弹窗共用。弹窗开合的真相（artifactTarget）也在父组件。
 */
const props = defineProps<{
  target: Item | null;
}>();

const emit = defineEmits<{
  /** 确认清理：勾选的「相对仓库根」产物路径 + 目标条目。 */
  confirm: [targets: string[], item: Item];
  cancel: [];
}>();

const rootEl = ref<HTMLElement | null>(null);
defineExpose({ rootEl });

useModalFocus(
  computed(() => props.target),
  rootEl,
);

const artifactPicked = ref<Set<string>>(new Set());

// 打开即全选（拆出前 askCleanArtifacts 的默认）；关闭时清空，下次打开重建。
watch(
  () => props.target,
  (target) => {
    artifactPicked.value = target
      ? new Set(artifactRowsOf(target).map((r) => r.relative_path))
      : new Set();
  },
);

// 勾选集合必须完整，不能复用检查器截断后的展示列表。
const artifactDialogRows = computed(() => artifactRowsOf(props.target));

const artifactPickedBytes = computed(() =>
  artifactDialogRows.value.reduce(
    (s, r) => s + (artifactPicked.value.has(r.relative_path) ? r.size_bytes : 0),
    0,
  ),
);

const artifactAllPicked = computed(
  () =>
    artifactDialogRows.value.length > 0 &&
    artifactDialogRows.value.every((r) => artifactPicked.value.has(r.relative_path)),
);

const artifactSomePicked = computed(() => {
  const rows = artifactDialogRows.value;
  const n = rows.filter((r) => artifactPicked.value.has(r.relative_path)).length;
  return n > 0 && n < rows.length;
});

function toggleAllArtifacts(on: boolean) {
  artifactPicked.value = new Set(
    on ? artifactDialogRows.value.map((r) => r.relative_path) : [],
  );
}

/** 单行勾选：reka-ui 的 v-model 只出 boolean | "indeterminate"，不认 Set——
 *  显式收敛成 Set 增删，Set 仍是唯一真相（确认时从它展开 targets）。 */
function toggleArtifact(rel: string, on: boolean) {
  const next = new Set(artifactPicked.value);
  if (on) next.add(rel);
  else next.delete(rel);
  artifactPicked.value = next;
}

function onConfirm() {
  const target = props.target;
  if (!target) return;
  emit("confirm", [...artifactPicked.value], target);
}
</script>

<template>
  <!-- 清理构建产物弹窗（P2）。与整仓删除的弹窗刻意分开：范围、复核方式、
       后果都不同。要点写在明处：删什么（白名单产物目录）、保什么（源码
       与 .git）、复核是什么（服务端点击那一刻按白名单现找，不信任前端路径）。 -->
  <div v-if="target" class="fixed inset-0 z-50 flex items-center justify-center">
    <div class="absolute inset-0 bg-black/40" @click="emit('cancel')" />
    <div
      ref="rootEl"
      role="dialog"
      aria-modal="true"
      class="relative w-[420px] rounded-lg border border-border bg-card p-4 shadow-lg"
    >
      <p class="text-[13px] font-medium">{{ t("clean.artifact.title") }}</p>
      <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
        {{ t("clean.artifact.body", { count: artifactPicked.size }) }}
      </p>
      <dl class="mt-3 space-y-1 rounded-md bg-muted/50 p-2 text-xs">
        <div class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">{{ t("clean.fieldRepo") }}</dt>
          <dd class="min-w-0 truncate font-medium">{{ rowFullName(target) }}</dd>
        </div>
        <div class="flex items-baseline justify-between gap-2">
          <dt class="shrink-0 text-muted-foreground">{{ t("common.path") }}</dt>
          <Hint :text="target.path">
            <dd class="min-w-0 truncate font-mono">{{ target.path }}</dd>
          </Hint>
        </div>
      </dl>

      <!-- 弹窗保留全部目录，滚动限高而不截断可选集合。 -->
      <div class="mt-3">
        <!-- 表头勾选框兼作列头，钉在行勾选列正上方：pl-2.5 = 列表盒 p-1.5 +
             行 px-1，差一像素就读不成一列。半选态表意，可见文案让位给节标题；
             「全选」语义走 title / aria-label，别把字样加回来——列头勾选框
             自身就是表格的通用范式。 -->
        <Hint :text="t('clean.artifact.selectAll')">
          <label
            class="flex items-center gap-1.5 pl-2.5 text-[11px] text-muted-foreground"
          >
            <Checkbox
              :aria-label="t('clean.artifact.selectAll')"
              :model-value="artifactAllPicked ? true : artifactSomePicked ? 'indeterminate' : false"
              @update:model-value="(v) => toggleAllArtifacts(v === true)"
            />
            <span class="font-semibold">{{ t("clean.artifact.dirs") }}</span>
          </label>
        </Hint>
        <!-- text-[11px] 与检查器子表同理：不给的话右侧体积 span 以继承的 16px 当 strut，
             行盒撑到约 19.3px；items-center 下虽对称，但行高偏离 18px 设计密度。 -->
        <ScrollArea class="mt-1 rounded-md bg-muted/50" viewport-class="max-h-[188px]">
          <ul class="p-1.5 text-[11px]">
            <li v-for="r in artifactDialogRows" :key="r.relative_path" class="leading-[18px]">
              <!-- items-center 不用 baseline：reka-ui 的框是 button，勾存在与否会改变
                   它的合成基线（mark 在 = 末段底边，不在 = 自身底边），勾选态会跳 3.25px；
                   center 与 mark 脱钩，且不依赖浏览器对 button 字体的基线合成，跨平台一致。
                   12px 盒子在 18px 行高里 flex 居中即对齐，别再补 translate——那是旧版
                   原生 input（items-baseline + y-1px）的认可位，搬过来会把框抬高 1px。 -->
              <Hint :text="r.relative_path" side="left">
                <label class="flex items-center gap-2 rounded-sm px-1 hover:bg-[var(--ink-hover)]">
                  <Checkbox
                    :aria-label="r.relative_path"
                    :model-value="artifactPicked.has(r.relative_path)"
                    @update:model-value="(v) => toggleArtifact(r.relative_path, v === true)"
                  />
                  <span class="flex min-w-0 flex-1 items-baseline font-mono text-[11px]">
                    <span v-if="r.dir" class="mr-0.5 min-w-0 truncate text-muted-foreground">{{ r.dir }}</span>
                    <!-- 没勾选的行整行退灰（末段也退）：本次不清的目录该往后退，
                         但只退到 muted——父路径本来就是这档灰，不再往上加透明度，
                         免得 11px 小字的对比度掉到 4.5:1 以下 -->
                    <span
                      class="shrink-0"
                      :class="artifactPicked.has(r.relative_path) ? 'text-foreground' : 'text-muted-foreground'"
                      >{{ r.base }}</span
                    >
                  </span>
                  <ByteValue
                    class="shrink-0"
                    :class="artifactPicked.has(r.relative_path) ? '' : 'text-muted-foreground'"
                    :bytes="r.size_bytes"
                  />
                </label>
              </Hint>
            </li>
          </ul>
        </ScrollArea>
      </div>

      <p class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
        {{ t("clean.artifact.review") }}
        <br />
        <span class="opacity-70">{{ t("clean.artifact.estimate") }}</span>
      </p>
      <div class="mt-4 flex items-center justify-between gap-2">
        <span class="text-[11px] tabular-nums text-muted-foreground">
          {{ t("clean.artifact.selectedSummary", { count: artifactPicked.size, bytes: { t: "b", v: artifactPickedBytes } }) }}
        </span>
        <div class="flex gap-2">
          <Button data-dialog-cancel size="xs" variant="outline" @click="emit('cancel')">{{ t("common.cancel") }}</Button>
          <Button
            size="xs"
            variant="destructive"
            :disabled="artifactPicked.size === 0"
            @click="onConfirm"
          >
            {{ t("clean.confirm") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>

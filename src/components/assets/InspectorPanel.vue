<script setup lang="ts">
import { computed } from "vue";
import { ChevronRight, FolderOpen, Trash2 } from "@lucide/vue";
import type { AssetGroup, Item } from "@/lib/types";
import { artifactBytes, groupReclaimable, itemSize, reclaimableOf } from "@/lib/types";
import { artifactRowsOf, canCleanArtifactsItem, canCleanItem, cell, tagHint } from "@/lib/assets";
import { isLocalPath, revealLabel } from "@/lib/path";
import { Button } from "@/components/ui/button";
import ByteValue from "@/components/ByteValue.vue";
import Hint from "@/components/Hint.vue";

/**
 * 检查器（2026-10-24 拆分计划阶段 3 从 Assets.vue 搬出，模板与文案逐字保留）：
 * 选中行 → 明细（路径 / 指标 / detail / 产物子表）；未选中 → 分组概览
 * （摘要 / notes / 清理规则折叠）。
 *
 * 职责边界：动作全部 emit 出去——`revealAt` 的 invoke、清理弹窗的开合
 * （cleanTarget / artifactTarget）都在父组件；`rulesOpen` 是 defineModel，
 * 真值也在父组件——检查器外层 `v-if="current && !isOverview"` 在总览视图会
 * 卸载本组件，「切分组不重置」的折叠语义靠父组件持值才保得住。
 */
const props = defineProps<{
  group: AssetGroup;
  selectedRow: Item | null;
  /** 可回收列是否独立（表格共用口径，父算好传入）。 */
  splitReclaim: boolean;
}>();

const rulesOpen = defineModel<boolean>("rulesOpen", { default: false });

const emit = defineEmits<{
  reveal: [item: Item];
  revealAt: [path: string];
  askClean: [item: Item];
  askCleanArtifacts: [item: Item];
}>();

/** 检查器内容切换时重放一次 page-in；把分组 id 也编进 key，
 *  否则「未选中行」在各分组间切换时 key 不变，页面没有进入动效。 */
const inspectorKey = computed(
  () => props.selectedRow?.id ?? `group:${props.group.id}`,
);

const detailEntries = computed(() => Object.entries(props.selectedRow?.detail ?? {}));

const MAX_ARTIFACT_PREVIEW = 12;
const artifactRows = computed(() =>
  artifactRowsOf(props.selectedRow).slice(0, MAX_ARTIFACT_PREVIEW),
);
const artifactOmitted = computed(() =>
  Math.max(0, (props.selectedRow?.artifact_entries?.length ?? 0) - MAX_ARTIFACT_PREVIEW),
);

function artifactPath(rel: string): string {
  const root = props.selectedRow?.path ?? "";
  return /[\\/]$/.test(root) ? root + rel : `${root}/${rel}`;
}

const reclaimTotal = computed(() => groupReclaimable(props.group));

/** detail 的键名带 `N. ` 排序前缀（Rust 侧为保证 HashMap 顺序加的），展示时去掉。 */
function detailLabel(key: string): string {
  return key.replace(/^\d+\.\s*/, "");
}
</script>

<template>
  <!-- 总览不进检查器（台账式首块把合计 / 可回收 / 占比带搬进了主区），
       这里的数字只会与主区重复；工具栏的检查器开关在总览一并隐藏。 -->
  <div :key="inspectorKey" class="page-in min-h-0">
    <template v-if="selectedRow">
      <h2 class="min-w-0 text-[14px] font-semibold leading-snug [overflow-wrap:anywhere]">{{ selectedRow.name }}</h2>
      <p v-if="isLocalPath(selectedRow.path)" class="mt-1.5 select-text font-mono text-[11px] leading-relaxed text-muted-foreground [overflow-wrap:anywhere]">
        {{ selectedRow.path }}
      </p>

      <!-- 路径动作条：只对宿主机绝对路径出现。flex-wrap：清理按钮进来后
           多个 xs 按钮在 228px 内容区里会超宽（S2）。复制路径不在这条上——
           2026-10-19 按用户要求移除，入口收进行右键菜单一处。 -->
      <div v-if="isLocalPath(selectedRow.path)" class="mt-2 flex flex-wrap items-center gap-1.5">
        <Button size="xs" variant="outline" @click="emit('reveal', selectedRow)">
          <FolderOpen />
          {{ revealLabel }}
        </Button>
        <Button
          v-if="canCleanItem(group, selectedRow)"
          size="xs"
          variant="outline"
          class="text-destructive"
          @click="emit('askClean', selectedRow)"
        >
          <Trash2 />
          清理
        </Button>
        <Button
          v-if="canCleanArtifactsItem(group, selectedRow)"
          size="xs"
          variant="outline"
          class="text-destructive"
          @click="emit('askCleanArtifacts', selectedRow)"
        >
          <Trash2 />
          清理产物
        </Button>
      </div>

      <!-- 标签与表格行同一形态：灰字不是药丸（U15），检查器里也不留一处例外。
           这里用原始 tags（不做多数派抑制）——检查器是细看一行，抑制规则是
           为「一屏两百行」定的，跟它无关。 -->
      <p v-if="selectedRow.tags.length" class="mt-2 flex flex-wrap items-baseline gap-x-1.5 text-[11px] text-muted-foreground">
        <template v-for="(t, ti) in selectedRow.tags" :key="t">
          <span v-if="ti" aria-hidden="true">·</span>
          <Hint :text="tagHint(t)">
            <span>{{ t }}</span>
          </Hint>
        </template>
      </p>

      <h3 class="mt-4 border-t border-border/60 pt-3 text-[11px] font-medium text-muted-foreground">指标</h3>
      <dl class="mt-2 space-y-2">
        <template v-for="f in group.fields.slice(0, 3)" :key="f.key">
          <div class="flex items-baseline justify-between gap-2">
            <dt class="shrink-0 text-[11px] text-muted-foreground">{{ f.label }}</dt>
            <!-- 与表格同一口径：可回收量与体积恒等的行（splitReclaim 为 false），
                 体积在指标区也染绿，不再单列一行「可回收」——那是同一个数印第二遍。
                 splitReclaim 的组保留独立的可回收行。 -->
            <dd
              class="min-w-0 text-right text-xs tabular-nums [overflow-wrap:anywhere]"
              :class="[
                f.key === 'size' && !splitReclaim && reclaimableOf(selectedRow) > 0 ? 'text-reclaim' : '',
                f.key === 'branch' ? 'font-mono text-[11px]' : '',
              ]"
            >
              <ByteValue v-if="f.key === 'size' && itemSize(selectedRow) > 0" :bytes="itemSize(selectedRow)" />
              <template v-else>{{ cell(selectedRow, f.key, f.temporal) }}</template>
            </dd>
          </div>
        </template>
        <div v-if="splitReclaim && reclaimableOf(selectedRow) > 0" class="flex items-baseline justify-between gap-2">
          <dt class="text-xs text-muted-foreground">可回收</dt>
          <dd class="text-xs text-reclaim">
            <ByteValue :bytes="reclaimableOf(selectedRow)" />
          </dd>
        </div>
        <div v-if="artifactBytes(selectedRow) > 0" class="flex items-baseline justify-between gap-2">
          <dt class="text-xs text-muted-foreground">构建产物</dt>
          <dd class="text-xs"><ByteValue :bytes="artifactBytes(selectedRow)" /></dd>
        </div>
      </dl>

      <template v-if="detailEntries.length || artifactRows.length">
        <template v-if="detailEntries.length">
          <h3 class="mt-4 border-t border-border/60 pt-3 text-[11px] font-medium text-muted-foreground">明细</h3>
          <dl class="mt-2 space-y-3">
            <div v-for="[k, v] in detailEntries" :key="k">
              <dt class="text-[11px] text-muted-foreground [overflow-wrap:anywhere]">{{ detailLabel(k) }}</dt>
              <dd
                class="mt-0.5 whitespace-pre-line text-xs leading-relaxed text-foreground [overflow-wrap:anywhere]"
                :class="isLocalPath(v) ? 'font-mono text-[11px]' : ''"
              >{{ v }}</dd>
            </div>
          </dl>
        </template>

        <!-- 产物明细：占满整条检查器宽度的子表，不再走「64px label + 一段等宽文本」。
             那条形态在 228px 里只剩 156px 给值，`apps/desktop/.output/server/node_modules
             · 64.1 MB` 必然折行、体积与条目分家，break-words 还会把 node_modules 劈成
             "node_m / odules"。改成一条一行：左路径（父段灰、末段黑、父段优先截断），
             右体积右对齐——纵向一眼能比大小。 -->
        <template v-if="artifactRows.length">
          <!-- 可点提示只在标题行给一个图标，行内不再各挂一个：228px 里每行多 16px，
               父路径就从 90px 掉到 68px（12 个字符），`apps/desktop/.output/server/
               node_modules` 与 `apps/desktop/node_modules` 会在行内截成同一个串。
               标题这一个图标 + 整行 hover 淡底，代价为零。 -->
          <div class="mt-4 flex items-center gap-1.5 border-t border-border/60 pt-3">
            <h3 class="text-[11px] font-medium text-muted-foreground">产物明细</h3>
            <Hint :text="`${revealLabel}：点任意一行`">
              <FolderOpen class="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
            </Hint>
          </div>
          <!-- text-[11px] 必须落在这里：dl 的 11px 是逐元素给的，管不到这个 ul。
               右侧体积 span 是块级 flex 项，缺了它就继承容器 16px 字号当 strut，
               行盒比 11px 文本高出约 2.5px；items-baseline 把多出的高度全顶在上方，
               文字在悬停高亮条里「坐底」。 -->
          <ul class="mt-1 text-[11px]">
            <li v-for="r in artifactRows" :key="r.relative_path" class="leading-[18px]">
              <!-- 整行是按钮：点一下在访达 / 资源管理器里定位这个产物目录 -->
              <Hint :text="`${revealLabel} · ${r.relative_path}`" side="left">
                <button
                  type="button"
                  class="flex w-full items-baseline gap-2 rounded-sm text-left px-1 transition-colors duration-[90ms] hover:bg-[var(--ink-hover)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  @click="emit('revealAt', artifactPath(r.relative_path))"
                >
                  <span class="flex min-w-0 flex-1 items-baseline font-mono text-[11px]">
                    <!-- mr-0.5：截断后的省略号会与末段贴成一个词（".output/serv…node_modules"）。
                         margin 而不是 padding——父路径为空时整段不渲染，末段才不会平白左移 -->
                    <span v-if="r.dir" class="mr-0.5 min-w-0 truncate text-muted-foreground">{{ r.dir }}</span>
                    <span class="shrink-0 text-foreground">{{ r.base }}</span>
                  </span>
                  <ByteValue class="shrink-0 text-xs" :bytes="r.size_bytes" />
                </button>
              </Hint>
            </li>
            <li v-if="artifactOmitted" class="text-[11px] leading-[18px] text-muted-foreground">
              …其余 {{ artifactOmitted }} 个目录略
            </li>
          </ul>
        </template>
      </template>
    </template>

    <template v-else>
      <h2 class="text-[14px] font-semibold leading-snug">{{ group.label }}</h2>
      <!-- 整组摘要保留一份；状态栏在筛选后展示的是筛后口径。 -->
      <p v-if="group.summary" class="mt-2 text-pretty text-xs leading-relaxed text-foreground [overflow-wrap:anywhere]">
        {{ group.summary }}
      </p>
      <div v-if="group.notes?.length" class="mt-4 space-y-3 border-t border-border/60 pt-3">
        <p
          v-for="(n, i) in group.notes"
          :key="i"
          class="text-[11px] leading-relaxed text-muted-foreground [overflow-wrap:anywhere]"
        >
          {{ n }}
        </p>
      </div>
      <!-- 清理规则默认收起。这三条讲的是「本工具怎么判可清理」，常驻等于把 README 塞进
           界面：占掉检查器三分之一屏，且每次打开都读一遍。**一字未改，只从常驻改成折叠**
           ——第一次真要清理的人会点开，天天用的人不必再看。措辞改动是另一把刀的事。 -->
      <template v-if="group.supports_clean">
        <button
          class="mt-1.5 flex items-center gap-1 rounded text-[11px] text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          :aria-expanded="rulesOpen"
          @click="rulesOpen = !rulesOpen"
        >
          <ChevronRight
            class="size-3 shrink-0 transition-transform"
            :class="rulesOpen ? 'rotate-90' : ''"
          />
          清理规则
        </button>
        <div class="rules" :class="{ open: rulesOpen }" :aria-hidden="!rulesOpen">
          <div>
            <ul class="mt-1 space-y-1.5 text-[11px] leading-relaxed text-muted-foreground">
              <li>
                「可清理」行可在检查器里发起清理（移入废纸篓）；带构建产物的仓库可以只清产物（node_modules
                / target 等，源码保留）。
              </li>
              <li>
                工具链缓存里那些有官方清理命令的行不标「可清理」——命令写在明细里，请自己运行，它比搬走整个目录精确。
              </li>
              <li>
                编辑器与 AI
                工具按「工具 × 类别」清日志 / 会话 / 缓存；会话含完整对话记录，删除后不会自动重建。
              </li>
            </ul>
          </div>
        </div>
      </template>
      <p v-else-if="reclaimTotal > 0" class="mt-2 text-[11px] leading-relaxed text-muted-foreground">
        本组只读：账面可回收不等于有删除入口。
      </p>
    </template>
  </div>
</template>

import { nextTick, watch, type Ref } from "vue";

/**
 * 弹窗焦点管理（2026-10-24 拆分计划阶段 1，从 Assets.vue 的 watchDialogFocus 收敛）。
 *
 * 破坏性确认弹窗是键盘用户的必经路径，三条都要有：
 * ① 打开时焦点进弹窗——落在「取消」上（安全默认，不会误触确认）；
 * ② 关闭时把焦点还给打开前那个元素（检查器里的「清理」按钮）；
 * ③ Tab 在弹窗内循环，不穿到背后的表格上——这条不在这里，弹窗组件把根元素
 *    （rootEl）expose 出去，Assets.vue 的全局键盘处理靠它圈 Tab。
 *
 * 缺了前两条，键盘用户打开弹窗后焦点还在背后的行上，Enter/空格会继续操作表格。
 * 元素被重渲染掉时 focus() 是无害空操作。
 */
export function useModalFocus(open: Ref<unknown>, dialogEl: Ref<HTMLElement | null>) {
  /** 弹窗打开前聚焦的元素；关闭时还回去。 */
  let returnFocus: HTMLElement | null = null;

  watch(open, (t) => {
    if (t) {
      returnFocus = document.activeElement as HTMLElement | null;
      // watch 默认在 DOM 更新前跑，弹窗是 v-if 出来的，要等下一 tick 才在文档里
      nextTick(() =>
        dialogEl.value?.querySelector<HTMLElement>("[data-dialog-cancel]")?.focus(),
      );
    } else {
      returnFocus?.focus();
      returnFocus = null;
    }
  });
}

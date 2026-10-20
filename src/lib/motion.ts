/**
 * 动效工具。时长与缓动照 `prototype-ledger`；所有入口统一尊重
 * prefers-reduced-motion——JS 动效不会自动被 CSS 媒体查询关掉。
 */

export const EASE_OUT = "cubic-bezier(0.22, 1, 0.36, 1)";

export function reducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** 数字补间（字节数滚动）。返回取消函数。 */
export function tweenNumber(
  from: number,
  to: number,
  duration: number,
  onFrame: (value: number) => void,
): () => void {
  if (reducedMotion() || duration <= 0 || from === to) {
    onFrame(to);
    return () => {};
  }
  const start = performance.now();
  let raf = 0;
  const tick = (now: number) => {
    const t = Math.min(1, (now - start) / duration);
    // ease-out cubic：数字先快后慢，最后一位停稳
    onFrame(from + (to - from) * (1 - Math.pow(1 - t, 3)));
    if (t < 1) raf = requestAnimationFrame(tick);
  };
  raf = requestAnimationFrame(tick);
  return () => cancelAnimationFrame(raf);
}

/** 记录行位置，供排序 / 筛选后做 FLIP。 */
export function captureRowTops(root: HTMLElement | null): Map<string, number> {
  const out = new Map<string, number>();
  if (!root) return out;
  for (const el of root.querySelectorAll<HTMLElement>("[data-row-id]")) {
    out.set(el.dataset.rowId!, el.getBoundingClientRect().top);
  }
  return out;
}

/** 对同一批行做位移补间；新出现的行只淡入。 */
export function playRowFlip(
  root: HTMLElement | null,
  prev: Map<string, number>,
): void {
  if (!root || reducedMotion()) return;
  for (const el of root.querySelectorAll<HTMLElement>("[data-row-id]")) {
    const id = el.dataset.rowId!;
    const before = prev.get(id);
    if (before === undefined) {
      el.animate([{ opacity: 0 }, { opacity: 1 }], {
        duration: 150,
        easing: EASE_OUT,
      });
      continue;
    }
    const dy = before - el.getBoundingClientRect().top;
    if (Math.abs(dy) < 0.5) continue;
    el.animate(
      [{ transform: `translateY(${dy}px)` }, { transform: "none" }],
      { duration: 280, easing: EASE_OUT },
    );
  }
}

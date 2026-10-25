import { afterEach, describe, expect, it, vi } from "vitest";
import { tweenNumber } from "@/lib/motion";

// tweenNumber 是「数字滚动」的全部语义所在：reduced-motion / 时长非正 / 值相等
// 三条捷径直接落终值——JS 动效不会被 CSS 媒体查询自动关掉，缺了这个守卫
// reduce 用户面前就是永远在滚的数字。动画路径用可控的 rAF / 时钟喂帧，
// 验证 ease-out cubic 的取值曲线与「到点停」。
afterEach(() => vi.unstubAllGlobals());

describe("tweenNumber", () => {
  it("reduced-motion：直接落终值，取消函数为空操作", () => {
    vi.stubGlobal("window", { matchMedia: () => ({ matches: true }) });
    const frames: number[] = [];
    const cancel = tweenNumber(0, 100, 1000, (v) => frames.push(v));
    expect(frames).toEqual([100]);
    expect(() => cancel()).not.toThrow();
  });

  it("时长非正与 from === to 同样直接落终值", () => {
    vi.stubGlobal("window", { matchMedia: () => ({ matches: false }) });
    const framesA: number[] = [];
    tweenNumber(0, 100, 0, (v) => framesA.push(v));
    expect(framesA).toEqual([100]);
    const framesB: number[] = [];
    tweenNumber(50, 50, 1000, (v) => framesB.push(v));
    expect(framesB).toEqual([50]);
  });

  it("动画路径按 ease-out cubic 取值，t=1 落终值并停止排帧", () => {
    vi.stubGlobal("window", { matchMedia: () => ({ matches: false }) });
    vi.stubGlobal("performance", { now: () => 0 });
    let raf: FrameRequestCallback | null = null;
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      raf = cb;
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", () => {});

    const frames: number[] = [];
    tweenNumber(0, 1000, 1000, (v) => frames.push(v));
    expect(raf).not.toBeNull();

    // t=0.5 → 1-(1-0.5)^3 = 0.875；ease-out 先快后慢
    raf!(500);
    expect(frames).toEqual([875]);
    // t=1 → 终值，且不再排下一帧
    raf!(1000);
    expect(frames).toEqual([875, 1000]);
  });
});

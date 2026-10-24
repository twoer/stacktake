import { describe, expect, it } from "vitest";
import { isLocalPath } from "@/lib/path";

// isLocalPath 是「能不能揭示位置 / 能不能清理」的共同守卫（硬约束 10）：
// 假阳性的后果是给 docker:// 这类虚拟路径挂上本地动作，假阴性只是少一个按钮，
// 所以用例偏向钉住「绝对不能算本地路径」的形态。
describe("isLocalPath", () => {
  it("认 POSIX 绝对路径", () => {
    expect(isLocalPath("/Users/demo/Code/repo-a")).toBe(true);
  });

  it("认 Windows 盘符（两种斜杠都算）", () => {
    expect(isLocalPath("C:\\Users\\demo\\AppData\\Local")).toBe(true);
    expect(isLocalPath("C:/Users/demo/AppData/Local")).toBe(true);
  });

  it("拒绝虚拟 scheme——盘符判据不能放宽到吞掉它们", () => {
    expect(isLocalPath("docker://postgres:16")).toBe(false);
    expect(isLocalPath("overview://repos")).toBe(false);
  });

  it("拒绝相对路径与空串", () => {
    expect(isLocalPath("repo-a/src")).toBe(false);
    expect(isLocalPath("")).toBe(false);
  });
});

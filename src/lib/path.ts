/**
 * 宿主机路径判定与平台相关的展示文案。
 *
 * 从 Assets.vue 下沉（2026-10-24 拆分计划阶段 0）：`isLocalPath` 有七个使用点，
 * 横跨右键菜单守卫、`canClean` / `canCleanArtifacts` 判据与检查器的路径显示；
 * 检查器和右键菜单拆成独立组件后都需要它，属于跨组件的路径面，不是某个视图的私事。
 */
import { t } from "@/lib/i18n";

/** Windows 上 Overlay 标题栏把原生最小化/最大化/关闭浮在右上，工具栏右侧要留位。 */
export const isWindows = navigator.userAgent.includes("Windows");

/**
 * 文案按平台给：macOS 说访达，Windows 说「打开所在位置」。
 * 检查器动作条内容区只有 228px，两个 xs 按钮要装得下——
 * Windows 的「在资源管理器中显示」9 个字会把「清理」挤到第二行，
 * 所以 Windows 用短文案（英文目录里两边都收成短句）。
 *
 * 做成函数而不是常量：常量在模块加载时就算死了语言，切语言不会重渲染。
 */
export function revealLabel(): string {
  return isWindows ? t("actions.revealWindows") : t("actions.revealMac");
}

/**
 * 只有宿主机绝对路径才谈得上「显示位置」或「复制了直接用」。
 * `Item.path` **不都是**宿主机路径——docker 的是 `docker://<镜像名>`，
 * 总览行的是 `overview://<分组id>`，都没有可揭示的位置。
 */
export function isLocalPath(p: string): boolean {
  return p.startsWith("/") || /^[A-Za-z]:[\\/]/.test(p);
}

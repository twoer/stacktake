import { createRouter, createWebHashHistory } from "vue-router";
import Assets from "@/views/Assets.vue";
import Settings from "@/views/Settings.vue";

/**
 * 必须用 hash history：dev 时跑在 http://localhost:1420 上，browser history 看着正常，
 * 但打包后页面走 tauri:// 协议，pushState 失效、刷新直接白屏。
 */
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", redirect: "/assets" },
    { path: "/assets/:groupId?", component: Assets },
    // 设置页：缓存管理（清空缓存 / 立即重扫）+ 更新检查。
    // 曾经还有一节「扫描根目录增删」（C1 / D2 拍板选 A），2026-10-19 删掉了。
    // 路由本身与分组无关，所以不走 /assets/:groupId? 那个参数。
    { path: "/settings", component: Settings },
  ],
});

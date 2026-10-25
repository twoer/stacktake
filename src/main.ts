import { createApp } from "vue";
import App from "./App.vue";
import { router } from "./router";
import { initLocale } from "./composables/useLocale";
import "./style.css";
// vue-sonner 的样式改由 style.css 顶部以 layer(sonner) 引入：vue-sonner 2.x 把样式
// 从 JS 里拆出去了，不导这份 CSS toast 就是裸 DOM（2026-10-19 实锤）；而以普通
// import 引入是 unlayered，会压过 utilities 里的主题 class（2026-10-23 实锤），
// 必须排进 utilities 之前的 layer，封装的 bg-background 等才能生效。

// 首帧用 navigator 猜语言（同步），随后由 Rust 的 system_locale 纠正；不 await，
// 免得系统语言读取挡住首屏。
void initLocale();

createApp(App).use(router).mount("#app");

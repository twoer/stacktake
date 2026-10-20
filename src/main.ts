import { createApp } from "vue";
import App from "./App.vue";
import { router } from "./router";
import "./style.css";
// vue-sonner 2.x 把样式从 JS 里拆出去了，不导这份 CSS toast 就是裸 DOM：
// 图标/标题/描述竖着堆、位置掉进 flex 流溢出窗口右缘（2026-10-19 实锤）。
import "vue-sonner/style.css";

createApp(App).use(router).mount("#app");

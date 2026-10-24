import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// 截图基线的独立构建（2026-10-24 拆分回路的配套设施）：入口是
// harness.html（mock IPC + 夹具数据），产物进 dist-shot，不混进 tauri 的 dist、
// 不随 app 发布。用法：
//   npx vite build -c vite.screenshot.config.ts
//   npx vite preview -c vite.screenshot.config.ts
export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  build: {
    outDir: "dist-shot",
    rollupOptions: {
      input: { harness: fileURLToPath(new URL("./harness.html", import.meta.url)) },
    },
  },
});

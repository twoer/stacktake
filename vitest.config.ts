import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vitest/config";

/**
 * 单测只跑逻辑层（D16 拍板 A）：node 环境够用，不挂组件树，也就不需要 jsdom。
 * alias 与 vite.config.ts 重复一份是刻意的——那份为 Tauri dev/build 服务（挂着
 * vue / tailwind 插件），跑几个纯函数不值得拉起整套构建插件；两处 alias 要一起改。
 */
export default defineConfig({
  test: { environment: "node" },
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
});

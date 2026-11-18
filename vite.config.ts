import { defineConfig, lazyPlugins } from "vite-plus";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import process from "node:process";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: lazyPlugins(() => [vue(), tailwindcss()]),

  resolve: {
    // 别名必须与 tsconfig paths 保持一致
    // 注：当前版本 shadcn-vue CLI 生成的组件直接 `import { cn } from "cn"`，
    // 这里把裸包名 cn 指到本地实现，避免引入同名第三方包
    alias: [
      { find: "@", replacement: fileURLToPath(new URL("./src", import.meta.url)) },
      { find: /^cn$/, replacement: fileURLToPath(new URL("./src/lib/utils.ts", import.meta.url)) },
    ],
  },

  css: {
    preprocessorOptions: {
      scss: {
        api: "modern-compiler",
        // 每个 .scss 自动注入变量与 mixin，免去重复 @use
        additionalData: `@use "@/styles/abstracts" as *;`,
      },
    },
  },

  // Oxlint 默认不启用 Vue 插件；plugins 为覆盖式配置，需显式保留默认三项
  // 模板规则覆盖有限，.vue 的类型检查仍交给 vue-tsc
  lint: {
    plugins: ["unicorn", "typescript", "oxc", "vue"],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));

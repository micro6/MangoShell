import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "@/styles/index.css";
// vue-sonner 自带样式必须显式引入，否则 toast 会退化成无定位的裸文本
import "vue-sonner/style.css";

// 主题跟随系统（后续接入设置项后改为读配置）
const prefersDark = window.matchMedia("(prefers-color-scheme: dark)");
const applyTheme = (dark: boolean) => {
  document.documentElement.classList.toggle("dark", dark);
};
applyTheme(prefersDark.matches);
prefersDark.addEventListener("change", (e) => applyTheme(e.matches));

createApp(App).use(createPinia()).mount("#app");

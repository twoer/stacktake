import { onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type Theme = "light" | "dark";

const STORAGE_KEY = "stacktake-theme";

/**
 * 深浅色状态。模块级单例，同 useAssets——目前只有顶栏一个写入点，
 * 但保持单例，免得以后第二个消费方各持一份状态。
 *
 * 三件事：
 * 1. 首次（没有存过选择）跟随系统 prefers-color-scheme，并持续跟随——
 *    macOS 用户开着「自动」外观时会日落切深色，开着的 app 也该跟着变。
 * 2. 用户一旦手动切换过就以存储值为准，系统再变也不打扰（写存储只发生在
 *    主动切换时；启动时应用初始值不写，否则「没有选择」这个状态就丢了）。
 * 3. class 挂在 <html> 上（style.css 的 .dark 变量与 dark: 变体都认它）。
 * 4. 同步**原生窗口外观**：`<html>` 的 class 只对 WebView 内的 CSS 有效，管不到 NSWindow。
 *    macOS 侧栏的 vibrancy 材质（U12）跟随的是窗口外观 —— app 内强制反向主题时不同步，
 *    侧栏会停在系统那一侧（深色主区 + 浅灰侧栏，字都看不清）。跟随系统传 null 交还系统。
 */
const theme = ref<Theme>(readStored() ?? systemTheme());

function readStored(): Theme | null {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v === "light" || v === "dark" ? v : null;
  } catch {
    return null; // 隐私模式等场景 localStorage 会抛，退化成跟随系统
  }
}

function systemTheme(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

watch(
  theme,
  (t) => {
    document.documentElement.classList.toggle("dark", t === "dark");
    syncNativeTheme();
  },
  { immediate: true },
);

/**
 * 把 WebView 里的主题通报给原生窗口。判据用 readStored() 而不是当前 theme 值：
 * 「用户有没有强制」才是要不要交给系统的依据 —— 存了值就钉住，没存就交还系统。
 * 非 Tauri 环境（浏览器里跑 dev）没有窗口，Linux 不支持该接口，一律吞掉。
 */
function syncNativeTheme() {
  try {
    getCurrentWindow()
      .setTheme(readStored())
      .catch(() => {});
  } catch {
    /* 同上：拿不到窗口就不同步，只影响侧栏材质，不影响主区 */
  }
}

export function useTheme() {
  const onSystemChange = (e: MediaQueryListEvent) => {
    if (!readStored()) theme.value = e.matches ? "dark" : "light";
  };

  let mq: MediaQueryList | null = null;
  onMounted(() => {
    mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", onSystemChange);
  });
  onUnmounted(() => {
    mq?.removeEventListener("change", onSystemChange);
  });

  function toggle() {
    theme.value = theme.value === "dark" ? "light" : "dark";
    try {
      localStorage.setItem(STORAGE_KEY, theme.value);
    } catch {
      /* 存不下不影响当前会话显示，下次开机重新跟随系统 */
    }
  }

  return { theme, toggle };
}

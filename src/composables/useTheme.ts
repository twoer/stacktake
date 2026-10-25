import { computed, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  THEME_STORAGE_KEY,
  nativeThemeFor,
  parseThemePreference,
  resolveTheme,
  type ThemePreference,
} from "@/lib/theme";

/**
 * 外观偏好。模块级单例，同 useAssets——App 外壳（toast）与设置页共享一份。
 *
 * 三态：system（跟随系统，缺省）/ light / dark。四件事：
 * 1. 存储只写用户的选择；`system` 也落盘，这样「显式跟随」与「从没选过」不再靠
 *    键是否存在来区分（旧版本写过的 light/dark 原样认）。
 * 2. 跟随系统时持续听 prefers-color-scheme——macOS「自动」外观日落切深色，
 *    开着的 app 也跟着变；钉住之后系统再变不打扰。
 * 3. class 挂在 <html> 上（style.css 的 .dark 变量与 dark: 变体都认它）。
 * 4. 同步**原生窗口外观**：`<html>` 的 class 只对 WebView 内的 CSS 有效，管不到 NSWindow。
 *    macOS 侧栏的 vibrancy 材质（U12）跟随的是窗口外观 —— app 内强制反向主题时不同步，
 *    侧栏会停在系统那一侧（深色主区 + 浅灰侧栏，字都看不清）。跟随系统传 null 交还系统。
 */
const preference = ref<ThemePreference>(readStoredPreference());

const mq =
  typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)")
    : null;
const systemDark = ref(mq?.matches ?? false);
mq?.addEventListener("change", (e) => {
  systemDark.value = e.matches;
});

const theme = computed(() => resolveTheme(preference.value, systemDark.value));

watch(
  [theme, preference],
  () => {
    document.documentElement.classList.toggle("dark", theme.value === "dark");
    syncNativeTheme();
  },
  { immediate: true },
);

function readStoredPreference(): ThemePreference {
  try {
    return parseThemePreference(localStorage.getItem(THEME_STORAGE_KEY));
  } catch {
    return "system"; // 隐私模式等场景 localStorage 会抛，退化成跟随系统
  }
}

/** 非 Tauri 环境（浏览器里跑 dev）没有窗口，Linux 不支持该接口，一律吞掉。 */
function syncNativeTheme() {
  try {
    getCurrentWindow()
      .setTheme(nativeThemeFor(preference.value))
      .catch(() => {});
  } catch {
    /* 同上：拿不到窗口就不同步，只影响侧栏材质，不影响主区 */
  }
}

export function useTheme() {
  function setPreference(next: ThemePreference) {
    preference.value = next;
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      /* 存不下不影响当前会话显示，下次开机回到跟随系统 */
    }
  }

  return { preference, theme, setPreference };
}

import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  LOCALE_STORAGE_KEY,
  navigatorLocaleTag,
  readStoredLocalePreference,
  resolveLocale,
  type AppLocale,
  type LocalePreference,
} from "@/lib/locale";
import { applyLocale } from "@/lib/i18n";

/**
 * 语言偏好。模块级单例，与 useTheme 同一套：设置页写入，启动时读同一份。
 *
 * 初始语言先用 `navigator.languages` 猜一帧（浏览器 / 截图 harness 也有值），
 * 再在 `initLocale` 里用 Rust 的 `system_locale` 纠正——这样首屏不会白等 IPC，
 * 也不会在系统语言与 WebView 猜测不一致时一直错下去。
 */
const preference = ref<LocalePreference>(readStoredLocalePreference());
const systemTag = ref<string | null>(navigatorLocaleTag());
const resolved = computed<AppLocale>(() => resolveLocale(preference.value, systemTag.value));

watch(resolved, (next) => applyLocale(next), { immediate: true });

/** Tauri 下读系统 BCP-47；浏览器里 invoke 会拒，保持 navigator 的猜测。 */
export async function initLocale(): Promise<void> {
  try {
    const tag = await invoke<string | null>("system_locale");
    if (tag) systemTag.value = tag;
  } catch {
    /* 非 Tauri 环境：navigatorLocaleTag 已经是当前最好的信息 */
  }
}

export function useLocale() {
  function setPreference(next: LocalePreference) {
    preference.value = next;
    try {
      localStorage.setItem(LOCALE_STORAGE_KEY, next);
    } catch {
      /* 存不下不影响本次会话显示，下次开机回到跟随系统 */
    }
  }

  return { preference, resolved, setPreference };
}

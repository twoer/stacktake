/**
 * 界面语言：系统标签解析、偏好存取与语言清单。**纯函数，不碰 DOM**——
 * 这是本文件与 `i18n.ts` 的分界：解析规则可以喂样本测，副作用归 i18n。
 */

export type AppLocale = "en" | "es" | "ja" | "zh-CN" | "zh-TW";
/** `system` = 跟随系统（缺省）；其余是用户显式钉住的语言。 */
export type LocalePreference = "system" | AppLocale;

export const LOCALE_STORAGE_KEY = "stacktake-locale";

export const APP_LOCALES: readonly AppLocale[] = ["en", "es", "ja", "zh-CN", "zh-TW"];

/**
 * 语言名用各自的母语写，**不进语言包**——切到任何语言都认得出自己那一项。
 * 「跟随系统」那句才需要翻译（见 `settings.language.systemWith`）。
 */
export const LOCALE_OPTIONS: readonly { value: AppLocale; label: string }[] = [
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "en", label: "English" },
  { value: "es", label: "Español" },
  { value: "ja", label: "日本語" },
];

export function isAppLocale(value: string): value is AppLocale {
  return (APP_LOCALES as readonly string[]).includes(value);
}

/** 只认五种语言；缺省、旧版本写入的非法值与 "system" 一律回到跟随系统。 */
export function parseLocalePreference(raw: string | null): LocalePreference {
  return raw !== null && isAppLocale(raw) ? raw : "system";
}

/**
 * 系统 BCP-47 → 本应用的五种语言。
 *
 * 繁体判定看 `hant` 或地区（TW / HK / MO）——本版没有独立的 zh-HK，港澳门户
 * 也归 zh-TW；`zh` 缺省按简体。**五种之外一律 en**（拍板过的兜底，不是
 * 「保持系统语言」）：宁可给英文，也不给一份没人校对过的机器译本。
 */
export function normalizeLocaleTag(tag: string | null | undefined): AppLocale {
  if (!tag) return "en";
  const lower = tag.toLowerCase();
  if (lower.startsWith("zh")) {
    return /hant|(^|[-_])(tw|hk|mo)($|[-_])/.test(lower) ? "zh-TW" : "zh-CN";
  }
  if (lower.startsWith("ja")) return "ja";
  if (lower.startsWith("es")) return "es";
  return "en";
}

export function resolveLocale(
  preference: LocalePreference,
  systemTag: string | null | undefined,
): AppLocale {
  return preference === "system" ? normalizeLocaleTag(systemTag) : preference;
}

export function readStoredLocalePreference(): LocalePreference {
  try {
    return parseLocalePreference(localStorage.getItem(LOCALE_STORAGE_KEY));
  } catch {
    return "system"; // 隐私模式等场景 localStorage 会抛，退化成跟随系统
  }
}

/** 浏览器 / 截图 harness 的兜底：Tauri 下会被 `system_locale` 的结果覆盖。 */
export function navigatorLocaleTag(): string | null {
  try {
    return navigator.languages?.[0] ?? navigator.language ?? null;
  } catch {
    return null;
  }
}

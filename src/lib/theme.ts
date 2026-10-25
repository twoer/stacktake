export type Theme = "light" | "dark";
/** system = 跟随系统（缺省状态）；light / dark = 用户显式钉住。 */
export type ThemePreference = "system" | Theme;

export const THEME_STORAGE_KEY = "stacktake-theme";

/**
 * 存储里只认 "light" / "dark" 为强制选择；缺省、旧版本写入的非法值与 "system"
 * 一律回到跟随系统。`index.html` 的首帧脚本按同一口径判色，改这里要同步。
 */
export function parseThemePreference(raw: string | null): ThemePreference {
  return raw === "light" || raw === "dark" ? raw : "system";
}

export function resolveTheme(preference: ThemePreference, systemDark: boolean): Theme {
  if (preference === "system") return systemDark ? "dark" : "light";
  return preference;
}

/**
 * 原生窗口外观（macOS 侧栏 vibrancy 跟随它）：null = 交还系统。
 * 与 WebView 的 .dark class 不同步就没法只改主区——见 useTheme 的注释。
 */
export function nativeThemeFor(preference: ThemePreference): Theme | null {
  return preference === "system" ? null : preference;
}

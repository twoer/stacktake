/**
 * 界面文案与本地化读数。
 *
 * **刻意不引 vue-i18n**：Tauri 的 CSP 没有 `unsafe-eval`，vue-i18n 的运行时
 * 消息编译会踩脚本策略；预编译那一套又要多一个构建插件。这里只需要查表、
 * 命名插值、复数和 Intl 格式化，一百来行足够，且键可以静态检查。
 *
 * 缓存里存的是键（后端 `Text`），所以切语言只重渲染，**不重扫、不重新 IPC**。
 * 五种语言之外的系统标签在 `locale.ts` 里一律归 en。
 */
import { ref } from "vue";
import en from "@/locales/en.json";
import es from "@/locales/es.json";
import ja from "@/locales/ja.json";
import zhCN from "@/locales/zh-CN.json";
import zhTW from "@/locales/zh-TW.json";
import {
  normalizeLocaleTag,
  navigatorLocaleTag,
  readStoredLocalePreference,
  type AppLocale,
} from "@/lib/locale";
import type { Text } from "@/lib/types";

type Params = Record<string, unknown>;
type Catalog = Record<string, string>;

const CATALOGS: Record<AppLocale, Catalog> = {
  en,
  es,
  ja,
  "zh-CN": zhCN,
  "zh-TW": zhTW,
};

/** 前端文案键以 en 为基准；其余语言缺键由 `check-i18n` 与单测拦住。 */
type CatalogKey = keyof typeof en;
/**
 * 复数键在表里只存 `.one` / `.other` 变体，调用方写的却是基名
 * （`t("assets.freed", { count })`），由 `resolveMessage` 按 count 选后缀。
 * 这里把基名也算作合法键，否则每处复数调用都要写死一个后缀、复数形同虚设。
 */
type PluralBase<K extends string = CatalogKey> = K extends `${infer Base}.${"one" | "other"}`
  ? Base
  : never;
export type MessageKey = CatalogKey | PluralBase;

function initialLocale(): AppLocale {
  const preference = readStoredLocalePreference();
  return preference === "system" ? normalizeLocaleTag(navigatorLocaleTag()) : preference;
}

/** 当前界面语言。模板里调 `t()` / `renderText()` 会读到它，因而随切换重渲染。 */
export const activeLocale = ref<AppLocale>(initialLocale());

function syncDocumentLang(): void {
  // 单测跑在 node 环境，没有 document；lang 只影响读屏与断词，缺了不影响逻辑。
  if (typeof document !== "undefined") {
    document.documentElement.lang = activeLocale.value;
  }
}
syncDocumentLang();

export function applyLocale(locale: AppLocale): void {
  activeLocale.value = locale;
  syncDocumentLang();
}

// ---- 文案 ----

/** 静态键入口：写错键名 vue-tsc 会报；后端下发的动态键走 `translate`。 */
export function t(key: MessageKey, params?: Params): string {
  return translate(key, params);
}

export function translate(key: string, params?: Params): string {
  const message = resolveMessage(key, params);
  if (!params) return message;
  return message.replace(/\{(\w+)\}/g, (whole, name: string) => {
    const value = params[name];
    return value === undefined ? whole : formatArg(value);
  });
}

function resolveMessage(key: string, params?: Params): string {
  const locale = activeLocale.value;
  if (typeof params?.count === "number") {
    const category = pluralRules(locale).select(params.count);
    for (const candidate of [`${key}.${category}`, `${key}.other`, key]) {
      const found = lookup(locale, candidate);
      if (found !== undefined) return found;
    }
  }
  return lookup(locale, key) ?? key;
}

function lookup(locale: AppLocale, key: string): string | undefined {
  return CATALOGS[locale][key] ?? CATALOGS.en[key];
}

/** 后端 `Text`：Raw 原样、Key 查表、Fmt 带命名参数。 */
export function renderText(text: Text | null | undefined): string {
  if (!text) return "";
  switch (text.t) {
    case "r":
      return text.v;
    case "k":
      return translate(text.v);
    case "f": {
      const params: Params = {};
      for (const [name, arg] of Object.entries(text.v.a)) params[name] = arg;
      return translate(text.v.k, params);
    }
  }
}

function formatArg(value: unknown): string {
  if (typeof value === "number") return formatNumber(value);
  if (typeof value === "string") return value;
  if (isTagged(value)) {
    if (value.t === "n" && typeof value.v === "number") return formatNumber(value.v);
    if (value.t === "b" && typeof value.v === "number") return formatBytesLocalized(value.v);
    if (value.t === "r" || value.t === "k" || value.t === "f") {
      return renderText(value as Text);
    }
  }
  return String(value);
}

function isTagged(value: unknown): value is { t: string; v: unknown } {
  return typeof value === "object" && value !== null && "t" in value && "v" in value;
}

// ---- 本地化读数 ----

const pluralFormats = new Map<AppLocale, Intl.PluralRules>();
function pluralRules(locale: AppLocale): Intl.PluralRules {
  let formatter = pluralFormats.get(locale);
  if (!formatter) {
    formatter = new Intl.PluralRules(locale);
    pluralFormats.set(locale, formatter);
  }
  return formatter;
}

const numberFormats = new Map<string, Intl.NumberFormat>();
function numberFormat(locale: AppLocale, grouping: boolean): Intl.NumberFormat {
  const cacheKey = `${locale}:${grouping ? "grouped" : "plain"}`;
  let formatter = numberFormats.get(cacheKey);
  if (!formatter) {
    formatter = new Intl.NumberFormat(locale, { useGrouping: grouping });
    numberFormats.set(cacheKey, formatter);
  }
  return formatter;
}

export function formatNumber(value: number, grouping = true): string {
  return numberFormat(activeLocale.value, grouping).format(value);
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

export function formatBytesLocalized(bytes: number): string {
  if (bytes <= 0) return "0 B";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  if (unit === 0) return `${formatNumber(bytes, false)} B`;
  // 只抹掉多余的「.0」，保留真正的非零小数——取整会让人分不出 1.4 GB 与 1.1 GB。
  // 尾数不参与千分位：es 的千分位是「.」，1023 MB 会变成有歧义的「1.023 MB」。
  const fixed = value.toFixed(1);
  const trimmed = fixed.endsWith(".0") ? fixed.slice(0, -2) : fixed;
  return `${formatNumber(Number(trimmed), false)} ${BYTE_UNITS[unit]}`;
}

const relativeFormats = new Map<AppLocale, Intl.RelativeTimeFormat>();
function relativeFormat(locale: AppLocale): Intl.RelativeTimeFormat {
  let formatter = relativeFormats.get(locale);
  if (!formatter) {
    formatter = new Intl.RelativeTimeFormat(locale, { numeric: "always" });
    relativeFormats.set(locale, formatter);
  }
  return formatter;
}

export function relativeTime(value: number, unit: Intl.RelativeTimeFormatUnit): string {
  return relativeFormat(activeLocale.value).format(value, unit);
}

const collators = new Map<AppLocale, Intl.Collator>();
export function compareText(a: string, b: string): number {
  const locale = activeLocale.value;
  let collator = collators.get(locale);
  if (!collator) {
    collator = new Intl.Collator(locale);
    collators.set(locale, collator);
  }
  return collator.compare(a, b);
}

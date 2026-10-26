// 目录对等门禁（P4，Track I）：五份 src/locales/*.json 的键集比对。
//
// 规则（缺一即红，退出码非 0）：
//   1. 五份目录的**基键集合**（剥掉复数变体后缀后）完全相等——一条译文只该
//      「缺整条」，不该「缺某种语言」；`translate()` 缺条目回显键名，漏一种
//      语言就是那种语言的界面印着 `data.xxx`。
//   2. 所有值必须是字符串。
//   3. 复数变体后缀只认 Intl.PluralRules 的六个类别；`.other` 是万能兜底、
//      各语言随意（有 `.other` 或基键都行，`resolveMessage` 依次回退）。
//   4. `.one` 这类**非 other 变体**是「有意的不对称」：哪些基键在哪些语言有
//      `.one` 由下方 PLURAL_ONE 白名单钉死——登记与事实对不上（多出、缺失、
//      陈旧）都算错。**别拉平**：ja / zh 没有复数范畴，永远不该出现 `.one`；
//      en / es 按各自语言的复数规则写。
//   5. 重复键：JSON.parse 会静默取后者，按行数比对抓出来。
//
// 用法：
//   node scripts/check-i18n.mjs              # 查仓库里的五份目录
//   node scripts/check-i18n.mjs --self-test  # 坏样本自检（接内环，先自检再查仓库）
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

export const LOCALES = ["en", "es", "ja", "zh-CN", "zh-TW"];

const PLURAL_CATEGORIES = new Set(["zero", "one", "two", "few", "many", "other"]);

/**
 * 单复数变体的**有意不对称**（P2a 的既成事实，这里钉死防漂移）：
 * 基键 → 哪些语言有 `.one`。2026-10-26 取数：10 个基键 en/es 都有、
 * `filter.active` 与 `clean.artifact.selectedSummary` 只有 es（西语复数比英语
 * 多两档）、`data.section` 谁都没有。新增带 count 的文案时：en/es 补 `.one`
 * 就在这里登记；**不要**为了「对齐」往 ja / zh 里加 `.one`。
 */
export const PLURAL_ONE = {
  "assets.batchSelected": ["en", "es"],
  "assets.freed": ["en", "es"],
  "assets.statusItems": ["en", "es"],
  "clean.artifact.body": ["en", "es"],
  "clean.artifact.selectedSummary": ["es"],
  "clean.batchAiBody": ["en", "es"],
  "clean.batchCacheBody": ["en", "es"],
  "clean.batchRepoBody": ["en", "es"],
  "clean.totalItems": ["en", "es"],
  "data.section": [],
  "filter.active": ["es"],
  "inspector.omitted": ["en", "es"],
};

/** 键 → { base, variant }；variant 是 Intl.PluralRules 类别，无变体为 null。 */
export function splitKey(k) {
  const i = k.lastIndexOf(".");
  if (i >= 0) {
    const v = k.slice(i + 1);
    if (PLURAL_CATEGORIES.has(v)) return { base: k.slice(0, i), variant: v };
  }
  return { base: k, variant: null };
}

/**
 * 核心校验，纯函数（自检喂夹具，仓库检查喂真实目录）。
 * @param {Record<string, Record<string, string>>} catalogs 语言名 → 键值表
 * @param {Record<string, string[]>} pluralOne `.one` 白名单
 * @returns {string[]} 问题列表（空 = 通过）
 */
export function checkCatalogs(catalogs, pluralOne = PLURAL_ONE) {
  const problems = [];
  const locs = Object.keys(catalogs);
  for (const expect of LOCALES) {
    if (!locs.includes(expect)) problems.push(`缺少语言 ${expect}`);
  }
  for (const loc of locs) {
    if (!LOCALES.includes(loc)) problems.push(`未知语言 ${loc}（合法：${LOCALES.join(" / ")}）`);
  }
  if (problems.length) return problems;

  const basesByLoc = {};
  for (const [loc, catalog] of Object.entries(catalogs)) {
    basesByLoc[loc] = new Set();
    for (const [k, v] of Object.entries(catalog)) {
      if (typeof v !== "string") {
        problems.push(`${loc}: ${k} 的值不是字符串（嵌套对象与数字都不进目录）`);
        continue;
      }
      const { base, variant } = splitKey(k);
      basesByLoc[loc].add(base);
      if (variant && variant !== "other" && variant !== "one") {
        problems.push(
          `${loc}: ${k} 用了 .${variant} 变体——本门禁只登记 .one，真需要时先改这里的结构再登记`,
        );
      }
    }
  }

  // 规则 1：基键集合五语相等
  const [ref, ...rest] = LOCALES;
  for (const loc of rest) {
    const missing = [...basesByLoc[ref]].filter((k) => !basesByLoc[loc].has(k));
    const extra = [...basesByLoc[loc]].filter((k) => !basesByLoc[ref].has(k));
    for (const k of missing) problems.push(`${loc} 缺基键 ${k}（${ref} 有）——界面会印键名`);
    for (const k of extra) problems.push(`${loc} 多出基键 ${k}（${ref} 没有）——别的语言印键名`);
  }

  // 规则 4：.one 白名单与事实双向一致
  const knownBases = new Set(Object.values(basesByLoc).flatMap((s) => [...s]));
  for (const [base, expectedLocs] of Object.entries(pluralOne)) {
    if (!knownBases.has(base)) {
      problems.push(`白名单陈旧：${base} 不在任何目录里——键删了白名单也要删`);
      continue;
    }
    for (const loc of LOCALES) {
      const catalog = catalogs[loc];
      const hasOne = catalog[`${base}.one`] !== undefined;
      const expected = expectedLocs.includes(loc);
      if (hasOne && !expected) {
        problems.push(`${loc}: ${base}.one 未登记（白名单说该语言没有）——要么删掉变体，要么更新 PLURAL_ONE`);
      }
      if (!hasOne && expected) {
        problems.push(`${loc}: ${base}.one 白名单登记了却不存在——语言目录或 PLURAL_ONE 有一边漂了`);
      }
      if (hasOne && catalog[`${base}.other`] === undefined && catalog[base] === undefined) {
        problems.push(`${loc}: ${base}.one 缺兜底（既无 .other 也无基键）——非 other 数会印键名`);
      }
    }
  }
  // 未登记的非 other 变体：基键不在白名单里却出现 .one
  for (const [loc, catalog] of Object.entries(catalogs)) {
    for (const k of Object.keys(catalog)) {
      const { base, variant } = splitKey(k);
      if (variant === "one" && !(base in pluralOne)) {
        problems.push(`${loc}: ${k} 未在 PLURAL_ONE 登记——有意的不对称要登记，别拉平`);
      }
    }
  }
  return problems;
}

/** 重复键检测：JSON.parse 静默取后者，只能按行数比对。 */
export function countDuplicateKeys(raw) {
  const keys = [...raw.matchAll(/^\s*"((?:[^"\\]|\\.)*)"\s*:/gm)].map((m) => m[1]);
  const seen = new Set();
  const dupes = [];
  for (const k of keys) {
    if (seen.has(k)) dupes.push(k);
    seen.add(k);
  }
  return dupes;
}

function selfTest() {
  // 小白名单 + 最小五语夹具，覆盖每条规则的正面与反面
  const pluralOne = { "a.freed": ["en", "es"], "a.section": [] };
  const good = {
    en: { "a.freed.other": "f", "a.freed.one": "1", "a.section.other": "s", "x.y": "y" },
    es: { "a.freed.other": "f", "a.freed.one": "1", "a.section.other": "s", "x.y": "y" },
    ja: { "a.freed.other": "f", "a.section.other": "s", "x.y": "y" },
    "zh-CN": { "a.freed.other": "f", "a.section.other": "s", "x.y": "y" },
    "zh-TW": { "a.freed.other": "f", "a.section.other": "s", "x.y": "y" },
  };
  const cases = [
    ["好样本", { ...good }, pluralOne, 0],
    ["基键缺失", { ...good, ja: { "a.freed.other": "f", "a.section.other": "s" } }, pluralOne, 1],
    ["基键多出", { ...good, "zh-CN": { ...good["zh-CN"], "x.z": "z" } }, pluralOne, 1],
    ["未登记 .one（ja）", { ...good, ja: { ...good.ja, "a.freed.one": "1" } }, pluralOne, 1],
    ["白名单漂移（en 丢了 .one）", {
      ...good,
      en: { "a.freed.other": "f", "a.section.other": "s", "x.y": "y" },
    }, pluralOne, 1],
    ["非法变体 .few", { ...good, en: { ...good.en, "a.freed.few": "f2" } }, pluralOne, 1],
    ["值不是字符串", { ...good, en: { ...good.en, "x.y": 42 } }, pluralOne, 1],
    [".one 缺兜底", {
      ...good,
      en: { "a.freed.one": "1", "a.section.other": "s", "x.y": "y" },
    }, pluralOne, 1],
    ["白名单陈旧", { ...good }, { ...pluralOne, "a.gone": ["en"] }, 1],
  ];
  let passed = 0;
  for (const [name, catalogs, wl, wantProblems] of cases) {
    const problems = checkCatalogs(catalogs, wl);
    const ok = wantProblems === 0 ? problems.length === 0 : problems.length >= wantProblems;
    if (ok) {
      passed++;
      console.log(`✓ ${name}`);
    } else {
      console.error(`✗ ${name}（期望 ${wantProblems === 0 ? "0 条" : "≥1 条"}问题，实际 ${problems.length}）：${problems.join(" | ")}`);
    }
  }
  // 重复键检测器
  const dupProblems = countDuplicateKeys('{\n  "a": "1",\n  "a": "2",\n  "b": "3"\n}');
  const noDup = countDuplicateKeys('{\n  "a": "1",\n  "b": "2"\n}');
  if (dupProblems.length === 1 && dupProblems[0] === "a" && noDup.length === 0) {
    passed++;
    console.log("✓ 重复键检测");
  } else {
    console.error(`✗ 重复键检测：${JSON.stringify({ dupProblems, noDup })}`);
  }
  console.log(`目录对等自检：${passed}/${cases.length + 1}`);
  return passed === cases.length + 1;
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const catalogs = {};
  for (const loc of LOCALES) {
    const file = join(root, "src", "locales", `${loc}.json`);
    const raw = readFileSync(file, "utf8");
    const dupes = countDuplicateKeys(raw);
    if (dupes.length) {
      console.error(`${loc}: 重复键（JSON.parse 会静默取后者）：${dupes.join(", ")}`);
      process.exit(1);
    }
    catalogs[loc] = JSON.parse(raw);
  }
  const problems = checkCatalogs(catalogs);
  if (problems.length) {
    console.error(`目录对等检查未通过（${problems.length} 条）：`);
    for (const p of problems) console.error(`  - ${p}`);
    console.error(" asymmetry 白名单在 check-i18n.mjs 的 PLURAL_ONE；有意的不对称登记它，别拉平。");
    process.exit(1);
  }
  const pluralKeys = new Set(
    Object.values(catalogs).flatMap((c) => Object.keys(c).filter((k) => splitKey(k).variant === "one")),
  );
  console.log(
    `目录对等通过（${LOCALES.length} 语 / ${new Set(Object.values(catalogs).flatMap((c) => Object.keys(c).map((k) => splitKey(k).base))).size} 基键 / .one 白名单 ${Object.keys(PLURAL_ONE).length} 条，现存在 ${pluralKeys.size} 条）`,
  );
}

if (process.argv.includes("--self-test")) {
  process.exit(selfTest() ? 0 : 1);
} else {
  main();
}

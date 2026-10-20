// 对比度实测工具（U14 引入，纪律继承自 U11）：直接读 src/style.css 的 oklch
// token，实算 WCAG 对比度并对照下面的期望清单。动任何配色前先跑一遍，
// 「只升不降」是硬要求——U11 把 muted-foreground 从 4.73 抬到过 4.34 的 chip
// 组合，靠的就是每次数值落纸，不靠「改深了所以更安全」的手感。
//
// 用法：node scripts/contrast.mjs
// 退出码：任一组合低于其下限 → 1（可挂进收口前的自检）。
// 注意：本文件不解析 alpha 叠加（ring/border 这类带 / 透明度的 token 不进文本组合）。
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../src/style.css", import.meta.url), "utf8");

// OKLCH → sRGB（BSD 参照实现的标准矩阵）→ WCAG 相对亮度 → 对比度。
function oklchToRgb(L, C, Hdeg) {
  const h = (Hdeg * Math.PI) / 180;
  const a = C * Math.cos(h);
  const b = C * Math.sin(h);
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = L - 0.0894841775 * a - 1.291485548 * b;
  const l = l_ ** 3, m = m_ ** 3, s = s_ ** 3;
  const r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
  const g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
  const b2 = -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s;
  const gam = (x) =>
    x <= 0.0031308 ? 12.92 * x : 1.055 * Math.pow(x, 1 / 2.4) - 0.055;
  return [r, g, b2].map((x) => Math.min(1, Math.max(0, gam(x))));
}
function luminance(rgb) {
  const f = (x) =>
    x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
  return 0.2126 * f(rgb[0]) + 0.7152 * f(rgb[1]) + 0.0722 * f(rgb[2]);
}
function contrast(c1, c2) {
  const [hi, lo] = [luminance(c1), luminance(c2)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// 从 style.css 抠 token：只取无 alpha 的 oklch（文本 token 都不带 alpha）。
function parseTokens(block) {
  const vars = {};
  const re = /--([\w-]+):\s*oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*\)/g;
  for (const m of block.matchAll(re)) {
    vars[m[1]] = oklchToRgb(+m[2], +m[3], +m[4]);
  }
  return vars;
}
// 「.dark」在 :root 的注释里也出现过（「深色侧见 .dark --reclaim」），
// 边界必须锚定行首的选择器，不能裸 indexOf。
const rootBlock = css.slice(css.indexOf(":root"), css.indexOf("\n.dark"));
const darkBlock = css.slice(css.indexOf("\n.dark"), css.indexOf("@theme"));
const themes = { 浅色: parseTokens(rootBlock), 深色: parseTokens(darkBlock) };

// 期望清单：组合 → 下限。全部是文本组合，底线 4.5:1（AA）；
// primary-foreground/primary 是实心按钮，顺带盯住。
const PA = [
  ["foreground", "background", 4.5],
  ["muted-foreground", "background", 4.5],
  ["muted-foreground", "muted", 4.5], // chip / kbd / 表头
  ["reclaim", "background", 4.5], // 「可回收」绿，11px 数字也在用
  ["primary-foreground", "primary", 4.5], // 实心按钮（墨底纸字 / 纸底墨字）
];

let fail = 0;
for (const [theme, vars] of Object.entries(themes)) {
  for (const [fg, bg, min] of PA) {
    if (!vars[fg] || !vars[bg]) {
      console.error(`${theme}: 缺 token --${fg} 或 --${bg}`);
      fail = 1;
      continue;
    }
    const ratio = contrast(vars[fg], vars[bg]);
    const mark = ratio >= min ? "PASS" : "FAIL";
    if (ratio < min) fail = 1;
    console.log(
      `${mark} ${theme} ${fg}/${bg}: ${ratio.toFixed(2)}:1（下限 ${min}）`,
    );
  }
}
process.exit(fail);

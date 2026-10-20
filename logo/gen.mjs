// Stacktake 图标生成：SVG 模板 -> 多配色 svg -> PNG（Chrome 无头 + sips）
// 用法: node gen.mjs
import { execFileSync } from 'node:child_process';
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(fileURLToPath(import.meta.url));
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

const PALETTES = {
  amber: { bg: '#111A2C', bg2: '#1C2A45', fg: '#F2F7FF', fg2: '#D3DEEE', fg3: '#A3B3CD', accent: '#FFB227' },
  teal: { bg: '#0F1A22', bg2: '#172C31', fg: '#E9F6F7', fg2: '#C0D7DA', fg3: '#8BABAF', accent: '#2FD3A6' },
  indigo: { bg: '#141C3C', bg2: '#25305F', fg: '#EEF2FF', fg2: '#C8D0F0', fg3: '#98A6D2', accent: '#FFC24D' },
};

const shell = (c) => c;

const frame = (p, body) => `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024" fill="none">
<defs>
<linearGradient id="bg" x1="140" y1="120" x2="900" y2="920" gradientUnits="userSpaceOnUse">
<stop stop-color="${p.bg}"/><stop offset="1" stop-color="${p.bg2}"/>
</linearGradient>
<radialGradient id="glow" cx="0" cy="0" r="1" gradientUnits="userSpaceOnUse" gradientTransform="translate(300 200) rotate(48) scale(560 520)">
<stop stop-color="#FFFFFF" stop-opacity="0.10"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/>
</radialGradient>
</defs>
<rect x="88" y="88" width="848" height="848" rx="196" fill="url(#bg)"/>
<rect x="88" y="88" width="848" height="848" rx="196" fill="url(#glow)"/>
${body}
</svg>`;

// ── 方案 A：横板堆叠 + 竖直量尺（stack + take）
// 三条自上而下变宽的板 = 技术栈占用的分层体积；右侧量尺 = 清点；
// 游标与中板末端的高亮段同色 = 「这一层里有可回收的部分」
const planA = (p) => frame(p, `
<!-- 堆叠：三条分层体积条 -->
<rect x="332" y="312" width="240" height="84" rx="24" fill="${p.fg3}"/>
<rect x="282" y="418" width="340" height="116" rx="28" fill="${p.fg2}"/>
<rect x="232" y="556" width="440" height="156" rx="32" fill="${p.fg}"/>
<!-- 中板末端：可回收的那一段 -->
<rect x="552" y="418" width="70" height="116" rx="24" fill="${p.accent}"/>
<!-- 量尺 -->
<rect x="700" y="262" width="56" height="500" rx="28" fill="${p.fg}"/>
<g stroke="${p.bg}" stroke-width="12" stroke-linecap="butt">
<path d="M700 312H756M700 418H756M700 556H756M700 712H756"/>
<path d="M700 365H738M700 487H738M700 634H738"/>
</g>
<!-- 游标：读数落在中板 -->
<path d="M700 452 L658 476 L700 500 Z" fill="${p.accent}"/>
`);

// ── 方案 B：占比环，其中一片被摘出来
const planB = (p) => {
  const C = 2 * Math.PI * 300;
  const seg = (deg) => (deg / 360) * C;
  return frame(p, `
<g transform="translate(0 0)" fill="none" stroke-linecap="butt" stroke-width="116">
<circle cx="512" cy="512" r="300" stroke="${p.fg}" stroke-dasharray="${seg(132).toFixed(1)} ${C.toFixed(1)}" transform="rotate(-90 512 512)"/>
<circle cx="512" cy="512" r="300" stroke="${p.fg3}" stroke-dasharray="${seg(132).toFixed(1)} ${C.toFixed(1)}" transform="rotate(128 512 512)"/>
<circle cx="512" cy="512" r="300" stroke="${p.accent}" stroke-dasharray="${seg(66).toFixed(1)} ${C.toFixed(1)}" transform="translate(26 -2) rotate(52 512 512)"/>
</g>
`);
};

// ── 方案 C：等距两层，上层被抬起（掀开清点）
const iso = (cx, cy, W, H) => {
  const top = `${cx} ${cy - W / 2} ${cx + W} ${cy} ${cx} ${cy + W / 2} ${cx - W} ${cy}`;
  const left = `${cx - W} ${cy} ${cx} ${cy + W / 2} ${cx} ${cy + W / 2 + H} ${cx - W} ${cy + H}`;
  const right = `${cx + W} ${cy} ${cx} ${cy + W / 2} ${cx} ${cy + W / 2 + H} ${cx + W} ${cy + H}`;
  return { top, left, right };
};
const planC = (p) => {
  const b = iso(512, 596, 246, 66);
  const t = iso(512, 384, 246, 66);
  return frame(p, `
<!-- 底层 -->
<polygon points="${b.left}" fill="${p.fg3}"/>
<polygon points="${b.right}" fill="${p.fg3}"/>
<polygon points="${b.top}" fill="${p.fg2}"/>
<!-- 抬起的上层 -->
<polygon points="${t.left}" fill="${p.fg3}"/>
<polygon points="${t.right}" fill="${p.fg}"/>
<polygon points="${t.top}" fill="${p.fg}"/>
<!-- 清点：抬起的那道间隙 -->
<path d="M512 486 V552" stroke="${p.accent}" stroke-width="20" stroke-linecap="round"/>
`);
};

// ── 方案 D：清单台账（界面微缩）—— 侧栏分组 + 分节头 + 三条条目行（长度=体积）+ 可回收标记
const planD = (p) => frame(p, `
<!-- 侧栏：分组 -->
<rect x="234" y="336" width="76" height="352" rx="32" fill="${p.fg3}"/>
<!-- 分节标题 -->
<rect x="360" y="336" width="170" height="34" rx="17" fill="${p.fg3}"/>
<!-- 三条条目，长度即体积 -->
<rect x="360" y="408" width="300" height="76" rx="28" fill="${p.fg2}"/>
<rect x="360" y="510" width="430" height="76" rx="28" fill="${p.fg}"/>
<rect x="360" y="612" width="360" height="76" rx="28" fill="${p.fg2}"/>
<!-- 最长行末端：可回收的那一段 -->
<rect x="714" y="510" width="76" height="76" rx="26" fill="${p.accent}"/>
`);

// ── 方案 E：本机（显示器）+ 屏幕内清单
const planE = (p) => frame(p, `
<rect x="204" y="286" width="616" height="404" rx="46" stroke="${p.fg}" stroke-width="36"/>
<rect x="300" y="376" width="240" height="56" rx="22" fill="${p.fg3}"/>
<rect x="300" y="462" width="400" height="56" rx="22" fill="${p.fg}"/>
<rect x="300" y="548" width="320" height="56" rx="22" fill="${p.fg2}"/>
<rect x="624" y="462" width="76" height="56" rx="20" fill="${p.accent}"/>
<rect x="452" y="690" width="120" height="34" rx="12" fill="${p.fg3}"/>
<rect x="372" y="716" width="280" height="34" rx="17" fill="${p.fg3}"/>
`);

// ── 方案 F：三柱（体积）+ 被挑出来的那一根（悬空 + 强调色）
const planF = (p) => frame(p, `
<rect x="234" y="700" width="556" height="42" rx="21" fill="${p.fg3}"/>
<rect x="272" y="470" width="110" height="230" rx="30" fill="${p.fg2}"/>
<rect x="612" y="400" width="110" height="300" rx="30" fill="${p.fg}"/>
<rect x="452" y="336" width="110" height="244" rx="30" fill="${p.accent}"/>
`);

// ── 方案 G（定稿）：极简几何 + 一个巧思
// 主体 = 一整块（本机全部开发资产的占用），三道横向负空间切出分层构成（各类资产的体积）；
// 唯一巧思 = 右边有一层被推出一截并着强调色 —— 像盘点时被拉开的抽屉，即「被清点出来、可清掉的那一份」。
const planG = (p) => `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024" fill="none">
<defs>
<linearGradient id="bg" x1="140" y1="120" x2="900" y2="920" gradientUnits="userSpaceOnUse">
<stop stop-color="${p.bg}"/><stop offset="1" stop-color="${p.bg2}"/>
</linearGradient>
<radialGradient id="glow" cx="0" cy="0" r="1" gradientUnits="userSpaceOnUse" gradientTransform="translate(300 200) rotate(48) scale(600 540)">
<stop stop-color="#FFFFFF" stop-opacity="0.10"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/>
</radialGradient>
<linearGradient id="body" x1="512" y1="222" x2="512" y2="802" gradientUnits="userSpaceOnUse">
<stop stop-color="${p.fg}"/><stop offset="1" stop-color="${p.fg2}"/>
</linearGradient>
<mask id="strata">
<rect x="222" y="222" width="580" height="580" rx="82" fill="#fff"/>
<g fill="#000">
<rect x="200" y="308" width="640" height="26"/>
<rect x="200" y="442" width="640" height="26"/>
<rect x="200" y="634" width="640" height="26"/>
</g>
</mask>
</defs>
<rect x="88" y="88" width="848" height="848" rx="196" fill="url(#bg)"/>
<rect x="88" y="88" width="848" height="848" rx="196" fill="url(#glow)"/>
<rect x="222" y="222" width="580" height="580" rx="82" fill="url(#body)" mask="url(#strata)"/>
<path d="M222 468 H848 A30 30 0 0 1 878 498 V578 A30 30 0 0 1 848 608 H222 Z" fill="${p.accent}"/>
</svg>`;

const PLANS = { a: planA, b: planB, c: planC, d: planD, e: planE, f: planF, g: planG };

// 生成 svg
mkdirSync(resolve(ROOT, 'svg'), { recursive: true });
mkdirSync(resolve(ROOT, 'png'), { recursive: true });
for (const [pn, fn] of Object.entries(PLANS)) {
  for (const [cn, pal] of Object.entries(PALETTES)) {
    const out = resolve(ROOT, `svg/${pn}-${cn}.svg`);
    writeFileSync(out, fn(pal));
  }
}

// 渲染 PNG（一次一个，避免 Chrome 被 SIGTERM）
const render = (svgPath, pngPath) => {
  execFileSync(CHROME, [
    '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-sandbox',
    '--user-data-dir=/tmp/chrome-icon-profile', '--force-device-scale-factor=1',
    '--default-background-color=00000000', '--window-size=1024,1024',
    `--screenshot=${pngPath}`, `file://${svgPath}`,
  ], { stdio: 'ignore', timeout: 60000 });
};

const shrink = (src, size, dst) => {
  execFileSync('sips', ['-z', String(size), String(size), src, '--out', dst], { stdio: 'ignore' });
};

const targets = process.argv[2] ? [process.argv[2]] : ['a', 'b', 'c'];
for (const pn of targets) {
  const src = resolve(ROOT, `svg/${pn}-amber.svg`);
  const png1024 = resolve(ROOT, `png/${pn}-1024.png`);
  render(src, png1024);
  for (const s of [512, 256, 144, 96, 56, 48]) {
    shrink(png1024, s, resolve(ROOT, `png/${pn}-${s}.png`));
  }
  console.log(`rendered ${pn}`);
}

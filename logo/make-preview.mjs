// 生成 preview.html：三方案对比 + 配色变体 + 小尺寸验收
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(fileURLToPath(import.meta.url));
const svg = (name) => readFileSync(resolve(ROOT, 'svg', `${name}.svg`), 'utf8');

const PALETTES = {
  amber: { bg: '#111A2C', bg2: '#1C2A45', fg: '#F2F7FF', fg2: '#D3DEEE', fg3: '#A3B3CD', accent: '#FFB227' },
  teal: { bg: '#0F1A22', bg2: '#172C31', fg: '#E9F6F7', fg2: '#C0D7DA', fg3: '#8BABAF', accent: '#2FD3A6' },
  indigo: { bg: '#141C3C', bg2: '#25305F', fg: '#EEF2FF', fg2: '#C8D0F0', fg3: '#98A6D2', accent: '#FFC24D' },
};
const recolor = (s, p) => s
  .replace(/#111A2C|#0F1A22|#141C3C/g, p.bg)
  .replace(/#1C2A45|#172C31|#25305F/g, p.bg2)
  .replace(/#F2F7FF|#E9F6F7|#EEF2FF/g, p.fg)
  .replace(/#D3DEEE|#C0D7DA|#C8D0F0/g, p.fg2)
  .replace(/#A3B3CD|#8BABAF|#98A6D2/g, p.fg3)
  .replace(/#FFB227|#2FD3A6|#FFC24D/g, p.accent);

const inline = (name, size, pal) => {
  let s = svg(name).replace('<svg ', `<svg style="width:${size}px;height:${size}px;display:block" `);
  if (pal) s = recolor(s, pal);
  return s;
};

const plans = [
  { id: 'g', title: '定稿 · 一整块 + 拉出的一层', desc: '主体只有一整块 = 这台机器上全部开发资产的占用；三道横向负空间切出分层构成（各类资产的体积）；唯一的巧思：右边一层被推出一截并着强调色 —— 像盘点时被拉开的抽屉，即「被清点出来、可清掉的那一份」。48px 下依然是一个轮廓明确的方块 + 一条橙色。' },
];

const rows = plans.map((pl) => `
<section style="margin:0 0 56px">
  <h2 style="font:600 17px/1.4 -apple-system,'PingFang SC',sans-serif;margin:0 0 4px">${pl.title}</h2>
  <p style="font:13px/1.7 -apple-system,'PingFang SC',sans-serif;color:#7d8aa0;margin:0 0 18px;max-width:720px">${pl.desc}</p>
  <div style="display:flex;align-items:flex-end;gap:28px">
    <div class="cell sq">${inline(pl.id + '-amber', 224)}</div>
    <div class="cell round">${inline(pl.id + '-amber', 224)}</div>
    <div class="cell sq">${inline(pl.id + '-amber', 144)}</div>
    <div class="cell sq">${inline(pl.id + '-amber', 56)}</div>
  </div>
  <div style="display:flex;gap:14px;margin-top:16px">
    ${Object.entries(PALETTES).map(([k, p]) => `<div style="text-align:center"><div class="cell sq" style="display:inline-block">${inline(pl.id + '-amber', 96, p)}</div><div style="font:11px/1.6 -apple-system;color:#7d8aa0;margin-top:6px">${k}</div></div>`).join('')}
  </div>
</section>`).join('');

const strip48 = plans.map((pl) => inline(pl.id + '-amber', 48)).join('');

const html = `<!doctype html><html><head><meta charset="utf-8"><title>Stacktake 图标方案对比 · 第二轮</title>
<style>
body{background:#0d1117;margin:0;padding:48px}
h1{color:#e8eef7;font:600 22px/1.4 -apple-system,'PingFang SC',sans-serif;margin:0 0 8px}
.wrap{max-width:960px}
.cell{background:repeating-conic-gradient(#20293a 0% 25%, #182030 0% 50%) 0 0/16px 16px;border-radius:18px;overflow:hidden}
.round{border-radius:50%}
</style></head><body><div class="wrap">
<h1>Stacktake 图标 · 三方案对比</h1>
<p style="font:13px/1.7 -apple-system,'PingFang SC',sans-serif;color:#7d8aa0;max-width:720px;margin:0 0 40px">按「极简几何 + 一个巧思」定稿：一个主体形状，靠错位制造记忆点，不堆元素。下方含三套配色备选与 48px 验收。</p>
${rows}
<section>
  <h2 style="font:600 15px/1.4 -apple-system,'PingFang SC',sans-serif;color:#e8eef7;margin:0 0 12px">48px 横向对照（按此排左右就近 App 列表里的真实大小）</h2>
  <div style="display:flex;align-items:center;gap:20px">${strip48}</div>
</section>
<section style="margin-top:48px;padding-top:24px;border-top:1px solid #232c3d">
  <h2 style="font:600 14px/1.4 -apple-system,'PingFang SC',sans-serif;color:#e8eef7;margin:0 0 8px">提交要求与改色</h2>
  <ul style="font:12.5px/1.9 -apple-system,'PingFang SC',sans-serif;color:#7d8aa0;padding-left:18px;max-width:720px">
    <li>macOS .icns / Windows .ico 由 <code style="color:#aab8cf">npx tauri icon logo/final/logo-1024.png</code> 一键产出，无需手工。</li>
    <li>改色只动 svg 里的 fill：底色 ${''}两组 stop、图形三层 fg/fg2/fg3、强调色 accent。svg 与 preview 中颜色一一对应。</li>
    <li>微信等圆形裁切场景已验证：主体都在中心半径 430 内，圆裁不切。</li>
  </ul>
</section>
</div></body></html>`;

writeFileSync(resolve(ROOT, 'preview.html'), html);
console.log('preview.html written');

#!/usr/bin/env node
// 文档门禁：常驻文档预算 + archive 索引完整性 + 活文档死链。
//
// 为什么要有它：文档是每个会话都要支付的成本，但预算、索引和链接都只靠纪律——
// H11/H12 压下来的东西会慢慢长回去，新刀归档也可能漏索引。把这三类规则做成
// 机器检查，和契约校验器同一个思路：校验器自己挂了比没有更危险，所以带坏样本自检。
//
//   node scripts/check-docs.mjs             检查当前仓库
//   node scripts/check-docs.mjs --self-test 跑坏样本自检
//
// 零依赖，Node 22+。

import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// 预算是「天花板」，不是目标值：要加内容先改这里的数字并在报告里说明理由。
// 三个文件分别是每会话常载 / 每次取刀必读 / 常被恢复的索引。
const BUDGETS = [
  // 17_500：2026-10-17 代码风格段的 UI 权威从「原型 v3」改为指向生产代码
  // （src/views/Assets.vue），表述变长一次；17_000 顶破。
  // 17_800：2026-10-20 同日落入两份硬约束——U14/U15 纸账本视觉身份（不画行条 /
  // 不做药丸 / 侧栏无图标 / 不自我解释，删过别加回来）与 H14 隐藏分组的恢复步骤；
  // 已先把本段其它表述压到最短，仍差 40 字符，才动数字。
  { file: "AGENTS.md", max: 17_800 },
  // 11_400：2026-10-26 同日三刀（M1 / N1 / M2）的登记与收口说明累积在此；
  // M1 归档时随首次归档回落。细节都在 docs/evidence/，backlog 只留结论。
  { file: "docs/backlog.md", max: 11_400 },
  { file: "docs/backlog-archive.md", max: 8_000 },
];

const LIVE_DOCS = [
  "AGENTS.md",
  "README.md",
  "docs/backlog.md",
  "docs/backlog-archive.md",
  "docs/harness-and-loop.md",
  "docs/known-limits.md",
];

// 本地开发工作流文档（取刀清单 / 归档正文 / 归档索引）不随仓库分发。
// 作者本机有时照常受检；clone 下来的树里没有就跳过——「没有内部文档」不是错误，
// 不能让一棵干净的 clone 因为缺它而红。判据只看文件在不在，不区分是谁在跑。
const LOCAL_ONLY_DOCS = [
  "docs/backlog.md",
  "docs/backlog-archive.md",
  "docs/harness-and-loop.md",
];

const read = (root, file) => readFileSync(join(root, file), "utf8");
const write = (root, file, text) => writeFileSync(join(root, file), text);
const chars = (text) => Array.from(text).length;

function walkMd(dir) {
  const out = [];
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walkMd(path));
    else if (entry.isFile() && entry.name.endsWith(".md")) out.push(path);
  }
  return out;
}

// 只抽 Markdown 链接目标；先剥掉代码块与行内代码，避免把示例当链接。
function markdownTargets(text) {
  const clean = text.replace(/```[\s\S]*?```/g, "").replace(/`[^`]*`/g, "");
  const targets = [];
  for (const match of clean.matchAll(/\]\(([^)]+)\)/g)) {
    let target = match[1].trim();
    if (target.startsWith("<") && target.endsWith(">")) target = target.slice(1, -1);
    target = target.split(/\s+/)[0];
    targets.push(target);
  }
  return targets;
}

// 返回本地绝对路径；外链、锚点、站内绝对路径不归这里管。
function localTarget(docFile, target) {
  if (!target || target.startsWith("#")) return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(target)) return null;
  if (target.startsWith("/")) return null;
  const path = target.split("#")[0];
  if (!path) return null;
  let decoded = path;
  try {
    decoded = decodeURIComponent(path);
  } catch {
    // 非法转义按原样处理，存在性检查会给出死链
  }
  return resolve(dirname(docFile), decoded);
}

function section(text, prefix) {
  const start = text.indexOf(prefix);
  if (start < 0) return null;
  const rest = text.slice(start);
  const next = rest.indexOf("\n## ", prefix.length);
  return next < 0 ? rest : rest.slice(0, next);
}

function idsIn(text, pattern) {
  return [...text.matchAll(pattern)].map((match) => match[1]);
}

function checkDocs(root) {
  const errors = [];

  for (const { file, max } of BUDGETS) {
    const path = join(root, file);
    if (!existsSync(path)) {
      // 本地专有文档缺了不算错：clone 的树里本来就没有
      if (!LOCAL_ONLY_DOCS.includes(file)) errors.push(`缺少文档 ${file}`);
      continue;
    }
    const count = chars(read(root, file));
    if (count > max) {
      errors.push(`${file} 为 ${count} 字符，超过预算 ${max}——加内容前先改预算并说明理由`);
    }
  }

  // 归档目录与索引都是本地专有的：两者都不在，说明这棵树没有内部文档，整组跳过。
  const archiveDir = join(root, "docs/archive");
  const indexPath = join(root, "docs/backlog-archive.md");
  const hasArchive = existsSync(archiveDir) || existsSync(indexPath);
  const archiveFiles = hasArchive ? walkMd(archiveDir) : [];
  const cutIds = new Map();
  const decisionIds = new Map();

  for (const absolute of archiveFiles) {
    const rel = relative(root, absolute).split(sep).join("/");
    const text = readFileSync(absolute, "utf8");
    for (const id of idsIn(text, /^- \[x\] \*\*([A-Z]+\d+)/gm)) {
      if (cutIds.has(id)) errors.push(`archive 重复刀号 ${id}: ${cutIds.get(id)} 与 ${rel}`);
      else cutIds.set(id, rel);
    }
    for (const id of idsIn(text, /^### (D\d+)/gm)) {
      if (decisionIds.has(id)) errors.push(`archive 重复决策号 ${id}: ${decisionIds.get(id)} 与 ${rel}`);
      else decisionIds.set(id, rel);
    }
  }

  if (!hasArchive) {
    // 跳过：本地专有的归档与索引都不在这棵树上
  } else if (!existsSync(indexPath)) {
    errors.push("缺少 docs/backlog-archive.md（归档索引）");
  } else {
    const index = readFileSync(indexPath, "utf8");
    const cutSection = section(index, "## 刀");
    const decisionSection = section(index, "## 【待确认】决策请求");
    if (cutSection === null) errors.push("索引缺少「## 刀」段");
    if (decisionSection === null) errors.push("索引缺少「## 【待确认】决策请求」段");

    const indexedCuts = new Set(cutSection ? idsIn(cutSection, /^\s*-\s+\*\*([A-Z]+\d+)/gm) : []);
    const indexedDecisions = new Set(
      decisionSection ? idsIn(decisionSection, /^\s*-\s+\*\*([A-Z]+\d+)/gm) : [],
    );

    for (const [id, file] of cutIds) {
      if (!indexedCuts.has(id)) errors.push(`索引缺少刀 ${id}（正文在 ${file}）`);
    }
    for (const [id, file] of decisionIds) {
      if (!indexedDecisions.has(id)) errors.push(`索引缺少决策 ${id}（正文在 ${file}）`);
    }
    for (const id of [...indexedCuts, ...indexedDecisions]) {
      if (!cutIds.has(id) && !decisionIds.has(id)) {
        errors.push(`索引有陈旧 ID ${id}（archive 中不存在）`);
      }
    }

    // 索引链接可达 + archive 无孤儿文件
    const linked = new Set();
    for (const target of markdownTargets(index)) {
      const absolute = localTarget(indexPath, target);
      if (!absolute) continue;
      if (existsSync(absolute)) linked.add(absolute);
      else errors.push(`索引死链 ${target}`);
    }
    for (const absolute of archiveFiles) {
      if (!linked.has(absolute)) {
        errors.push(`archive 孤儿文件 ${relative(root, absolute).split(sep).join("/")}（索引没有链接）`);
      }
    }
  }

  for (const doc of LIVE_DOCS) {
    const absolute = join(root, doc);
    if (!existsSync(absolute)) {
      // 同上：本地专有文档不在不算错
      if (!LOCAL_ONLY_DOCS.includes(doc)) errors.push(`缺少活文档 ${doc}`);
      continue;
    }
    for (const target of markdownTargets(readFileSync(absolute, "utf8"))) {
      const local = localTarget(absolute, target);
      if (local && !existsSync(local)) errors.push(`${doc} 死链 ${target}`);
    }
  }

  return errors;
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "stacktake-docs-"));
  const files = {
    "AGENTS.md": "# A\n\n[harness](./docs/harness-and-loop.md)\n",
    "README.md": "# R\n",
    "docs/backlog.md": "# B\n",
    "docs/harness-and-loop.md": "# H\n",
    "docs/known-limits.md": "# K\n",
    "docs/backlog-archive.md": [
      "# idx",
      "",
      "## 刀",
      "- **A1 测试刀** (abc1234) → [track-a.md](./archive/track-a.md)",
      "",
      "## 【待确认】决策请求（非刀）",
      "- **D1 测试决策** → [d01.md](./archive/decisions/d01.md)",
      "",
    ].join("\n"),
    "docs/archive/track-a.md": "- [x] **A1 测试刀** (abc1234)\n",
    "docs/archive/decisions/d01.md": "### D1 测试决策\n",
  };
  for (const [rel, content] of Object.entries(files)) {
    const path = join(root, rel);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  return root;
}

function selfTest() {
  const cases = [
    ["基线通过", () => {}, null],
    ["AGENTS 预算", (root) => write(root, "AGENTS.md", "x".repeat(17_801)), "超过预算"],
    ["backlog 预算", (root) => write(root, "docs/backlog.md", "x".repeat(11_401)), "超过预算"],
    ["索引预算", (root) => write(root, "docs/backlog-archive.md", "x".repeat(8_001)), "超过预算"],
    [
      "缺少刀的索引行",
      (root) => {
        const file = join(root, "docs/backlog-archive.md");
        const text = readFileSync(file, "utf8").replace("- **A1 测试刀** (abc1234) → [track-a.md](./archive/track-a.md)\n", "");
        writeFileSync(file, text);
      },
      "索引缺少刀 A1",
    ],
    [
      "缺少决策的索引行",
      (root) => {
        const file = join(root, "docs/backlog-archive.md");
        const text = readFileSync(file, "utf8").replace("- **D1 测试决策** → [d01.md](./archive/decisions/d01.md)\n", "");
        writeFileSync(file, text);
      },
      "索引缺少决策 D1",
    ],
    [
      "索引死链",
      (root) => {
        const file = join(root, "docs/backlog-archive.md");
        const text = readFileSync(file, "utf8").replace("./archive/track-a.md", "./archive/nope.md");
        writeFileSync(file, text);
      },
      "索引死链",
    ],
    [
      "archive 孤儿文件",
      (root) => write(root, "docs/archive/track-b.md", "- [x] **B1 孤儿** (abc1234)\n"),
      "孤儿文件",
    ],
    [
      "重复刀号",
      (root) => write(root, "docs/archive/track-b.md", "- [x] **A1 重复** (abc1234)\n"),
      "重复刀号 A1",
    ],
    [
      "陈旧索引 ID",
      (root) => {
        const file = join(root, "docs/backlog-archive.md");
        const text = readFileSync(file, "utf8").replace(
          "- **A1 测试刀** (abc1234) → [track-a.md](./archive/track-a.md)\n",
          "- **A1 测试刀** (abc1234) → [track-a.md](./archive/track-a.md)\n- **Z9 陈旧** → [track-a.md](./archive/track-a.md)\n",
        );
        writeFileSync(file, text);
      },
      "陈旧 ID Z9",
    ],
    [
      "活文档死链",
      (root) => {
        const file = join(root, "AGENTS.md");
        writeFileSync(file, readFileSync(file, "utf8") + "[x](./docs/nope.md)\n");
      },
      "死链",
    ],
    [
      "缺少索引段",
      (root) => {
        const file = join(root, "docs/backlog-archive.md");
        writeFileSync(file, readFileSync(file, "utf8").replace("## 刀\n", ""));
      },
      "索引缺少「## 刀」段",
    ],
  ];

  let passed = 0;
  for (const [name, mutate, expected] of cases) {
    const root = fixture();
    try {
      mutate(root);
      const errors = checkDocs(root);
      const ok = expected === null ? errors.length === 0 : errors.some((error) => error.includes(expected));
      if (ok) {
        passed += 1;
        console.log(`  PASS ${name}`);
      } else {
        const actual = errors.length ? errors.join(" | ") : "无错误";
        console.error(`  FAIL ${name}: 期望 ${expected === null ? "通过" : `包含「${expected}」`}，实际 ${actual}`);
      }
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }

  console.log(`文档门禁自检：${passed}/${cases.length}`);
  return passed === cases.length;
}

const selfTestMode = process.argv.includes("--self-test");

if (selfTestMode) {
  process.exit(selfTest() ? 0 : 1);
}

const root = process.argv[2] ? resolve(process.argv[2]) : SCRIPT_ROOT;
const errors = checkDocs(root);
if (errors.length) {
  for (const error of errors) console.error(`  ${error}`);
  process.exit(1);
}
const archiveFiles = walkMd(join(root, "docs/archive")).length;
console.log(`文档门禁通过（预算 ${BUDGETS.length} 项 / archive ${archiveFiles} 个文件 / 活文档 ${LIVE_DOCS.length} 份）`);

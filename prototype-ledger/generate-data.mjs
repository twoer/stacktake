// 从本机真实扫描缓存生成原型数据。用法：node prototype-ledger/generate-data.mjs
// 数据只落 prototype-ledger/data.js（本地原型用，不进 app 构建）。
import { readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const cachePath =
  process.env.STACKTAKE_CACHE ??
  join(homedir(), "Library/Application Support/com.twoer.stacktake/scan-cache.json");
const cache = JSON.parse(readFileSync(cachePath, "utf8"));

const sizeOf = (it) => {
  const m = it.metrics?.size;
  return m && (m.t === "b" || m.t === "i") ? m.v : 0;
};
const reclaimOf = (it) => (it.cleanable ? it.reclaimable ?? 0 : 0);

const hb = (n) => {
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
  if (n >= 1024 ** 2) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${n} B`;
};

const groups = cache.groups.map((g) => ({
  id: g.id,
  label: g.label,
  summary: g.summary ?? "",
  notes: g.notes ?? [],
  supports_clean: !!g.supports_clean,
  fields: (g.fields ?? []).slice(0, 3),
  items: g.items.map((it) => ({
    id: it.id,
    name: it.name,
    group_key: it.group_key ?? null,
    path: it.path,
    metrics: it.metrics,
    detail: it.detail ?? null,
    tags: it.tags ?? [],
    cleanable: !!it.cleanable,
    reclaimable: it.reclaimable ?? 0,
  })),
}));

const sum = (fn) => groups.reduce((s, g) => s + fn(g), 0);
const total = sum((g) => g.items.reduce((s, it) => s + sizeOf(it), 0));
const reclaim = sum((g) => g.items.reduce((s, it) => s + reclaimOf(it), 0));

const overview = {
  id: "overview",
  label: "总览",
  summary: `${groups.length} 个分组 · 合计 ${hb(total)} · 可回收 ${hb(reclaim)}`,
  notes: [],
  supports_clean: false,
  fields: [
    { key: "size", label: "体积", numeric: true },
    { key: "reclaimable", label: "可回收", numeric: true },
    { key: "count", label: "条目", numeric: true },
  ],
  items: groups.map((g) => ({
    id: `overview:${g.id}`,
    name: g.label,
    group_key: null,
    path: `overview://${g.id}`,
    metrics: {
      size: { t: "b", v: g.items.reduce((s, it) => s + sizeOf(it), 0) },
      reclaimable: { t: "b", v: g.items.reduce((s, it) => s + reclaimOf(it), 0) },
      count: { t: "i", v: g.items.length },
    },
    detail: null,
    tags: [],
    cleanable: false,
    reclaimable: null,
  })),
};

const out = {
  schema: cache.schema,
  saved_at: cache.saved_at,
  generated_at: Math.floor(Date.now() / 1000),
  groups: [overview, ...groups],
};

const dest = new URL("./data.js", import.meta.url);
writeFileSync(dest, `window.PROTO_DATA=${JSON.stringify(out)};\n`);
console.log(
  `prototype-ledger/data.js · ${groups.length} 组 · ${groups.reduce((s, g) => s + g.items.length, 0)} 项 · ${hb(total)}`,
);

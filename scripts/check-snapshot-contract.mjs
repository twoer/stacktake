#!/usr/bin/env node
// 快照契约评估：对扫描结果 JSON 校验前端依赖的全部不变量。
//
// 用法：
//   node scripts/check-snapshot-contract.mjs <snapshot.json>   校验 dump 输出或缓存文件
//   node scripts/check-snapshot-contract.mjs --self-test       用故意构造的坏样本自检（毫秒级）
//   --strict                                               把软警告升级为失败
//
// 与 Rust 侧 `tests.rs::assert_frontend_contract` 同源：两边守同一份契约。
// 这侧多一样本事——直接验真实 JSON，能抓住 Rust 类型系统看不见的序列化问题
// （标签错位、null 混进 detail、字段超限在缓存里落地等）。
// self-test 让校验器本身也被校验：它挂了比没有校验器更危险。

import { readFileSync } from 'node:fs'

const METRIC_TAGS = new Set(['s', 'b', 'i', 'z'])
const MAX_FIELDS = 3

function checkMetricValue(v, where, problems) {
  if (v === null || typeof v !== 'object' || Array.isArray(v)) {
    problems.push(`${where}: 指标值不是对象：${JSON.stringify(v)}`)
    return
  }
  if (!METRIC_TAGS.has(v.t)) {
    problems.push(`${where}: 未知类型标签 t=${JSON.stringify(v.t)}（合法：s/b/i/z）`)
    return
  }
  const t = typeof v.v
  const typeOk =
    (v.t === 's' && t === 'string') ||
    (v.t === 'z' && t === 'boolean') ||
    ((v.t === 'b' || v.t === 'i') && t === 'number' && Number.isFinite(v.v))
  if (!typeOk) problems.push(`${where}: t=${v.t} 与 v 的类型 ${t} 不匹配`)
  // Int 允许负数（如 -1 哨兵值），Bytes 不允许
  if (v.t === 'b' && typeof v.v === 'number' && v.v < 0) {
    problems.push(`${where}: 字节数出现负数 ${v.v}`)
  }
}

function checkItem(item, groupId, problems, warnings) {
  const where = `[${groupId}] ${item?.id ?? '(无 id)'}`
  if (!item || typeof item !== 'object' || Array.isArray(item)) {
    problems.push(`${where}: 条目不是对象`)
    return
  }
  for (const k of ['id', 'name', 'path']) {
    if (typeof item[k] !== 'string' || item[k].length === 0) {
      problems.push(`${where}: ${k} 为空或不是字符串`)
    }
  }
  if (item.tags !== undefined) {
    if (!Array.isArray(item.tags) || item.tags.some((t) => typeof t !== 'string')) {
      problems.push(`${where}: tags 不是字符串数组`)
    }
  }
  // 分节渲染：` · ` 前缀做小节标题，必须非空
  if (typeof item.name === 'string' && item.name.includes(' · ')) {
    const prefix = item.name.split(' · ')[0]
    if (prefix.trim().length === 0) problems.push(`${where}: 名称分节前缀为空`)
  }

  // group_key（G1 起）：分节与筛选的数据来源。缺省 = 不分节；给了就必须是非空字符串
  if (item.group_key !== undefined && item.group_key !== null) {
    if (typeof item.group_key !== 'string' || item.group_key.trim().length === 0) {
      problems.push(`${where}: group_key 不是非空字符串：${JSON.stringify(item.group_key)}`)
    }
  }

  const metrics = item.metrics
  if (metrics === null || typeof metrics !== 'object' || Array.isArray(metrics)) {
    problems.push(`${where}: metrics 缺失或不是对象`)
  } else {
    for (const [k, v] of Object.entries(metrics)) {
      if (!k) problems.push(`${where}: 空指标键`)
      checkMetricValue(v, `${where}.metrics.${k}`, problems)
    }
    if (Object.keys(metrics).length === 0) {
      warnings.push(`${where}: metrics 为空，表格行没有可排序列`)
    }
  }

  if (item.detail !== undefined && item.detail !== null) {
    if (typeof item.detail !== 'object' || Array.isArray(item.detail)) {
      problems.push(`${where}: detail 不是对象`)
    } else {
      for (const [k, v] of Object.entries(item.detail)) {
        if (!k || typeof v !== 'string') {
          problems.push(`${where}: detail[${JSON.stringify(k)}] 不是字符串`)
        }
      }
    }
  }

  if (item.cleanable !== undefined && typeof item.cleanable !== 'boolean') {
    problems.push(`${where}: cleanable 不是布尔`)
  }
  if (item.reclaimable !== undefined && item.reclaimable !== null) {
    const r = item.reclaimable
    if (typeof r !== 'number' || !Number.isFinite(r) || r < 0) {
      problems.push(`${where}: reclaimable 不是非负数字：${JSON.stringify(r)}`)
    } else {
      if (item.cleanable !== true) problems.push(`${where}: 有回收量却未标 cleanable`)
      const size = item.metrics?.size
      if (size && size.t === 'b' && r > size.v) {
        problems.push(`${where}: 回收量 ${r} 超过体积 ${size.v}`)
      }
      if (!size) warnings.push(`${where}: 有回收量但缺 size 指标`)
    }
  }
}

function checkGroup(g, problems, warnings) {
  const id = g?.id ?? '(无 id)'
  const where = `[${id}]`
  if (!g || typeof g !== 'object' || Array.isArray(g)) {
    problems.push('分组不是对象')
    return
  }
  for (const k of ['id', 'label', 'summary']) {
    if (typeof g[k] !== 'string' || g[k].length === 0) {
      problems.push(`${where}: ${k} 为空或不是字符串`)
    }
  }

  if (!Array.isArray(g.fields)) {
    problems.push(`${where}: fields 缺失或不是数组`)
  } else {
    if (g.fields.length > MAX_FIELDS) {
      problems.push(
        `${where}: fields 声明了 ${g.fields.length} 个，超过 ${MAX_FIELDS}（前端只渲染前三个，多出来的静默丢失）`,
      )
    }
    for (const f of g.fields) {
      if (!f || typeof f !== 'object' || Array.isArray(f)) {
        problems.push(`${where}: field 不是对象`)
        continue
      }
      for (const k of ['key', 'label']) {
        if (typeof f[k] !== 'string' || !f[k]) problems.push(`${where}: field.${k} 为空`)
      }
      for (const k of ['numeric', 'temporal']) {
        if (f[k] !== undefined && typeof f[k] !== 'boolean') {
          problems.push(`${where}: field.${k} 不是布尔`)
        }
      }
    }
  }

  if (g.error !== undefined && g.error !== null) {
    if (typeof g.error !== 'string') problems.push(`${where}: error 不是字符串`)
    if (Array.isArray(g.items) && g.items.length > 0) {
      problems.push(`${where}: 出错时仍有 ${g.items.length} 条明细（前端错误态不渲染明细）`)
    }
  }

  if (!Array.isArray(g.items)) {
    problems.push(`${where}: items 缺失或不是数组`)
    return
  }
  for (const it of g.items) checkItem(it, id, problems, warnings)

  // 列覆盖（软警告）：声明了字段但所有条目都没有对应指标，前端该列整列空白
  if (g.fields.length > 0 && g.items.length > 0) {
    for (const f of g.fields) {
      const covered = g.items.some(
        (it) => it?.metrics && Object.prototype.hasOwnProperty.call(it.metrics, f.key),
      )
      if (!covered) warnings.push(`${where}: 字段 ${f.key} 在任何条目里都没有对应指标，该列将空白`)
    }
  }

  if (g.notes !== undefined && g.notes !== null) {
    if (!Array.isArray(g.notes) || g.notes.some((n) => typeof n !== 'string')) {
      problems.push(`${where}: notes 不是字符串数组`)
    }
  }
}

/** 校验快照。返回 { ok, problems, warnings, groups }。 */
export function validateSnapshot(data, { strict = false } = {}) {
  const problems = []
  const warnings = []
  let groups = data

  // 兼容两种输入：dump 的分组数组，或缓存信封 { schema, saved_at, groups }
  if (data && typeof data === 'object' && !Array.isArray(data) && Array.isArray(data.groups)) {
    groups = data.groups
    if (typeof data.schema !== 'number') problems.push('[envelope]: schema 不是数字')
    if (typeof data.saved_at !== 'number') problems.push('[envelope]: saved_at 不是数字')
  }

  if (!Array.isArray(groups)) {
    problems.push('顶层既不是分组数组，也不是带 groups 字段的缓存信封')
    return { ok: false, problems, warnings, groups: [] }
  }
  for (const g of groups) checkGroup(g, problems, warnings)

  const ok = problems.length === 0 && (!strict || warnings.length === 0)
  return { ok, problems, warnings, groups }
}

function report(r) {
  const items = r.groups.reduce(
    (n, g) => n + (Array.isArray(g?.items) ? g.items.length : 0),
    0,
  )
  console.log(`分组 ${r.groups.length} 个 · 条目 ${items} 条`)
  for (const g of r.groups) {
    if (!g || typeof g !== 'object') continue
    const n = Array.isArray(g.items) ? g.items.length : 0
    const f = Array.isArray(g.fields) ? g.fields.length : 0
    console.log(`  ${String(g.id).padEnd(10)} ${String(n).padStart(4)} 条 · ${f} 字段${g.error ? ' · error' : ''}`)
  }
  for (const w of r.warnings) console.log(`  ⚠ ${w}`)
  for (const p of r.problems) console.log(`  ✗ ${p}`)
  if (r.problems.length) {
    console.log(`契约违规 ${r.problems.length} 处${r.warnings.length ? `，另有 ${r.warnings.length} 处警告` : ''}`)
  } else if (r.warnings.length) {
    console.log(`契约通过，${r.warnings.length} 处警告`)
  } else {
    console.log('契约通过')
  }
}

/** 校验器自检：每个坏样本都必须被抓住，好样本必须通过。 */
function selfTest() {
  const goodItem = {
    id: '/tmp/a',
    name: '仓库 · a',
    path: '/tmp/a',
    tags: ['可清理'],
    metrics: { size: { t: 'b', v: 100 }, last_commit: { t: 'i', v: 1700000000 } },
    cleanable: true,
    reclaimable: 50,
  }
  const good = {
    id: 'g',
    label: '测试分组',
    summary: '1 条',
    fields: [{ key: 'size', label: '体积', numeric: true }],
    items: [goodItem],
  }
  const cases = [
    ['字段超过三个', { ...good, fields: [{ key: 'a', label: 'a' }, { key: 'b', label: 'b' }, { key: 'c', label: 'c' }, { key: 'd', label: 'd' }] }],
    ['未知指标标签', { ...good, items: [{ ...goodItem, metrics: { size: { t: 'x', v: 1 } } }] }],
    ['标签与值类型不符', { ...good, items: [{ ...goodItem, metrics: { size: { t: 'b', v: '100' } } }] }],
    ['字节数为负', { ...good, items: [{ ...goodItem, metrics: { size: { t: 'b', v: -1 } } }] }],
    ['回收量超过体积', { ...good, items: [{ ...goodItem, reclaimable: 101 }] }],
    ['有回收量未标 cleanable', { ...good, items: [{ ...goodItem, cleanable: false }] }],
    ['分节前缀为空', { ...good, items: [{ ...goodItem, name: ' · a' }] }],
    ['出错却有明细', { ...good, error: 'docker 未运行' }],
    ['detail 值不是字符串', { ...good, items: [{ ...goodItem, detail: { 清理方式: 42 } }] }],
    ['条目缺 name', { ...good, items: [{ ...goodItem, name: '' }] }],
    ['空指标键', { ...good, items: [{ ...goodItem, metrics: { '': { t: 'b', v: 1 } } }] }],
    ['顶层不是数组', { nope: true }],
  ]

  let failed = 0
  const base = validateSnapshot([good])
  if (!base.ok) {
    console.log(`✗ 好样本没通过：${base.problems.join('；')}`)
    failed++
  } else {
    console.log('✓ 好样本通过')
  }
  for (const [name, data] of cases) {
    const r = validateSnapshot(Array.isArray(data) || data.groups ? data : [data])
    if (r.ok) {
      console.log(`✗ 坏样本没被抓住：${name}`)
      failed++
    } else {
      console.log(`✓ 抓住：${name} → ${r.problems[0]}`)
    }
  }
  // 软警告不挡通过：字段无对应指标只警告
  const soft = validateSnapshot([{ ...good, fields: [{ key: 'ghost', label: '幽灵列' }] }])
  console.log(soft.ok && soft.warnings.length === 1 ? '✓ 软警告只警告不挡通过' : '✗ 软警告行为不对')
  if (!(soft.ok && soft.warnings.length === 1)) failed++

  console.log(failed === 0 ? '自检通过' : `自检失败 ${failed} 项`)
  process.exit(failed === 0 ? 0 : 1)
}

function main() {
  const args = process.argv.slice(2)
  if (args.includes('--self-test')) {
    selfTest()
    return
  }
  const strict = args.includes('--strict')
  const file = args.find((a) => !a.startsWith('--'))
  if (!file) {
    console.error('用法：node scripts/check-snapshot-contract.mjs <snapshot.json> [--strict]')
    console.error('      node scripts/check-snapshot-contract.mjs --self-test')
    process.exit(2)
  }
  let data
  try {
    data = JSON.parse(readFileSync(file, 'utf8'))
  } catch (e) {
    console.error(`读不到或不是 JSON：${e.message}`)
    process.exit(2)
  }
  const r = validateSnapshot(data, { strict })
  report(r)
  process.exit(r.ok ? 0 : 1)
}

main()

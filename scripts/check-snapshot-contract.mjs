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

// `Text` 是展示文案的三态（r 成品 / k 文案键 / f 带参文案键），Rust 侧
// `scan::Text` 的 serde 形态。P2a 起分组名、概览、字段名、脚注、标签、行的四个
// 展示位全换成它——界面切语言时只重渲染、不重扫，靠的就是这些位置下发的是键。
// 校验器只验**形状**（键是不是成品中文由 Rust 侧那条门禁加语言目录查）。
const TEXT_TAGS = new Set(['r', 'k', 'f'])
const TEXT_ARG_TAGS = new Set(['r', 'k', 'f', 'n', 'b'])
// 'k' = `MetricValue::TextKey`：指标格里的界面文案（「未知」「无提交」），存键不存成品。
const METRIC_TAGS = new Set(['s', 'b', 'i', 'z', 'k'])
const MAX_FIELDS = 3

/** 递归验 `Text`；`where` 已经带上槽位名。 */
function checkText(v, where, problems) {
  if (v === null || typeof v !== 'object' || Array.isArray(v)) {
    problems.push(`${where}: 展示文案不是 {t,v} 对象：${JSON.stringify(v)}`)
    return
  }
  if (!TEXT_TAGS.has(v.t)) {
    problems.push(`${where}: 未知文案标签 t=${JSON.stringify(v.t)}（合法：r/k/f）`)
    return
  }
  if (v.t === 'f') {
    const f = v.v
    if (f === null || typeof f !== 'object' || Array.isArray(f)) {
      problems.push(`${where}: f 的载荷不是 {k,a} 对象`)
      return
    }
    if (typeof f.k !== 'string' || !f.k) problems.push(`${where}: f.k 文案键为空`)
    if (f.a !== undefined && (f.a === null || typeof f.a !== 'object' || Array.isArray(f.a))) {
      problems.push(`${where}: f.a 参数表不是对象`)
      return
    }
    for (const [name, arg] of Object.entries(f.a ?? {})) {
      if (!name) problems.push(`${where}: 参数名为空`)
      checkTextArg(arg, `${where}.f.a[${name}]`, problems)
    }
    return
  }
  // r / k 都是非空字符串：空串到界面上就是一格空白，和没下发没有区别
  if (typeof v.v !== 'string' || !v.v) {
    problems.push(`${where}: ${v.t} 的值应是非空字符串：${JSON.stringify(v.v)}`)
  }
}

/** `TextArg`：文案之外还能直接塞数字 / 字节，单位换算与千分位留给前端按语言做。 */
function checkTextArg(v, where, problems) {
  if (v === null || typeof v !== 'object' || Array.isArray(v)) {
    problems.push(`${where}: 文案参数不是对象：${JSON.stringify(v)}`)
    return
  }
  if (!TEXT_ARG_TAGS.has(v.t)) {
    problems.push(`${where}: 未知参数标签 t=${JSON.stringify(v.t)}（合法：r/k/f/n/b）`)
    return
  }
  if (v.t === 'n' || v.t === 'b') {
    if (typeof v.v !== 'number' || !Number.isFinite(v.v)) {
      problems.push(`${where}: t=${v.t} 的值应是数字：${JSON.stringify(v.v)}`)
    } else if (v.t === 'b' && v.v < 0) {
      problems.push(`${where}: 字节数出现负数 ${v.v}`)
    }
    return
  }
  checkText(v, where, problems)
}

/** 标签：`key` 是筛选/抑制/阻断的取值，`label` 是展示文案。 */
function checkTag(t, where, problems) {
  if (t === null || typeof t !== 'object' || Array.isArray(t)) {
    problems.push(`${where}: 标签不是 {key,label} 对象：${JSON.stringify(t)}`)
    return
  }
  if (typeof t.key !== 'string' || !t.key) {
    problems.push(`${where}: 标签 key 为空或不是字符串`)
  }
  checkText(t.label, `${where}.label`, problems)
}

function checkMetricValue(v, where, problems) {
  if (v === null || typeof v !== 'object' || Array.isArray(v)) {
    problems.push(`${where}: 指标值不是对象：${JSON.stringify(v)}`)
    return
  }
  if (!METRIC_TAGS.has(v.t)) {
    problems.push(`${where}: 未知类型标签 t=${JSON.stringify(v.t)}（合法：s/b/i/z/k）`)
    return
  }
  const t = typeof v.v
  const typeOk =
    (v.t === 's' && t === 'string') ||
    (v.t === 'k' && t === 'string' && v.v.length > 0) ||
    (v.t === 'z' && t === 'boolean') ||
    ((v.t === 'b' || v.t === 'i') && t === 'number' && Number.isFinite(v.v))
  if (!typeOk) {
    // 键位空串单独说一句：它类型没错，但界面会印出一个空指标格
    problems.push(
      v.t === 'k' && t === 'string'
        ? `${where}: 文案键是空串（前端会直接印键名或留空格）`
        : `${where}: t=${v.t} 与 v 的类型 ${t} 不匹配`,
    )
  }
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
    if (!Array.isArray(item.tags)) {
      problems.push(`${where}: tags 不是数组`)
    } else {
      for (const t of item.tags) checkTag(t, `${where}.tag`, problems)
    }
  }
  // 行的四个展示位（P2a）：可选，给了就必须是合法文案对象。
  for (const k of ['label', 'short_label', 'group_label', 'kind_label']) {
    if (item[k] !== undefined && item[k] !== null) {
      checkText(item[k], `${where}.${k}`, problems)
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

  const artifacts = item.artifact_entries
  const artifactTotal = metrics?.artifacts
  if (artifacts !== undefined && !Array.isArray(artifacts)) {
    problems.push(`${where}: artifact_entries 不是数组`)
  }
  if (artifactTotal !== undefined && (!Array.isArray(artifacts) || artifacts.length === 0)) {
    problems.push(`${where}: 有产物指标但缺少完整产物集合`)
  }
  if (Array.isArray(artifacts) && artifacts.length > 0) {
    const paths = new Set()
    let total = 0
    for (const entry of artifacts) {
      if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
        problems.push(`${where}: 产物条目不是对象`)
        continue
      }
      const rel = entry.relative_path
      if (typeof rel !== 'string' || rel.length === 0 ||
          /^(?:[\\/]|[A-Za-z]:)/.test(rel) || rel.split(/[\\/]/).includes('..')) {
        problems.push(`${where}: 产物路径必须是仓库内非空相对路径`)
      } else if (paths.has(rel)) {
        problems.push(`${where}: 产物路径重复`)
      } else {
        paths.add(rel)
      }
      if (!Number.isSafeInteger(entry.size_bytes) || entry.size_bytes < 0) {
        problems.push(`${where}: 产物 size_bytes 不是非负安全整数`)
      } else {
        total += entry.size_bytes
      }
    }
    if (!artifactTotal || artifactTotal.t !== 'b' || artifactTotal.v !== total) {
      problems.push(`${where}: 产物合计与 metrics.artifacts 不一致`)
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
  if (typeof g.id !== 'string' || !g.id) {
    problems.push(`${where}: id 为空或不是字符串`)
  }
  checkText(g.label, `${where}.label`, problems)
  checkText(g.summary, `${where}.summary`, problems)

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
      if (typeof f.key !== 'string' || !f.key) problems.push(`${where}: field.key 为空`)
      checkText(f.label, `${where}.field.label`, problems)
      for (const k of ['numeric', 'temporal']) {
        if (f[k] !== undefined && typeof f[k] !== 'boolean') {
          problems.push(`${where}: field.${k} 不是布尔`)
        }
      }
    }
  }

  if (g.error !== undefined && g.error !== null) {
    checkText(g.error, `${where}.error`, problems)
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
    if (!Array.isArray(g.notes)) {
      problems.push(`${where}: notes 不是数组`)
    } else {
      for (const n of g.notes) checkText(n, `${where}.note`, problems)
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
  // 展示文案的三种形态各留一个合法样本：成品（品牌名）、键、带参键。
  const goodItem = {
    id: '/tmp/a',
    name: 'repo-a',
    path: '/tmp/a',
    label: { t: 'k', v: 'data.item.cargoRegistry' },
    tags: [{ key: 'cleanable', label: { t: 'k', v: 'data.tag.cleanable' } }],
    metrics: { size: { t: 'b', v: 100 }, last_commit: { t: 'i', v: 1700000000 }, branch: { t: 'k', v: 'data.value.noCommit' } },
    cleanable: true,
    reclaimable: 50,
  }
  const good = {
    id: 'g',
    label: { t: 'k', v: 'data.group.repos' },
    summary: { t: 'f', v: { k: 'data.summary.repos', a: { count: { t: 'n', v: 1 }, total: { t: 'b', v: 100 } } } },
    fields: [{ key: 'size', label: { t: 'k', v: 'data.field.size' }, numeric: true }],
    items: [goodItem],
    notes: [{ t: 'r', v: '只读文件名，不读内容' }],
  }
  const artifactEntry = { relative_path: 'packages/repo-a/node_modules', size_bytes: 100 }
  const artifactItem = {
    ...goodItem,
    metrics: { ...goodItem.metrics, artifacts: { t: 'b', v: 100 } },
    artifact_entries: [artifactEntry],
  }
  const artifactGroup = { ...good, id: 'repos', items: [artifactItem] }
  const withArtifactEntries = (entries) => ({
    ...artifactGroup,
    items: [{ ...artifactItem, artifact_entries: entries }],
  })
  const cases = [
    ['产物集合不是数组', withArtifactEntries({})],
    ['产物集合为 null', withArtifactEntries(null)],
    ['有产物指标但缺集合', withArtifactEntries(undefined)],
    ['有产物指标但集合为空', withArtifactEntries([])],
    ['产物条目为 null', withArtifactEntries([null])],
    ['产物相对路径缺失', withArtifactEntries([{ size_bytes: 100 }])],
    ['产物相对路径为空', withArtifactEntries([{ ...artifactEntry, relative_path: '' }])],
    ['产物路径是 Unix 绝对路径', withArtifactEntries([{ ...artifactEntry, relative_path: '/tmp/target' }])],
    ['产物路径是 Windows 绝对路径', withArtifactEntries([{ ...artifactEntry, relative_path: 'C:\\target' }])],
    ['产物路径是 Windows 盘符相对路径', withArtifactEntries([{ ...artifactEntry, relative_path: 'C:target' }])],
    ['产物路径跳出仓库', withArtifactEntries([{ ...artifactEntry, relative_path: 'packages/../target' }])],
    ['产物 Windows 路径跳出仓库', withArtifactEntries([{ ...artifactEntry, relative_path: '..\\target' }])],
    ['产物路径重复', withArtifactEntries([artifactEntry, { ...artifactEntry, size_bytes: 0 }])],
    ['产物字节缺失', withArtifactEntries([{ relative_path: 'target' }])],
    ['产物字节为字符串', withArtifactEntries([{ ...artifactEntry, size_bytes: '100' }])],
    ['产物字节为负', withArtifactEntries([{ ...artifactEntry, size_bytes: -1 }])],
    ['产物字节有小数', withArtifactEntries([{ ...artifactEntry, size_bytes: 100.1 }])],
    ['产物字节为 NaN', withArtifactEntries([{ ...artifactEntry, size_bytes: NaN }])],
    ['产物字节为无穷', withArtifactEntries([{ ...artifactEntry, size_bytes: Infinity }])],
    ['产物合计不一致', withArtifactEntries([{ ...artifactEntry, size_bytes: 99 }])],
    ['产物集合缺合计指标', { ...artifactGroup, items: [{ ...artifactItem, metrics: goodItem.metrics }] }],
    ['字段超过三个', { ...good, fields: [{ key: 'a', label: { t: 'r', v: 'a' } }, { key: 'b', label: { t: 'r', v: 'b' } }, { key: 'c', label: { t: 'r', v: 'c' } }, { key: 'd', label: { t: 'r', v: 'd' } }] }],
    ['未知指标标签', { ...good, items: [{ ...goodItem, metrics: { size: { t: 'x', v: 1 } } }] }],
    ['标签与值类型不符', { ...good, items: [{ ...goodItem, metrics: { size: { t: 'b', v: '100' } } }] }],
    ['字节数为负', { ...good, items: [{ ...goodItem, metrics: { size: { t: 'b', v: -1 } } }] }],
    ['回收量超过体积', { ...good, items: [{ ...goodItem, reclaimable: 101 }] }],
    ['有回收量未标 cleanable', { ...good, items: [{ ...goodItem, cleanable: false }] }],
    ['分节前缀为空', { ...good, items: [{ ...goodItem, name: ' · a' }] }],
    ['出错却有明细', { ...good, error: { t: 'r', v: 'docker 未运行' } }],
    ['detail 值不是字符串', { ...good, items: [{ ...goodItem, detail: { 清理方式: 42 } }] }],
    ['条目缺 name', { ...good, items: [{ ...goodItem, name: '' }] }],
    ['空指标键', { ...good, items: [{ ...goodItem, metrics: { '': { t: 'b', v: 1 } } }] }],
    ['顶层不是数组', { nope: true }],
    // ---- P2a：展示文案从成品串换成 {t,v} 对象，形状坏了要在契约层就抓住 ----
    ['分组名是成品字符串', { ...good, label: '测试分组' }],
    ['分组名标签不认识', { ...good, label: { t: 'x', v: 'data.group.repos' } }],
    ['分组名键为空', { ...good, label: { t: 'k', v: '' } }],
    ['分组名为 null', { ...good, label: null }],
    ['带参文案缺键', { ...good, summary: { t: 'f', v: { a: {} } } }],
    ['带参文案参数表不是对象', { ...good, summary: { t: 'f', v: { k: 'data.summary.repos', a: [] } } }],
    ['带参文案参数标签不认识', { ...good, summary: { t: 'f', v: { k: 'data.summary.repos', a: { count: { t: 'x', v: 1 } } } } }],
    ['带参文案数字参数是字符串', { ...good, summary: { t: 'f', v: { k: 'data.summary.repos', a: { count: { t: 'n', v: '1' } } } } }],
    ['带参文案字节参数为负', { ...good, summary: { t: 'f', v: { k: 'data.summary.repos', a: { total: { t: 'b', v: -1 } } } } }],
    ['脚注是成品字符串', { ...good, notes: ['只读文件名'] }],
    ['字段名不是文案对象', { ...good, fields: [{ key: 'size', label: '体积' }] }],
    ['指标键标签值为空串', { ...good, items: [{ ...goodItem, metrics: { branch: { t: 'k', v: '' } } }] }],
    ['标签不是对象', { ...good, items: [{ ...goodItem, tags: ['可清理'] }] }],
    ['标签缺 key', { ...good, items: [{ ...goodItem, tags: [{ label: { t: 'r', v: '可清理' } }] }] }],
    ['标签展示位不是文案对象', { ...good, items: [{ ...goodItem, tags: [{ key: 'cleanable', label: '可清理' }] }] }],
    ['行的展示位不是文案对象', { ...good, items: [{ ...goodItem, short_label: 'Cargo 依赖缓存' }] }],
  ]

  const fullEntries = Array.from({ length: 14 }, (_, i) => ({
    relative_path: `packages/repo-${i}/node_modules`,
    size_bytes: i * 1025,
  }))
  fullEntries[0].relative_path = 'packages/空 格 · 分隔\n换行/node_modules'
  fullEntries[1].relative_path = 'packages\\repo-a\\node_modules'
  const full = {
    ...artifactGroup,
    items: [{
      ...artifactItem,
      metrics: { ...goodItem.metrics, artifacts: { t: 'b', v: fullEntries.reduce((n, e) => n + e.size_bytes, 0) } },
      artifact_entries: fullEntries,
    }],
  }
  const zero = {
    ...artifactGroup,
    items: [{ ...artifactItem, metrics: { ...goodItem.metrics, artifacts: { t: 'b', v: 0 } }, artifact_entries: [{ ...artifactEntry, size_bytes: 0 }] }],
  }
  const validCases = [
    ['基础样本', good],
    ['完整产物集合', artifactGroup],
    ['空产物集合省略指标', { ...good, items: [{ ...goodItem, artifact_entries: [] }] }],
    ['零字节产物', zero],
    ['超过12项与特殊路径往返', JSON.parse(JSON.stringify(full))],
  ]
  let failed = 0
  for (const [name, data] of validCases) {
    const result = validateSnapshot([data])
    if (!result.ok) {
      console.log(`FAIL ${name}: ${result.problems.join('；')}`)
      failed++
    } else {
      console.log(`PASS ${name}`)
    }
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
  const soft = validateSnapshot([{ ...good, fields: [{ key: 'ghost', label: { t: 'k', v: 'data.field.ghost' } }] }])
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

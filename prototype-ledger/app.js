/* 活页台账 · 动效原型
   数据契约、表格口径、检查器内容都照抄 app；这里多出来的只有动效与演示控制。
   原型不触发任何真实操作：清理、揭示、清缓存一律只给 toast。 */

(() => {
  "use strict";

  const DATA = window.PROTO_DATA;
  const PREFIX_LABEL = { toolchain: "栈", ai_tools: "工具", java: "组件", local_models: "来源" };
  const NUMERIC_KEYS = new Set(["size", "reclaimable", "share", "count"]);
  const MONO_KEYS = new Set(["branch"]);
  const CONTENT_KEYS = new Set(["kind"]);
  const SCAN_ORDER = ["repos", "toolchain", "ai_tools", "local_models"];
  const SCAN_DELTA = {
    repos: 335544320,
    toolchain: -188743680,
    ai_tools: 125829120,
    local_models: 167772160,
  };
  const REVEAL_LABEL = "在访达中显示";

  const $ = (sel, root = document) => root.querySelector(sel);
  const content = $("#content");
  const scroll = $("#scroll");
  const inspector = $("#inspector");
  const toolbar = $("#toolbar");
  const statusbar = $("#statusbar");
  const scanbar = $("#scanbar");
  const scanfill = $("#scanfill");
  const scannib = $("#scannib");
  const nav = $("#nav");
  const sidebar = $("#sidebar");
  const navScroll = $("#nav-scroll");
  const marker = $("#side-marker");
  const popoverRoot = $("#popover-root");
  const ctxmenu = $("#ctxmenu");
  const toasts = $("#toasts");

  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let data = structuredClone(DATA);
  let scanToken = 0;
  let markerY = 0;
  let ctxTarget = null;
  let ctxHideTimer = 0;
  let checkTimer = 0;

  const state = {
    route: "overview",
    sortKey: "size",
    sortDesc: true,
    dims: {},
    selected: null,
    filterOpen: false,
    inspectorOpen: window.innerWidth >= 1040,
    rulesOpen: false,
    scanning: false,
    scanDone: 0,
    scanTotal: SCAN_ORDER.length,
    savedAt: DATA.saved_at,
    theme: document.documentElement.classList.contains("dark") ? "dark" : "light",
    motion: reduced ? 0 : 1,
    firstPaint: true,
  };

  const sideValues = new Map();

  // ---------------- 基础口径（逐条照抄 src/lib/types.ts + views/Assets.vue） ----------------

  function esc(s) {
    return String(s).replace(/[&<>"']/g, (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c],
    );
  }

  function formatBytes(b) {
    if (b <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = b;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    if (i === 0) return `${b} B`;
    const s = v.toFixed(1);
    return `${s.endsWith(".0") ? s.slice(0, -2) : s} ${units[i]}`;
  }

  function bytesParts(b) {
    const s = formatBytes(b);
    const i = s.lastIndexOf(" ");
    return { n: s.slice(0, i), u: s.slice(i + 1) };
  }

  function byteInner(b) {
    const p = bytesParts(b);
    return `<span class="bv-n">${p.n}</span><span class="bv-u">${p.u}</span>`;
  }

  function byteHTML(b, cls = "") {
    return `<span class="bv${cls ? ` ${cls}` : ""}">${byteInner(b)}</span>`;
  }

  function formatAgo(unixSeconds) {
    const diff = Date.now() / 1000 - unixSeconds;
    if (diff < 0) return "刚刚";
    const d = Math.floor(diff / 86400);
    if (d === 0) {
      const h = Math.floor(diff / 3600);
      if (h === 0) {
        const m = Math.floor(diff / 60);
        return m <= 0 ? "刚刚" : `${m} 分钟前`;
      }
      return `${h} 小时前`;
    }
    if (d < 30) return `${d} 天前`;
    const mo = Math.floor(d / 30);
    if (mo < 12) return `${mo} 个月前`;
    return `${Math.floor(mo / 12)} 年前`;
  }

  const ic = (name, cls = "") =>
    `<svg class="icon${cls ? ` ${cls}` : ""}" aria-hidden="true"><use href="#i-${name}"/></svg>`;

  function sizeOf(it) {
    const m = it.metrics.size;
    return m && (m.t === "b" || m.t === "i") ? m.v : 0;
  }

  function reclaimOf(it) {
    if (it.reclaimable != null) return it.reclaimable;
    const m = it.metrics.reclaimable;
    return m && (m.t === "b" || m.t === "i") ? m.v : 0;
  }

  function artifactBytes(it) {
    const m = it.metrics.artifacts;
    return m && m.t === "b" ? m.v : 0;
  }

  const groupSize = (g) => g.items.reduce((s, it) => s + sizeOf(it), 0);
  const groupReclaimable = (g) =>
    g.items.reduce((s, it) => (it.cleanable ? s + reclaimOf(it) : s), 0);

  function currentGroup() {
    return data.groups.find((g) => g.id === state.route) ?? null;
  }

  function machineStats() {
    const real = data.groups.filter((g) => g.id !== "overview");
    const total = real.reduce((s, g) => s + groupSize(g), 0);
    const reclaim = real.reduce((s, g) => s + groupReclaimable(g), 0);
    const repos = real.find((g) => g.id === "repos");
    const week = 7 * 86400;
    const now = Date.now() / 1000;
    const recent = repos
      ? repos.items.filter((it) => {
          const m = it.metrics.last_commit;
          return m && (m.t === "b" || m.t === "i") && now - m.v < week;
        }).length
      : 0;
    return { total, reclaim, count: real.length, recent };
  }

  function cell(it, key, temporal) {
    const m = it.metrics[key];
    if (!m) return "—";
    if (key === "size") {
      if ((m.t === "b" || m.t === "i") && m.v <= 0) return "—";
      return m.t === "b" ? formatBytes(m.v) : m.v.toLocaleString("zh-CN");
    }
    if (m.t === "z") return m.v ? "是" : "否";
    if (m.t === "i" || m.t === "b") return temporal ? formatAgo(m.v) : m.v.toLocaleString("zh-CN");
    return m.v;
  }

  function splitReclaimOf(g) {
    return g.items.some((it) => {
      const r = reclaimOf(it);
      return r > 0 && r !== sizeOf(it);
    });
  }

  function colsFor(g) {
    const out = [{ key: "name", label: "名称" }];
    for (const f of g.fields.slice(0, 3)) {
      out.push({ key: f.key, label: f.label, temporal: f.temporal ?? undefined });
      if (f.key === "size" && g.id === "overview") out.push({ key: "share", label: "占比" });
      if (
        f.key === "size" &&
        groupReclaimable(g) > 0 &&
        splitReclaimOf(g) &&
        !g.fields.some((x) => x.key === "reclaimable")
      ) {
        out.push({ key: "reclaimable", label: "可回收" });
      }
    }
    return out.map((c) => {
      let width = "minmax(0,1.2fr)";
      let align = "text-left";
      if (c.key === "name") width = "minmax(0,2.2fr)";
      else if (NUMERIC_KEYS.has(c.key)) {
        width = "minmax(0,1fr)";
        align = "text-right";
      } else if (CONTENT_KEYS.has(c.key)) width = "minmax(auto,max-content)";
      else if (c.temporal) {
        width = "minmax(0,0.9fr)";
        align = "text-right";
      }
      return { ...c, width, align, mono: MONO_KEYS.has(c.key) };
    });
  }

  function sortValue(it, key) {
    if (key === "name") return it.name;
    if (key === "reclaimable") return reclaimOf(it);
    if (key === "share") return sizeOf(it);
    const m = it.metrics[key];
    if (!m) return "";
    if (m.t === "i" || m.t === "b") return m.v;
    if (m.t === "z") return m.v ? 1 : 0;
    return m.v;
  }

  function toggleSort(key) {
    if (state.sortKey === key) state.sortDesc = !state.sortDesc;
    else {
      state.sortKey = key;
      state.sortDesc = key !== "name";
    }
  }

  function matchDim(it, key, val) {
    if (key === "prefix") return it.group_key === val;
    if (key === "kind") {
      const m = it.metrics.kind;
      return !!m && m.t === "s" && m.v === val;
    }
    if (key === "tag") return it.tags.includes(val);
    return true;
  }

  function dimensionsFor(g) {
    if (!g || !g.items.length) return [];
    const out = [];
    const pl = PREFIX_LABEL[g.id];
    if (pl) {
      const vals = new Set();
      for (const it of g.items) if (it.group_key) vals.add(it.group_key);
      if (vals.size >= 2) out.push({ key: "prefix", label: pl, options: [...vals].sort() });
    }
    const kinds = new Set();
    for (const it of g.items) {
      const m = it.metrics.kind;
      if (m && m.t === "s") kinds.add(m.v);
    }
    if (kinds.size >= 2) out.push({ key: "kind", label: "类别", options: [...kinds].sort() });
    const tags = new Set();
    for (const it of g.items) for (const t of it.tags) tags.add(t);
    if (tags.size >= 2) out.push({ key: "tag", label: "标签", options: [...tags].sort() });
    return out;
  }

  const anyFilter = () => Object.values(state.dims).some(Boolean);
  const activeDims = () => Object.values(state.dims).filter(Boolean).length;

  function optionCount(g, dimKey, value) {
    const rest = Object.entries(state.dims).filter(([k, v]) => k !== dimKey && v);
    return g.items.filter(
      (it) =>
        rest.every(([k, v]) => matchDim(it, k, v)) &&
        (value === null || matchDim(it, dimKey, value)),
    ).length;
  }

  function visibleRows(g) {
    let rows = g.items.filter((it) => {
      const active = Object.entries(state.dims).filter(([, v]) => v);
      return active.every(([k, v]) => matchDim(it, k, v));
    });
    if (state.sortKey) {
      const dir = state.sortDesc ? -1 : 1;
      rows = [...rows].sort((a, b) => {
        const va = sortValue(a, state.sortKey);
        const vb = sortValue(b, state.sortKey);
        if (typeof va === "number" && typeof vb === "number") return (va - vb) * dir;
        return String(va).localeCompare(String(vb), "zh-CN") * dir;
      });
    }
    return rows;
  }

  function rowName(it) {
    const prefix = it.group_key ? `${it.group_key} · ` : "";
    return it.name.startsWith(prefix) ? it.name.slice(prefix.length) : it.name;
  }

  function suppressedTags(rows) {
    const out = new Set();
    if (rows.length < 6) return out;
    const counts = new Map();
    for (const it of rows) for (const t of it.tags) counts.set(t, (counts.get(t) ?? 0) + 1);
    for (const [t, n] of counts) if (n / rows.length >= 0.7) out.add(t);
    return out;
  }

  function visibleTags(it, suppressed) {
    return it.tags.filter((t) => {
      if (suppressed.has(t)) return false;
      if (t === "可清理" && reclaimOf(it) > 0) return false;
      if (t === "大") return false;
      return true;
    });
  }

  function sectionsFor(g, rows) {
    if (!g || !rows.length || !rows.every((it) => it.group_key)) {
      return [{ key: null, rows }];
    }
    const map = new Map();
    for (const it of rows) {
      if (!map.has(it.group_key)) map.set(it.group_key, []);
      map.get(it.group_key).push(it);
    }
    return [...map.entries()].map(([key, rs]) => ({ key, rows: rs }));
  }

  function shareLabel(it) {
    const st = machineStats();
    const share = st.total > 0 ? sizeOf(it) / st.total : 0;
    if (share <= 0) return "";
    if (share >= 0.1) return `${Math.round(share * 100)}%`;
    if (share >= 0.005) return `${(share * 100).toFixed(1)}%`;
    return "<0.1%";
  }

  function isLocalPath(p) {
    return p.startsWith("/") || /^[A-Za-z]:[\\/]/.test(p);
  }

  // ---------------- 动效工具 ----------------

  function ms(base) {
    return Math.max(0, Math.round(base * state.motion));
  }

  function applyMotion() {
    document.documentElement.style.setProperty("--motion-scale", String(state.motion));
    const label = $("#rail-motion");
    if (label) label.textContent = state.motion === 0 ? "关" : `${state.motion}×`;
  }

  function tweenByteEl(el, from, to, duration = 520) {
    if (!el) return;
    if (state.motion === 0 || from === to || !Number.isFinite(from)) {
      el.innerHTML = byteInner(to);
      return;
    }
    const dur = Math.max(1, duration * state.motion);
    const start = performance.now();
    const frame = (now) => {
      const t = Math.min(1, (now - start) / dur);
      const e = 1 - Math.pow(1 - t, 3);
      el.innerHTML = byteInner(from + (to - from) * e);
      if (t < 1) requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }

  function pageEnter() {
    content.classList.remove("page-enter");
    void content.offsetWidth;
    content.classList.add("page-enter");
  }

  function playFlip(prev) {
    if (state.motion === 0) return;
    content.querySelectorAll(".row[data-row-id]").forEach((el) => {
      const before = prev.get(el.dataset.rowId);
      if (before == null) {
        el.animate([{ opacity: 0 }, { opacity: 1 }], {
          duration: ms(150),
          easing: "cubic-bezier(0.22,1,0.36,1)",
        });
        return;
      }
      const dy = before - el.getBoundingClientRect().top;
      if (Math.abs(dy) < 0.5) return;
      el.animate([{ transform: `translateY(${dy}px)` }, { transform: "none" }], {
        duration: ms(280),
        easing: "cubic-bezier(0.22,1,0.36,1)",
      });
    });
  }

  function rerenderWithFlip() {
    const prev = new Map();
    content.querySelectorAll(".row[data-row-id]").forEach((el) => {
      prev.set(el.dataset.rowId, el.getBoundingClientRect().top);
    });
    renderContent();
    applySelection();
    playFlip(prev);
  }

  // ---------------- toast ----------------

  function toast(message, kind = "check") {
    while (toasts.children.length >= 3) toasts.firstElementChild.remove();
    const el = document.createElement("div");
    el.className = `toast ${kind === "check" ? "success" : "info"}`;
    el.innerHTML = `${ic(kind === "check" ? "check" : "dot")}<span>${esc(message)}</span>`;
    toasts.appendChild(el);
    if (state.motion > 0) {
      el.animate(
        [
          { opacity: 0, transform: "translateY(-6px) scale(.98)" },
          { opacity: 1, transform: "none" },
        ],
        { duration: ms(180), easing: "cubic-bezier(0.22,1,0.36,1)" },
      );
    }
    setTimeout(
      () => {
        if (state.motion === 0) return el.remove();
        const out = el.animate(
          [
            { opacity: 1, transform: "none" },
            { opacity: 0, transform: "translateY(-4px)" },
          ],
          { duration: ms(160), easing: "ease-in", fill: "forwards" },
        );
        out.onfinish = () => el.remove();
      },
      2200 + ms(400),
    );
  }

  // ---------------- 侧栏 ----------------

  function renderSidebar() {
    const real = data.groups.filter((g) => g.id !== "overview");
    const total = machineStats().total;
    sideValues.clear();
    sideValues.set("__total", total);
    nav.innerHTML = `
      <button class="side-item" data-route="overview" title="总览">
        <span class="side-name">总览</span>
        <span class="side-size" data-side-size="__total"><span class="bv">${byteInner(total)}</span></span>
      </button>
      <div class="side-label">分组</div>
      ${real
        .map((g) => {
          const v = groupSize(g);
          sideValues.set(g.id, v);
          return `<button class="side-item" data-route="${esc(g.id)}" title="${esc(g.label)}">
            <span class="side-name">${esc(g.label)}</span>
            <span class="side-size" data-side-size="${esc(g.id)}">${
              v > 0 ? `<span class="bv">${byteInner(v)}</span>` : "—"
            }</span>
          </button>`;
        })
        .join("")}`;
    updateSidebarSelection(false);
  }

  function updateSidebarSizes(from = null) {
    document.querySelectorAll("[data-side-size]").forEach((el) => {
      const key = el.dataset.sideSize;
      const g = key === "__total" ? null : data.groups.find((x) => x.id === key);
      const to = key === "__total" ? machineStats().total : g ? groupSize(g) : 0;
      const old = from && from[key] != null ? from[key] : sideValues.get(key) ?? to;
      const bv = el.querySelector(".bv");
      if (to > 0) {
        if (!bv) el.innerHTML = `<span class="bv">${byteInner(to)}</span>`;
        tweenByteEl(el.querySelector(".bv"), old, to, 480);
      } else {
        el.textContent = "—";
      }
      sideValues.set(key, to);
    });
  }

  function updateSidebarSelection(animate = true) {
    document.querySelectorAll("[data-route]").forEach((el) => {
      const active = el.dataset.route === state.route;
      el.classList.toggle("selected", active);
      if (active) el.setAttribute("aria-current", "page");
      else el.removeAttribute("aria-current");
    });
    placeMarker(animate);
  }

  function placeMarker(animate) {
    const el = document.querySelector(".side-item.selected");
    if (!el) return;
    const sr = sidebar.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    const to = r.top - sr.top + r.height / 2 - 9;
    if (!animate || state.motion === 0 || !markerY) {
      marker.style.transform = `translateY(${to}px)`;
      markerY = to;
      return;
    }
    const from = markerY;
    const dur = ms(300);
    marker.animate(
      [
        { transform: `translateY(${from}px)` },
        { transform: `translateY(${(from + to) / 2}px) scaleY(1.35)`, offset: 0.55 },
        { transform: `translateY(${to}px)` },
      ],
      { duration: dur, easing: "cubic-bezier(0.22,1,0.36,1)" },
    );
    marker.style.transform = `translateY(${to}px)`;
    markerY = to;
  }

  // ---------------- 工具栏 ----------------

  function renderToolbar() {
    if (state.route === "settings") {
      toolbar.innerHTML = `<h1>设置</h1><div class="toolbar-spacer"></div>`;
      return;
    }
    const g = currentGroup();
    const isOverview = state.route === "overview";
    const hasDims = dimensionsFor(g).length > 0;
    const active = activeDims();
    toolbar.innerHTML = `
      <h1>${esc(g?.label ?? "总览")}</h1>
      <div class="toolbar-spacer"></div>
      ${
        hasDims
          ? `<button class="tool-btn${active ? " active" : ""}" data-action="filter" title="${
              active ? `筛选（已生效 ${active} 项）` : "筛选"
            }" aria-label="筛选">
              ${ic("filter")}${active ? `<span class="badge">${active}</span>` : ""}
            </button>`
          : ""
      }
      <button class="tool-btn icon-only" data-action="theme" title="${
        state.theme === "dark" ? "切换到浅色" : "切换到深色"
      }" aria-label="${state.theme === "dark" ? "切换到浅色模式" : "切换到深色模式"}">
        ${ic(state.theme === "dark" ? "sun" : "moon")}
      </button>
      ${
        isOverview
          ? ""
          : `<button class="tool-btn icon-only" data-action="inspector" title="${
              state.inspectorOpen ? "隐藏检查器" : "显示检查器"
            }" aria-label="${state.inspectorOpen ? "隐藏检查器" : "显示检查器"}">
              ${ic("panel-left", state.inspectorOpen ? "" : "dim")}
            </button>`
      }
      <button class="tool-btn rescan" data-action="rescan" ${
        state.scanning ? "disabled" : ""
      } title="重新扫描本机资产">
        ${ic("refresh", state.scanning ? "spin" : "")}
        <span>${state.scanning ? `扫描中 ${state.scanDone}/${state.scanTotal}` : "重扫"}</span>
      </button>`;
  }

  // ---------------- 筛选菜单 ----------------

  function openFilterMenu() {
    closeFilterMenu();
    const g = currentGroup();
    const dims = dimensionsFor(g);
    if (!dims.length) return;
    const menu = document.createElement("div");
    menu.className = "menu";
    menu.id = "filter-menu";
    menu.setAttribute("role", "menu");
    menu.innerHTML = `
      ${dims
        .map(
          (d, i) => `
        ${i ? '<div class="menu-sep"></div>' : ""}
        <div class="menu-label">${esc(d.label)}</div>
        <button class="menu-item${!state.dims[d.key] ? " active" : ""}" data-fdim="${esc(
          d.key,
        )}" data-fval="">
          <span class="label"><span class="dot"></span>全部</span><span class="count">${optionCount(
            g,
            d.key,
            null,
          )}</span>
        </button>
        ${d.options
          .map(
            (v) => `<button class="menu-item${
              state.dims[d.key] === v ? " active" : ""
            }" data-fdim="${esc(d.key)}" data-fval="${esc(v)}">
              <span class="label"><span class="dot"></span>${esc(v)}</span><span class="count">${optionCount(
                g,
                d.key,
                v,
              )}</span>
            </button>`,
          )
          .join("")}`,
        )
        .join("")}
      ${
        anyFilter()
          ? `<div class="menu-sep"></div><button class="menu-item menu-clear" data-clear-filters>
              <span class="label">清除筛选条件</span>
            </button>`
          : ""
      }`;
    popoverRoot.appendChild(menu);
    const btn = toolbar.querySelector('[data-action="filter"]');
    const r = btn.getBoundingClientRect();
    menu.style.top = `${r.bottom + 4}px`;
    const left = Math.min(r.right - menu.offsetWidth, window.innerWidth - menu.offsetWidth - 8);
    menu.style.left = `${Math.max(8, left)}px`;
    state.filterOpen = true;
    requestAnimationFrame(() => {
      menu.classList.add("open");
      menu.querySelector(".menu-item")?.focus({ preventScroll: true });
    });
  }

  function closeFilterMenu() {
    state.filterOpen = false;
    const menu = $("#filter-menu");
    if (!menu) return;
    menu.classList.remove("open");
    setTimeout(() => menu.remove(), ms(150));
  }

  function toggleFilterMenu() {
    if (state.filterOpen) closeFilterMenu();
    else openFilterMenu();
  }

  // ---------------- 内容 ----------------

  function renderTableHead(meta) {
    const grid = meta.map((c) => c.width).join(" ");
    return `<div class="table-head" style="grid-template-columns:${grid}">
      ${meta
        .map((c, i) => {
          const arrow = state.sortKey === c.key ? (state.sortDesc ? " ↓" : " ↑") : "";
          return `<button data-sort="${esc(c.key)}" class="${
            c.align === "text-right" ? "right" : ""
          }" aria-label="按${esc(c.label)}排序">${esc(c.label)}${arrow}</button>`;
        })
        .join("")}
    </div>`;
  }

  function renderRows(rows, g, meta, suppressed, flashId, overview) {
    const split = splitReclaimOf(g);
    const grid = meta.map((c) => c.width).join(" ");
    return rows
      .map((it) => {
        const tags = visibleTags(it, suppressed);
        const reclaim = reclaimOf(it);
        const cells = meta
          .slice(1)
          .map((c) => {
            let body = "";
            if (c.key === "size") {
              const green = !split && reclaim > 0;
              body = byteHTML(sizeOf(it), green ? "reclaim" : "");
            } else if (c.key === "reclaimable") {
              body = reclaim > 0 ? byteHTML(reclaim, "reclaim") : "";
            } else if (c.key === "share") {
              body = esc(shareLabel(it));
            } else {
              body = esc(cell(it, c.key, c.temporal));
            }
            const cls = ["cell", c.align === "text-right" ? "right" : "", c.temporal ? "muted" : "", c.mono ? "mono" : ""]
              .filter(Boolean)
              .join(" ");
            return `<span class="${cls}">${body}</span>`;
          })
          .join("");
        return `<button class="row${overview ? " overview-row" : ""}${
          state.selected === it.id ? " selected" : ""
        }${flashId === it.id ? " flash" : ""}" data-row-id="${esc(it.id)}" style="grid-template-columns:${grid}"${
          overview ? "" : ` title="${esc(it.path)}"`
        }>
          <span class="row-wash" aria-hidden="true"></span>
          <span class="row-tick" aria-hidden="true"></span>
          <span class="row-name"><span class="text">${esc(rowName(it))}</span>${
            tags.length ? `<span class="row-tags">${esc(tags.join(" · "))}</span>` : ""
          }</span>
          ${cells}
        </button>`;
      })
      .join("");
  }

  function renderContent({ enter = false, drawRule = false, flashId = null } = {}) {
    if (state.route === "settings") {
      content.innerHTML = settingsHTML();
      pageEnter();
      return;
    }
    const g = currentGroup();
    if (!g) {
      content.innerHTML = `<p class="empty">尚无数据</p>`;
      return;
    }
    const overview = g.id === "overview";
    const meta = colsFor(g);
    const rows = visibleRows(g);
    const st = machineStats();
    const suppressed = suppressedTags(rows);
    const secs = sectionsFor(g, rows);
    const hero =
      overview && st.total > 0
        ? `<div class="hero">
            <div class="hero-row">
              <div>
                <div class="hero-label">本机合计</div>
                <div class="hero-value"><span class="bv" data-hero>${byteInner(st.total)}</span></div>
              </div>
              <p class="hero-summary">可回收 ${byteHTML(st.reclaim, "reclaim")} · ${
                st.count
              } 个分组 · 近 7 天 ${st.recent} 个仓库有提交</p>
            </div>
            <div class="head-rule${drawRule ? " draw" : ""}"></div>
          </div>`
        : "";
    const table = rows.length
      ? `${renderTableHead(meta)}
        ${secs
          .map(
            (sec) => `<section class="section">
              ${
                sec.key
                  ? `<div class="section-head">
                      <span class="name">${esc(sec.key)}</span>
                      <span class="totals">${byteHTML(
                        sec.rows.reduce((s, it) => s + sizeOf(it), 0),
                      )}${
                        sec.rows.reduce((s, it) => s + (it.cleanable ? reclaimOf(it) : 0), 0) > 0
                          ? ` · 可回收 ${byteHTML(
                              sec.rows.reduce(
                                (s, it) => s + (it.cleanable ? reclaimOf(it) : 0),
                                0,
                              ),
                              "reclaim",
                            )}`
                          : ""
                      }</span>
                    </div>`
                  : ""
              }
              <div class="section-body">${renderRows(
                sec.rows,
                g,
                meta,
                suppressed,
                flashId,
                overview,
              )}</div>
            </section>`,
          )
          .join("")}`
      : `<div class="empty"><p>无匹配项</p>${
          anyFilter() ? `<button data-action="clear-filters">清除筛选条件</button>` : ""
        }</div>`;

    content.innerHTML = hero + table;
    if (enter) pageEnter();
  }

  function applySelection() {
    content.querySelectorAll(".row[data-row-id]").forEach((el) => {
      el.classList.toggle("selected", el.dataset.rowId === state.selected);
    });
  }

  // ---------------- 检查器 ----------------

  function selectedRow() {
    const g = currentGroup();
    if (!g || !state.selected) return null;
    return g.items.find((it) => it.id === state.selected) ?? null;
  }

  function renderInspector({ pageIn = false } = {}) {
    const g = currentGroup();
    if (!g || g.id === "overview") {
      inspector.hidden = true;
      inspector.inert = true;
      inspector.innerHTML = "";
      return;
    }
    inspector.hidden = false;
    inspector.inert = !state.inspectorOpen;
    inspector.classList.toggle("closed", !state.inspectorOpen);
    const it = selectedRow();
    let inner = "";
    if (it) {
      const split = splitReclaimOf(g);
      const path = isLocalPath(it.path);
      const reclaim = reclaimOf(it);
      const metrics = g.fields
        .slice(0, 3)
        .map((f) => {
          const green = f.key === "size" && !split && reclaim > 0;
          return `<div class="kv">
            <dt>${esc(f.label)}</dt>
            <dd class="${green ? "reclaim" : ""}">${esc(cell(it, f.key, f.temporal))}</dd>
          </div>`;
        })
        .join("");
      const splitRow =
        split && reclaim > 0
          ? `<div class="kv"><dt>可回收</dt><dd class="reclaim">${byteHTML(reclaim)}</dd></div>`
          : "";
      const artifacts = artifactBytes(it);
      const artifactRow =
        artifacts > 0
          ? `<div class="kv"><dt>构建产物</dt><dd>${byteHTML(artifacts)}</dd></div>`
          : "";
      const detail = it.detail
        ? `<div class="detail">
            <div class="detail-label">明细</div>
            <dl class="detail-list">
              ${Object.entries(it.detail)
                .map(
                  ([k, v]) => `<div class="detail-row">
                    <dt class="detail-key" title="${esc(k.replace(/^\d+\.\s*/, ""))}">${esc(
                      k.replace(/^\d+\.\s*/, ""),
                    )}</dt>
                    <dd class="detail-val">${esc(v)}</dd>
                  </div>`,
                )
                .join("")}
            </dl>
          </div>`
        : "";
      inner = `
        <p class="ins-title">${esc(it.name)}</p>
        ${path ? `<p class="ins-path">${esc(it.path)}</p>` : ""}
        ${
          path
            ? `<div class="ins-actions">
                <button class="btn-outline" data-action="reveal">${ic("folder-open")}${REVEAL_LABEL}</button>
                ${
                  g.supports_clean && it.cleanable
                    ? `<button class="btn-outline danger" data-action="clean">${ic(
                        "trash",
                      )}清理</button>`
                    : ""
                }
                ${
                  g.supports_clean && artifacts > 0
                    ? `<button class="btn-outline danger" data-action="artifacts">${ic(
                        "trash",
                      )}清理产物</button>`
                    : ""
                }
              </div>`
            : ""
        }
        ${it.tags.length ? `<p class="ins-tags">${esc(it.tags.join(" · "))}</p>` : ""}
        <div class="ins-label">指标</div>
        <dl class="ins-metrics">${metrics}${splitRow}${artifactRow}</dl>
        ${detail}`;
    } else {
      const reclaimTotal = groupReclaimable(g);
      const rules =
        g.supports_clean
          ? `<button class="rules-btn" data-action="rules" aria-expanded="${
              state.rulesOpen
            }">${ic("chevron", "chev")}清理规则</button>
            <div class="rules${state.rulesOpen ? " open" : ""}"><div><ul>
              <li>「可清理」行可在检查器里发起清理（移入废纸篓）；带构建产物的仓库可以只清产物（node_modules / target 等，源码保留）。</li>
              <li>工具链缓存里那些有官方清理命令的行不标「可清理」——命令写在明细里，请自己运行，它比搬走整个目录精确。</li>
              <li>编辑器与 AI 工具按「工具 × 类别」清日志 / 会话 / 缓存；会话含完整对话记录，删除后不会自动重建。</li>
            </ul></div></div>`
          : reclaimTotal > 0
            ? `<p class="ins-note">本组只读：账面可回收不等于有删除入口。</p>`
            : "";
      inner = `
        <div class="ins-label" style="margin-top:0">${esc(g.label)}</div>
        ${g.summary ? `<p class="ins-summary">${esc(g.summary)}</p>` : ""}
        ${(g.notes ?? [])
          .map((n) => `<p class="ins-note">${esc(n)}</p>`)
          .join("")}
        ${rules}`;
    }
    inspector.innerHTML = `<div class="ins-body${pageIn ? " page-in" : ""}">${inner}</div>`;
  }

  // ---------------- 状态栏 ----------------

  function renderStatus({ draw = false } = {}) {
    const g = currentGroup();
    if (state.route === "settings" || !g) {
      statusbar.hidden = true;
      statusbar.innerHTML = "";
      return;
    }
    statusbar.hidden = false;
    const rows = visibleRows(g);
    const sum = rows.reduce((s, it) => s + sizeOf(it), 0);
    const reclaim = rows.reduce((s, it) => s + (it.cleanable ? reclaimOf(it) : 0), 0);
    const stale = state.savedAt != null && Date.now() / 1000 - state.savedAt > 86400;
    statusbar.innerHTML = `
      <span class="status-left">${rows.length} 项 · ${byteHTML(sum)}</span>
      <div class="status-right">
        <span class="updated${stale ? " stale" : ""}${draw ? " draw" : ""}" title="${
          stale ? "已超过一天，建议重扫" : "缓存快照时间"
        }">数据更新于 ${formatAgo(state.savedAt)}</span>
        ${
          reclaim > 0
            ? `<span>可回收 ${byteHTML(reclaim, "reclaim")}</span>`
            : ""
        }
      </div>`;
  }

  // ---------------- 设置 ----------------

  function settingsHTML() {
    return `<div class="settings">
      <section>
        <h2>缓存</h2>
        <p>扫描结果存在本机缓存里，启动直接读（毫秒级），只有首次运行或缓存被清才真扫。
          当前数据更新于 <span class="tabular-nums" data-settings-updated>${formatAgo(
            state.savedAt,
          )}</span>。</p>
        <div class="settings-actions">
          <button class="btn-outline" data-action="settings-rescan" ${
            state.scanning ? "disabled" : ""
          }>${ic("refresh", state.scanning ? "spin" : "")}${
            state.scanning ? "扫描中…" : "立即重扫"
          }</button>
          <button class="btn-ghost" data-action="settings-clear">${ic("trash")}清空缓存</button>
        </div>
        <p>全量扫描约 26 秒（原型里压缩成数秒来演示进度与数字补间）。清空缓存不删任何资产数据，只是下次启动重新扫一遍。</p>
      </section>
      <section>
        <h2>更新</h2>
        <p>当前版本 <span class="tabular-nums">v0.1.0</span>。<strong>不做应用内自动更新</strong>——
          查出新版本只在侧栏给个标记，装哪个包、什么时候装由你自己决定。</p>
        <div class="settings-actions">
          <button class="btn-outline" data-action="settings-check">检查更新</button>
        </div>
      </section>
    </div>`;
  }

  // ---------------- 路由与整体渲染 ----------------

  function parseRoute() {
    const h = location.hash.replace(/^#\/?/, "");
    if (h === "settings") return "settings";
    if (h && h !== "overview" && data.groups.some((g) => g.id === h)) return h;
    return "overview";
  }

  function navigate(route) {
    const hash = route === "overview" ? "#/overview" : route === "settings" ? "#/settings" : `#/${route}`;
    if (location.hash === hash) {
      closeFilterMenu();
      closeContextMenu();
      return;
    }
    location.hash = hash;
  }

  function resetView() {
    state.sortKey = "size";
    state.sortDesc = true;
    state.dims = {};
    state.selected = null;
    state.filterOpen = false;
  }

  function applyRoute({ initial = false, force = false } = {}) {
    const route = parseRoute();
    const changed = route !== state.route;
    state.route = route;
    if (changed || force) resetView();
    closeFilterMenu();
    closeContextMenu();
    updateSidebarSelection(!state.firstPaint);
    renderToolbar();
    renderContent({ enter: changed || initial, drawRule: changed || initial });
    renderInspector();
    renderStatus({ draw: initial });
    scroll.scrollTop = 0;
    state.firstPaint = false;
  }

  // ---------------- 行与检查器交互 ----------------

  function onRowClick(it) {
    if (state.route === "overview") {
      const gid = it.id.startsWith("overview:") ? it.id.slice("overview:".length) : "";
      if (gid) navigate(gid);
      return;
    }
    state.selected = state.selected === it.id ? null : it.id;
    applySelection();
    renderInspector({ pageIn: true });
    closeContextMenu();
  }

  function openContextMenu(e, it) {
    if (state.route === "overview" || !isLocalPath(it.path)) return;
    e.preventDefault();
    closeFilterMenu();
    ctxTarget = it;
    clearTimeout(ctxHideTimer);
    ctxmenu.hidden = false;
    ctxmenu.innerHTML = `
      <button class="ctx-item" role="menuitem" data-ctx="reveal">${ic("folder-open")}${REVEAL_LABEL}</button>
      <button class="ctx-item" role="menuitem" data-ctx="copy">${ic("copy")}复制路径</button>`;
    const left = Math.max(4, Math.min(e.clientX, window.innerWidth - 184));
    const top = Math.max(4, Math.min(e.clientY, window.innerHeight - 72));
    ctxmenu.style.left = `${left}px`;
    ctxmenu.style.top = `${top}px`;
    requestAnimationFrame(() => ctxmenu.classList.add("open"));
  }

  function closeContextMenu() {
    if (ctxmenu.hidden) return;
    ctxmenu.classList.remove("open");
    ctxTarget = null;
    ctxHideTimer = setTimeout(() => {
      if (!ctxmenu.classList.contains("open")) ctxmenu.hidden = true;
    }, ms(150));
  }

  function reveal(item) {
    toast(`原型演示：会在访达中显示「${item.name}」`, "info");
  }

  async function copyPath(path) {
    try {
      await navigator.clipboard.writeText(path);
      toast("路径已复制");
    } catch {
      const ta = document.createElement("textarea");
      ta.value = path;
      ta.style.cssText = "position:fixed;opacity:0;pointer-events:none";
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand("copy");
        toast("路径已复制");
      } catch {
        toast("复制失败（原型）", "info");
      }
      ta.remove();
    }
  }

  // ---------------- 重扫模拟 ----------------

  function showScanbar() {
    scanbar.hidden = false;
    scanfill.style.transform = "scaleX(0)";
    scannib.style.left = "0%";
    scannib.style.opacity = "1";
  }

  function hideScanbar() {
    scannib.style.opacity = "0";
    scanbar.hidden = true;
    scanfill.style.transform = "scaleX(0)";
  }

  function updateSettingsScanUI() {
    const btn = content.querySelector('[data-action="settings-rescan"]');
    if (!btn) return;
    btn.disabled = state.scanning;
    btn.innerHTML = `${ic("refresh", state.scanning ? "spin" : "")}${
      state.scanning ? "扫描中…" : "立即重扫"
    }`;
    const up = content.querySelector("[data-settings-updated]");
    if (up) up.textContent = formatAgo(state.savedAt);
  }

  function patchSummary(text, from, to) {
    if (!text) return text;
    const a = formatBytes(from);
    const b = formatBytes(to);
    if (a === b) return text;
    return text.replace(a, b);
  }

  function applyScanStep(gid, oldSizes, prevTotal) {
    const group = data.groups.find((g) => g.id === gid);
    if (!group) return;
    const delta = SCAN_DELTA[gid] ?? 0;
    const target = group.items.reduce((a, b) => (sizeOf(b) > sizeOf(a) ? b : a), group.items[0]);
    if (!target) return;
    const oldItem = sizeOf(target);
    const newItem = Math.max(1024, oldItem + delta);
    const m = target.metrics.size;
    target.metrics.size = { t: m?.t ?? "b", v: newItem };

    const ov = data.groups.find((g) => g.id === "overview");
    const ovRow = ov?.items.find((it) => it.id === `overview:${gid}`);
    const oldGroup = oldSizes[gid] ?? groupSize(group);
    const newGroup = groupSize(group);
    if (ovRow) ovRow.metrics.size = { t: "b", v: newGroup };

    const total = machineStats().total;
    group.summary = patchSummary(group.summary, oldGroup, newGroup);
    if (ov) ov.summary = patchSummary(ov.summary, prevTotal, total);
    state.scanDone = Math.min(state.scanTotal, state.scanDone + 1);

    updateSidebarSizes({ [gid]: oldGroup, __total: prevTotal });
    renderToolbar();

    if (state.route === "overview") {
      renderContent({ flashId: `overview:${gid}` });
      applySelection();
      tweenByteEl(content.querySelector("[data-hero]"), prevTotal, total, 520);
      const row = [...content.querySelectorAll(".row[data-row-id]")].find(
        (el) => el.dataset.rowId === `overview:${gid}`,
      );
      tweenByteEl(row?.querySelector(".cell .bv"), oldGroup, newGroup, 520);
      renderStatus();
    } else if (state.route === gid) {
      renderContent({ flashId: target.id });
      applySelection();
      const row = [...content.querySelectorAll(".row[data-row-id]")].find(
        (el) => el.dataset.rowId === target.id,
      );
      tweenByteEl(row?.querySelector(".cell .bv"), oldItem, newItem, 520);
      renderInspector();
      renderStatus();
    } else if (state.route === "settings") {
      updateSettingsScanUI();
    }
  }

  async function startRescan({ fromSettings = false } = {}) {
    if (state.scanning) return;
    closeFilterMenu();
    closeContextMenu();
    const token = ++scanToken;
    state.scanning = true;
    state.scanDone = 0;
    const oldSizes = {};
    for (const gid of SCAN_ORDER) {
      const g = data.groups.find((x) => x.id === gid);
      if (g) oldSizes[gid] = groupSize(g);
    }
    let prevTotal = machineStats().total;
    renderToolbar();
    updateSettingsScanUI();
    showScanbar();

    const duration = state.motion === 0 ? 700 : 2600 * state.motion;
    const start = performance.now();
    let step = 0;

    const finish = async () => {
      if (token !== scanToken) return;
      state.scanning = false;
      state.scanDone = state.scanTotal;
      state.savedAt = Math.floor(Date.now() / 1000);
      scanfill.style.transform = "scaleX(1)";
      scannib.style.opacity = "0";
      await new Promise((r) => setTimeout(r, ms(240)));
      if (token !== scanToken) return;
      hideScanbar();
      renderToolbar();
      if (content.querySelector("[data-settings-updated]")) updateSettingsScanUI();
      renderStatus({ draw: true });
      if (fromSettings) {
        toast("重扫完成");
        navigate("overview");
      } else {
        toast("扫描完成 · 数据已更新");
      }
    };

    const frame = (now) => {
      if (token !== scanToken) return;
      const t = Math.min(1, (now - start) / duration);
      scanfill.style.transform = `scaleX(${t})`;
      scannib.style.left = `${(t * 100).toFixed(2)}%`;
      while (step < SCAN_ORDER.length && t >= (step + 1) / SCAN_ORDER.length) {
        const gid = SCAN_ORDER[step];
        applyScanStep(gid, oldSizes, prevTotal);
        prevTotal = machineStats().total;
        step++;
      }
      if (t < 1) requestAnimationFrame(frame);
      else finish();
    };
    requestAnimationFrame(frame);
  }

  // ---------------- 主题 / 演示控制 ----------------

  function applyTheme() {
    document.documentElement.classList.toggle("dark", state.theme === "dark");
    try {
      localStorage.setItem("stacktake-proto-theme", state.theme);
    } catch {}
    const rail = $("#rail-theme");
    if (rail) rail.textContent = state.theme === "dark" ? "浅色" : "深色";
    renderToolbar();
  }

  function toggleTheme() {
    state.theme = state.theme === "dark" ? "light" : "dark";
    applyTheme();
  }

  function cycleMotion() {
    state.motion = state.motion === 1 ? 2.2 : state.motion === 2.2 ? 0 : 1;
    applyMotion();
  }

  function resetDemo() {
    scanToken++;
    state.scanning = false;
    state.scanDone = 0;
    hideScanbar();
    data = structuredClone(DATA);
    state.savedAt = DATA.saved_at;
    state.inspectorOpen = window.innerWidth >= 1040;
    state.rulesOpen = false;
    renderSidebar();
    if (location.hash === "#/overview") {
      applyRoute({ initial: true, force: true });
    } else {
      location.hash = "#/overview";
    }
    state.firstPaint = false;
    updateRail();
  }

  function updateRail() {
    applyMotion();
    const railTheme = $("#rail-theme");
    if (railTheme) railTheme.textContent = state.theme === "dark" ? "浅色" : "深色";
  }

  // ---------------- 事件 ----------------

  nav.addEventListener("click", (e) => {
    const b = e.target.closest("[data-route]");
    if (b) navigate(b.dataset.route);
  });

  $("#settings-btn").addEventListener("click", () => navigate("settings"));

  $("#toolbar").addEventListener("click", (e) => {
    const b = e.target.closest("[data-action]");
    if (!b) return;
    const action = b.dataset.action;
    if (action === "filter") toggleFilterMenu();
    else if (action === "theme") toggleTheme();
    else if (action === "inspector") {
      state.inspectorOpen = !state.inspectorOpen;
      renderInspector();
      renderToolbar();
      placeMarker(false);
    } else if (action === "rescan") startRescan();
  });

  content.addEventListener("click", (e) => {
    const sort = e.target.closest("[data-sort]");
    if (sort) {
      toggleSort(sort.dataset.sort);
      rerenderWithFlip();
      return;
    }
    const row = e.target.closest(".row[data-row-id]");
    if (row && content.contains(row)) {
      const g = currentGroup();
      const it = g?.items.find((x) => x.id === row.dataset.rowId);
      if (it) onRowClick(it);
      return;
    }
    const act = e.target.closest("[data-action]")?.dataset.action;
    if (act === "clear-filters") {
      state.dims = {};
      state.selected = null;
      renderToolbar();
      renderInspector();
      rerenderWithFlip();
      renderStatus();
    } else if (act === "settings-rescan") {
      startRescan({ fromSettings: true });
    } else if (act === "settings-clear") {
      toast("原型不触碰真实缓存：这里只会提示，不会清空任何数据", "info");
    } else if (act === "settings-check") {
      if (checkTimer) return;
      const btn = e.target.closest("[data-action]");
      btn.disabled = true;
      btn.textContent = "检查中…";
      checkTimer = setTimeout(() => {
        checkTimer = 0;
        if (btn.isConnected) {
          btn.disabled = false;
          btn.textContent = "检查更新";
        }
        toast("已是最新版本", "info");
      }, 900 * Math.max(state.motion, 0.4));
    }
  });

  content.addEventListener("contextmenu", (e) => {
    const row = e.target.closest(".row[data-row-id]");
    if (!row) return;
    e.preventDefault();
    const g = currentGroup();
    const it = g?.items.find((x) => x.id === row.dataset.rowId);
    if (it) openContextMenu(e, it);
  });

  inspector.addEventListener("click", (e) => {
    const b = e.target.closest("[data-action]");
    if (!b) return;
    const it = selectedRow();
    if (b.dataset.action === "rules") {
      state.rulesOpen = !state.rulesOpen;
      b.setAttribute("aria-expanded", String(state.rulesOpen));
      const rules = inspector.querySelector(".rules");
      if (rules) rules.classList.toggle("open", state.rulesOpen);
    } else if (b.dataset.action === "reveal" && it) {
      reveal(it);
    } else if (b.dataset.action === "clean" && it) {
      toast("原型不执行删除；真实 app 会在二次确认后移入废纸篓", "info");
    } else if (b.dataset.action === "artifacts" && it) {
      toast("原型不执行删除；真实 app 只清白名单内的构建产物", "info");
    }
  });

  popoverRoot.addEventListener("click", (e) => {
    const clear = e.target.closest("[data-clear-filters]");
    if (clear) {
      state.dims = {};
      state.selected = null;
      closeFilterMenu();
      renderToolbar();
      renderInspector();
      rerenderWithFlip();
      renderStatus();
      return;
    }
    const b = e.target.closest("[data-fdim]");
    if (!b) return;
    const key = b.dataset.fdim;
    const val = b.dataset.fval || null;
    state.dims = { ...state.dims, [key]: val };
    state.selected = null;
    closeFilterMenu();
    renderToolbar();
    renderInspector();
    rerenderWithFlip();
    renderStatus();
  });

  ctxmenu.addEventListener("click", (e) => {
    const b = e.target.closest("[data-ctx]");
    if (!b || !ctxTarget) return;
    const it = ctxTarget;
    if (b.dataset.ctx === "reveal") reveal(it);
    else if (b.dataset.ctx === "copy") copyPath(it.path);
    closeContextMenu();
  });

  document.querySelector(".proto-rail").addEventListener("click", (e) => {
    const b = e.target.closest("[data-rail]");
    if (!b) return;
    const action = b.dataset.rail;
    if (action === "rescan") startRescan();
    else if (action === "motion") cycleMotion();
    else if (action === "theme") toggleTheme();
    else if (action === "reset") resetDemo();
  });

  document.addEventListener("pointerdown", (e) => {
    if (
      state.filterOpen &&
      !e.target.closest("#filter-menu") &&
      !e.target.closest('[data-action="filter"]')
    ) {
      closeFilterMenu();
    }
    if (!ctxmenu.hidden && !e.target.closest("#ctxmenu")) closeContextMenu();
  });

  window.addEventListener(
    "scroll",
    () => {
      closeFilterMenu();
      closeContextMenu();
    },
    true,
  );

  window.addEventListener("resize", () => {
    closeFilterMenu();
    closeContextMenu();
    placeMarker(false);
  });

  window.addEventListener("keydown", (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const tag = (e.target.tagName || "").toLowerCase();
    if (tag === "input" || tag === "textarea" || e.target.isContentEditable) return;
    if (e.key === "Escape") {
      closeFilterMenu();
      closeContextMenu();
      return;
    }
    const k = e.key.toLowerCase();
    if (k === "r") {
      e.preventDefault();
      startRescan();
    } else if (k === "d") {
      e.preventDefault();
      toggleTheme();
    } else if (k === "f") {
      if (dimensionsFor(currentGroup()).length) {
        e.preventDefault();
        toggleFilterMenu();
      }
    }
  });

  navScroll.addEventListener("scroll", () => placeMarker(false), { passive: true });

  window.addEventListener("hashchange", () => applyRoute());

  // ---------------- 启动 ----------------

  if (!location.hash) history.replaceState(null, "", "#/overview");
  state.route = parseRoute();
  applyMotion();
  renderSidebar();
  applyTheme();
  updateRail();
  applyRoute({ initial: true, force: true });

  // 首屏稳定后再标一次：字体/布局落定后 marker 才不会差半行
  requestAnimationFrame(() => placeMarker(false));
})();

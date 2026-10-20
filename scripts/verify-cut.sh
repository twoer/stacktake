#!/usr/bin/env bash
# verify-cut.sh —— 「刀」的完成判据（门禁脚本）。全绿退出 0，任何一门失败非 0。
#
# 用法:
#   scripts/verify-cut.sh --pre [刀号]   # 取刀闸: 刀可开工 + 按序 + 锁空闲 + 回填真实 + 工作区干净
#                                        #        + 档位可读 + 所在 Track 无未拍板的开工前置
#                                        #        （无参自动取第一把未收口刀）
#   scripts/verify-cut.sh [--cut 刀号]   # 收口闸: 门3 先判工作区——有未提交变更直接列出并停
#                                        #          （不烧档位验证）；干净态才跑 门1 档位验证 + 硬校验
#
# 一刀完成 = 收口闸通过 + commit 落盘 + backlog 勾选回填 commit 号（契约见 AGENTS.md「工作循环」）。
# 循环靠取刀闸闭合: 下一刀 --pre 要求工作区干净（= 上一刀已 commit）且已勾选条目均附真实存在的 commit 号。
#
# Track 级开工前置: 整个 Track 被某个未拍板决策挡着时，在它的 Track 头下写一行
#   > 开工前置：【待确认】D4 —— 拍板前本 Track 整体跳过
# 取刀闸读到该行即拒绝（不留到「取到了才发现」）。拍板后由人工删掉那行。
#
# ⚠️ 本门禁只读: 不执行任何清理/删除/移动真实文件的操作（硬约束 4）。
#    档位门禁复用 scripts/verify.sh（内环/外环/评估档），不另造一套。
#
# 兼容 macOS 自带 bash 3.2: 不用关联数组、不用 ${var^^}；sed -i 走 .bak 兼容 GNU/BSD。

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BACKLOG="$ROOT/docs/backlog.md"
VERIFY="$ROOT/scripts/verify.sh"

PRE=false
CUT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --pre) PRE=true; shift ;;
    --cut) CUT="${2:?--cut 需要刀号}"; shift 2 ;;
    -h|--help) grep '^#' "$0" | sed '1d; s/^# \{0,1\}//'; exit 0 ;;
    -*) echo "未知参数: $1 (--help 看用法)" >&2; exit 64 ;;
    *) CUT="$1"; shift ;;
  esac
done

say()  { printf '\033[1;34m[门禁]\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m  [OK]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m  [警告]\033[0m %s\n' "$*"; }
note() { printf '\033[1;33m  [注意]\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31m  [FAIL] %s\033[0m\n' "$*" >&2; exit 1; }

# ── backlog 读取 ──────────────────────────────────────────────

holding() {
  [[ -f "$BACKLOG" ]] || return 0
  grep -m1 '^> 当前持刀：' "$BACKLOG" 2>/dev/null | sed -E 's/^> 当前持刀：//' || true
}

# 持刀者；「空」与空串（含（空；…）注记形态）都视为空闲
cut_of() {
  local h
  h="$(holding | sed -e 's/（.*$//' -e 's/(.*$//' -e 's/[[:space:]]*$//')"
  case "$h" in
    ""|"空"*) printf '' ;;
    *) printf '%s' "$h" ;;
  esac
}

# 刀在 backlog 中的状态: done | open | pending | missing
cut_state() {
  local cut="$1" line
  line="$(grep -E "^- \[[ x]\] \*\*${cut}(\*\*|[[:space:]（])" "$BACKLOG" 2>/dev/null | head -1 || true)"
  if [[ -z "$line" ]]; then
    if grep -qE "${cut}.*【待确认】|【待确认】.*${cut}" "$BACKLOG" 2>/dev/null; then
      printf 'pending'
    else
      printf 'missing'
    fi
  elif [[ "$line" == "- [x]"* ]]; then
    printf 'done'
  else
    printf 'open'
  fi
}

first_open() {
  local line
  line="$(grep -E '^- \[ \] \*\*[^ *]+' "$BACKLOG" 2>/dev/null | head -1 || true)"
  [[ -n "$line" ]] && printf '%s' "$line" | sed -E 's/^- \[ \] \*\*([^ *]+).*/\1/'
  return 0
}

cut_line() {
  grep -E "^- \[[ x]\] \*\*$1(\*\*|[[:space:]（])" "$BACKLOG" 2>/dev/null | head -1 || true
}

# 从 backlog 行提取「门禁:`档位`」/「证据:`路径`」
cut_field() {
  local line
  line="$(cut_line "$1")"
  [[ -n "$line" ]] || return 0
  printf '%s' "$line" | sed -n -E "s/.*$2:\`([^\`]+)\`.*/\1/p"
}

TIERS=" 内环 外环 评估档 "

# 门禁档位必须可读且合法。取刀闸与收口闸都调用——**取刀闸这一次是关键**：
# cut_field 只读条目第一行，档位登记畸形（写在续行）或漏写时，收口闸才报错，
# 而那时活已经干完了（H3 就是这么炸的，代价接近一把刀的工期）。
check_tier() {
  local cut="$1" tier
  tier="$(cut_field "$cut" 门禁)"
  if [[ -z "$tier" ]]; then
    fail "$cut 未登记门禁档位——按 backlog 文件头规则写在条目**首行**：门禁:\`外环\`（内环/外环/评估档）"
  fi
  case "$TIERS" in
    *" $tier "*) ;;
    *) fail "$cut 登记的门禁档位无法识别: ${tier}（合法: 内环/外环/评估档）" ;;
  esac
  ok "$cut 门禁档位可读: ${tier}"
}

# Track 级开工前置: 从本刀所在行向上找最近的「## 」段头，路上若有 `> 开工前置…` 行，
# 说明整个 Track 被一个未拍板决策挡着，整节不许开工。
#
# 为什么必须有这一条: 文件头写着「未拍板期间按序取刀整体跳过本 Track（不算跳刀）」，
# 而 cut_state 只认条目自己附近的【待确认】、first_open 只认文件序——两条规则打架的
# 结果是裸 `--pre` 会取到被挡住的刀（2026-10-13 实测取到了 Track G 的 G1 并登记了锁）。
# 只匹配行首的 `>`（说明行）而不是全文件搜「开工前置」四个字: 口径说明、表格里也可能
# 提到这个词，那不该拦住任何刀。
track_gate() {
  local cut="$1" n i line
  n="$(grep -nE "^- \[[ x]\] \*\*${cut}(\*\*|[[:space:]（])" "$BACKLOG" 2>/dev/null | head -1 | cut -d: -f1 || true)"
  [[ -n "$n" ]] || return 0
  i=$((n - 1))
  while (( i >= 1 )); do
    line="$(sed -n "${i}p" "$BACKLOG")"
    case "$line" in
      '## '*)
        case "$line" in '>'*开工前置*) printf '%s' "$line" ;; esac
        return 0 ;;
      '>'*开工前置*) printf '%s' "$line"; return 0 ;;
    esac
    i=$((i - 1))
  done
  return 0
}

check_track_gate() {
  local cut="$1" gate
  gate="$(track_gate "$cut")"
  [[ -z "$gate" ]] && return 0
  note "Track 头的门禁行: ${gate}"
  fail "$cut 所在的 Track 被未拍板的决策挡住——先拍板、删掉那行「开工前置」，再取刀"
}

# 收口不变量: 凡已勾选的刀, 必须回填一个本仓库真实存在的 commit 号
check_closed() {
  local bad=0 line cut sha
  [[ -f "$BACKLOG" ]] || return 0
  while IFS= read -r line; do
    cut="$(printf '%s' "$line" | sed -E 's/^- \[x\] \*\*([^ *]+).*/\1/')"
    sha="$(printf '%s' "$line" | grep -oE '\([0-9a-f]{7,40}\)' 2>/dev/null | head -1 | tr -d '()' || true)"
    if [[ -z "$sha" ]]; then
      printf '  [FAIL] %s 已勾选但未回填 commit 号\n' "$cut" >&2
      bad=1
    elif ! git -C "$ROOT" cat-file -e "${sha}^{commit}" 2>/dev/null; then
      printf '  [FAIL] %s 回填的 commit %s 在本仓库中不存在\n' "$cut" "$sha" >&2
      bad=1
    else
      ok "${cut} 已收口（${sha}）"
    fi
  done < <(grep -E '^- \[x\]' "$BACKLOG" 2>/dev/null || true)
  return "$bad"
}

dirty_list() { git -C "$ROOT" status --porcelain 2>/dev/null | grep -vE '(^|/)\.DS_Store$' || true; }

show_dirty() {
  warn "工作区有未提交变更:"
  dirty_list | sed 's/^/      /'
}

# 按序: 文件序中先于本刀的正式条目必须已收口
check_order() {
  local cut="$1" line id state
  while IFS= read -r line; do
    id="$(printf '%s' "$line" | sed -E 's/^- \[[ x]\] \*\*([^ *]+).*/\1/')"
    [[ "$id" == "$cut" ]] && break
    state="$(cut_state "$id")"
    [[ "$state" == done ]] || fail "按序取刀: ${id}（${state}）未收口，不能开 ${cut}——跳刀须人工调整 backlog"
  done < <(grep -E '^- \[[ x]\] \*\*[^ *]+' "$BACKLOG" 2>/dev/null || true)
}

# 门1 档位 → verify.sh 参数
run_tier() {
  local tier="$1"
  case "$tier" in
    内环)   say "门1 内环（fmt + clippy + tsc + 契约自检）"; "$VERIFY" --quick ;;
    外环)   say "门1 外环（内环 + cargo test + 构建）"; "$VERIFY" ;;
    评估档) say "门1 评估档（外环 + 全量扫描契约校验）"; "$VERIFY" --full ;;
    *) fail "本刀登记的门禁档位无法识别: ${tier}（合法: 内环/外环/评估档）" ;;
  esac
}

# ── 取刀闸 ───────────────────────────────────────────────────

if [[ "$PRE" == true ]]; then
  [[ -f "$BACKLOG" ]] || fail "缺少 docs/backlog.md（取刀清单）"
  [[ -z "$CUT" ]] && CUT="$(first_open)"
  [[ -n "$CUT" ]] || fail "backlog 里没有未收口的正式刀——勘测并挂【待确认】后停下等拍板，不得自行新增刀直接开工"

  state="$(cut_state "$CUT")"
  [[ "$state" == open ]] || fail "$CUT 的状态是 ${state}（待确认/已完成/不存在都不可开工）"

  lock="$(cut_of)"
  if [[ -n "$lock" && "$lock" != "$CUT" ]]; then
    fail "持刀锁被 ${lock} 占用——同一时刻只允许一个会话下刀（他人只读；自己的在制刀可用同一刀号重入）"
  fi

  check_order "$CUT"
  check_closed
  check_tier "$CUT"
  check_track_gate "$CUT"

  if [[ -n "$(dirty_list)" ]]; then
    show_dirty
    fail "工作区不干净——先 commit 上一刀再开工（未完成的改动可 git stash）"
  fi

  if [[ -z "$lock" ]]; then
    sed -i.bak -E "s|^> 当前持刀：.*|> 当前持刀：${CUT}|" "$BACKLOG"
    rm -f "$BACKLOG.bak"
    say "已登记持刀锁: ${CUT}（随本刀第一个 commit 落库）"
  fi
  ok "取刀闸通过: ${CUT} 可以开工"
  exit 0
fi

# ── 收口闸 ───────────────────────────────────────────────────

[[ -n "$CUT" ]] || CUT="$(cut_of)"
[[ -n "$CUT" ]] || fail "没有指定刀号，且持刀锁为空——用法: scripts/verify-cut.sh --cut <刀号>"

state="$(cut_state "$CUT")"
[[ "$state" == open || "$state" == done ]] || fail "$CUT 状态为 ${state}（不存在或仍是【待确认】）"

# 门3 先判工作区: 脏态下这次调用的全部产出就是「先 commit」这句话，档位验证留给提交后的
# 干净态调用——原先门1 排在前面，脏态也要先烧一遍档位验证才告诉你去 commit，同一套验证
# 每刀要跑两遍。想在提交前验证，直接跑 scripts/verify.sh 对应层（工作节拍本就要求改完当场跑）。
if [[ -n "$(dirty_list)" ]]; then
  warn "工作区有未提交变更——本刀尚未落盘:"
  dirty_list | sed 's/^/      /'
  say "收口未完成。先 commit（含 backlog 勾选 + commit 号回填 + 持刀锁清空），干净态下再跑收口闸（届时跑门1 档位验证）"
  exit 1
fi

# 门1: 该刀登记的档位（门禁定义与 backlog 条目同步登记，不许口头门禁）
check_tier "$CUT"
tier="$(cut_field "$CUT" 门禁)"
run_tier "$tier"
ok "门1 通过（${tier}）"

# 干净态: 硬校验
[[ "$(cut_state "$CUT")" == done ]] || fail "$CUT 未在 backlog 勾选（- [x]）"
sha="$(cut_line "$CUT" | grep -oE '\([0-9a-f]{7,40}\)' | head -1 | tr -d '()' || true)"
[[ -n "$sha" ]] || fail "$CUT 已勾选但未回填 commit 号"
git -C "$ROOT" cat-file -e "${sha}^{commit}" 2>/dev/null || fail "回填的 commit ${sha} 在本仓库中不存在"
check_closed

lock="$(cut_of)"
if [[ -n "$lock" ]]; then
  fail "持刀锁未清空（当前: ${lock}）——收口 commit 需把「当前持刀」改回（空），否则下一刀开不了工"
fi

ev="$(cut_field "$CUT" 证据)"
if [[ -n "$ev" ]]; then
  [[ -s "$ROOT/$ev" ]] || fail "缺少证据存档 $ev（评估档刀的收口前提: 命令 + 时间 + 结果摘录）"
  ok "证据存档落盘: $ev"
fi

ok "收口闸通过: ${CUT}（${sha}）"

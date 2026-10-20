#!/usr/bin/env bash
# Stacktake 验证回路（harness 入口）。
#
# 分层是刻意的：内环秒级、随手跑；外环约 20 秒、提交前跑；评估档带全量扫描，
# 改了扫描逻辑才跑。哪层挂了就先修哪层，别带着红往下走。
#
#   scripts/verify.sh            外环：内环 + cargo test + 前端 build
#   scripts/verify.sh --quick    内环：fmt + clippy + tsc + 契约/文档门禁（几秒到几十秒）
#   scripts/verify.sh --full     外环 + 全量扫描契约评估（约 45 秒，扫真实数据）
#
# 退出码：0 全绿；非零表示有层没过。CI 跑的是「外环」那一套（见 .github/workflows）。
# 静音：非 TTY 且非 CI 时每层输出写入 <git-dir>/verify-logs/<mode>.log，终端只打 PASS 行；
# 失败打末尾 30 行 + 日志路径。TTY 与 CI 保持流式输出。

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE=outer
case "${1:-}" in
  --quick) MODE=quick ;;
  --full) MODE=full ;;
  "") ;;
  *) echo "未知参数：$1（可选 --quick / --full）" >&2; exit 2 ;;
esac

# 颜色只在 TTY 下开，CI 日志里不留转义序列
if [[ -t 1 ]]; then
  G=$'\e[32m'; R=$'\e[31m'; Y=$'\e[33m'; D=$'\e[2m'; N=$'\e[0m'
else
  G=""; R=""; Y=""; D=""; N=""
fi

# 流式 vs 静音：TTY 与 CI 保持原样（人要看进度、CI 要完整日志）；其余（agent 调用、
# 重定向到文件）把每层输出收进 <git-dir>/verify-logs/<mode>.log，终端只留 PASS 行与
# 失败尾巴——一次外环原本打印 100+ 行 PASS，token 成本高且淹没失败信号。
VERBOSE=false
if [[ -t 1 || -n "${CI:-}" ]]; then
  VERBOSE=true
fi
LOG=""
if [[ "$VERBOSE" == false ]]; then
  LOG_DIR="$(git -C "$ROOT" rev-parse --absolute-git-dir 2>/dev/null || printf '%s' "$ROOT")/verify-logs"
  mkdir -p "$LOG_DIR"
  LOG="$LOG_DIR/$MODE.log"
  : > "$LOG"
fi

FAILED=()

run() {
  local name="$1"; shift
  local start=$SECONDS
  if [[ "$VERBOSE" == true ]]; then
    printf '%s==>%s %s\n' "$D" "$N" "$name"
    if "$@"; then
      printf '%s  PASS%s %s\n\n' "$G" "$N" "$name"
    else
      printf '%s  FAIL%s %s\n\n' "$R" "$N" "$name"
      FAILED+=("$name")
      return 1
    fi
    return 0
  fi

  local layer_log
  layer_log="$(mktemp "${TMPDIR:-/tmp}/stacktake-verify-layer-XXXXXX")"
  printf '\n===== %s =====\n' "$name" >> "$LOG"
  if "$@" >"$layer_log" 2>&1; then
    cat "$layer_log" >> "$LOG"
    rm -f "$layer_log"
    printf '%s  PASS%s %s (%ss)\n' "$G" "$N" "$name" "$((SECONDS - start))"
    return 0
  fi
  cat "$layer_log" >> "$LOG"
  printf '%s  FAIL%s %s (%ss)\n' "$R" "$N" "$name" "$((SECONDS - start))"
  printf '  --- 末尾 30 行（完整日志: %s） ---\n' "$LOG"
  tail -n 30 "$layer_log"
  rm -f "$layer_log"
  FAILED+=("$name")
  return 1
}

summary() {
  if [[ ${#FAILED[@]} -eq 0 ]]; then
    printf '%s全部通过%s [%s]\n' "$G" "$N" "$MODE"
    return 0
  fi
  printf '%s未通过：%s%s [%s]\n' "$R" "$N" "${FAILED[*]}" "$MODE"
  printf '%s修复提示：%s\n' "$Y" "$N"
  printf '  rust fmt      → cd src-tauri && cargo fmt --all\n'
  printf '  rust clippy   → cd src-tauri && cargo clippy --all-targets -- -D warnings，看首条警告的 --> 定位\n'
  printf '  前端类型检查   → npx vue-tsc --noEmit（结构变更先同步 src/lib/types.ts）\n'
  printf '  rust 测试     → cd src-tauri && cargo test；只跑某组用 cargo test <关键字>\n'
  printf '  前端构建      → npm run build\n'
  printf '  快照契约评估  → node scripts/check-snapshot-contract.mjs <snapshot.json>\n'
  printf '  文档门禁      → node scripts/check-docs.mjs --self-test && node scripts/check-docs.mjs\n'
  return 1
}

show_log() {
  if [[ "$VERBOSE" == false ]]; then
    printf '%s日志:%s %s\n' "$D" "$N" "$LOG"
  fi
}

rust_fmt()  { (cd "$ROOT/src-tauri" && cargo fmt --all -- --check); }
rust_lint() { (cd "$ROOT/src-tauri" && cargo clippy --all-targets -- -D warnings); }
rust_test() {
  # 有 nextest 用 nextest（逐测试独立进程，失败隔离与单测计时更好），没有回 cargo test——
  # CI runner 上没装 nextest，走的就是兜底。两者覆盖一致：doctest 为 0、ignored 同样跳过。
  # 只报失败与摘要：H10 起 PASS 行不再刷屏（静音模式会把这里整个收进日志）。
  if command -v cargo-nextest >/dev/null 2>&1; then
    (cd "$ROOT/src-tauri" && cargo nextest run --status-level fail --final-status-level fail)
  else
    (cd "$ROOT/src-tauri" && cargo test --quiet)
  fi
}
fe_type()   { (cd "$ROOT" && npx --no-install vue-tsc --noEmit); }
fe_build()  { (cd "$ROOT" && npm run build); }
contract_selftest() { node "$ROOT/scripts/check-snapshot-contract.mjs" --self-test; }
docs_gate() {
  # 文档门禁同样先自检再查仓库：坏样本抓不住的门禁等于没有
  node "$ROOT/scripts/check-docs.mjs" --self-test &&
    node "$ROOT/scripts/check-docs.mjs"
}
contract()  {
  local tmp rc
  tmp="$(mktemp /tmp/stacktake-snapshot-XXXXXX.json)"
  # dump 把 JSON 写 stdout、诊断写 stderr；全量扫描约 25 秒
  (cd "$ROOT/src-tauri" && cargo run --example dump 2>/dev/null >"$tmp")
  rc=$?
  if [[ $rc -ne 0 ]]; then rm -f "$tmp"; return $rc; fi
  node "$ROOT/scripts/check-snapshot-contract.mjs" "$tmp"
  rc=$?
  rm -f "$tmp"
  return $rc
}

printf '%sStacktake verify [%s]%s\n' "$D" "$MODE" "$N"

# ---- 内环：秒级，改完就跑。任何一层红就停，别浪费后面几十秒 ----
run "rust fmt"     rust_fmt  || { summary; exit 1; }
run "rust clippy"  rust_lint || { summary; exit 1; }
run "前端类型检查"  fe_type   || { summary; exit 1; }
# 校验器自己挂了比没有校验器更危险，所以自检留在内环；文档门禁同理
run "契约校验器自检" contract_selftest || { summary; exit 1; }
run "文档门禁" docs_gate || { summary; exit 1; }

if [[ "$MODE" == quick ]]; then
  rc=0
  summary || rc=$?
  show_log
  exit $rc
fi

# ---- 外环：提交前。测试约 10 秒（H9 起嵌套口径走临时目录夹具，真实数据版 #[ignore]），build 约 1 秒 ----
# 外环各层都跑完再汇总：已经花了十几秒，一次拿全失败信息比逐层刹车划算
run "rust 测试"    rust_test || true
run "前端构建"     fe_build  || true

if [[ "$MODE" == full ]]; then
  # ---- 评估档：真实全量扫描 + 快照契约校验 ----
  run "快照契约评估" contract || true
fi

rc=0
summary || rc=$?
show_log

# 全绿且工作区干净时写验证戳（<git-dir>/gate-stamp：树 hash + 档位）。pre-push 据此跳过对
# 同一棵树的重复外环——外环档的刀原本「收口闸跑一遍、push 再跑一遍」，有一遍是纯重复。
# 只在干净树上写：脏树下 HEAD 树不代表刚测过的内容；内环不写（pre-push 不认它）。
if [[ $rc -eq 0 && "$MODE" != quick ]]; then
  if [[ -z "$(git -C "$ROOT" status --porcelain 2>/dev/null)" ]]; then
    stamp_dir="$(git -C "$ROOT" rev-parse --git-dir 2>/dev/null)" &&
      tree="$(git -C "$ROOT" rev-parse 'HEAD^{tree}' 2>/dev/null)" &&
      printf '%s %s\n' "$tree" "$MODE" > "$stamp_dir/gate-stamp"
  fi
fi
exit $rc

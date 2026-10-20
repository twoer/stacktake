#!/usr/bin/env bash
# 安装 git 钩子（opt-in，不会自动执行）。
#
#   bash scripts/hooks/install.sh     安装
#   bash scripts/hooks/install.sh --uninstall  卸载（恢复 git 默认行为）
#
# 装了什么：
#   pre-commit  内环（秒级）：fmt + clippy + 前端类型 + 契约校验器自检
#   pre-push    外环（约 20 秒）：内环 + cargo test + 前端构建
#
# 用 core.hooksPath 指到 scripts/hooks，不碰 .git/hooks，卸载无残留。

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

if [[ "${1:-}" == "--uninstall" ]]; then
  git -C "$ROOT" config --unset core.hooksPath 2>/dev/null || true
  echo "已卸载：git 恢复默认钩子路径"
  exit 0
fi

chmod +x "$ROOT/scripts/hooks/pre-commit" "$ROOT/scripts/hooks/pre-push"
git -C "$ROOT" config core.hooksPath scripts/hooks
echo "已安装：pre-commit 跑内环，pre-push 跑外环"
echo "绕过单次：git commit --no-verify / git push --no-verify"

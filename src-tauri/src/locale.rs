//! 系统界面语言，只作为前端解析语言偏好的输入。
//!
//! 走 Rust `sys-locale` 而不是 webview 的 `navigator.language`：Tauri 的 webview
//! 报告的是进程/CDP 的 locale，不一定是用户在系统设置里选的那一项（截图 harness
//! 与 CI 上尤其如此）。映射到五种语言、兜底英文等规则全部在前端 `lib/locale.ts`
//! ——这里只回原始 BCP-47 标签，不替前端做决定。

/// 取系统语言标签（如 `zh-Hant-TW`、`es-419`）。探不到返回 `None`，
/// 前端会退回 `navigator.languages`。
#[tauri::command]
pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

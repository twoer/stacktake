//! 应用内下载更新。
//!
//! B 方案的延伸：**版本检查仍在 webview 里**（`src/lib/release.ts` 查
//! api.github.com 拿最新 tag），但下载走这里。为什么必须在 Rust 侧：
//! 下载会 302 到 `objects.githubusercontent.com`，而 webview 的 CSP
//! `connect-src` 只放了 `api.github.com`（`tauri.conf.json`），前端 fetch
//! 会被**静默**拦掉——控制台一行 CSP 报错，界面上什么都不发生。Rust 侧
//! 不受 CSP 约束，还能报真实进度。
//!
//! 与硬约束 10 的关系：那条禁的是前端动作面上的「打开」（对扫到的条目，
//! 打开 = 执行）。这里打开的是**本工具刚刚下载完的那一个安装包**，
//! 由用户点「下载」显式发起（2026-10-21 用户拍板选 B：下完自动打开）。
//! 范围锁死在这一个文件上：没有暴露「打开任意路径」的 command，前端也
//! 拿不到这条路径以外的任何东西。

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

/// GitHub latest release API。与前端 `src/lib/release.ts` 的 `LATEST_API`
/// 是同一个 URL——两边没有共享常量机制，改这里记得同步那边。
const LATEST_API: &str = "https://api.github.com/repos/twoer/stacktake/releases/latest";

/// 下载进度事件名。前端 `useUpdate.ts` 监听同一个名字。
const UPDATE_PROGRESS_EVENT: &str = "update-download-progress";

/// GitHub API 要求请求带 User-Agent，reqwest 默认不带——不带会被 403 挡掉。
const USER_AGENT: &str = "stacktake";

/// 进度事件载荷。`total` 为 `None` = 服务端没给 Content-Length（GitHub 资产
/// 总是给，但流式响应的 `content_length()` 读不到时不该让整条链路失败）。
#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    /// 已写盘的字节数
    pub received: u64,
    /// 总字节数（可能拿不到）
    pub total: Option<u64>,
}

/// GitHub release 资产，只取用得到的两个字段。API 还返回二十多个
/// （state、content_type、…），serde 默认忽略未知字段，不逐一声明。
#[derive(serde::Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// 只声明 `assets`：tag_name 前端已经拿到过（版本比对在 webview 做），
/// 这里重新查一次是为了拿**下载地址**——点击那一刻重新查，比把启动那次
/// 检查的响应存到点击时用更不容易过期。
#[derive(serde::Deserialize)]
struct GitHubRelease {
    assets: Vec<GitHubAsset>,
}

/// 平台匹配：给一份资产列表和「正在运行的二进制所面向的」平台，返回该平台的安装包。
///
/// **纯函数、喂假数据测**（硬约束 9 的同一条理由：匹配规则不该由跑测试的
/// 机器决定）。调用处才用 `cfg!` 取真实平台——那是对的：这里选的是「正在
/// 跑的这个二进制」的安装包，不是把某套布局映射到另一套。
///
/// 规则刻意从严：找不到**精确匹配架构**的包就返回 `None`，不做「随便给一个」。
/// 给错架构的包比不给更坏——Intel Mac 下到 arm64 的 dmg，装得上但跑不起来，
/// 用户只会觉得是工具坏了。发布矩阵只出 macOS arm64 与 Windows x64
/// （`release.yml`，2026-10-19 拍板不做 Intel / universal），所以缺的架构
/// 走这条，由调用方说人话，前端还留着「前往下载页」的人工兜底。
fn pick_asset_for<'a>(assets: &'a [GitHubAsset], os: &str, arch: &str) -> Option<&'a GitHubAsset> {
    // 后缀必须与 tauri-action 的产物名一致：`Stacktake_<版本>_aarch64.dmg`、
    // `Stacktake_<版本>_x64-setup.exe`（见 v0.1.0 的实际资产名）。
    let suffix = match (os, arch) {
        ("macos", "aarch64") => "_aarch64.dmg",
        ("windows", "x86_64") => "x64-setup.exe",
        _ => return None,
    };
    assets.iter().find(|a| a.name.ends_with(suffix))
}

/// 运行平台的人话名字，只用于错误信息。
fn platform_label() -> &'static str {
    match (
        cfg!(target_os = "macos"),
        cfg!(target_os = "windows"),
        cfg!(target_arch = "aarch64"),
    ) {
        (true, _, true) => "macOS (Apple Silicon)",
        (true, _, false) => "macOS (Intel)",
        (_, true, true) => "Windows (ARM64)",
        (_, true, false) => "Windows x64",
        _ => "当前平台",
    }
}

/// 下载最新版安装包到「下载」目录，下完**直接打开**（2026-10-21 用户拍板选 B）。
///
/// 流程：查 latest release → 按平台挑资产 → 流式写盘（进度事件）→ 自动打开。
/// 全程由用户显式发起（设置页的「下载」按钮），没有任何自动触发路径。
/// 返回落盘路径。
#[tauri::command]
pub async fn download_latest_update(app: AppHandle) -> Result<String, String> {
    let client = reqwest::Client::new();

    // ── 1. 查最新 release，拿资产列表 ──
    let res = client
        .get(LATEST_API)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|e| format!("无法连接 GitHub：{e}"))?;
    // 404 的语义是「仓库还没有 release」，不是网络故障
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("仓库还没有发布任何版本".into());
    }
    if !res.status().is_success() {
        return Err(format!("GitHub 返回 {}", res.status()));
    }
    let release: GitHubRelease = res
        .json()
        .await
        .map_err(|e| format!("无法解析 GitHub 响应：{e}"))?;

    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "other"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "other"
    };
    let asset = pick_asset_for(&release.assets, os, arch).ok_or_else(|| {
        format!(
            "没有找到适合本平台（{}）的安装包，请前往下载页手动选择",
            platform_label()
        )
    })?;

    // ── 2. 流式下载到「下载」目录 ──
    let dest_dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("无法定位「下载」文件夹：{e}"))?;
    let dest = dest_dir.join(&asset.name);

    let mut res = client
        .get(&asset.browser_download_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}"))?;
    if !res.status().is_success() {
        return Err(format!("下载失败：GitHub 返回 {}", res.status()));
    }
    let total = res.content_length();

    let mut file = tokio::fs::File::create(&dest)
        .await
        .map_err(|e| format!("无法写入 {}：{e}", dest.display()))?;
    let mut received = 0u64;
    // 进度节流：chunk 通常 64KB 一级，2.5MB 的 dmg 就是几十个事件，不节流也没事；
    // 但包涨到几十 MB 时事件会糊脸，100ms 一刀两边都够用
    let mut last_emit = Instant::now();
    while let Some(chunk) = res.chunk().await.map_err(|e| format!("下载中断：{e}"))? {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入中断：{e}"))?;
        received += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(100) {
            let _ = app.emit(UPDATE_PROGRESS_EVENT, DownloadProgress { received, total });
            last_emit = Instant::now();
        }
    }
    file.flush().await.map_err(|e| format!("写入中断：{e}"))?;
    drop(file);
    // 收尾必报一次：整个包 <100ms 下完时，节流会把中间态全跳过，
    // 少了这次前端就停在 0%（或干脆没进度）
    let _ = app.emit(UPDATE_PROGRESS_EVENT, DownloadProgress { received, total });

    // ── 3. 自动打开（B 方案）。范围见模块注释：只开这一个刚下载的文件 ──
    tauri_plugin_opener::open_path(&dest, None::<&str>)
        .map_err(|e| format!("已下载到 {}，但打开失败：{e}", dest.display()))?;

    Ok(dest.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.to_string(),
            browser_download_url: format!("https://example.invalid/{name}"),
        }
    }

    /// 与 v0.1.0 真实产物同构的两个资产（dmg + nsis exe）。
    fn both_platforms() -> Vec<GitHubAsset> {
        vec![
            asset("Stacktake_0.1.0_aarch64.dmg"),
            asset("Stacktake_0.1.0_x64-setup.exe"),
        ]
    }

    #[test]
    fn macos_arm_picks_the_dmg() {
        let assets = both_platforms();
        let picked = pick_asset_for(&assets, "macos", "aarch64").expect("应有 macOS 包");
        assert_eq!(picked.name, "Stacktake_0.1.0_aarch64.dmg");
    }

    #[test]
    fn windows_x64_picks_the_exe() {
        let assets = both_platforms();
        let picked = pick_asset_for(&assets, "windows", "x86_64").expect("应有 Windows 包");
        assert_eq!(picked.name, "Stacktake_0.1.0_x64-setup.exe");
    }

    #[test]
    fn missing_arch_gets_nothing_rather_than_a_wrong_one() {
        let assets = both_platforms();
        // Intel Mac 下到 arm64 dmg = 装得上跑不起来，比不给更坏
        assert!(pick_asset_for(&assets, "macos", "x86_64").is_none());
        // ARM Windows 同理（矩阵里没有，将来加了也是新后缀）
        assert!(pick_asset_for(&assets, "windows", "aarch64").is_none());
        // Linux 不进发布矩阵
        assert!(pick_asset_for(&assets, "linux", "x86_64").is_none());
    }

    #[test]
    fn one_platform_assets_do_not_leak_to_the_other() {
        // 只有 Windows 资产时，macOS 不该被塞一个 exe
        let assets = vec![asset("Stacktake_0.1.0_x64-setup.exe")];
        assert!(pick_asset_for(&assets, "macos", "aarch64").is_none());
        // 反过来同理
        let assets = vec![asset("Stacktake_0.1.0_aarch64.dmg")];
        assert!(pick_asset_for(&assets, "windows", "x86_64").is_none());
    }

    #[test]
    fn empty_assets_gets_none() {
        assert!(pick_asset_for(&[], "macos", "aarch64").is_none());
    }

    #[test]
    fn suffix_match_is_exact_not_prefix_fuzzy() {
        // 名字里含 dmg 但不是资产后缀的，不算
        let assets = vec![asset("Stacktake_0.1.0.dmg.asc")];
        assert!(pick_asset_for(&assets, "macos", "aarch64").is_none());
    }
}

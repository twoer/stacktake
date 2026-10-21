pub mod cache;
pub mod roots;
pub mod scan;
pub mod update;

use scan::{
    AssetGroup, CleanPlan, CleanResult, CleanScope, Scanner, ai_tools::AiToolsScanner,
    dev_env::dev_env_scanners, local_models::LocalModelsScanner, repos::RepoScanner,
    toolchain::ToolchainScanner, wsl::wsl_scanners,
};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// java 与 harmony 两组**暂时隐藏**（H14，2026-10-20 用户拍板）：不是「没装上」，
/// 是「装了也不进侧栏」。`dev_env` 的构造与测试原样保留（喂假 home 的用例照跑），
/// 只在 [`scanners_with_roots`] 注册时撤下——与 ssh/docker 的「文件在树里、不注册」同一先例。
///
/// 恢复显示 = 清空这份名单 + 同步 `scanner_registration_is_pinned` 里的
/// 名单钉死与缺席断言 + `cache::SCHEMA_VERSION` +1（老缓存里还挂着这两组）。
pub const HIDDEN_GROUPS: &[&str] = &["java", "harmony"];

/// 所有扫描器，按侧栏顺序排列。Git 仓库用**默认根目录**（`roots::default_roots()`）。
///
/// `pub` 是为了让 `examples/dump.rs` 走同一条路径——早先示例里另写了一份列表，
/// 结果新增 Scanner 后示例扫出来的分组和 app 里的对不上。
///
/// **SSH 与 Docker 不在这里**：D6 拍板两个分组都不进 V1，H3 移出了注册。
/// `ssh.rs` / `docker.rs` 两个文件保留在树里（重新注册只是一行），但别顺手加回来。
///
/// **顺序 = [`scan::GROUP_ORDER`] 的固定语义序**（D13 拍板，取代 D3 的体积降序）：
/// 源码 → 开发环境 → 依赖缓存 → 工具数据 → 模型 → 虚拟机。注册顺序与它对齐，
/// `with_overview` 还会再排一次（缓存里的旧快照也要按新序显示）。
/// 总览分组不在这里（它由 `with_overview` 合成，见下）。
///
/// 真实扫描路径走 [`scanners_with_roots`]，根目录固定取 [`roots::default_roots`]——
/// 这个函数保留给「就用默认根目录」的调用方（`dump.rs`、`scanner_registration_is_pinned`）。
///
/// **java 与 harmony 也不在这里**：H14 暂时隐藏（[`HIDDEN_GROUPS`]），恢复路径见那份名单的注释。
pub fn scanners() -> Vec<Box<dyn Scanner>> {
    scanners_with_roots(roots::default_roots())
}

/// 同 [`scanners`]，但 Git 仓库的根目录由调用方给。
///
/// roots 曾经可配置（设置页增删，落盘 `scan-config.json`），2026-10-19 删掉了，
/// 函数本身留下：**它是测试与示例唯一能注入假根目录的入口**
/// （`scanners_with_roots_scans_only_the_given_roots` 靠它钉住「roots 真的传到了
/// RepoScanner」，且不需要本机真有仓库）。删了它，这条覆盖就没了。
pub fn scanners_with_roots(roots: Vec<PathBuf>) -> Vec<Box<dyn Scanner>> {
    let mut v: Vec<Box<dyn Scanner>> = Vec::new();

    // 源码排最前（D13 固定语义序）。
    v.push(Box::new(RepoScanner::new(roots)));

    // 开发环境：Java / iOS / Android / 鸿蒙。环境不存在就不生成分组，侧栏不留空 tab。
    // 判断在 dev_env_scanners 内部做，这里只看它给了什么。
    // java 与 harmony 构造照旧，注册前撤下（[`HIDDEN_GROUPS`]，H14）。
    if let Some(home) = dirs::home_dir() {
        v.append(&mut dev_env_scanners(&home));
        v.retain(|s| !HIDDEN_GROUPS.contains(&s.id()));
    }

    if let Some(home) = dirs::home_dir() {
        // 依赖缓存：探测型，白名单里的缓存目录一个都没有时不进侧栏。
        let toolchain = ToolchainScanner::new(home);
        if toolchain.exists() {
            v.push(Box::new(toolchain));
        }
    }

    // 工具数据：编辑器与 AI 工具（G1 起含编辑器数据，label 已随之改名）。
    if let Some(home) = dirs::home_dir() {
        v.push(Box::new(AiToolsScanner::new(home)));
    }

    // 模型（G2 / D4 选 A：独立分组）。模型是资产不是缓存，整组不可回收。
    // 探测型：三个来源目录一个都不存在时不进侧栏（硬约束 8，同 toolchain）。
    if let Some(home) = dirs::home_dir() {
        let models = LocalModelsScanner::new(home);
        if models.exists() {
            v.push(Box::new(models));
        }
    }

    // 虚拟机：WSL 发行版的虚拟磁盘。只存在于 Windows，非 Windows 平台不注册
    // （硬约束 8：环境不存在就不占侧栏）。
    v.append(&mut wsl_scanners());

    v
}

/// 给分组列表加上总览（放在最前），并按 [`scan::GROUP_ORDER`] 重排。**幂等**：已有总览原样返回。
///
/// 总览是现算的，不进缓存——缓存里存的是各扫描器的原始快照。所以两条路径
/// （扫完返回、读缓存返回）都要过这一道，否则「重启后总览不见了」。
///
/// 重排也放在这里而不是只依赖注册顺序：缓存里的快照是旧顺序，版本升级后
/// 不重扫也应该看到新顺序。排序不改缓存内容，所以不需要 `SCHEMA_VERSION` +1。
pub fn with_overview(groups: Vec<AssetGroup>) -> Vec<AssetGroup> {
    if groups.iter().any(|g| g.id == scan::OVERVIEW_ID) {
        return groups;
    }
    let mut groups = scan::order_groups(groups);
    let overview = scan::synthesize_overview(&groups);
    groups.insert(0, overview);
    groups
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录：{}", e))
}

// ============================================================================
// 启动策略：默认吃缓存，不扫描
//
// 实测全量扫描约 26 秒，瓶颈是目录体积统计（Git 仓库 237 个 + 工具链 36 个条目），
// git 子进程只占 0.14 秒。
//
// 所以启动时**不做任何后台扫描**：
// 1. 有缓存 → 直接返回，窗口毫秒级有内容（读缓存实测毫秒级，具体值见 README）
// 2. 无缓存（首次运行）→ 必须扫一次，否则没有任何数据可显示
// 3. 之后只有用户点「重扫」才真扫
//
// 早先的版本是「先出缓存 + 后台自动重扫」。实测下来自动刷新没有价值：
// 数据变化极慢（本机两百多个仓库里近 7 天有提交的只有个位数），
// 而它换来的是每次打开都在后台啃几十秒磁盘、笔记本风扇转起来、电池掉电。
// 把这个决定权交回用户——想看最新的自己点。
// ============================================================================

/// 启动响应。前端据此决定要不要显示 loading。
#[derive(Clone, serde::Serialize)]
struct BootPayload {
    groups: Vec<AssetGroup>,
    /// 缓存写入时间。前端渲染成「数据更新于 3 小时前」。
    saved_at: i64,
    /// 本次是否真的扫描了。`false` = 吃的缓存。
    scanned: bool,
}

/// 扫描命令的返回。
///
/// `saved_at` 必须由后端给，前端不能自己造：一次全量扫描几十秒到几分钟，用
/// `Date.now()` 近似会让「数据更新于 X」凭空多算一个扫描时长；而且缓存写失败时
/// 前后端就对不上了。它就是 `cache::save` 真正写进文件的那个时间戳，
/// 所以「刚扫完」与「重启后读同一份缓存」看到的基准是同一个。
#[derive(Clone, serde::Serialize)]
struct ScanResult {
    groups: Vec<AssetGroup>,
    saved_at: i64,
}

/// 扫描进度事件名。前端 `useAssets.ts` 里监听同一个名字（两边没有共享常量机制，
/// 改这里记得同步那边）。
const SCAN_PROGRESS_EVENT: &str = "scan-progress";

/// 扫描进度。发射时机：开扫时报一次 `done: 0`（前端点完按钮立刻有进度条可画，
/// 不用等第一个分组扫完），之后每完成一个分组报一次。
///
/// 扫描器是**并行**的，完成顺序不固定，所以前端只按 `done / total` 画比例，
/// 不指望顺序；`label` 只是给「刚刚完成的是谁」做展示。
#[derive(Clone, serde::Serialize)]
struct ScanProgress {
    done: usize,
    total: usize,
    label: String,
}

/// 真扫一次并写缓存。缓存写失败只影响下次启动速度，不该影响本次结果。
///
/// 各扫描器**并行**跑：仓库与工具链缓存的体积统计最重（合起来占全量 26 秒的绝大部分），串行执行会让
/// 每加一个分组重扫时间就累加一层。collect 保序，侧栏顺序不受影响。
async fn run_full_scan(app: &AppHandle) -> Result<ScanResult, String> {
    // 闭包里要 own 一个 AppHandle 来 emit；原引用留着，后面写缓存还要用
    let app_for_scan = app.clone();
    // Git 仓库的根目录固定取默认列表（`~/Documents/Code` 等存在的目录，见 `roots` 模块注释）。
    // 这里曾经读一份可配置文件（C1：设置页增删根目录），2026-10-19 删掉了——
    // 现在扫哪些目录由代码说了算，不随任何外部状态变。
    let roots = roots::default_roots();
    let groups = tauri::async_runtime::spawn_blocking(move || {
        use rayon::prelude::*;
        let scanners = scanners_with_roots(roots);
        let total = scanners.len();

        let _ = app_for_scan.emit(
            SCAN_PROGRESS_EVENT,
            ScanProgress {
                done: 0,
                total,
                label: String::new(),
            },
        );

        // 原子计数供并行闭包共享；Relaxed 足够——只要最终每个完成都被数到一次，
        // 不需要借助它建立别的内存顺序。
        let done = std::sync::atomic::AtomicUsize::new(0);
        scanners
            .par_iter()
            .map(|s| {
                let g = s.scan();
                let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                // emit 失败只影响进度条，不该让扫描本身失败
                let _ = app_for_scan.emit(
                    SCAN_PROGRESS_EVENT,
                    ScanProgress {
                        done: n,
                        total,
                        label: g.label.clone(),
                    },
                );
                g
            })
            .collect::<Vec<AssetGroup>>()
    })
    .await
    .map_err(|e| e.to_string())?;

    // 「数据更新于」以后端写进缓存的那个时间为准；写失败（磁盘满、权限）时退回
    // 当前时间——那种情况下前端下次启动会自己走一遍首扫，这点偏差无关紧要
    //
    // 缓存里存的是各扫描器的原始快照（**不含总览**，总览每次现算），
    // 返回给前端前再过一遍 `with_overview`。
    let mut saved_at = cache::now_secs();
    if let Some(dir) = data_dir(app).ok()
        && let Ok(ts) = cache::save(&dir, &groups)
    {
        saved_at = ts;
    }
    Ok(ScanResult {
        groups: with_overview(groups),
        saved_at,
    })
}

/// 启动：有缓存用缓存，没缓存才扫。
#[tauri::command]
async fn bootstrap(app: AppHandle) -> Result<BootPayload, String> {
    let dir = data_dir(&app).ok();

    // 先直接读一次缓存。读到了就用，没有才扫。
    //
    // 这里不再走 `cache::decide_boot`：那个函数是给测试和示例用的单一决策点，
    // 真调用处直接 `load` 一次更简单，也避免了「判断说有、实际读不出」的二次分支。
    if let Some(snap) = dir.as_deref().and_then(cache::load) {
        return Ok(BootPayload {
            // 总览现算：缓存里不存它（存了就会出现「缓存里的总览与分组对不上」）
            groups: with_overview(snap.groups),
            saved_at: snap.saved_at,
            scanned: false,
        });
    }

    // 首次运行（或缓存被清）：没有数据就是空壳，只能扫。
    let r = run_full_scan(&app).await?;
    Ok(BootPayload {
        groups: r.groups,
        saved_at: r.saved_at,
        scanned: true,
    })
}

/// 手动重扫：等扫完再返回，前端据此更新 loading 态与「数据更新于」的时间。
///
/// H1 删掉了与它逐字相同的 `scan_all`（两个命令体都是 `run_full_scan(&app).await`，
/// 而前端只调 `rescan_now`）。别再加回来——重复的 IPC 面只是多一个可被 webview 调的入口。
#[tauri::command]
async fn rescan_now(app: AppHandle) -> Result<ScanResult, String> {
    run_full_scan(&app).await
}

/// 在系统文件管理器里**揭示**一个路径（访达 / 资源管理器里选中它）。
///
/// **只揭示、不打开**：打开一个路径会执行它（`.app` / `.command` / 安装包），
/// 与「不读文件内容」同一条理由——不该碰的东西就不碰。这条边界写在 README
/// 「安全边界」里，别为了省一次点击把它放宽成 `open`。
///
/// 前端已经用 `isLocalPath()` 挡掉过非宿主机路径（docker 的 `docker://…` 是
/// 虚拟机里的东西，没有可揭示的位置），这里再判一次**存在性**：缓存可能已经过期
/// （条目被删了、外置盘没挂上）。自己判一次才能给出人话，而不是把插件的
/// 英文错误原样抛给用户。
#[tauri::command]
async fn reveal_path(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("路径已不存在，重扫一次".into());
    }
    tauri_plugin_opener::reveal_item_in_dir(&path)
        .map_err(|e| format!("无法在文件管理器中定位：{e}"))
}

/// 清空缓存。下次启动会走完整扫描。
///
/// C1 起它有了第一个前端调用者：设置页的「清空缓存」。H1 时它是「有 handler 无
/// 调用者」的三个 command 之一，D2 拍板留它就是因为这个去处——另两个
/// （`load_cached` 被 bootstrap 包含、`scan_group` 的懒加载前端没采用）已删。
#[tauri::command]
async fn clear_cache(app: AppHandle) -> Result<(), String> {
    let dir = data_dir(&app)?;
    cache::remove(&dir);
    Ok(())
}

/// 清理入口（S2 / D1 拍板选 A）。**唯一的 destructive command**，三道闸：
/// ① `CleanPlan.confirmed` 缺省反序列化即拒、显式 `false` 服务端再拒；
/// ② 逐项重跑复核——范围不同复核不同：整条目（P2 前）查未推送提交与
///    未提交改动；产物范围（P2）查白名单与「里面没藏仓库」（扫描到点击
///    之间状态可能变了，缓存里的账面不是许可证）；
/// ③ `trash::delete` 移入废纸篓，绝不硬删。
///
/// 只服务 repos 分组（P2 起组内有两种删除：整仓 / 构建产物，按
/// `CleanPlan.scope` 派发）、toolchain 分组（仅整条目）与 ai_tools 分组
/// （B1：整条目 = 一个「工具 × 类别」下的全部匹配条目）。确认弹窗把
/// 「删什么」写在明处。
///
/// 执行清理必须人显式发起：门禁只验代码与确认流，测试不碰 `trash::delete`
/// 的成功路径（真删留给真人）。
#[tauri::command]
async fn clean_items(plan: CleanPlan) -> Result<CleanResult, String> {
    match plan.group_id.as_str() {
        // 复核在各自 Scanner 内部做（repos 的两种范围各有复核，toolchain 查
        // 白名单与是否仍可清理），这里只负责派发——**别把判据收到 command 里**，
        // 收上来就成了「只有这一条路径会复核」的错觉。
        "repos" => {
            // 与 `run_full_scan` 同一个来源：删掉可配置根目录后，扫描与清理
            // 复核用的是同一份默认列表，不会出现「扫的时候是 A、复核时是 B」。
            let scanner = RepoScanner::new(roots::default_roots());
            match plan.scope {
                CleanScope::Repo => Ok(scanner.clean_repo(&plan)),
                CleanScope::Artifacts => Ok(scanner.clean_artifacts(&plan)),
            }
        }
        "toolchain" => {
            if plan.scope != CleanScope::Repo {
                return Err("toolchain 分组没有产物清理范围".into());
            }
            let home = dirs::home_dir().ok_or("无法定位用户主目录，无法定位缓存目录")?;
            Ok(ToolchainScanner::new(home).clean_cache(&plan))
        }
        // B1：ai_tools 的一行 = 一个「工具 × 类别」。复核（白名单根、类别仍
        // 可清理、条目当前分类）都在 `clean_ai` 里做，这里只给 home。
        "ai_tools" => {
            if plan.scope != CleanScope::Repo {
                return Err("ai_tools 分组没有产物清理范围".into());
            }
            let home = dirs::home_dir().ok_or("无法定位用户主目录，无法定位工具数据目录")?;
            Ok(AiToolsScanner::new(home).clean_ai(&plan))
        }
        other => Err(format!("分组 {} 不支持清理", other)),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            rescan_now,
            clear_cache,
            reveal_path,
            clean_items,
            // 应用内下载更新（Rust 侧，原因见 update.rs 模块注释）
            update::download_latest_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

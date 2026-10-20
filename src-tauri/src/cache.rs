//! 扫描结果缓存。
//!
//! # 为什么需要
//!
//! 实测本机全量扫描约 26 秒（2026-10-13 重测，debug 构建，**6 组**并行）。
//! 首测是 7 组（U3），SSH 与 Docker 两组随后移出注册（H3）——那两组一个是 0 B、
//! 一个只有 notes 没有 `items`，对耗时不构成量级影响，所以这个 26 秒没有变化。拆开看：
//! - 目录体积统计（原生遍历，`scan/dirsize.rs`）：Git 仓库 237 个 + 工具链 36 个条目（瓶颈）
//! - git 子进程（每仓库 5 次）：0.14 秒（可忽略）
//! - 读缓存：毫秒级（具体值与出处只在 README「扫描策略」写一次，别在这里再写一个数）
//!
//! 也就是说时间几乎全花在读文件元数据上，而数据变化极慢——
//! 本机两百多个仓库里近 7 天有提交的只有个位数。每次打开都重扫没有意义。
//!
//! # 策略：默认吃缓存
//!
//! 1. 有缓存 → 直接返回，窗口毫秒级有内容，**不做任何后台扫描**
//! 2. 无缓存（首次运行）→ 扫一次，否则没有数据可显示
//! 3. 之后只有用户手动点「重扫」才真扫
//!
//! 早先的版本是「先出缓存 + 后台自动重扫」。去掉自动刷新是因为
//! 它没有实际收益（数据很少变），却让每次打开都在后台啃几十秒磁盘。
//!
//! # 失效
//!
//! 缓存带 `schema` 版本号。`AssetGroup` 字段一变就 `+1`，
//! 老缓存读出来直接丢弃，不需要手工清理。

use crate::scan::AssetGroup;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 缓存文件格式版本。`AssetGroup` 结构变更时 +1，旧缓存自动作废。
///
/// 还有一种情况也要 +1：**新增或移除 Scanner**。启动逻辑是「有缓存直接返回、不扫描」，
/// 老用户拿到的会是缺某个分组（或还挂着已移除的分组）的旧快照，而且界面上没有任何提示
/// 告诉他该点「重扫」。版本 +1 让老缓存作废、走一次首扫，代价是一次性的。
///
/// **第三种：可回收边界变更**（S3，9 → 10）。结构没动，但**账面数字变了**——
/// toolchain 从 23 条可回收收窄到 17 条（6 条有官方清理命令的改不可回收）。
/// 版本不变的话老用户读到的旧快照里那些条目仍标着 `cleanable`，界面会给出
/// 「清理」按钮，而服务端按新白名单会拒——**账面与真删对不上，且老用户看不出原因**。
/// **第四种：id 或能力字段变更**（B1，10 → 11）。ai_tools 的 id 从
/// `ai:<工具>:<类别>` 改为 `ai:<类别键>:<工具根路径>`，组级 `supports_clean`
/// 也从 false 变 true。结构没动，但老缓存里的 id 解析不出路径、界面也不会
/// 出现清理入口——版本不变的话用户点了「重扫」仍看到旧 id 的快照。
///
/// **第五种：扫描范围变更**（删可配置根目录，11 → 12）。结构没动，但**扫哪些目录变了**：
/// 根目录曾经可由设置页配置（`scan-config.json`），2026-10-19 删掉后固定为默认列表。
/// 自定义过根目录的老用户，缓存里存的是**按旧范围扫出来的**快照——界面会继续显示
/// 那些已经不在扫描范围里的仓库，且没有任何提示说明它们是旧的。
/// 与「增删 Scanner」同一类问题，同一套处理：版本 +1，让老缓存作废走一次首扫。
///
/// **第六种：口径变更**（硬链接去重，S4 / D14，12 → 13）。结构与范围都没动，
/// 但**数字的算法变了**：行走按 (dev, inode) 去重后普遍变小（硬链接密集的
/// cargo `target` 差 4 倍多），detail 还多了「产物明细」键。老缓存里的数字
/// 与新口径不可比——不跳版本的话，「数据更新于」会让人误以为数字没变。
///
/// **第七种：暂时隐藏分组**（H14，13 → 14）。java 与 harmony 两组从侧栏撤下、
/// 扫描器代码原样保留——对缓存的影响与「移除 Scanner」完全相同：老快照里还挂着
/// 这两组，版本不变的话它们会一直显示到用户点「重扫」，而界面上没有任何提示。
pub const SCHEMA_VERSION: u32 = 14;

const CACHE_FILE: &str = "scan-cache.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEnvelope {
    /// 结构版本，不匹配则丢弃缓存
    pub schema: u32,
    /// 写入时的 unix 时间戳（秒），用于「数据更新于 X」提示
    pub saved_at: i64,
    pub groups: Vec<AssetGroup>,
}

/// 前端拿到的完整响应。
#[derive(Debug, Clone, Serialize)]
pub struct CachedSnapshot {
    pub groups: Vec<AssetGroup>,
    pub saved_at: i64,
    /// 距今多久（秒），前端渲染成「3 小时前更新」
    pub age_seconds: i64,
}

pub fn cache_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CACHE_FILE)
}

/// 读缓存。任何异常都当作「没有缓存」——缓存是加速手段，绝不能因为它启动失败。
pub fn load(app_data_dir: &Path) -> Option<CachedSnapshot> {
    let raw = std::fs::read_to_string(cache_path(app_data_dir)).ok()?;
    let env: CacheEnvelope = serde_json::from_str(&raw).ok()?;

    if env.schema != SCHEMA_VERSION {
        return None;
    }
    if env.groups.is_empty() {
        return None;
    }

    let age = now_secs() - env.saved_at;
    Some(CachedSnapshot {
        groups: env.groups,
        saved_at: env.saved_at,
        // 时钟回拨会让 age 变负，钳到 0
        age_seconds: age.max(0),
    })
}

/// 写缓存。原子写（先写临时文件再 rename），避免进程被杀时留下半截 JSON。
///
/// 返回**写进文件的那个时间戳**。调用方（`lib.rs::run_full_scan`）要把它交给前端做
/// 「数据更新于 X」的基准：前端自己用 `Date.now()` 近似会带上一次扫描的时长偏差
/// （一次全量扫描几十秒到几分钟），而且缓存写失败时两边就对不上了。
pub fn save(app_data_dir: &Path, groups: &[AssetGroup]) -> std::io::Result<i64> {
    std::fs::create_dir_all(app_data_dir)?;

    let env = CacheEnvelope {
        schema: SCHEMA_VERSION,
        saved_at: now_secs(),
        groups: groups.to_vec(),
    };
    let json = serde_json::to_string(&env)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let target = cache_path(app_data_dir);
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &target)?;
    Ok(env.saved_at)
}

/// 启动决策的取值。抽成独立函数是为了能单测——`bootstrap` command 本身
/// 需要 `AppHandle`，在测试里构造起来太重。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boot {
    /// 有缓存可用，直接返回，不许触发任何扫描。
    UseCache,
    /// 没有缓存（首次运行），必须扫一次。
    NeedScan,
}

/// 「默认吃缓存，不扫描」这条规则的显式表达。
///
/// 只给测试和 `examples/dump.rs` 用。真正的启动路径在 `lib.rs::bootstrap`，
/// 那里直接 `load` 一次读缓存即可，不需要这层间接——真实数据源只有一个，
/// 多包一层反而容易让两处逻辑走偏。
pub fn decide_boot(app_data_dir: Option<&Path>) -> Boot {
    match app_data_dir.and_then(load) {
        Some(_) => Boot::UseCache,
        None => Boot::NeedScan,
    }
}

pub fn remove(app_data_dir: &Path) {
    let _ = std::fs::remove_file(cache_path(app_data_dir));
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

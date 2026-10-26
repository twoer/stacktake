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

use crate::scan::{AssetGroup, MetricValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
///
/// **第八种：单组重扫与逐组时间戳**（14 → 15）。信封新增 `group_saved_at`：
/// 单组重扫之后不同分组的新鲜度不同，只有信封级 `saved_at` 的话，「数据更新于」
/// 会拿最新一次写入的时间冒充全体数据的新鲜度。老缓存里没有这份映射，
/// 各分组读不出各自的扫描时间，作废走一次首扫。
///
/// **第九种：repos 清理口径收紧**（15 → 16）。推送状态核查改 fail-closed
/// （没配 upstream / detached HEAD / git 失败的仓库从前按「无未推送」放行）、
/// stash 非空开始一票否决——老缓存里这些仓库的 `cleanable` 是按旧口径算的，
/// 不作废的话账面会宣称一批点下去会被拒的「可回收」。
///
/// **第十种：本轮三处账目与范围变更**（16 → 19）。三处都在同一批未提交的改动里，
/// 中间那些版本号没有对外发布过，所以按理由合写而不是逐个编号——分开写只能编造
/// 先后，写错的版本号比不写更害人（它会让下一次改动照着错的沿革往下推）：
/// - **产物明细从截断文本改成完整集合**（`Item::artifact_entries`）：老快照里只有
///   「构建产物」那一行总账，没有可勾选的目录集合，「清理产物」点开是空的。字段带
///   `serde(default)`，老缓存**读得出来**——正因如此才必须跳版本：它不报错，只是
///   静默少一块能力。
/// - **ai_tools 新增只读根**：`READ_ONLY_SPECS` 补进一批工具目录（含 Windows 实机核对
///   过的 14 个位置），老快照里没有这些条目，界面只会当它们不存在。
/// - **补充目录进信封**（`extra_repo_roots`）：信封新增字段且**没有** `serde(default)`，
///   老缓存反序列化直接失败、等于自动失效——但仍要登记：哪天为了兼容性给它加上
///   default，这条就是「那时必须换个失效办法」的依据。
///
/// **第十一种：明细与文案契约（P2b）**（19 → 20 → 21）。两刀同处一批未发布的改动，
/// 20 号只活在中间态，按第十种的先例合写：
/// - 20：后端下发**文案键**（P2a，Track I）——`Text` / `Tag{key,label}` /
///   `MetricValue::TextKey` 进缓存，成品中文从此不出现在展示位。
/// - 21：`Item.detail` 从 `HashMap<String, String>` 改成 `Vec<DetailRow{label, value}>`
///   （两边都是 `Text`，顺序即数组顺序，`N. ` 数字前缀随之消亡）。字段带
///   `serde(default)`，老缓存读得出来但 detail 是空数组——静默缺失正是第十种
///   立下的「仍要跳版本」的理由。`CleanFailure` 不进缓存，但同批改了 `Text`，
///   记在这条里免得下次沿革断档。
///
/// **第十二种：上一版基线进信封**（L1，21 → 22）。新增 `prev`（[`PrevLedger`]），
/// 带 `serde(default)`——空 = 无基线是合法态（首次运行 / 清缓存后），老缓存读得出来
/// 但基线是空的，差额一栏会静默消失。与第十种给 `extra_repo_roots` 留的备注同理：
/// 带 default 的静默缺失正是要跳版本防的陷阱，不跳的话老用户在界面上看不出
/// 「没有差额」是「没有基线」还是「功能坏了」，还以为下次扫描就会补上。
///
/// **第十三种：ai_tools 白名单扩充**（M1，22 → 23）。AI_SPECS +6、READ_ONLY_SPECS
/// +1——启动是「有缓存直接返回」，版本不跳的话，装了这些工具的老用户拿到的旧快照
/// 里永远缺这几行，界面上没有任何提示让他点「重扫」（硬约束 2 的同款理由）。
///
/// **第十四种：ai_tools 白名单第二批**（M2，23 → 24）。AI_SPECS +4、READ_ONLY_SPECS
/// +1（Zed），同第十三种的理由。
///
/// **第十五种：两栏表合并 + goose 进表**（M3，24 → 25）。`EDITOR_SPECS` /
/// `READ_ONLY_SPECS` 合并为 `PLATFORM_SPECS`（清理开关下放到条目，win 侧补
/// Option），goose 两条新进——扫描范围变了，同第十三种的理由跳版本。
///
/// **第十六种：本机实测缺口五条**（M4，25 → 26）。AI_SPECS +2（Hermes Agent /
/// Acode）、PLATFORM_SPECS +3（QClaw / OpenClaw AutoClaw / Trae 桌面版），同
/// 第十三种的理由。
pub const SCHEMA_VERSION: u32 = 26;

const CACHE_FILE: &str = "scan-cache.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEnvelope {
    /// 结构版本，不匹配则丢弃缓存
    pub schema: u32,
    /// 写入时的 unix 时间戳（秒），用于「数据更新于 X」提示
    pub saved_at: i64,
    /// 各分组各自的写入时间。全量扫描时全部等于 `saved_at`；单组重扫后只有
    /// 被重扫的组更新，其余组保留旧值——「数据更新于」按它逐组取，不拿
    /// 信封级时间冒充全体新鲜度。BTreeMap 让 JSON 键序确定（detail 的
    /// HashMap 键序教训，见 scan::Item）。
    pub group_saved_at: BTreeMap<String, i64>,
    pub groups: Vec<AssetGroup>,
    pub extra_repo_roots: Vec<PathBuf>,
    /// 上一版基线（差额台账）。空 = 无基线，整个字段省略——首扫与清缓存后
    /// 的合法态，前端对缺失与空同义看待。带 `default` 仍要跳 `SCHEMA_VERSION`
    /// 的理由见常量注释第十二种。
    #[serde(default, skip_serializing_if = "PrevLedger::is_empty")]
    pub prev: PrevLedger,
}

/// 上一版基线（L1 / D18 拍板 A：只留上一版，不做 N 版历史）。
///
/// 差额台账回答的只有一个问题——「较上次谁涨了多少」。「上次」在单组重扫
/// 语境下天然是逐组的，与 `group_saved_at` 同构：每次写入把「当前」轮转成
/// 「上一版」（[`rotate`]），基线逐组更新，各组的对比基准互不牵连。
///
/// **只记 `metrics["size"]` 为 `Bytes` 的条目**：repos 遍历超时写 `Text("—")`、
/// wsl / local_models 体积未知时不写 size——这些行没有可比的体积，记进去
/// 只能靠前端拿 0 兜底，会把超时行显示成假负差额。这里与前端（严格 Bytes
/// 口径）一致地跳过它们。
///
/// 空 = 无基线（首次运行 / 清缓存后），是合法态；三个映射全空时序列化整个省略。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrevLedger {
    /// 基线时刻各分组的扫描时间。检查器整组差额的提示按它渲染「与 X 前的扫描对比」。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub group_saved_at: BTreeMap<String, i64>,
    /// 组 → 条目 id → 体积。**按组成组存放**而不是一张扁平大表：单组轮转
    /// （Refresh / Drop）要能明确圈出「哪些条目属于这个组」——repos 的条目 id
    /// 是裸路径，从 id 本身判断不了归属。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub item_sizes: BTreeMap<String, BTreeMap<String, u64>>,
    /// 组 → 合计体积（[`AssetGroup::total_size`] 口径：只数 Bytes，缺失按 0）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub group_totals: BTreeMap<String, u64>,
}

impl PrevLedger {
    pub fn is_empty(&self) -> bool {
        self.group_saved_at.is_empty() && self.item_sizes.is_empty() && self.group_totals.is_empty()
    }

    /// 只挑出有可比体积（`size` 为 `Bytes`）的条目。非 Bytes 的 size（超时的
    /// `Text("—")`）与缺失的 size 都不算——理由见结构体注释。
    fn item_sizes_of(g: &AssetGroup) -> BTreeMap<String, u64> {
        g.items
            .iter()
            .filter_map(|it| match it.metrics.get("size") {
                Some(MetricValue::Bytes(b)) => Some((it.id.clone(), *b)),
                _ => None,
            })
            .collect()
    }

    /// 把一份旧快照整体转成基线。`times` 里指向不存在分组的残留不收
    /// （基线的键集与分组对齐，死组不留在映射里）。
    fn from_snapshot(groups: &[AssetGroup], times: &BTreeMap<String, i64>) -> Self {
        let mut ledger = PrevLedger::default();
        for g in groups {
            if let Some(t) = times.get(&g.id) {
                ledger.group_saved_at.insert(g.id.clone(), *t);
            }
            ledger
                .item_sizes
                .insert(g.id.clone(), Self::item_sizes_of(g));
            ledger.group_totals.insert(g.id.clone(), g.total_size());
        }
        ledger
    }

    /// 从基线里摘掉一个分组的全部条目。
    fn remove_group(&mut self, id: &str) {
        self.group_saved_at.remove(id);
        self.item_sizes.remove(id);
        self.group_totals.remove(id);
    }
}

/// 轮转依据：这次写入把哪个范围的「当前」变成「上一版」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotateBasis {
    /// 全量重扫：基线整体换成旧当前值。旧基线丢弃——比较对象始终是
    /// 「上一次扫描」，不是「上上次」。
    Full,
    /// 单组重扫：该组基线换成旧当前值，其余组沿用旧基线（它们的「上次」没变）。
    Refresh(String),
    /// 摘组（环境消失，`remove_group_from_cache`）：该组基线全部删除，
    /// 死 id 不永久留在映射里。
    Drop(String),
}

/// 轮转：由「旧基线 + 旧当前快照 + 旧逐组时间」算出下一次写入该带的基线。
///
/// **纯函数**：不碰磁盘、不读缓存，喂样本即可测。调用方在覆盖缓存**之前**调它
/// ——旧当前值一旦被新快照覆盖就再也拿不回来了。
///
/// `Refresh` 对「旧当前里没有这个组，或它已经空了」按 [`RotateBasis::Drop`] 处理：
/// 空组换上来的基线会让界面把整组当前值显示成「较上次 +全组」。补充目录变更后
/// `load` 会把 repos 清空，那种情况走单组重扫正落在这里——跨范围的对比从根上
/// 就不该出现，不留基线比留一个假基线诚实。
pub fn rotate(
    prev_old: &PrevLedger,
    groups_old: &[AssetGroup],
    times_old: &BTreeMap<String, i64>,
    basis: &RotateBasis,
) -> PrevLedger {
    match basis {
        RotateBasis::Full => PrevLedger::from_snapshot(groups_old, times_old),
        RotateBasis::Drop(id) => {
            let mut next = prev_old.clone();
            next.remove_group(id);
            next
        }
        RotateBasis::Refresh(id) => {
            let mut next = prev_old.clone();
            match groups_old.iter().find(|g| g.id == *id) {
                Some(g) if !g.items.is_empty() => {
                    next.item_sizes
                        .insert(id.clone(), PrevLedger::item_sizes_of(g));
                    next.group_totals.insert(id.clone(), g.total_size());
                    if let Some(t) = times_old.get(id) {
                        next.group_saved_at.insert(id.clone(), *t);
                    }
                    next
                }
                _ => {
                    next.remove_group(id);
                    next
                }
            }
        }
    }
}

/// 前端拿到的完整响应。
#[derive(Debug, Clone, Serialize)]
pub struct CachedSnapshot {
    pub groups: Vec<AssetGroup>,
    pub saved_at: i64,
    /// 各分组各自的写入时间，语义见 [`CacheEnvelope::group_saved_at`]。
    pub group_saved_at: BTreeMap<String, i64>,
    /// 距今多久（秒），前端渲染成「3 小时前更新」
    pub age_seconds: i64,
    pub extra_repo_roots: Vec<PathBuf>,
    /// 上一版基线（差额台账）。空时不序列化，前端按「无基线」处理。
    #[serde(skip_serializing_if = "PrevLedger::is_empty")]
    pub prev: PrevLedger,
}

pub fn cache_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CACHE_FILE)
}

/// 读缓存。任何异常都当作「没有缓存」——缓存是加速手段，绝不能因为它启动失败。
pub fn load(app_data_dir: &Path) -> Option<CachedSnapshot> {
    let raw = std::fs::read_to_string(cache_path(app_data_dir)).ok()?;
    let mut env: CacheEnvelope = serde_json::from_str(&raw).ok()?;

    if env.schema != SCHEMA_VERSION {
        return None;
    }
    if env.groups.is_empty() {
        return None;
    }

    // 配置落盘后扫描可能中断；不展示旧范围的可清理条目，也不因此启动全量扫描。
    let config_error = match crate::roots::load_extra(app_data_dir) {
        Ok(extra) if extra != env.extra_repo_roots => {
            env.extra_repo_roots = extra;
            Some(crate::scan::Text::key("data.error.repoRootsChanged"))
        }
        Ok(_) => None,
        Err(e) => Some(
            crate::scan::Text::fmt("data.error.repoRootsRead")
                .arg("error", crate::scan::Text::raw(e.to_string())),
        ),
    };
    if let Some(message) = config_error
        && let Some(repos) = env.groups.iter_mut().find(|g| g.id == "repos")
    {
        repos.items.clear();
        repos.summary = message.clone();
        repos.error = Some(message);
        repos.supports_clean = false;
        let roots =
            crate::roots::merged_roots(crate::roots::default_roots(), &env.extra_repo_roots);
        repos.notes = Some(vec![
            crate::scan::Text::fmt("data.note.reposRoots")
                .arg("roots", crate::scan::repos::display_roots(&roots)),
        ]);
    }

    let age = now_secs() - env.saved_at;
    Some(CachedSnapshot {
        groups: env.groups,
        saved_at: env.saved_at,
        group_saved_at: env.group_saved_at,
        // 时钟回拨会让 age 变负，钳到 0
        age_seconds: age.max(0),
        extra_repo_roots: env.extra_repo_roots,
        prev: env.prev,
    })
}

/// 写缓存。原子写（先写临时文件再 rename），避免进程被杀时留下半截 JSON。
///
/// 返回**写进文件的那个时间戳**。调用方（`lib.rs::run_full_scan`）要把它交给前端做
/// 「数据更新于 X」的基准：前端自己用 `Date.now()` 近似会带上一次扫描的时长偏差
/// （一次全量扫描几十秒到几分钟），而且缓存写失败时两边就对不上了。
pub fn save(app_data_dir: &Path, groups: &[AssetGroup]) -> std::io::Result<i64> {
    save_with_times(
        app_data_dir,
        groups,
        &BTreeMap::new(),
        &[],
        &PrevLedger::default(),
    )
}

/// 同 [`save`]，但各分组的写入时间由调用方给。单组重扫用：只有被重扫的组记
/// 新时间，其余组沿用它们上一次的值。映射里缺的组按**本次**写入时间记——
/// 调用方（`lib.rs::run_group_scan`）负责把旧映射传进来，这里不读旧缓存。
///
/// `prev` 是写进信封的基线，由调用方用 [`rotate`] 在覆盖旧缓存**之前**算好：
/// 旧当前值一旦被新快照覆盖就再也拿不回来了。首扫（没有旧快照）传 default。
pub fn save_with_times(
    app_data_dir: &Path,
    groups: &[AssetGroup],
    group_saved_at: &BTreeMap<String, i64>,
    extra_repo_roots: &[PathBuf],
    prev: &PrevLedger,
) -> std::io::Result<i64> {
    std::fs::create_dir_all(app_data_dir)?;

    let now = now_secs();
    let times: BTreeMap<String, i64> = groups
        .iter()
        .map(|g| {
            let t = group_saved_at.get(&g.id).copied().unwrap_or(now);
            (g.id.clone(), t)
        })
        .collect();
    let env = CacheEnvelope {
        schema: SCHEMA_VERSION,
        saved_at: now,
        group_saved_at: times,
        groups: groups.to_vec(),
        extra_repo_roots: extra_repo_roots.to_vec(),
        prev: prev.clone(),
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

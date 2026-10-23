//! 扫描器统一契约。
//!
//! 每种资产（git repo / npm cache / 编辑器数据…）实现一个 `Scanner`。
//! 前端只认 `AssetGroup`，不关心来源——加新资产类型不需要动前端。
//!
//! **这里刻意没有模块级 `#![allow(dead_code)]`**（H1 之前有）。它会连整棵扫描树的
//! 死代码警告一起咽掉——`ssh.rs::_unused` 那种函数能在里面藏几个月不被发现。
//! 要留一个暂时没有调用者的项，就在**那一项**上加 `#[allow(dead_code)]` 并写清为什么。

pub mod ai_tools;
pub mod dev_env;
pub mod dirsize;
pub mod docker;
pub mod local_models;
pub mod platform;
pub mod repos;
pub mod ssh;
pub mod tests;
pub mod toolchain;
pub mod wsl;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 前端据此渲染表头。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeric: Option<bool>,
    /// 相对时间展示（多久之前）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal: Option<bool>,
}

/// 指标值。
///
/// # 为什么不用 untagged
///
/// 最初写成 `#[serde(untagged)]`，序列化后是裸值（`1700000000`），
/// 反序列化时无法区分 `Int` 和 `Bytes`——两者在 JSON 里长得一样。
/// 结果缓存读回来后时间戳全变成字节，界面把「最后提交」显示成 `1.7 GB`。
///
/// 改为显式带标签的形式：`{"t":"i","v":1700000000}`。
/// 前端的 `formatMetric` 需要同步改成分派 `t` 字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "v")]
pub enum MetricValue {
    #[serde(rename = "s")]
    Text(String),
    /// 字节数。JSON 里是大数字，必须和 Int 严格区分。
    #[serde(rename = "b")]
    Bytes(u64),
    #[serde(rename = "i")]
    Int(i64),
    #[serde(rename = "z")]
    Bool(bool),
}

impl MetricValue {
    pub fn text(s: impl Into<String>) -> Self {
        MetricValue::Text(s.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    /// 分组标签，如「公司 / 个人」「陈旧 / 大」
    #[serde(default)]
    pub tags: Vec<String>,
    pub name: String,
    /// 分节依据：这一条属于本组哪个「维度取值」（G1 起）。`None` = 本组不分节。
    ///
    /// 在此之前前端是靠解析 `name` 里的 ` · ` 前缀分节的——那是个显示约定，
    /// 改一次名字分节就会散，而且同一个分隔符在 toolchain 是「栈」、在 ai_tools
    /// 是「工具名」、在 java 是「组件」，前端无从分辨。正式化成字段之后
    /// 分节与（后续的）筛选都读它，名字只是给人看的。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_key: Option<String>,
    pub path: String,
    #[serde(default)]
    pub metrics: HashMap<String, MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cleanable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reclaimable: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetGroup {
    pub id: String,
    pub label: String,
    /// 单行概览
    pub summary: String,
    pub fields: Vec<Field>,
    pub items: Vec<Item>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// **本组有没有删除实现**（S3 新增）。
    ///
    /// 前端据此决定要不要渲染清理入口，**别在前端写死分组 id**——S3 之前
    /// `canClean` 判的就是 `current.id === "repos"`，加第二个分组就得去改那个字符串，
    /// 而漏改的后果是「界面给按钮、服务端拒绝」或反过来（更坏：界面不给、用户
    /// 以为没有清理功能）。**能力的唯一真相在 Rust 侧**，前端只读。
    ///
    /// 与行级的 `Item.cleanable` 是两个粒度：本字段说「这一组能不能清」，
    /// 行级说「这一行给不给按钮」——toolchain 组内 `supports_clean` 为 true，
    /// 但有官方清理命令的那 6 条 `cleanable` 恒 false；ai_tools 组也是 true，
    /// 但只有日志 / 会话 / 缓存三类行可清理。
    ///
    /// `skip_serializing_if` 是 **false 才省**——为 true 的组（repos / toolchain /
    /// ai_tools）会正常下发这个字段，前端 `undefined` 视作 false，缺省安全。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub supports_clean: bool,
}

/// 总览分组的 id。前端靠它认出「这是合成分组」：它的行点了会跳进对应分组。
pub const OVERVIEW_ID: &str = "overview";

/// 侧栏分组的固定顺序（D13 拍板）：源码 → 开发环境 → 依赖缓存 → 工具数据 → 模型 → 虚拟机。
///
/// **不按体积排**。D3 原先拍的是体积降序，但落地成了一份写死的注册顺序快照——
/// 体积是活的，快照不会跟着变；真做成动态排序，分组又会在每次重扫后跳位置，
/// 用户失去空间记忆。固定序的代价是体积不再单调，换来的是位置稳定与同类相邻。
///
/// 顺序的**唯一真相**在这里：`scanners_with_roots` 的注册顺序与它对齐，
/// `with_overview` 在合成总览前也按它重排（缓存里的旧快照同样受益）。
/// 新增 Scanner 时把 id 加进来；`scanner_registration_follows_group_order` 守着。
pub const GROUP_ORDER: &[&str] = &[
    "repos",
    "java",
    "ios",
    "android",
    "harmony",
    "toolchain",
    "ai_tools",
    "local_models",
    "wsl",
];

/// 按 [`GROUP_ORDER`] 稳定重排。未知 id 排在最后且保持原有相对顺序——
/// 将来新增分组忘了登记时只表现为位置靠后，不会静默消失。
pub fn order_groups(mut groups: Vec<AssetGroup>) -> Vec<AssetGroup> {
    groups.sort_by_key(|g| {
        GROUP_ORDER
            .iter()
            .position(|id| *id == g.id)
            .unwrap_or(GROUP_ORDER.len())
    });
    groups
}

/// 把一份新扫的分组并入既有快照：按 id 替换，快照里没有就追加。
///
/// 单组重扫的合并逻辑（`lib.rs::run_group_scan`）。**纯函数**：不碰磁盘、
/// 不起 Scanner，喂样本即可测。顺序不用这里操心——调用方随后过
/// `with_overview`，内部的 `order_groups` 会按 [`GROUP_ORDER`] 重排。
pub fn upsert_group(mut groups: Vec<AssetGroup>, fresh: AssetGroup) -> Vec<AssetGroup> {
    match groups.iter_mut().find(|g| g.id == fresh.id) {
        Some(slot) => *slot = fresh,
        None => groups.push(fresh),
    }
    groups
}

impl AssetGroup {
    /// 本组合计体积（只数 `size` 指标里真是字节的项）。
    pub fn total_size(&self) -> u64 {
        self.items
            .iter()
            .filter_map(|it| match it.metrics.get("size") {
                Some(MetricValue::Bytes(b)) => Some(*b),
                _ => None,
            })
            .sum()
    }

    /// 本组可回收合计。只累加标了 `cleanable` 的——没标的回收量不该进总数
    /// （前端契约里也守着这条：有回收量必须标 cleanable）。
    pub fn total_reclaimable(&self) -> u64 {
        self.items
            .iter()
            .filter(|it| it.cleanable)
            .filter_map(|it| it.reclaimable)
            .sum()
    }
}

/// 合成「总览」分组（G1 / D3 拍板：范围 A）。
///
/// **纯函数**：只读传入的分组，不碰磁盘、不起 Scanner、不占 `scanners()`。
/// 两条调用路径都过它——`run_full_scan` 扫完返回前，与 `bootstrap` 读缓存后——
/// 所以「刚扫完」与「重启吃缓存」看到的总览一模一样。
///
/// 为什么不让某个 Scanner 产它：总览描述的是**别的分组的集合**，让一个 Scanner
/// 去扫别的 Scanner 的结果，依赖方向是反的；而且它不进缓存（缓存存的是各扫描器
/// 的原始快照，总览每次现算，不会出现「缓存里的总览与分组对不上」）。
///
/// 行形状：一行一个分组（体积 / 可回收 / 条目数）。`path` 给 `overview://<分组id>`——
/// 它不是宿主机路径（与 docker 的 `docker://` 同一类），前端的 `isLocalPath()`
/// 守卫会自然地不给它「在访达中显示」。
pub fn synthesize_overview(groups: &[AssetGroup]) -> AssetGroup {
    let items: Vec<Item> = groups
        .iter()
        .map(|g| Item {
            id: format!("{OVERVIEW_ID}:{}", g.id),
            tags: vec![],
            name: g.label.clone(),
            group_key: None,
            path: format!("overview://{}", g.id),
            metrics: HashMap::from([
                ("size".into(), MetricValue::Bytes(g.total_size())),
                (
                    "reclaimable".into(),
                    MetricValue::Bytes(g.total_reclaimable()),
                ),
                ("count".into(), MetricValue::Int(g.items.len() as i64)),
            ]),
            detail: None,
            cleanable: false,
            reclaimable: None,
        })
        .collect();

    let total: u64 = groups.iter().map(AssetGroup::total_size).sum();
    let reclaim: u64 = groups.iter().map(AssetGroup::total_reclaimable).sum();

    AssetGroup {
        id: OVERVIEW_ID.into(),
        label: "总览".into(),
        summary: format!(
            "{} 个分组 · 合计 {} · 可回收 {}",
            groups.len(),
            crate::scan::repos::human_bytes(total),
            crate::scan::repos::human_bytes(reclaim),
        ),
        fields: vec![
            Field {
                key: "size".into(),
                label: "体积".into(),
                numeric: Some(true),
                temporal: None,
            },
            Field {
                key: "reclaimable".into(),
                label: "可回收".into(),
                numeric: Some(true),
                temporal: None,
            },
            Field {
                key: "count".into(),
                label: "条目".into(),
                numeric: Some(true),
                temporal: None,
            },
        ],
        items,
        notes: Some(vec![
            "总占用只统计侧栏已注册的分组。未纳入 V1 的范围（如 Docker Desktop）不计入——这是既定取舍，不是漏扫。".into(),
            "点任意一行进入对应分组。".into(),
        ]),
        error: None,
        // 总览本身没有可清理的行（每行是 `overview://<组>`，不是宿主机路径，
        // 前端 `isLocalPath` 天然不给动作），所以恒 false。清理要在具体分组里做。
        supports_clean: false,
    }
}

// ============================================================================
// 清理相关类型。S2 / D1 拍板（选 A）后 `RepoScanner::clean_repo` 经
// `clean_items` command 接入前端，开启条件三条齐备：前端二次确认 UI +
// `confirmed` 字段校验 + 废纸篓兜底（`trash::delete`）。**执行清理必须人显式
// 发起**——门禁只验代码与确认流，绝不真删（AGENTS 硬约束 4）。
// ============================================================================

/// 清理范围（P2）。缺省 [`CleanScope::Repo`] = 整条目删除（repos 是整仓进
/// 废纸篓、toolchain 是整个缓存目录、ai_tools 是一行「工具 × 类别」下的全部
/// 同类条目）；[`CleanScope::Artifacts`] = 只清白名单内的构建产物目录（目前
/// 只有 repos 组实现）。两个范围的复核也不同：整条目查 git 状态（未推送 /
/// 未提交改动）或白名单，产物查白名单——清产物不动源码与 `.git`，脏工作区
/// 不影响清产物。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanScope {
    #[default]
    Repo,
    Artifacts,
}

/// 清理计划。前端勾选后提交，服务端二次校验才执行。
#[derive(Debug, Clone, Deserialize)]
pub struct CleanPlan {
    pub group_id: String,
    pub item_ids: Vec<String>,
    /// 必须显式确认，服务端不接受缺省
    pub confirmed: bool,
    /// 清理范围。缺省 = 整条目（与 P2 之前的计划兼容）。未知取值反序列化
    /// 直接失败——范围打错不该退化成整仓删除。
    #[serde(default)]
    pub scope: CleanScope,
    /// 产物清理的**子集**：`Some([...])` = 只清这些「相对仓库根」的产物目录，
    /// `None` = 仓库内全部（默认，也是「全选」的语义）。只在
    /// `scope = Artifacts` 时有意义。
    ///
    /// **前端选过不等于可以删**：传进来的每一条都要过与全量路径完全相同的复核
    /// （在仓库内、白名单、不藏 Git），见 `repos::resolve_artifact_target` 与
    /// `RepoScanner::clean_artifacts`。空 `Vec` 会被当成「什么都没选中」拒掉，
    /// 不会退化成清全部。
    #[serde(default)]
    pub artifact_targets: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanFailure {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanResult {
    pub group_id: String,
    pub succeeded: Vec<String>,
    pub failed: Vec<CleanFailure>,
    pub freed_bytes: u64,
}

/// 扫描器契约。`Send + Sync` 是硬要求：`run_full_scan` 用 rayon 并行跑各个
/// 扫描器——全量一次约 26 秒（2026-10-13 实测），串行的话每加一个分组
/// 重扫时间就累加一层。新增实现都是普通结构体，自动满足，不用做什么。
pub trait Scanner: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn scan(&self) -> AssetGroup;

    /// 清理能力标志，前端据此决定是否渲染清理入口。
    /// 第一版全部返回 false——先把只读看板做准，再开删除。
    #[allow(dead_code)]
    fn supports_clean(&self) -> bool {
        false
    }
}

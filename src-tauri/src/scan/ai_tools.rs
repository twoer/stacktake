//! 编辑器与 AI 工具的数据目录扫描（G1 起，分组 label 是「编辑器与 AI 工具」）。
//!
//! 为什么做：本机的 AI 编码工具与编辑器数据合计 17.4 GB（G1 之前），
//! 比 Git 仓库侧更有清理价值，但完全没有人管。
//! G1 补上编辑器那半：VS Code 是开发者标配，之前**一个字节都没扫**——
//! `~/.vscode`（扩展）与 `~/Library/Application Support/Code`（用户数据、日志、
//! GPUCache、Extensions 缓存）都是零覆盖，而白名单里几个 fork 倒一直在扫。
//!
//! 分节维度是**工具 / 编辑器名**（`Item.group_key`），与 toolchain 的「栈」、
//! dev_env 的「组件」不是同一根轴——维度名由分组自己决定，前端只按 `group_key` 分节。
//!
//! # 安全约束（比功能本身重要）
//!
//! **只读文件名，绝不读文件内容。** 凭据藏在哪根本枚举不完——`auth.json`、
//! `config.toml`、`settings.json` 里都可能写着 key。逐个文件做内容过滤是堵不完的漏洞，
//! 不读内容则从根本上消除了这条路径：凭据压根不进入数据流，不是「读出来再抹掉」。
//!
//! 所以这里只允许 `metadata()` 与 `read_dir`（拿到的是路径名与元数据），不允许
//! `fs::read` / `read_to_string` / `File::open`。体积统计走 `scan/dirsize.rs` 的
//! 原生遍历，**W1 之后整棵树里已经没有 `du` 子进程**，别拿它当先例。
//! `tests.rs::ai_tools_source_never_reads_file_contents` 用 `include_str!`
//! 从源码层面守着这条，比事后检查扫描结果更硬。
//!
//! # 为什么按语义分类而不是按工具
//!
//! 九个工具的目录命名毫无共性：同一个东西 WorkBuddy 叫 `logs`，opencode 叫 `log`；
//! 会话历史有的叫 `sessions`，有的叫 `projects`，有的叫 `file-history`。
//! 按工具建模要写九套字段，按目录名硬套又会漏。折中办法是抽一层语义类别，
//! 工具只作为归属标签——这样筛选「可清理」能跨工具一次看全。
//!
//! 覆盖率不必追求完美：**未知一律不可清理**，漏识别的代价只是少显示一块可回收空间，
//! 不会误删。规则可以后续迭代补。

use crate::scan::platform::Platform;
use crate::scan::repos::{dir_size_fast, human_bytes};
use crate::scan::{
    AssetGroup, CleanFailure, CleanPlan, CleanResult, Field, Item, MetricValue, Scanner,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 构成清单最多显示几项。再多是几 KB 的配置文件，会淹没真正的大块。
const TOP_DETAIL: usize = 5;

pub struct AiToolsScanner {
    tools: Vec<Tool>,
}

pub struct Tool {
    label: String,
    root: PathBuf,
}

impl AiToolsScanner {
    pub fn new(home: PathBuf) -> Self {
        Self {
            tools: tools_for(&home, Platform::current()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Category {
    Log,
    Session,
    Cache,
    Skill,
    Index,
    Config,
    Runtime,
    Unknown,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Category::Log => "日志",
            Category::Session => "会话",
            Category::Cache => "缓存",
            Category::Skill => "技能",
            Category::Index => "索引",
            Category::Config => "配置",
            Category::Runtime => "运行时",
            Category::Unknown => "未知",
        }
    }

    /// 只有纯历史数据才置 `cleanable`。
    ///
    /// 技能类语义上可清理，但删了要重装，用户未必知道自己在删什么，第一版不开放。
    /// 索引类体积大（Codex 1.3 GB）但是数据库文件，删了可能损坏工具状态。
    pub(crate) fn cleanable(self) -> bool {
        matches!(self, Category::Log | Category::Session | Category::Cache)
    }

    /// id 里的稳定 ASCII 键。**别用 `label()`**：中文既是展示文案，也会让
    /// `ai:<类别>:<路径>` 的分隔与解析绑到 UI 用语上。
    pub(crate) fn key(self) -> &'static str {
        match self {
            Category::Log => "log",
            Category::Session => "session",
            Category::Cache => "cache",
            Category::Skill => "skill",
            Category::Index => "index",
            Category::Config => "config",
            Category::Runtime => "runtime",
            Category::Unknown => "unknown",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "log" => Category::Log,
            "session" => Category::Session,
            "cache" => Category::Cache,
            "skill" => Category::Skill,
            "index" => Category::Index,
            "config" => Category::Config,
            "runtime" => Category::Runtime,
            "unknown" => Category::Unknown,
            _ => return None,
        })
    }
}

/// 按目录名 / 文件名判定语义类别。纯函数，测试里全量覆盖。
///
/// 三层匹配，顺序不能随便调：
/// 1. 目录名精确——覆盖绝大多数，九个工具命名没有共性，靠这层兜住
/// 2. 后缀——一级散文件按载体分，Codex 的 `logs_2.sqlite` 单个就有 838 MB
/// 3. 包含——放最后，否则 `history.jsonl` 会被 `history` 截走变成会话类
pub(crate) fn classify(name: &str) -> Category {
    let n = name.to_ascii_lowercase();
    let n = n.as_str();

    match n {
        "logs" | "log" | "traces" | "trace" | "debug" => return Category::Log,
        "sessions" | "projects" | "file-history" | "memory" => return Category::Session,
        "cache" | "caches" | "snapshots" | "paste-cache" | "tool-output" | "blobs" | "tmp"
        | "temp" | ".tmp" => return Category::Cache,
        "plugins" | "skills" | "extensions" | "commands" | "agents" => return Category::Skill,
        "sqlite" | "db" | "index" => return Category::Index,
        "config" | "config.d" | "settings" => return Category::Config,
        "bin" | "binaries" | "app" | "entry" | "models" | "runtime" => return Category::Runtime,
        _ => {}
    }

    if n.ends_with(".sqlite")
        || n.ends_with(".sqlite3")
        || n.ends_with(".db")
        || n.ends_with("-wal")
        || n.ends_with("-shm")
    {
        return Category::Index;
    }
    if n.ends_with(".log") || n.ends_with(".jsonl") || n.ends_with(".trace") {
        return Category::Log;
    }
    if n.ends_with(".tmp")
        || n.contains(".tmp-")
        || n.ends_with(".bak")
        || n.ends_with(".old")
        || n.ends_with(".part")
    {
        return Category::Cache;
    }
    if n.ends_with(".json")
        || n.ends_with(".toml")
        || n.ends_with(".yaml")
        || n.ends_with(".yml")
        || n.ends_with(".ini")
        || n.ends_with(".md")
        || n.starts_with("config.")
        || n.starts_with("settings.")
        || n.starts_with("auth.")
    {
        return Category::Config;
    }

    if n.contains("otel") {
        return Category::Log;
    }
    if n.contains("cache") || n.contains("snapshot") {
        return Category::Cache;
    }
    if n.contains("session") || n.contains("history") {
        return Category::Session;
    }
    if n.contains("extension") || n.contains("plugin") || n.contains("skill") {
        return Category::Skill;
    }
    if n.contains("model") {
        return Category::Runtime;
    }

    Category::Unknown
}

/// AI 工具白名单。只在已知路径里扫，不 glob `~/.*`——否则会误伤 `.ssh`、`.aws`
/// 这类结构相似但高度敏感的目录。
///
/// 这十条都是 dot-dir，**两边平台路径相同**（macOS / Linux 与 Windows 一样），
/// 所以不拆两栏；需要分平台的是下面的编辑器数据目录。
const AI_SPECS: [(&str, &str); 10] = [
    ("WorkBuddy", ".workbuddy"),
    ("Codex", ".codex"),
    ("Windsurf", ".windsurf"),
    ("Qoder", ".qoder"),
    ("Trae", ".trae"),
    ("Cursor", ".cursor"),
    ("Claude", ".claude"),
    ("Gemini", ".gemini"),
    ("opencode", ".local/share/opencode"),
    // opencode 的配置在 XDG 配置目录，数据在数据目录，两个都要看
    ("opencode 配置", ".config/opencode"),
];

/// 编辑器数据（G1 新增）。(标签, macOS 相对 home, Windows 相对 home)
///
/// 为什么单列一张表：VS Code 与 JetBrains 的数据目录**两边平台完全不同**
/// （`~/Library/Application Support/Code` vs `%APPDATA%\Code`），必须显式给出，
/// 不能像 dot-dir 那样只写一条。（硬约束 9：平台是传进来的值，不是 `cfg`。）
const EDITOR_SPECS: [(&str, &str, &str); 4] = [
    // VS Code 的扩展装在 `~/.vscode/extensions`，与用户数据分开
    ("VS Code 扩展", ".vscode", ".vscode"),
    (
        "VS Code",
        "Library/Application Support/Code",
        "AppData/Roaming/Code",
    ),
    (
        "JetBrains 配置",
        "Library/Application Support/JetBrains",
        "AppData/Roaming/JetBrains",
    ),
    (
        "JetBrains 缓存",
        "Library/Caches/JetBrains",
        "AppData/Local/JetBrains",
    ),
];

/// 按平台解析出的工具与编辑器清单。`pub(crate)` 是为了让测试能喂假 home +
/// `Platform::Windows` 断言 Windows 那半条映射（本机是 macOS）。
pub(crate) fn tools_for(home: &Path, platform: Platform) -> Vec<Tool> {
    let mut tools: Vec<Tool> = Vec::new();
    let mut jb_cache: Option<PathBuf> = None;

    for (label, mac, win) in EDITOR_SPECS {
        let rel = if platform.is_windows() { win } else { mac };
        let root = home.join(rel);
        if !root.exists() {
            continue;
        }
        if label == "JetBrains 缓存" {
            // 交给下面按产品展开：父目录一整行既说不了能不能删、也给不出清理方式
            jb_cache = Some(root);
            continue;
        }
        tools.push(Tool {
            label: label.into(),
            root,
        });
    }

    // JetBrains 的缓存目录下面还有一层「产品 + 版本」（`GoLand2023.3`、
    // `IntelliJIdea2024.1`）：只有在产品层看，`caches` / `index` / `log`
    // 才分得出来。
    if let Some(root) = jb_cache {
        tools.extend(expand_products(&root));
    }

    tools.extend(AI_SPECS.iter().filter_map(|(label, rel)| {
        let root = home.join(rel);
        root.exists().then(|| Tool {
            label: (*label).into(),
            root,
        })
    }));

    tools
}

/// 把「一层产品目录」展开成多行。名字认不出来（没有子目录 / 读不到）就退回父目录一行。
fn expand_products(root: &Path) -> Vec<Tool> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return vec![Tool {
            label: "JetBrains 缓存".into(),
            root: root.to_path_buf(),
        }];
    };
    // `file_type()` 不跟随链接：指向别处的软链产品目录不算进来（同 collect_entries）
    let mut subs: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    if subs.is_empty() {
        return vec![Tool {
            label: "JetBrains 缓存".into(),
            root: root.to_path_buf(),
        }];
    }
    subs.sort();
    subs.into_iter()
        .map(|p| {
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            Tool {
                label: format!("JetBrains {name}"),
                root: p,
            }
        })
        .collect()
}

pub(crate) struct RawEntry {
    pub(crate) tool: usize,
    pub(crate) name: String,
    pub(crate) cat: Category,
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
    pub(crate) mtime: Option<i64>,
}

/// 遍历一个工具根目录的一级条目。返回条目与跳过的符号链接数。
fn collect_entries(tool: usize, root: &Path) -> (Vec<RawEntry>, usize) {
    let Ok(dir) = std::fs::read_dir(root) else {
        return (vec![], 0);
    };

    let mut entries = Vec::new();
    let mut symlinks = 0usize;

    for entry in dir.filter_map(|e| e.ok()) {
        // `DirEntry::file_type` 不跟随链接，能识别出软链。
        // 软链目录要是按普通目录累加体积，会顺着它遍历到别的地方去，体积和归属都是错的。
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            symlinks += 1;
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let md = std::fs::metadata(entry.path()).ok();
        let mtime = md
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        entries.push(RawEntry {
            tool,
            cat: classify(&name),
            path: entry.path(),
            name,
            size: 0,
            mtime,
        });
    }

    (entries, symlinks)
}

/// 按「工具 × 类别」聚合成 Item。
///
/// 这个粒度刚好落在决策点上：用户要决定的是「删不删这一块」，
/// 而不是「删不删这个工具」。按工具只有 10 行、看不出哪块能删；
/// 按子目录有 250 行、几 KB 的配置文件会淹没真正的大块。
fn aggregate(tools: &[Tool], entries: &[RawEntry]) -> Vec<Item> {
    let mut by_key: BTreeMap<(usize, Category), Vec<&RawEntry>> = BTreeMap::new();
    for e in entries {
        by_key.entry((e.tool, e.cat)).or_default().push(e);
    }

    let mut rows: Vec<(usize, u64, Item)> = by_key
        .into_iter()
        .map(|((ti, cat), mut group)| {
            group.sort_by_key(|a| std::cmp::Reverse(a.size));

            let size: u64 = group.iter().map(|e| e.size).sum();
            let last_activity = group.iter().filter_map(|e| e.mtime).max();
            let tool = &tools[ti];
            let cleanable = cat.cleanable();

            let mut detail: HashMap<String, String> = HashMap::new();
            // 键名必须前缀化：`Item.detail` 是 HashMap，序列化顺序不确定，
            // 前端按对象键序直接渲染，不编号的话构成清单会乱序。
            detail.insert("0. 路径".into(), tool.root.display().to_string());
            let shown = group.len().min(TOP_DETAIL);
            for (i, e) in group.iter().take(shown).enumerate() {
                detail.insert(format!("{}. {}", i + 1, e.name), human_bytes(e.size));
            }
            detail.insert(format!("{}. 条目", shown + 1), group.len().to_string());

            let mut metrics: HashMap<String, MetricValue> = HashMap::new();
            metrics.insert("size".into(), MetricValue::Bytes(size));
            metrics.insert(
                "last_activity".into(),
                match last_activity {
                    Some(ts) => MetricValue::Int(ts),
                    None => MetricValue::text("未知"),
                },
            );
            metrics.insert("kind".into(), MetricValue::text(cat.label()));

            // 标签只留「可清理」。类别已经在行名里（name = 工具 · 类别，前端按分隔符
            // 分节渲染），再做成标签是重复显示；工具名做成标签更是灾难——9 个工具
            // 加 8 个类别十几颗 pill，挤爆单行 tab。
            let tags = if cleanable {
                vec!["可清理".to_string()]
            } else {
                vec![]
            };

            let item = Item {
                // id 必须带路径（B1）：清理要落到「工具根 + 类别」这一组具体条目上，
                // 而 `aggregate` 的粒度就是「工具 × 类别」。类别用 ASCII 键，
                // 路径放在第二个冒号之后——Windows 盘符的冒号因此不参与分割。
                id: ai_id(&tool.root, cat),
                tags,
                name: format!("{} · {}", tool.label, cat.label()),
                group_key: Some(tool.label.clone()),
                path: tool.root.display().to_string(),
                metrics,
                detail: Some(detail),
                cleanable,
                reclaimable: if cleanable { Some(size) } else { None },
            };
            (ti, size, item)
        })
        .collect();

    // 工具按白名单顺序（大工具在前面），同一工具内按体积降序。
    // 前端默认不排序，这个顺序就是用户第一眼看到的顺序。
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    rows.into_iter().map(|(_, _, it)| it).collect()
}

/// `Item.id`：`ai:<类别键>:<工具根路径>`。第二个冒号之后整段都是路径，
/// 所以 Windows 的 `C:\...` 不会被误切。
pub(crate) fn ai_id(root: &Path, cat: Category) -> String {
    format!("ai:{}:{}", cat.key(), root.display())
}

/// [`ai_id`] 的逆运算。**不是通用反序列化**：前缀、类别键、非空路径任一不合法
/// 都返回 `None`，让清理入口拒掉，而不是去猜一个路径。
pub(crate) fn parse_ai_id(id: &str) -> Option<(Category, PathBuf)> {
    let rest = id.strip_prefix("ai:")?;
    let (key, path) = rest.split_once(':')?;
    let cat = Category::from_key(key)?;
    if path.is_empty() {
        return None;
    }
    Some((cat, PathBuf::from(path)))
}

/// 点击那一刻重新读工具根目录，只留下**当前**仍归入该语义类别的直接子项。
///
/// 这是 ai_tools 清理与 toolchain 白名单复核的对应物：不信任前端传来的 id，
/// 也不信任扫描时的分类结果——两次点击之间目录可能改名、换内容。分类仍是
/// 文件名规则，未知 / 配置 / 索引 / 技能一律选不中。
pub(crate) fn clean_candidates(root: &Path, cat: Category) -> Vec<RawEntry> {
    let (entries, _) = collect_entries(0, root);
    entries.into_iter().filter(|e| e.cat == cat).collect()
}

impl AiToolsScanner {
    /// 清理一个「工具 × 类别」行（B1）。安全红线与 repos / toolchain 同构：
    /// 确认 → 服务端复核 → 逐项 `trash::delete`，绝不硬删。
    ///
    /// 三条复核：
    /// 1. 根路径必须命中**本次** `tools_for` 的结果（id 里的路径不可信）；
    /// 2. 类别必须 `cleanable`；
    /// 3. 候选集合由点击那一刻重读目录得到，只含当前分类仍等于该类别的一级子项。
    ///
    /// **会话类删了不会再生**：前端确认弹窗必须把这条写在明处。
    pub fn clean_ai(&self, plan: &CleanPlan) -> CleanResult {
        let mut result = CleanResult {
            group_id: plan.group_id.clone(),
            succeeded: vec![],
            failed: vec![],
            freed_bytes: 0,
        };

        if !plan.confirmed {
            result.failed.push(CleanFailure {
                id: "*".into(),
                reason: "未确认".into(),
            });
            return result;
        }

        for id in &plan.item_ids {
            let Some((cat, root)) = parse_ai_id(id) else {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: "条目 id 不合法".into(),
                });
                continue;
            };
            if !cat.cleanable() {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: format!("{} 类别不开放清理", cat.label()),
                });
                continue;
            }
            if !root.is_absolute() {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: "不是绝对路径".into(),
                });
                continue;
            }
            let Some(tool) = self.tools.iter().find(|t| t.root == root) else {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: "不在 AI 工具扫描白名单内".into(),
                });
                continue;
            };

            let candidates = clean_candidates(&tool.root, cat);
            if candidates.is_empty() {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: format!("没有可清理的{}条目，重扫一次", cat.label()),
                });
                continue;
            }

            let mut all_ok = true;
            for e in &candidates {
                // collect_entries 已跳过软链；删除前再确认一次，避免两次读取之间
                // 被替换成软链后顺着它删到别处。
                match std::fs::symlink_metadata(&e.path) {
                    Ok(m) if m.file_type().is_symlink() => {
                        all_ok = false;
                        result.failed.push(CleanFailure {
                            id: e.path.display().to_string(),
                            reason: "是符号链接，跳过".into(),
                        });
                        continue;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        all_ok = false;
                        result.failed.push(CleanFailure {
                            id: e.path.display().to_string(),
                            reason: "路径已不存在，跳过".into(),
                        });
                        continue;
                    }
                }
                let size = dir_size_fast(&e.path).unwrap_or(0);
                match trash::delete(&e.path) {
                    Ok(()) => result.freed_bytes += size,
                    Err(err) => {
                        all_ok = false;
                        result.failed.push(CleanFailure {
                            id: e.path.display().to_string(),
                            reason: err.to_string(),
                        });
                    }
                }
            }
            if all_ok {
                result.succeeded.push(id.clone());
            }
        }

        result
    }
}

impl Scanner for AiToolsScanner {
    fn id(&self) -> &'static str {
        "ai_tools"
    }

    fn label(&self) -> &'static str {
        "编辑器与 AI 工具"
    }

    fn scan(&self) -> AssetGroup {
        let mut entries: Vec<RawEntry> = Vec::new();
        let mut symlinks = 0usize;
        for (i, t) in self.tools.iter().enumerate() {
            let (mut got, skipped) = collect_entries(i, &t.root);
            symlinks += skipped;
            entries.append(&mut got);
        }

        if entries.is_empty() {
            return AssetGroup {
                id: self.id().into(),
                label: self.label().into(),
                summary: "未找到 AI 工具或编辑器数据目录".into(),
                fields: ai_fields(),
                items: vec![],
                notes: Some(vec![
                    "扫描位置是白名单里的已知路径：AI 编码工具的 dot-dir + 编辑器数据目录（VS Code 的 ~/.vscode 与用户数据、JetBrains 的配置与缓存，见 ai_tools.rs 的两张 SPECS 表）。不 glob ~/.*，免得扫到 .ssh、.aws 这类敏感目录".into(),
                ]),
                error: None,
                supports_clean: true,
            };
        }

        // 体积统计并行跑：123 个一级子目录实测 1.9 秒（并行 12），
        // 顺序跑要 3.6 秒。层级浅、没有 node_modules 那种百万级文件树，所以便宜。
        entries.par_iter_mut().for_each(|e| {
            e.size = dir_size_fast(&e.path).unwrap_or(0);
        });

        let items = aggregate(&self.tools, &entries);

        // 各类别之和**可能略大于**工具根目录的实测大小，但这不是重复计算：
        // 各类别是互不重叠的一级子目录，逐项累加本该小于整棵。超出只可能来自
        // 「先量各类别、再量根目录」之间那段真实时间里目录在变（日志在写、
        // 缓存在清）。误差在 1% 上下，不值得为此放弃分类明细
        // ——那正是这个分组的全部价值。测试侧用 2% 容差粗筛，见
        // `tests.rs::ai_tools_sizes_stay_within_root`。
        let total: u64 = items
            .iter()
            .filter_map(|i| match i.metrics.get("size") {
                Some(MetricValue::Bytes(b)) => Some(*b),
                _ => None,
            })
            .sum();
        let cleanable_count = items.iter().filter(|i| i.cleanable).count();
        let reclaimable: u64 = items.iter().filter_map(|i| i.reclaimable).sum();

        let has = |c: Category| {
            items.iter().any(
                |i| matches!(i.metrics.get("kind"), Some(MetricValue::Text(s)) if s == c.label()),
            )
        };

        let unknown_bytes: u64 = entries
            .iter()
            .filter(|e| e.cat == Category::Unknown)
            .map(|e| e.size)
            .sum();
        let unknown_count = entries
            .iter()
            .filter(|e| e.cat == Category::Unknown)
            .count();

        let mut notes: Vec<String> =
            vec!["只统计文件名与体积，不读取任何文件内容，凭据不会进入扫描结果".into()];
        if has(Category::Session) {
            notes.push(
                "会话数据含完整对话记录与代码片段，删除不可逆，清理前先确认还要不要留".into(),
            );
        }
        if has(Category::Index) {
            notes.push(
                "索引类含 SQLite 数据库文件（Codex 的 logs_2.sqlite 名义是日志、载体是数据库），直接删除可能损坏工具状态".into(),
            );
        }
        if has(Category::Skill) {
            notes.push("技能与插件删除后需重新安装，本工具不开放清理".into());
        }
        if unknown_count > 0 {
            notes.push(format!(
                "未识别 {} 项（合计 {}）归入「未知」，一律不开放清理",
                unknown_count,
                human_bytes(unknown_bytes)
            ));
        }
        if symlinks > 0 {
            notes.push(format!("跳过 {} 个符号链接，不跟随统计", symlinks));
        }

        let summary = format!(
            "{} 个位置 · {} 个条目 · 合计 {} · {} 项可回收 · 共 {}",
            self.tools.len(),
            items.len(),
            human_bytes(total),
            cleanable_count,
            human_bytes(reclaimable)
        );

        AssetGroup {
            id: self.id().into(),
            label: self.label().into(),
            summary,
            fields: ai_fields(),
            items,
            notes: Some(notes),
            error: None,
            // B1：日志 / 会话 / 缓存三类有删除实现（clean_ai）；技能 / 索引 /
            // 配置 / 运行时不标 cleanable，服务端也会按类别拒绝。
            supports_clean: true,
        }
    }
}

// ---- 测试辅助：把内部纯函数暴露给测试模块，避免重复实现 ----

#[cfg(all(test, unix))]
pub(crate) fn collect_entries_for_test(root: &Path) -> (Vec<RawEntry>, usize) {
    collect_entries(0, root)
}

/// 前端只渲染 `fields.slice(0, 3)`，第四个字段不会进表头也不会进列。
/// 所以这里正好三个，条目数挪进 detail。
fn ai_fields() -> Vec<Field> {
    vec![
        Field {
            key: "size".into(),
            label: "体积".into(),
            numeric: Some(true),
            temporal: None,
        },
        Field {
            key: "last_activity".into(),
            label: "最后活动".into(),
            numeric: Some(true),
            temporal: Some(true),
        },
        Field {
            key: "kind".into(),
            label: "类别".into(),
            numeric: None,
            temporal: None,
        },
    ]
}

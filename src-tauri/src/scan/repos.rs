//! Git 仓库扫描。
//!
//! 两个要点：
//! 1. **嵌套仓库**——`parent/` 里套着 `parent/third_party/child`，直接累加体积会重复计算同一块磁盘。
//!    做法是先扫出全部仓库路径，再按路径长度排序，把子仓库路径从父仓库的体积统计里排除。
//! 2. **体积统计很贵**——实测整个扫描的瓶颈就在这一步（本机 pnpm store 单项就要十几秒）。
//!    所以并发跑，且单独设超时，避免个别超大目录拖住整个看板。

use crate::scan::dirsize;
use crate::scan::{
    AssetGroup, CleanFailure, CleanPlan, CleanResult, Field, Item, MetricValue, Scanner,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// 多大算「大」。超过这个体积的仓库在 UI 上高亮为清理候选。
const LARGE_REPO_BYTES: u64 = 200 * 1024 * 1024;
/// 多久算「久」。超过这个时间没提交的仓库标记为陈旧。
const STALE_DAYS: i64 = 180;

/// 一个仓库算不算清理候选。**纯函数**——判据是产品口径，值得脱离 git 子进程
/// 直接喂样本测（`repos_clean_candidate_mirrors_deletion_judgment` 守着它）。
///
/// 两条入选口径：大且久未动（>200 MB 且 180 天没提交），或从未有过提交
/// （`days == i64::MAX` 即 log 为空，体积 > 0）。
/// **一票否决**：有未推送提交或未提交改动——与 [`RepoScanner::clean_repo`]
/// 真删前的复核逐条对应。曾经显示判据不含这两条，账面「可回收」里有近八成
/// 点下去会被拒（S2 / D1 修的就是这个：显示的数必须等于真能删的数）。
pub(crate) fn is_clean_candidate(
    size: Option<u64>,
    days: i64,
    has_unpushed: bool,
    dirty_files: usize,
) -> bool {
    let big = size.map(|s| s > LARGE_REPO_BYTES).unwrap_or(false);
    let stale = days > STALE_DAYS;
    let candidate = (big && stale) || (size.unwrap_or(0) > 0 && days == i64::MAX);
    candidate && !has_unpushed && dirty_files == 0
}

// ============================================================================
// 构建产物白名单（P2）。
//
// 「项目不删、把可再生的构建产物清掉」是 npkill / clean-dev-dirs 验证过的那个
// 更高频的诉求。目录名**照单 clean-dev-dirs 调研过的 16 生态清单**（P2 登记），
// 不 glob：名字不在表里的一律不是产物。
//
// 每个条目还要求**兄弟位置有项目标记文件**才算数——单凭名字太险：`bin` 会
// 撞上 Cargo 约定的源码目录 `src/bin`，`build` 可能是手写的打包脚本目录。
// 标记不齐 = 不计账、不可清（报不满比报虚好）。
//
// 两条刻意不进表的（clean-dev-dirs 清，我们不学）：
// - `vendor`（Go / PHP / Deno）——vendor 目录常被**刻意提交**进仓库，删它不是
//   「可再生」而是「要重新 checkout」；
// - Ruby 的 `vendor/bundle`——两段路径，单层名字模型装不下。
//
// 名字与标记都是**跨平台一致的规则**，没有 win_rel 之分（这是名字规则不是
// 路径规则）。账面与删除判据共用同一套函数（[`is_artifact_dir_under`]）：
// 扫描时记的账就是点「清理产物」时服务端复核的账，逐条对齐。
// ============================================================================

/// 项目标记文件。Exact 精确名；Suffix 按后缀认（`.csproj` / `.cabal` 这类
/// 项目文件名字不定）——这只是标记的认法，不是 glob 扫描。
#[derive(Clone, Copy)]
enum Marker {
    Exact(&'static str),
    Suffix(&'static str),
}

const M_PYTHON: &[Marker] = &[
    Marker::Exact("pyproject.toml"),
    Marker::Exact("setup.py"),
    Marker::Exact("setup.cfg"),
    Marker::Exact("requirements.txt"),
    Marker::Exact("Pipfile"),
    Marker::Exact("poetry.lock"),
    Marker::Exact("uv.lock"),
    Marker::Exact("tox.ini"),
];

const M_DOTNET: &[Marker] = &[
    Marker::Suffix(".csproj"),
    Marker::Suffix(".fsproj"),
    Marker::Suffix(".vbproj"),
];

const M_ZIG: &[Marker] = &[Marker::Exact("build.zig")];

struct ArtifactRule {
    name: &'static str,
    markers: &'static [Marker],
}

/// `target` 由 Rust / Maven / Scala 三家共用，标记分不开就都认（三家删了都能重建）。
const ARTIFACT_RULES: &[ArtifactRule] = &[
    ArtifactRule {
        name: "target",
        markers: &[
            Marker::Exact("Cargo.toml"),
            Marker::Exact("pom.xml"),
            Marker::Exact("build.sbt"),
        ],
    },
    ArtifactRule {
        name: "node_modules",
        markers: &[
            Marker::Exact("package.json"),
            Marker::Exact("deno.json"),
            Marker::Exact("deno.jsonc"),
        ],
    },
    ArtifactRule {
        name: "__pycache__",
        markers: M_PYTHON,
    },
    ArtifactRule {
        name: ".venv",
        markers: M_PYTHON,
    },
    ArtifactRule {
        name: "venv",
        markers: M_PYTHON,
    },
    ArtifactRule {
        name: ".tox",
        markers: M_PYTHON,
    },
    ArtifactRule {
        name: ".pytest_cache",
        markers: M_PYTHON,
    },
    // `build`：Gradle / Flutter / CMake / setuptools 四家的产物目录
    ArtifactRule {
        name: "build",
        markers: &[
            Marker::Exact("build.gradle"),
            Marker::Exact("build.gradle.kts"),
            Marker::Exact("settings.gradle"),
            Marker::Exact("settings.gradle.kts"),
            Marker::Exact("pubspec.yaml"),
            Marker::Exact("CMakeLists.txt"),
            Marker::Exact("pyproject.toml"),
        ],
    },
    // `dist`：打包器（vite / webpack / setuptools…）的输出目录。package.json
    // 也放行——JS 生态 dist 惯例极强，撞上手写 dist 的代价只是进废纸篓（可恢复）。
    ArtifactRule {
        name: "dist",
        markers: &[
            Marker::Exact("package.json"),
            Marker::Exact("pyproject.toml"),
            Marker::Exact("setup.py"),
            Marker::Exact("setup.cfg"),
        ],
    },
    ArtifactRule {
        name: "bin",
        markers: M_DOTNET,
    },
    ArtifactRule {
        name: "obj",
        markers: M_DOTNET,
    },
    ArtifactRule {
        name: "zig-cache",
        markers: M_ZIG,
    },
    ArtifactRule {
        name: "zig-out",
        markers: M_ZIG,
    },
    ArtifactRule {
        name: ".stack-work",
        markers: &[Marker::Exact("stack.yaml")],
    },
    ArtifactRule {
        name: "dist-newstyle",
        markers: &[Marker::Exact("cabal.project"), Marker::Suffix(".cabal")],
    },
    ArtifactRule {
        name: "_build",
        markers: &[Marker::Exact("mix.exs")],
    },
    ArtifactRule {
        name: ".build",
        markers: &[Marker::Exact("Package.swift")],
    },
    ArtifactRule {
        name: ".dart_tool",
        markers: &[Marker::Exact("pubspec.yaml")],
    },
];

fn artifact_rule(name: &str) -> Option<&'static ArtifactRule> {
    ARTIFACT_RULES.iter().find(|r| r.name == name)
}

/// 白名单的测试视图。`artifact_whitelist_is_pinned` 钉住全部条目与标记——
/// 新增 / 删除条目时它会红，想清楚再动表。
#[cfg(test)]
pub(crate) fn artifact_rules_for_test() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    ARTIFACT_RULES
        .iter()
        .map(|r| {
            (
                r.name,
                r.markers
                    .iter()
                    .map(|m| match m {
                        Marker::Exact(x) => ("exact", *x),
                        Marker::Suffix(s) => ("suffix", *s),
                    })
                    .collect(),
            )
        })
        .collect()
}

/// 兄弟位置（候选目录的父目录里）有没有这条规则要的项目标记文件。
/// 只认普通文件；标记缺失 / 目录读不了都按「没有」处理。
fn parent_has_marker(parent: &Path, rule: &ArtifactRule) -> bool {
    let Ok(rd) = std::fs::read_dir(parent) else {
        return false;
    };
    rd.flatten().any(|e| {
        let Ok(ft) = e.file_type() else { return false };
        if !ft.is_file() {
            return false;
        }
        let name = e.file_name();
        let name = name.to_string_lossy();
        rule.markers.iter().any(|m| match m {
            Marker::Exact(x) => name == *x,
            Marker::Suffix(s) => {
                let b = name.as_bytes();
                b.len() >= s.len() && b[b.len() - s.len()..].eq_ignore_ascii_case(s.as_bytes())
            }
        })
    })
}

/// `p` 是否位于 `root` 下某个**嵌套仓库**内部（`root` 自己的 `.git` 不算）。
///
/// 嵌套仓库自成一行，它的产物记它自己的账；再记到父仓库头上就是双计。
/// 删除路径用同一个判据——两边看到的必须是同一份账。只查候选目录的
/// 祖先链（浅），不给整棵树每个目录都加一次 `.git` stat。
fn under_nested_repo(root: &Path, p: &Path) -> bool {
    p.ancestors()
        .skip(1)
        .take_while(|a| a.starts_with(root) && *a != root)
        .any(|a| a.join(".git").exists())
}

/// 目录是不是「构建产物」：名字在白名单 + 兄弟位置有项目标记 + 不在任何
/// 嵌套仓库内部。扫描记账与删除复核共用——两边同账（P2 的对齐要求）。
///
/// `.git` 显式排除：表里没有它，写出来是防将来有人往表里加。
pub(crate) fn is_artifact_dir_under(root: &Path, p: &Path) -> bool {
    let Some(name) = p.file_name() else {
        return false;
    };
    let name = name.to_string_lossy();
    if name == ".git" {
        return false;
    }
    let Some(rule) = artifact_rule(&name) else {
        return false;
    };
    let Some(parent) = p.parent() else {
        return false;
    };
    parent_has_marker(parent, rule) && !under_nested_repo(root, p)
}

/// 把前端勾选的**相对路径**解析回仓库内的产物目录（清理产物弹窗用）。
///
/// 四道关，任何一道不过都返回 `None`：
/// ① 不能为空、不能是绝对路径、不能含 `..` 分量——`Path::join` 不规范化，
///    `/repo` 拼 `../../etc` 在分量前缀上仍「属于」`/repo`，光靠 `starts_with`
///    拦不住，必须在拼之前就把 `..` 挡掉；
/// ② 拼回来仍以仓库根开头（分量前缀）；
/// ③ 目录本身不是软链——扫描侧（[`find_artifact_dirs`]）不跟随软链，账上
///    根本没有它，勾选侧也不收；
/// ④ `canonicalize` 双查——此刻目录必须真的存在，且真实位置（祖先经软链
///    进来的也算）仍在仓库内，软链逃逸走这条拦。
///
/// 通过后返回的是 `repo.join(rel)` 的**原始**路径，不是 canonicalize 的结果：
/// 删除复核（`d.starts_with(repo)`、[`is_artifact_dir_under`]、
/// [`under_nested_repo`]）全部工作在仓库 id 的路径空间里，喂 canonicalize
/// 路径会前缀失配——macOS 的 `/var → /private/var`、Windows 的 `\\?\`
/// verbatim 前缀都永不相等，复核会拒掉一切勾选。
///
/// 返回 `Some` 只代表「路径没跑出仓库」，**不等于可以删**：调用方仍要过
/// [`is_artifact_dir_under`] 与 [`subtree_contains_git`] 复核，与全量路径同一套。
pub(crate) fn resolve_artifact_target(repo: &Path, rel: &str) -> Option<PathBuf> {
    use std::path::Component;
    let p = Path::new(rel);
    if rel.trim().is_empty()
        || p.is_absolute()
        || p.components().any(|c| matches!(c, Component::ParentDir))
    {
        return None;
    }
    let d = repo.join(p);
    if !d.starts_with(repo) || d.is_symlink() {
        return None;
    }
    let cd = d.canonicalize().ok()?;
    let cr = repo.canonicalize().ok()?;
    if !cd.starts_with(&cr) {
        return None;
    }
    Some(d)
}

/// 从仓库根**现找**一遍全部产物目录（删除路径用）。
///
/// 与扫描记账走同一个判据 [`is_artifact_dir_under`]；点中就不再深入
/// （产物内部的嵌套产物属于同一棵被删的子树）。嵌套仓库整棵跳过——
/// 它的产物归它自己那一行。`.git` 整棵跳过是纯性能与卫生考虑，判据
/// 本身不会在里面命中（git 的内部目录名是固定的）。结果按路径排序，
/// 账目与测试都要确定性。
pub(crate) fn find_artifact_dirs(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if !ft.is_dir() {
                continue; // 软链不跟随（file_type 给的是软链自身）
            }
            let p = e.path();
            if p.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if p.join(".git").exists() {
                continue; // 嵌套仓库：产物归它自己那一行
            }
            if is_artifact_dir_under(root, &p) {
                found.push(p);
            } else {
                stack.push(p);
            }
        }
    }
    found.sort();
    found
}

/// 产物子树里有没有藏着 Git 仓库。删除前的保险：把一个仓库当产物连带
/// 移进废纸篓，超出「清理产物」的承诺——发现即拒，宁可让用户手动处理。
fn subtree_contains_git(dir: &Path) -> bool {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                if p.file_name().is_some_and(|n| n == ".git") {
                    return true;
                }
                stack.push(p);
            }
        }
    }
    false
}

pub struct RepoScanner {
    roots: Vec<PathBuf>,
}

impl RepoScanner {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }
}

#[derive(Debug)]
struct RepoHit {
    path: PathBuf,
}

impl Scanner for RepoScanner {
    fn id(&self) -> &'static str {
        "repos"
    }
    fn label(&self) -> &'static str {
        "Git 仓库"
    }

    /// S2 / D1 拍板后返回 `true`：repos 是唯一接了清理入口的分组
    /// （`clean_items` command）。其它分组的 `cleanable` 只是「可回收」口径，
    /// 没有对应的删除实现——返回 true 会宣称一个点不动的能力。
    fn supports_clean(&self) -> bool {
        true
    }

    fn scan(&self) -> AssetGroup {
        let mut notes: Vec<String> = Vec::new();

        // 空根目录守卫：根目录固定取 `roots::default_roots()`，正常不会为空——
        // 只有 `dirs::home_dir()` 返回 None（拿不到主目录）时才会走到这里。
        // 原来这里会越界 panic（下面的 `WalkBuilder::new(&self.roots[0])`）——
        // 明确报出来比崩掉好。
        //
        // 这条曾经还有第二个触发路径：用户在设置里清空了全部根目录（C1）。
        // 可配置的根目录 2026-10-19 删掉了，但守卫留着——home 拿不到仍然会发生。
        if self.roots.is_empty() {
            return AssetGroup {
                id: self.id().into(),
                label: self.label().into(),
                summary: "未配置根目录".into(),
                fields: repo_fields(),
                items: vec![],
                notes: Some(vec!["未能定位用户主目录，无法推断扫描根目录".into()]),
                error: None,
                supports_clean: false,
            };
        }

        // ---- 1. 发现仓库 ----
        let mut walker = ignore::WalkBuilder::new(&self.roots[0]);
        for root in &self.roots[1..] {
            walker.add(root);
        }
        walker
            .hidden(true) // 含隐藏目录，仓库可能在 .worktrees 之类的地方
            .git_ignore(true) // 尊重 .gitignore，被忽略的目录不算
            .git_global(false) // 不读全局 gitignore，那是给别人项目用的
            .parents(false)
            .require_git(false); // 不强制，否则 nested git 里的目录会被跳过

        let hits: Vec<RepoHit> = walker
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter(|e| e.path().join(".git").exists())
            .map(|e| RepoHit {
                path: e.path().to_path_buf(),
            })
            .collect();

        if hits.is_empty() {
            return AssetGroup {
                id: self.id().into(),
                label: self.label().into(),
                summary: "未找到仓库".into(),
                fields: repo_fields(),
                items: vec![],
                notes: Some(vec![format!("扫描根目录：{}", display_roots(&self.roots))]),
                error: None,
                supports_clean: false,
            };
        }

        let mut repos: Vec<PathBuf> = hits.into_iter().map(|h| h.path).collect();
        repos.sort_by_key(|p| p.components().count());

        // ---- 2. 识别嵌套 ----
        // 短路径在前，长的在后；用 starts_with 判定父子关系。
        let mut nested: HashSet<PathBuf> = HashSet::new();
        for i in 0..repos.len() {
            for j in (i + 1)..repos.len() {
                if repos[j].starts_with(&repos[i]) && repos[j] != repos[i] {
                    nested.insert(repos[j].clone());
                }
            }
        }
        if !nested.is_empty() {
            notes.push(format!(
                "检测到 {} 个嵌套仓库，父仓库体积已排除子仓库部分",
                nested.len()
            ));
        }

        // ---- 3. 并发采集 ----
        let tasks: Vec<(PathBuf, bool)> = repos
            .iter()
            .map(|p| (p.clone(), nested.contains(p)))
            .collect();

        let collected: Vec<RepoInfo> = tasks
            .par_iter()
            .map(|(p, is_nested)| collect_repo(p, *is_nested, &nested))
            .collect();

        // ---- 4. 汇总 ----
        let items: Vec<Item> = collected.into_iter().map(|r| r.into_item()).collect();

        let total: u64 = items
            .iter()
            .filter_map(|i| match i.metrics.get("size") {
                Some(MetricValue::Bytes(b)) => Some(*b),
                _ => None,
            })
            .sum();

        let cleanable_count = items.iter().filter(|i| i.cleanable).count();
        let reclaimable: u64 = items.iter().filter_map(|i| i.reclaimable).sum();

        let summary = format!(
            "{} 个仓库 · 合计 {} · {} 个可回收 · 共 {}",
            items.len(),
            human_bytes(total),
            cleanable_count,
            human_bytes(reclaimable)
        );

        AssetGroup {
            id: self.id().into(),
            label: self.label().into(),
            summary,
            fields: repo_fields(),
            items,
            notes: if notes.is_empty() { None } else { Some(notes) },
            error: None,
            supports_clean: true,
        }
    }
}

/// 表头列。**最多三个**：前端 `Assets.vue` 是 `fields.slice(0, 3)`，
/// 第四个起静默丢失（`tests.rs::assert_frontend_contract` 守着这条）。
///
/// 曾经声明过五个，`dirty` / `remote` 两列从来没渲染过——`remote` 的内容
/// 一直在 detail 里，`dirty`（↑↓ 与改动数）只留在 metrics 供 JSON 消费。
/// 想把哪一列加回来，先从这三个里换下去一个。
fn repo_fields() -> Vec<Field> {
    vec![
        Field {
            key: "size".into(),
            label: "体积".into(),
            numeric: Some(true),
            temporal: None,
        },
        Field {
            key: "last_commit".into(),
            label: "最后提交".into(),
            numeric: Some(true),
            temporal: Some(true),
        },
        Field {
            key: "branch".into(),
            label: "分支".into(),
            numeric: None,
            temporal: None,
        },
    ]
}

struct RepoInfo {
    path: PathBuf,
    name: String,
    size: Option<u64>,
    git_only: Option<u64>,
    /// 白名单内的构建产物目录及其各自体积（已按路径排序）。
    /// 体积来自与仓库总体积同一次遍历（dirsize 的 noting 钩子），
    /// 总体积超时（size = None）时丢弃——账面口径不能裂成两半。
    artifacts: Vec<(PathBuf, u64)>,
    last_commit_ts: Option<i64>,
    last_commit_msg: String,
    branch: String,
    dirty_files: usize,
    remote: String,
    ahead: usize,
    behind: usize,
    is_nested: bool,
    has_unpushed: bool,
}

impl RepoInfo {
    fn into_item(self) -> Item {
        let days = self.last_commit_ts.map(days_since).unwrap_or(i64::MAX);

        // 清理候选：大且久未动，或从未有过提交（size > 0 而 log 为空）
        let big = self.size.map(|s| s > LARGE_REPO_BYTES).unwrap_or(false);
        let stale = days > STALE_DAYS;
        let risky = self.has_unpushed;
        let cleanable = is_clean_candidate(self.size, days, self.has_unpushed, self.dirty_files);

        let mut tags = vec![];
        if stale {
            tags.push("陈旧".to_string());
        }
        if big {
            tags.push("大".to_string());
        }
        if risky {
            tags.push("未推送".to_string());
        }
        if self.dirty_files > 0 {
            // 只做布尔标记，具体处数已在 dirty 指标里。
            // 早先写成 `脏{N}`，结果 tag 栏按字符串排序变成「脏1 脏10 脏100 脏108 脏11」，
            // 筛选器完全失去意义。
            tags.push("有改动".to_string());
        }
        if self.is_nested {
            tags.push("嵌套".to_string());
        }

        let mut metrics: HashMap<String, MetricValue> = HashMap::new();
        metrics.insert(
            "size".into(),
            match self.size {
                Some(b) => MetricValue::Bytes(b),
                None => MetricValue::text("—"),
            },
        );
        metrics.insert(
            "git_only".into(),
            match self.git_only {
                Some(b) => MetricValue::Bytes(b),
                None => MetricValue::text("—"),
            },
        );
        metrics.insert(
            "last_commit".into(),
            match self.last_commit_ts {
                Some(ts) => MetricValue::Int(ts),
                None => MetricValue::text("无提交"),
            },
        );
        metrics.insert(
            "branch".into(),
            MetricValue::text(if self.branch.is_empty() {
                "游离".into()
            } else {
                self.branch.clone()
            }),
        );
        let status = if risky {
            format!("↑{} ↓{}", self.ahead, self.behind)
        } else if self.dirty_files > 0 {
            format!("{} 处改动", self.dirty_files)
        } else {
            "干净".into()
        };
        metrics.insert("dirty".into(), MetricValue::text(status));

        let mut detail: HashMap<String, String> = HashMap::new();
        detail.insert("路径".into(), self.path.display().to_string());
        detail.insert(
            "远端".into(),
            if self.remote.is_empty() {
                "无".into()
            } else {
                self.remote.clone()
            },
        );
        detail.insert(
            "最后提交".into(),
            if self.last_commit_msg.is_empty() {
                "—".into()
            } else {
                self.last_commit_msg.clone()
            },
        );
        if let Some(g) = self.git_only {
            detail.insert(".git 体积".into(), human_bytes(g));
        }
        // 产物账面：不动 `reclaimable` 语义（那仍是「可整仓删除」的口径），
        // 在 metrics / detail / 标签三处给「构建产物 X GB」。前端据此给
        // 「清理产物」按钮（P2），与整仓清理是两个动作。
        let artifact_bytes: u64 = self.artifacts.iter().map(|(_, n)| n).sum();
        if !self.artifacts.is_empty() {
            metrics.insert("artifacts".into(), MetricValue::Bytes(artifact_bytes));
            tags.push("有产物".to_string());
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for (p, _) in &self.artifacts {
                let n = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "?".into());
                *kinds.entry(n).or_default() += 1;
            }
            let kinds = kinds
                .iter()
                .map(|(n, c)| {
                    if *c > 1 {
                        format!("{n}×{c}")
                    } else {
                        n.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join("、");
            detail.insert(
                "构建产物".into(),
                format!(
                    "{} · {} 个目录（{}）",
                    human_bytes(artifact_bytes),
                    self.artifacts.len(),
                    kinds
                ),
            );
            // 产物明细（D14）：每个产物目录一行「相对路径 · 各自体积」，体积降序。
            // detail 是 HashMap、序列化序随机，多行收进单键、前端按 \n 分行；
            // 行数封顶——几十个产物的 monorepo 不该把检查器撑成一屏账本，
            // 没列进的目录在「构建产物」的总账里仍然都在。
            let mut rows: Vec<(u64, String)> = self
                .artifacts
                .iter()
                .map(|(p, n)| {
                    let rel = p
                        .strip_prefix(&self.path)
                        .unwrap_or(p)
                        .to_string_lossy()
                        .into_owned();
                    (*n, rel)
                })
                .collect();
            rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            const MAX_DETAIL_ROWS: usize = 12;
            let mut lines: Vec<String> = rows
                .iter()
                .take(MAX_DETAIL_ROWS)
                .map(|(n, rel)| format!("{rel} · {}", human_bytes(*n)))
                .collect();
            if rows.len() > MAX_DETAIL_ROWS {
                lines.push(format!("…其余 {} 个目录略", rows.len() - MAX_DETAIL_ROWS));
            }
            detail.insert("产物明细".into(), lines.join("\n"));
        }

        Item {
            id: self.path.display().to_string(),
            tags,
            name: self.name,
            // 仓库名本身就是这一行的全部信息，不分节（G1：group_key 为 None 即不分节）
            group_key: None,
            path: self.path.display().to_string(),
            metrics,
            detail: Some(detail),
            cleanable,
            reclaimable: if cleanable { self.size } else { None },
        }
    }
}

fn collect_repo(path: &Path, is_nested: bool, nested: &HashSet<PathBuf>) -> RepoInfo {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());

    let git_dir = path.join(".git");
    let git_only = dir_size_fast(&git_dir);

    // 产物记账与总体积走**同一次遍历**（dirsize 的 noting 钩子）：预判闭包
    // 对每个子目录只做名字比对（便宜），标记文件与祖先检查只在名字命中时
    // 发生。产物子树单独计量一次，总体积照常包含它——口径不变，26 秒量级
    // 的扫描成本也几乎不变。
    let noted = dirsize::Noted {
        items: std::sync::Mutex::new(Vec::new()),
    };
    let pred = |p: &Path| is_artifact_dir_under(path, p);
    let size = dir_size_excluding(path, nested, Some((&pred, &noted)));
    // 总体积超时（None）时记账一并丢弃：仓库体积显示「—」而产物行却报数，
    // 两边口径就裂了
    let mut artifacts: Vec<(PathBuf, u64)> = if size.is_some() {
        noted
            .items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect()
    } else {
        Vec::new()
    };
    artifacts.sort();

    let branch = git_output(path, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();

    let (last_ts, last_msg) = match git_output(path, &["log", "-1", "--format=%ct%x1f%s"]) {
        Some(raw) => {
            let mut it = raw.splitn(2, '\u{1f}');
            let ts = it.next().unwrap_or("").trim().parse::<i64>().unwrap_or(0);
            let msg = it.next().unwrap_or("").trim().to_string();
            if ts > 0 {
                (Some(ts), msg)
            } else {
                (None, String::new())
            }
        }
        None => (None, String::new()),
    };

    let dirty_files = match git_output(path, &["status", "--porcelain"]) {
        Some(s) => s.lines().filter(|l| !l.trim().is_empty()).count(),
        None => 0,
    };

    let remote = git_output(path, &["config", "--get", "remote.origin.url"]).unwrap_or_default();

    // `@{u}...HEAD` 的 `--left-right --count` 输出两列：**左 = 落后数**（远端有、
    // 本地没有），**右 = 未推送数**（本地有、远端没有）。曾经这两列被赋反过——
    // 只是落后的仓库被当成「有未推送」拒绝，而真有未推送提交的仓库反而不被
    // `clean_repo` 拦，直接进废纸篓（S2 的测试抓到的：方向反了比不判更危险）。
    let (ahead, behind) = match git_output(
        path,
        &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
    ) {
        Some(s) => {
            let parts: Vec<&str> = s.split_whitespace().collect();
            if parts.len() == 2 {
                (parts[1].parse().unwrap_or(0), parts[0].parse().unwrap_or(0))
            } else {
                (0, 0)
            }
        }
        None => (0, 0),
    };
    let has_unpushed = ahead > 0;

    RepoInfo {
        path: path.to_path_buf(),
        name,
        size,
        git_only,
        artifacts,
        last_commit_ts: last_ts,
        last_commit_msg: last_msg,
        branch,
        dirty_files,
        remote,
        ahead,
        behind,
        is_nested,
        has_unpushed,
    }
}

fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// 目录占用空间（字节）。**不排除任何子目录**，要排除用 [`dir_size_excluding`]。
///
/// AI 工具 / Docker / 移动端三个扫描器都复用它，所以是 `pub(crate)`
/// ——不要各写一份，超时语义得一致。实现与口径见 [`crate::scan::dirsize`]。
pub(crate) fn dir_size_fast(dir: &Path) -> Option<u64> {
    dirsize::dir_size(dir, &HashSet::new())
}

/// 一批目录的占用空间，key 是传入的路径字符串。
///
/// 上限由**内存带宽与 inode 缓存**决定，不再有命令行长度问题：`du` 时代要
/// 分块是为了不撑爆 `ARG_MAX`，原生遍历没有这个约束。并行度交给 rayon。
pub(crate) fn dir_sizes_batch(dirs: &[PathBuf]) -> HashMap<String, u64> {
    dirsize::dir_sizes_batch(dirs)
}

/// 按路径查批量结果，查不到就单独算一次。
///
/// 换掉 `du` 之后「查不到」已近乎不可能（key 就是调用方给的原路径，
/// 不再有 du 回显路径与传入路径不一致的问题），但这个兜底留着——宁可慢一点，
/// 也不要把 380 MB 显示成 0。
pub(crate) fn lookup_size(sizes: &HashMap<String, u64>, p: &Path) -> u64 {
    sizes
        .get(&p.to_string_lossy().to_string())
        .copied()
        .or_else(|| dir_size_fast(p))
        .unwrap_or(0)
}

pub(crate) fn mtime_secs(p: &Path) -> Option<i64> {
    std::fs::metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// 目录占用空间，**排除嵌套的子仓库**。
///
/// 只把「最浅的那一层」后代放进排除集：`nested` 是扁平集合，若 `a`、`a/b`、
/// `a/b/c` 都在里面，全排除一遍会重复扣 `a/b/c` 两次，父仓库会比子仓库还小。
/// 最浅的那些已经包含各自后代，所以排除它们就够了。
///
/// 换掉 `du` 后这里顺带省掉一趟：du 时代得「先算总量、再分别算各嵌套目录、
/// 相减」，等于把嵌套子树走两遍；原生遍历支持按路径剪枝，走一遍就够。
///
/// `noting` 是 P2 的产物记账钩子，透传给 dirsize——见 [`collect_repo`]。
fn dir_size_excluding(
    path: &Path,
    nested: &HashSet<PathBuf>,
    noting: Option<dirsize::Noting<'_>>,
) -> Option<u64> {
    let children: Vec<&Path> = nested
        .iter()
        .map(|p| p.as_path())
        .filter(|p| p.starts_with(path) && *p != path)
        .collect();

    let shallow: HashSet<PathBuf> = children
        .iter()
        .copied()
        .filter(|c| !children.iter().any(|o| *o != *c && c.starts_with(*o)))
        .map(|p| p.to_path_buf())
        .collect();

    match noting {
        Some((pred, noted)) => dirsize::dir_size_noting(path, &shallow, pred, noted),
        None => dirsize::dir_size(path, &shallow),
    }
}

fn days_since(ts: i64) -> i64 {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    (now - ts) / 86_400
}

pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", b, UNITS[i])
    } else {
        format!("{:.1} {}", v, UNITS[i])
    }
}

fn display_roots(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|p| {
            dirs::home_dir()
                .and_then(|h| p.strip_prefix(&h).ok().map(|r| format!("~{}", r.display())))
                .unwrap_or_else(|| p.display().to_string())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// 清理实现（S2 / D1 拍板选 A 后接入 `clean_items` command；前端入口是检查器
/// 动作条的「清理」按钮 + 确认弹窗）。
///
/// 安全红线：
/// - 走 `trash::delete` 移入废纸篓，绝不 `rm -rf`
/// - 有未推送提交或未提交改动的一律拒绝，这是不可逆损失
/// - `confirmed` 为假时整个计划直接拒绝
/// - 删的是**仓库根目录**（源码与 `.git` 一起进废纸篓）——确认弹窗必须把这一点
///   写在明处，`is_clean_candidate` 的入选口径也据此收紧（S2 修的是「显示的
///   可回收」与「真能删的」不一致：两者现在逐条对齐）
impl RepoScanner {
    pub fn clean_repo(&self, plan: &CleanPlan) -> CleanResult {
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
            let path = PathBuf::from(id);
            let info = collect_repo(&path, false, &HashSet::new());

            if info.has_unpushed || info.dirty_files > 0 {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: if info.has_unpushed {
                        "有未推送提交".into()
                    } else {
                        format!("{} 处未提交改动", info.dirty_files)
                    },
                });
                continue;
            }

            match trash::delete(&path) {
                Ok(()) => {
                    result.succeeded.push(id.clone());
                    result.freed_bytes += info.size.unwrap_or(0);
                }
                Err(e) => result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: e.to_string(),
                }),
            }
        }

        result
    }

    /// 产物清理（P2）。与 [`Self::clean_repo`] 并列的**独立复核路径**——
    /// 清产物不碰源码与 `.git`，所以**不查**未推送 / 未提交改动：那两条拦的
    /// 是「源码丢了找不回」，产物是可再生的，仓库有脏改动也不影响清产物。
    ///
    /// 安全红线（与整仓删除同源，三道闸对齐）：
    /// - `confirmed` 为假整个计划拒绝；
    /// - **不信任前端传来的任何产物路径**——plan 里只有仓库 id，产物集合由
    ///   服务端此刻按白名单现找（[`find_artifact_dirs`]），与扫描记账同判据；
    /// - 每个目录进废纸篓前再验一次「仍在仓库内 + 判据仍成立 + 里面没藏
    ///   Git 仓库」（藏了仓库的整目录拒，见 [`subtree_contains_git`]）；
    /// - `trash::delete` 移入废纸篓，绝不硬删。
    pub fn clean_artifacts(&self, plan: &CleanPlan) -> CleanResult {
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
            let path = PathBuf::from(id);
            if !path.is_absolute() {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: "不是绝对路径".into(),
                });
                continue;
            }
            if !path.is_dir() {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: "仓库目录不存在，重扫一次".into(),
                });
                continue;
            }
            if !path.join(".git").exists() {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: "不是 Git 仓库（.git 不存在）".into(),
                });
                continue;
            }

            let mut all_ok = true;
            let dirs: Vec<PathBuf> = match &plan.artifact_targets {
                // 勾选子集：把相对路径解析回仓库内的目录。解析不出来的（`..`、
                // 绝对路径、空串、已不存在）单独报失败，**不静默跳过**——账面与
                // 实际删掉的东西必须逐条对得上（S2）。
                Some(rels) => {
                    let mut picked: Vec<PathBuf> = Vec::new();
                    for rel in rels {
                        match resolve_artifact_target(&path, rel) {
                            Some(d) => picked.push(d),
                            None => {
                                all_ok = false;
                                result.failed.push(CleanFailure {
                                    id: rel.clone(),
                                    reason: "不是本仓库内的产物目录路径".into(),
                                });
                            }
                        }
                    }
                    picked.sort();
                    picked.dedup();
                    picked
                }
                None => find_artifact_dirs(&path),
            };
            if dirs.is_empty() {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: "未发现可清理的构建产物".into(),
                });
                continue;
            }

            let sizes = dir_sizes_batch(&dirs);
            for d in &dirs {
                // 进废纸篓前的最后复核（点得动才删得了）：缓存到点击之间
                // 目录可能被改名 / 换了内容，判据必须此刻仍成立
                let safe = d.as_path() != path.as_path()
                    && d.starts_with(&path)
                    && is_artifact_dir_under(&path, d)
                    && !subtree_contains_git(d);
                if !safe {
                    all_ok = false;
                    result.failed.push(CleanFailure {
                        id: d.display().to_string(),
                        reason: "复核未通过（目录里藏着 Git 仓库或已不是产物目录），跳过".into(),
                    });
                    continue;
                }
                let size = sizes
                    .get(&d.to_string_lossy().to_string())
                    .copied()
                    .unwrap_or(0);
                match trash::delete(d) {
                    Ok(()) => result.freed_bytes += size,
                    Err(e) => {
                        all_ok = false;
                        result.failed.push(CleanFailure {
                            id: d.display().to_string(),
                            reason: e.to_string(),
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

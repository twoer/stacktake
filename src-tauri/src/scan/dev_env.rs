//! JVM 与移动端开发环境扫描。
//!
//! # 为什么分成两类
//!
//! Java 环境在本机是真实存在的：Maven 仓库 1.5 GB、JDK 632 MB、Gradle 419 MB，
//! 合计约 2.5 GB，有数据可看。iOS / Android / 鸿蒙在本机**根本没装**——没有 Xcode，
//! 没有 Android SDK，鸿蒙只剩一个空的 `Sdk` 目录（44 KB 的许可文件）。
//!
//! 所以后三者做成探测型：**根目录一个都不存在，就压根不生成这个 Scanner**，
//! 侧栏不会出现空分组。现在界面干净，将来装上 Xcode 或换台机器，分组自己会冒出来。
//!
//! # 安全约束
//!
//! 同 `ai_tools.rs`：只用 `metadata()` 与 `read_dir`（文件名与元数据），不读任何文件内容。
//! JDK 目录里有 `cacerts` 信任库，Android SDK 里可能有打包进去的 debug keystore，
//! 鸿蒙 SDK 里可能有签名材料——内容一律不碰，体积与路径就够了。

use crate::scan::platform::{Platform, first_existing};
use crate::scan::repos::{dir_size_fast, dir_sizes_batch, human_bytes, lookup_size, mtime_secs};
use crate::scan::{AssetGroup, Field, Item, MetricValue, Scanner};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Maven 二级 groupId 保留前几项，其余归入「其它」。
///
/// 实测 215 个二级目录，全列出来长尾（几 MB 的那些）会把真正的大块淹没掉，
/// 而用户真正会动手的只有头部十来个。
const MAVEN_TOP: usize = 15;

fn count_subdirs(p: &Path) -> usize {
    std::fs::read_dir(p)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| !t.is_symlink()).unwrap_or(false))
                .filter(|e| e.path().is_dir())
                .count()
        })
        .unwrap_or(0)
}

fn detail(pairs: &[(&str, String)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

// ============================================================================
// Java：Maven / Gradle / JDK
// ============================================================================

pub struct JavaScanner {
    home: PathBuf,
}

impl JavaScanner {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }

    /// 三个来源一个都没有就不该出现在侧栏。
    pub fn exists(&self) -> bool {
        self.maven_root().is_some() || self.gradle_root().exists() || jdk_root().exists()
    }

    fn maven_root(&self) -> Option<PathBuf> {
        let p = self.home.join(".m2").join("repository");
        if p.is_dir() { Some(p) } else { None }
    }

    fn gradle_root(&self) -> PathBuf {
        self.home.join(".gradle")
    }
}

/// JDK 装在系统目录，不在 home 下。
fn jdk_root() -> PathBuf {
    PathBuf::from("/Library/Java/JavaVirtualMachines")
}

struct MavenRow {
    /// 相对仓库根的两级路径，如 `org/apache`
    name: String,
    path: PathBuf,
    size: u64,
    mtime: Option<i64>,
    artifacts: usize,
}

/// Maven 仓库按**二级** groupId 拆分。
///
/// 粒度是量出来的：一级前缀只有 47 个，但 `org` 单独就 907 MB，看不出是谁占的；
/// 三级（`org/apache/maven`）上千个，表格装不下。二级（`org/apache` 380 MB）
/// 正好落在「删不删这个依赖的缓存」这个决策粒度上。
fn maven_rows(root: &Path) -> Vec<MavenRow> {
    let Ok(first) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut seconds: Vec<PathBuf> = Vec::new();
    for a in first.filter_map(|e| e.ok()) {
        // 软链不跟随：跟着它可能跑出仓库之外，体积和归属都是错的
        if a.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
            continue;
        }
        if !a.path().is_dir() {
            continue;
        }
        let Ok(second) = std::fs::read_dir(a.path()) else {
            continue;
        };
        for b in second.filter_map(|e| e.ok()) {
            if b.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            if b.path().is_dir() {
                seconds.push(b.path());
            }
        }
    }

    if seconds.is_empty() {
        return Vec::new();
    }

    let sizes = dir_sizes_batch(&seconds);

    // 构件组数要再下一层，只对最终进表的那些算，215 次 read_dir 很便宜
    let mut rows: Vec<MavenRow> = seconds
        .into_iter()
        .map(|p| MavenRow {
            name: p
                .strip_prefix(root)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| p.display().to_string()),
            size: lookup_size(&sizes, &p),
            mtime: mtime_secs(&p),
            artifacts: count_subdirs(&p),
            path: p,
        })
        .collect();

    rows.sort_by_key(|a| std::cmp::Reverse(a.size));
    rows
}

fn maven_items(root: &Path) -> Vec<Item> {
    let rows = maven_rows(root);
    if rows.is_empty() {
        return Vec::new();
    }

    let mut items: Vec<Item> = rows
        .iter()
        .take(MAVEN_TOP)
        .map(|r| {
            let mut metrics: HashMap<String, MetricValue> = HashMap::new();
            metrics.insert("size".into(), MetricValue::Bytes(r.size));
            metrics.insert(
                "last_modified".into(),
                match r.mtime {
                    Some(ts) => MetricValue::Int(ts),
                    None => MetricValue::text("未知"),
                },
            );
            metrics.insert("kind".into(), MetricValue::text("依赖缓存"));

            Item {
                id: format!("java:mvn:{}", r.name),
                // 不带「可清理」标签：Maven 缓存删了要联网重拉，代价和 Gradle 旧分发版不是一回事
                tags: vec![],
                name: format!("Maven 仓库 · {}", r.name),
                group_key: Some("Maven 仓库".into()),
                path: r.path.display().to_string(),
                metrics,
                detail: Some(detail(&[
                    ("0. 路径", r.path.display().to_string()),
                    ("1. 构件组", r.artifacts.to_string()),
                ])),
                cleanable: false,
                reclaimable: None,
            }
        })
        .collect();

    let rest = rows.len().saturating_sub(MAVEN_TOP);
    if rest > 0 {
        let rest_size: u64 = rows.iter().skip(MAVEN_TOP).map(|r| r.size).sum();
        let rest_mtime = rows.iter().skip(MAVEN_TOP).filter_map(|r| r.mtime).max();

        let mut metrics: HashMap<String, MetricValue> = HashMap::new();
        metrics.insert("size".into(), MetricValue::Bytes(rest_size));
        metrics.insert(
            "last_modified".into(),
            match rest_mtime {
                Some(ts) => MetricValue::Int(ts),
                None => MetricValue::text("未知"),
            },
        );
        metrics.insert("kind".into(), MetricValue::text("依赖缓存"));

        items.push(Item {
            id: "java:mvn:__rest__".into(),
            tags: vec![],
            name: format!("Maven 仓库 · 其它 {} 项", rest),
            group_key: Some("Maven 仓库".into()),
            path: root.display().to_string(),
            metrics,
            detail: Some(detail(&[
                ("0. 路径", root.display().to_string()),
                ("1. 说明", format!("体积未进前 {} 的 groupId", MAVEN_TOP)),
            ])),
            cleanable: false,
            reclaimable: None,
        });
    }

    items
}

/// 从 `gradle-7.4.1-bin` 这类目录名里取出版本号。取不出来就原样返回。
fn gradle_version(dir_name: &str) -> String {
    let stem = dir_name.trim_end_matches("-bin").trim_end_matches("-all");
    stem.trim_start_matches("gradle-").to_string()
}

/// 只看主版本号，用来判断谁是最新的那个（保留它，其余算残留）。
fn version_major(v: &str) -> u64 {
    v.split('.')
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

fn gradle_items(home: &Path) -> Vec<Item> {
    let root = home.join(".gradle");
    if !root.is_dir() {
        return Vec::new();
    }

    let mut items: Vec<Item> = Vec::new();

    // --- wrapper 分发版：保留最新的，其余是老项目留下的残留 ---
    let dists = root.join("wrapper").join("dists");
    if let Ok(dir) = std::fs::read_dir(&dists) {
        let entries: Vec<PathBuf> = dir
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| !t.is_symlink()).unwrap_or(false))
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect();

        if !entries.is_empty() {
            let sizes = dir_sizes_batch(&entries);

            // 版本号在目录名里，挑最新的那个；并列时取体积大的（更可能正在用）
            let newest = entries
                .iter()
                .map(|p| {
                    (
                        p,
                        version_major(&gradle_version(
                            &p.file_name().unwrap_or_default().to_string_lossy(),
                        )),
                    )
                })
                .max_by_key(|(p, major)| (*major, lookup_size(&sizes, p)))
                .map(|(p, _)| p.clone());

            for p in &entries {
                let dir_name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let version = gradle_version(&dir_name);
                let size = lookup_size(&sizes, p);
                let keep = newest.as_deref() == Some(p.as_path());

                let mut metrics: HashMap<String, MetricValue> = HashMap::new();
                metrics.insert("size".into(), MetricValue::Bytes(size));
                metrics.insert(
                    "last_modified".into(),
                    match mtime_secs(p) {
                        Some(ts) => MetricValue::Int(ts),
                        None => MetricValue::text("未知"),
                    },
                );
                metrics.insert("kind".into(), MetricValue::text("分发版"));

                items.push(Item {
                    id: format!("java:gradle:dist:{}", version),
                    tags: if keep {
                        vec![]
                    } else {
                        vec!["可清理".into()]
                    },
                    name: format!(
                        "Gradle · {} {}",
                        version,
                        if keep { "（最新）" } else { "（旧版）" }
                    ),
                    group_key: Some("Gradle".into()),
                    path: p.display().to_string(),
                    metrics,
                    detail: Some(detail(&[
                        ("0. 路径", p.display().to_string()),
                        (
                            "1. 说明",
                            if keep {
                                "版本最高，判为在用的那个，不计入可回收".into()
                            } else {
                                "有更新的分发版存在，旧的可安全移除".into()
                            },
                        ),
                    ])),
                    cleanable: !keep,
                    reclaimable: if keep { None } else { Some(size) },
                });
            }
        }
    }

    // --- 固定子目录：只有日志是纯历史，缓存删了要重建 ---
    const SUBS: [(&str, &str, bool); 3] = [
        ("caches", "构建缓存", false),
        ("daemon", "守护日志", true),
        ("native", "本地库", false),
    ];

    for (sub, kind, cleanable) in SUBS {
        let p = root.join(sub);
        if !p.is_dir() {
            continue;
        }
        let size = dir_size_fast(&p).unwrap_or(0);

        let mut metrics: HashMap<String, MetricValue> = HashMap::new();
        metrics.insert("size".into(), MetricValue::Bytes(size));
        metrics.insert(
            "last_modified".into(),
            match mtime_secs(&p) {
                Some(ts) => MetricValue::Int(ts),
                None => MetricValue::text("未知"),
            },
        );
        metrics.insert("kind".into(), MetricValue::text(kind));

        items.push(Item {
            id: format!("java:gradle:{}", sub),
            tags: if cleanable {
                vec!["可清理".into()]
            } else {
                vec![]
            },
            name: format!("Gradle · {}", kind),
            group_key: Some("Gradle".into()),
            path: p.display().to_string(),
            metrics,
            detail: Some(detail(&[
                ("0. 路径", p.display().to_string()),
                (
                    "1. 说明",
                    if cleanable {
                        "Gradle 守护进程的运行日志，删掉不影响构建".into()
                    } else {
                        "删了下次构建会重新生成或重新下载".into()
                    },
                ),
            ])),
            cleanable,
            reclaimable: if cleanable { Some(size) } else { None },
        });
    }

    items.sort_by(|a, b| {
        let sa = match a.metrics.get("size") {
            Some(MetricValue::Bytes(v)) => *v,
            _ => 0,
        };
        let sb = match b.metrics.get("size") {
            Some(MetricValue::Bytes(v)) => *v,
            _ => 0,
        };
        sb.cmp(&sa)
    });

    items
}

/// 从 `jdk-17.jdk` / `temurin-21.jdk` / `zulu-17.0.9.jdk` 里取版本段。
fn jdk_version(dir_name: &str) -> String {
    let stem = dir_name.trim_end_matches(".jdk");
    stem.split('-')
        .find(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .unwrap_or(stem)
        .to_string()
}

fn jdk_items() -> Vec<Item> {
    let root = jdk_root();
    let Ok(dir) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    let entries: Vec<PathBuf> = dir
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_type().map(|t| t.is_symlink()).unwrap_or(true))
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();

    if entries.is_empty() {
        return Vec::new();
    }

    let sizes = dir_sizes_batch(&entries);

    let mut items: Vec<Item> = entries
        .iter()
        .map(|p| {
            let dir_name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let version = jdk_version(&dir_name);
            let size = lookup_size(&sizes, p);

            let mut metrics: HashMap<String, MetricValue> = HashMap::new();
            metrics.insert("size".into(), MetricValue::Bytes(size));
            metrics.insert(
                "last_modified".into(),
                match mtime_secs(p) {
                    Some(ts) => MetricValue::Int(ts),
                    None => MetricValue::text("未知"),
                },
            );
            metrics.insert("kind".into(), MetricValue::text("运行时"));

            Item {
                id: format!("java:jdk:{}", version),
                // JDK 不标可回收：本机 17 与 21 都是主流长期支持版，
                // 装几个版本是主动选择（不同项目要不同版本），不是残留。
                tags: vec![],
                name: format!("JDK · {}", version),
                group_key: Some("JDK".into()),
                path: p.display().to_string(),
                metrics,
                detail: Some(detail(&[
                    ("0. 路径", p.display().to_string()),
                    ("1. 目录", dir_name),
                ])),
                cleanable: false,
                reclaimable: None,
            }
        })
        .collect();

    items.sort_by(|a, b| {
        let sa = match a.metrics.get("size") {
            Some(MetricValue::Bytes(v)) => *v,
            _ => 0,
        };
        let sb = match b.metrics.get("size") {
            Some(MetricValue::Bytes(v)) => *v,
            _ => 0,
        };
        sb.cmp(&sa)
    });

    items
}

impl Scanner for JavaScanner {
    fn id(&self) -> &'static str {
        "java"
    }

    fn label(&self) -> &'static str {
        "Java 环境"
    }

    fn scan(&self) -> AssetGroup {
        let mut items: Vec<Item> = Vec::new();

        if let Some(root) = self.maven_root() {
            items.extend(maven_items(&root));
        }
        items.extend(gradle_items(&self.home));
        items.extend(jdk_items());

        let total: u64 = items
            .iter()
            .filter_map(|i| match i.metrics.get("size") {
                Some(MetricValue::Bytes(b)) => Some(*b),
                _ => None,
            })
            .sum();
        let reclaimable: u64 = items.iter().filter_map(|i| i.reclaimable).sum();
        let cleanable_count = items.iter().filter(|i| i.cleanable).count();

        let mut notes: Vec<String> = vec![
            "只统计文件名与体积，不读取任何文件内容".into(),
            "Maven 依赖缓存可从网络重建，但删掉要重新下载，不计入可回收".into(),
        ];
        if items.iter().any(|i| i.id.starts_with("java:jdk:")) {
            notes.push(
                "多个 JDK 并存在这里不算残留：17 与 21 都是主流长期支持版，不同项目要求不同，留谁是主动选择".into(),
            );
        }
        if cleanable_count > 0 {
            notes.push("Gradle 旧分发版与守护日志标为可回收，清理入口启用前只显示不删除".into());
        }

        AssetGroup {
            id: self.id().into(),
            label: self.label().into(),
            summary: format!(
                "{} 项 · 合计 {} · {} 项可回收 · 共 {}",
                items.len(),
                human_bytes(total),
                cleanable_count,
                human_bytes(reclaimable)
            ),
            fields: dev_fields(),
            items,
            notes: Some(notes),
            error: None,
            supports_clean: false,
        }
    }
}

// ============================================================================
// 移动端：iOS / Android / 鸿蒙
//
// 这三个在本机都没装。做成探测型而不是直接实现完整扫描，
// 是因为扫出来必然是空分组——与其让侧栏挂三个 0 条目的 tab，
// 不如让它们在没有环境时整体消失。
// ============================================================================

/// 探测位置。精确路径最常见，但 IDE 缓存目录带版本号（`AndroidStudio2023`），
/// 只能用前缀在父目录里匹配。
enum Spot {
    Exact(PathBuf),
    Prefix {
        parent: PathBuf,
        prefix: &'static str,
    },
}

impl Spot {
    fn resolve(&self) -> Vec<PathBuf> {
        match self {
            Spot::Exact(p) => {
                if p.exists() {
                    vec![p.clone()]
                } else {
                    vec![]
                }
            }
            Spot::Prefix { parent, prefix } => std::fs::read_dir(parent)
                .map(|d| {
                    d.filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| {
                            p.file_name()
                                .map(|n| n.to_string_lossy().starts_with(prefix))
                                .unwrap_or(false)
                        })
                        .filter(|p| p.is_dir())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

struct Probe {
    name: &'static str,
    kind: &'static str,
    spot: Spot,
    cleanable: bool,
}

pub struct DevEnvScanner {
    id: &'static str,
    label: &'static str,
    /// 分组里说明「扫了哪些位置」，环境没装时用户能自己核对
    locations: Vec<String>,
    probes: Vec<Probe>,
}

impl DevEnvScanner {
    /// 一个探测位置都不存在就不生成 Scanner——侧栏不该出现空分组。
    pub(crate) fn any_present(&self) -> bool {
        self.probes.iter().any(|p| !p.spot.resolve().is_empty())
    }

    pub fn ios(home: &Path, p: Platform) -> Self {
        // Xcode 只有 macOS 有。Windows 上给空 probes —— `any_present()` 为假，
        // 分组自然不注册（探测型分组的既有语义，见 AGENTS 硬约束 8）。
        // 不是特例处理，是「环境不存在就不占侧栏」这条规则在跨平台下的自然结果。
        if p.is_windows() {
            return Self {
                id: "ios",
                label: "iOS 环境",
                locations: vec![],
                probes: vec![],
            };
        }
        let dev = home.join("Library").join("Developer");
        Self {
            id: "ios",
            label: "iOS 环境",
            locations: vec![
                "/Applications/Xcode*.app".into(),
                "~/Library/Developer/Xcode/DerivedData".into(),
                "~/Library/Developer/Xcode/Archives".into(),
                "~/Library/Developer/Xcode/iOS DeviceSupport".into(),
                "~/Library/Developer/CoreSimulator/Devices".into(),
                "~/Library/Caches/com.apple.dt.Xcode".into(),
            ],
            probes: vec![
                Probe {
                    name: "DerivedData",
                    kind: "构建产物",
                    spot: Spot::Exact(dev.join("Xcode").join("DerivedData")),
                    cleanable: true,
                },
                Probe {
                    name: "Archives",
                    kind: "归档",
                    spot: Spot::Exact(dev.join("Xcode").join("Archives")),
                    cleanable: false,
                },
                Probe {
                    name: "iOS DeviceSupport",
                    kind: "设备支持",
                    spot: Spot::Exact(dev.join("Xcode").join("iOS DeviceSupport")),
                    cleanable: true,
                },
                Probe {
                    name: "CoreSimulator",
                    kind: "模拟器",
                    spot: Spot::Exact(dev.join("CoreSimulator").join("Devices")),
                    cleanable: true,
                },
                Probe {
                    name: "Xcode 缓存",
                    kind: "缓存",
                    spot: Spot::Exact(
                        home.join("Library")
                            .join("Caches")
                            .join("com.apple.dt.Xcode"),
                    ),
                    cleanable: true,
                },
                // IDE 本体才是 iOS 环境最大的一块（装满约 12 GB），不扫就漏掉大头
                Probe {
                    name: "Xcode",
                    kind: "IDE",
                    spot: Spot::Prefix {
                        parent: PathBuf::from("/Applications"),
                        prefix: "Xcode",
                    },
                    cleanable: false,
                },
            ],
        }
    }

    pub fn android(home: &Path, p: Platform) -> Self {
        // Android SDK 两平台的默认位置完全不同：macOS 在 ~/Library/Android/sdk，
        // Windows 在 %LOCALAPPDATA%\Android\sdk。
        let (sdk_rel, ide_candidates, cache_rel, sdk_show, ide_show): (
            &str,
            &[&str],
            &str,
            &str,
            &str,
        ) = if p.is_windows() {
            (
                "AppData/Local/Android/sdk",
                // 用户级安装（Android Studio 的默认）与机器级安装都要认
                &[
                    "AppData/Local/Programs/Android Studio",
                    "Program Files/Android/Android Studio",
                ],
                "AppData/Local/Google",
                "%LOCALAPPDATA%\\Android\\sdk",
                "%LOCALAPPDATA%\\Programs\\Android Studio",
            )
        } else {
            (
                "Library/Android/sdk",
                &["/Applications"],
                "Library/Caches/Google",
                "~/Library/Android/sdk",
                "/Applications/Android Studio*.app",
            )
        };
        let sdk = home.join(sdk_rel);
        // 候选里挑第一个存在的；都不在就退回主候选，让 Prefix 探空（不出行）。
        // 多候选是既有惯例（见 docker.rs::find_docker）——安装器不同路径就不同。
        let ide_parent =
            first_existing(home, ide_candidates).unwrap_or_else(|| home.join(ide_candidates[0]));
        Self {
            id: "android",
            label: "Android 环境",
            locations: vec![
                ide_show.into(),
                format!("{sdk_show}/system-images"),
                format!("{sdk_show}/platforms"),
                format!("{sdk_show}/build-tools"),
                format!("{sdk_show}/ndk"),
                format!("{sdk_show}/emulator"),
                "~/.android/avd".into(),
                format!("{cache_rel}/AndroidStudio*"),
            ],
            probes: vec![
                Probe {
                    name: "system-images",
                    kind: "系统镜像",
                    spot: Spot::Exact(sdk.join("system-images")),
                    cleanable: true,
                },
                Probe {
                    name: "platforms",
                    kind: "SDK 组件",
                    spot: Spot::Exact(sdk.join("platforms")),
                    cleanable: false,
                },
                Probe {
                    name: "build-tools",
                    kind: "SDK 组件",
                    spot: Spot::Exact(sdk.join("build-tools")),
                    cleanable: false,
                },
                Probe {
                    name: "ndk",
                    kind: "SDK 组件",
                    spot: Spot::Exact(sdk.join("ndk")),
                    cleanable: false,
                },
                Probe {
                    name: "emulator",
                    kind: "模拟器",
                    spot: Spot::Exact(sdk.join("emulator")),
                    cleanable: false,
                },
                Probe {
                    name: "avd",
                    kind: "模拟器",
                    spot: Spot::Exact(home.join(".android").join("avd")),
                    cleanable: true,
                },
                Probe {
                    name: "Android Studio 缓存",
                    kind: "缓存",
                    spot: Spot::Prefix {
                        parent: home.join(cache_rel),
                        prefix: "AndroidStudio",
                    },
                    cleanable: true,
                },
                Probe {
                    name: "Android Studio",
                    kind: "IDE",
                    spot: Spot::Prefix {
                        parent: ide_parent,
                        prefix: "Android Studio",
                    },
                    cleanable: false,
                },
            ],
        }
    }

    pub fn harmony(home: &Path, p: Platform) -> Self {
        // **鸿蒙是倒挂的**：DevEco Studio 的主战场在 Windows，而这里原先只扫 macOS 路径。
        // Windows 侧按惯例布局给候选，**待实机核对**——核错的后果是探不到该探的目录
        // （少显示一块），不会显示成错的数。
        let (ide_candidates, sdk_candidates, ide_cfg_candidates, ide_show, sdk_show): (
            &[&str],
            &[&str],
            &[&str],
            &str,
            &str,
        ) = if p.is_windows() {
            (
                &[
                    "AppData/Local/Programs/Huawei/DevEco Studio",
                    "Program Files/Huawei/DevEco Studio",
                ],
                &["AppData/Local/Huawei/Sdk", "AppData/Local/OpenHarmony/Sdk"],
                &[
                    "AppData/Roaming/Huawei/DevEcoStudio",
                    "AppData/Local/Huawei/DevEcoStudio",
                ],
                "%LOCALAPPDATA%\\Programs\\Huawei\\DevEco Studio",
                "%LOCALAPPDATA%\\Huawei\\Sdk",
            )
        } else {
            (
                &["/Applications"],
                &["Library/Huawei/Sdk"],
                &["Library/Application Support/Huawei"],
                "/Applications/DevEco*.app",
                "~/Library/Huawei/Sdk",
            )
        };
        let ide_parent =
            first_existing(home, ide_candidates).unwrap_or_else(|| home.join(ide_candidates[0]));
        let sdk =
            first_existing(home, sdk_candidates).unwrap_or_else(|| home.join(sdk_candidates[0]));
        let ide_cfg = first_existing(home, ide_cfg_candidates)
            .unwrap_or_else(|| home.join(ide_cfg_candidates[0]));
        let ide_cfg_show = if p.is_windows() {
            "%APPDATA%\\Huawei\\DevEcoStudio*"
        } else {
            "~/Library/Application Support/Huawei/DevEco*"
        };
        Self {
            id: "harmony",
            label: "鸿蒙环境",
            locations: vec![
                ide_show.into(),
                sdk_show.into(),
                "~/.hvigor".into(),
                "~/.ohpm".into(),
                ide_cfg_show.into(),
                "~/.deveco".into(),
                "~/HarmonyOS".into(),
            ],
            probes: vec![
                Probe {
                    name: "DevEco Studio",
                    kind: "IDE",
                    spot: Spot::Prefix {
                        parent: ide_parent,
                        prefix: "DevEco",
                    },
                    cleanable: false,
                },
                Probe {
                    name: "SDK",
                    kind: "SDK 组件",
                    spot: Spot::Exact(sdk),
                    cleanable: false,
                },
                // hvigor 是鸿蒙的构建工具，ohpm 是包管理器——这两个缓存目录
                // 才是鸿蒙环境的主体，只扫 Sdk 会完全漏掉它们。
                Probe {
                    name: "hvigor",
                    kind: "构建缓存",
                    spot: Spot::Exact(home.join(".hvigor")),
                    cleanable: false,
                },
                Probe {
                    name: "ohpm",
                    kind: "依赖缓存",
                    spot: Spot::Exact(home.join(".ohpm")),
                    cleanable: false,
                },
                Probe {
                    name: "DevEco Studio 配置",
                    kind: "IDE 数据",
                    spot: Spot::Prefix {
                        parent: ide_cfg,
                        prefix: "DevEco",
                    },
                    cleanable: false,
                },
                Probe {
                    name: "命令行工具",
                    kind: "配置",
                    spot: Spot::Exact(home.join(".deveco")),
                    cleanable: false,
                },
                Probe {
                    name: "HarmonyOS",
                    kind: "工程目录",
                    spot: Spot::Exact(home.join("HarmonyOS")),
                    cleanable: false,
                },
            ],
        }
    }
}

impl Scanner for DevEnvScanner {
    fn id(&self) -> &'static str {
        self.id
    }

    fn label(&self) -> &'static str {
        self.label
    }

    fn scan(&self) -> AssetGroup {
        // 先摊平成 (probe, path)，再并行 du——模拟器镜像这类大目录一次就好几秒
        let mut targets: Vec<(&Probe, PathBuf)> = Vec::new();
        for p in &self.probes {
            for path in p.spot.resolve() {
                targets.push((p, path));
            }
        }

        let sized: Vec<(&Probe, PathBuf, u64)> = targets
            .into_par_iter()
            .map(|(p, path)| {
                let size = dir_size_fast(&path).unwrap_or(0);
                (p, path, size)
            })
            .collect();

        let mut items: Vec<Item> = sized
            .into_iter()
            .map(|(p, path, size)| {
                let mut metrics: HashMap<String, MetricValue> = HashMap::new();
                metrics.insert("size".into(), MetricValue::Bytes(size));
                metrics.insert(
                    "last_modified".into(),
                    match mtime_secs(&path) {
                        Some(ts) => MetricValue::Int(ts),
                        None => MetricValue::text("未知"),
                    },
                );
                metrics.insert("kind".into(), MetricValue::text(p.kind));

                Item {
                    id: format!("{}:{}", self.id, path.display()),
                    tags: if p.cleanable {
                        vec!["可清理".into()]
                    } else {
                        vec![]
                    },
                    name: p.name.into(),
                    // iOS / Android / 鸿蒙的条目名不带分节前缀（每行就是一条独立的
                    // 目录/组件），所以 group_key 留 None——分节是给「同一工具下多个
                    // 类别」那种结构用的。
                    group_key: None,
                    path: path.display().to_string(),
                    metrics,
                    detail: Some(detail(&[("0. 路径", path.display().to_string())])),
                    cleanable: p.cleanable,
                    reclaimable: if p.cleanable { Some(size) } else { None },
                }
            })
            .collect();

        items.sort_by(|a, b| {
            let sa = match a.metrics.get("size") {
                Some(MetricValue::Bytes(v)) => *v,
                _ => 0,
            };
            let sb = match b.metrics.get("size") {
                Some(MetricValue::Bytes(v)) => *v,
                _ => 0,
            };
            sb.cmp(&sa)
        });

        let total: u64 = items
            .iter()
            .filter_map(|i| match i.metrics.get("size") {
                Some(MetricValue::Bytes(b)) => Some(*b),
                _ => None,
            })
            .sum();
        let reclaimable: u64 = items.iter().filter_map(|i| i.reclaimable).sum();
        let cleanable_count = items.iter().filter(|i| i.cleanable).count();

        let mut notes: Vec<String> = vec!["只统计文件名与体积，不读取任何文件内容".into()];
        if items.is_empty() {
            notes.push(format!(
                "以上 {} 个位置都不存在，环境可能未安装",
                self.locations.len()
            ));
        }

        AssetGroup {
            id: self.id.into(),
            label: self.label.into(),
            summary: format!(
                "{} 项 · 合计 {} · {} 项可回收 · 共 {}",
                items.len(),
                human_bytes(total),
                cleanable_count,
                human_bytes(reclaimable)
            ),
            fields: dev_fields(),
            items,
            notes: Some(notes),
            error: None,
            supports_clean: false,
        }
    }
}

/// 按 home 组装开发环境扫描器。**环境不存在就不返回**，
/// 侧栏因此不会出现空分组。
pub fn dev_env_scanners(home: &Path) -> Vec<Box<dyn Scanner>> {
    dev_env_scanners_on(home, Platform::current())
}

/// 指定平台构造三个探测型分组。**测试靠它在本机断言 Windows 那半边**——
/// 本机是 macOS，`cfg` 一写就永远测不到另一侧。
pub(crate) fn dev_env_scanners_on(home: &Path, p: Platform) -> Vec<Box<dyn Scanner>> {
    let mut v: Vec<Box<dyn Scanner>> = Vec::new();

    let java = JavaScanner::new(home.to_path_buf());
    if java.exists() {
        v.push(Box::new(java));
    }

    for probe in [
        DevEnvScanner::ios(home, p),
        DevEnvScanner::android(home, p),
        DevEnvScanner::harmony(home, p),
    ] {
        if probe.any_present() {
            v.push(Box::new(probe));
        }
    }

    v
}

/// 前端只渲染 `fields.slice(0, 3)`，这里正好三个。
fn dev_fields() -> Vec<Field> {
    vec![
        Field {
            key: "size".into(),
            label: "体积".into(),
            numeric: Some(true),
            temporal: None,
        },
        Field {
            key: "last_modified".into(),
            label: "最后修改".into(),
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

// ---- 测试辅助：把纯函数暴露给 tests.rs，避免重复实现 ----

#[cfg(test)]
pub(crate) fn gradle_version_for_test(s: &str) -> String {
    gradle_version(s)
}

#[cfg(test)]
pub(crate) fn jdk_version_for_test(s: &str) -> String {
    jdk_version(s)
}

#[cfg(test)]
pub(crate) fn version_major_for_test(s: &str) -> u64 {
    version_major(s)
}

/// 给测试用的简化视图：只要 groupId 与体积，够验证拆分粒度。
#[cfg(test)]
pub(crate) fn maven_rows_for_test(root: &Path) -> Vec<(String, u64)> {
    maven_rows(root)
        .into_iter()
        .map(|r| (r.name, r.size))
        .collect()
}

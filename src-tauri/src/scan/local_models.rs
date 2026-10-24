//! 本地模型扫描（G2 / D4 拍板选 A：独立分组「本地模型」）。
//!
//! # 为什么单列一组
//!
//! 模型是**资产**不是缓存：删一个要重新下载，几十 GB 起步，且往往是刻意
//! 留着的（天天用的那个）。所以本组的口径与其它组相反——**永不标可回收**，
//! 台账价值在「装了哪些模型、各占多少」。三个来源覆盖了最常见的下载端：
//! Ollama、LM Studio、HuggingFace。
//!
//! # 各来源的行怎么来
//!
//! - **Ollama**（`~/.ollama/models`）：模型清单在 `manifests/`（一个模型一个
//!   小 JSON，记录它引用哪些权重层），权重本体是全模型共享的 `blobs/` 池。
//!   所以一个模型占多少 = 它 manifest 引用的 blob 在磁盘上的占用之和。
//!   **共享层不去重**（一个层被两个 tag 引用就在两边各计一次）——与 S4 的
//!   「跨行共享各算各的」同一条口径：行加总可以大于全局唯一字节数，而
//!   「按引用计」才是「这个模型攒出来要多大」的答案。高估的另一面是「删掉
//!   这个 tag 实际腾出多少」答不了——P3 把它补在 detail 的共享层一行：数每
//!   个 blob 被几个 manifest 引用，真有共享的行给出「M 个层被其它模型引用
//!   （X）——删这个模型实际腾出 Y」（借鉴 HF `huggingface-cli delete-cache`
//!   的会计方式；纯 manifest 引用计数，不写盘、不做 symlink）。权重文件缺失
//!   时只计磁盘上有的部分，并在明细里报缺失层数——本机 `qwen2.5:7b` 的
//!   4.7 GB 权重层就是缺失状态（manifest 在、blob 被删了），照 manifest 里的
//!   size 报会报出一个 4.7 GB 的幻影。
//! - **LM Studio**（`~/.lmstudio`）：`models/<发布者>/<模型>` 一个目录一个模型；
//!   `hub/models/` 是下载暂存（清单与缩略图在、权重没下完）。**只扫这两棵**：
//!   `~/.lmstudio` 下还有会话、日志、扩展与 `credentials/`，那些不是模型资产，
//!   也不该被一个「不接触任何凭据」的工具枚举。
//! - **HuggingFace**（`~/.cache/huggingface/hub`）：`models--<org>--<name>`
//!   一个目录一个模型。`snapshots/` 里是指向 `blobs/` 的软链，`dir_size` 不
//!   跟随软链，所以按目录整体计就等于只计权重本体，不会双计。
//!
//! # 读内容的地方（比其它组多读一点，理由与边界写在这）
//!
//! 全库的默认纪律是「只读文件名与元数据」（`ai_tools.rs` 的安全约束）。
//! 本组破例一处，限定在**模型元数据**、不碰凭据：
//!
//! 1. **Ollama 的 manifest JSON**：不读它就无法知道一个模型引用哪些 blob
//!    （blob 按 sha256 命名，文件名里没有任何模型信息）。内容是层清单
//!    （digest + size + mediaType），不含凭据。
//! 2. 除此之外**什么都不读**：权重本体、LM Studio 的 `settings.json` /
//!    `manifest.json`、HF 的快照，一律只走 `metadata()` 与 `read_dir`。
//!
//! # 平台
//!
//! 三个根目录都是 dot-dir，macOS 与 Windows 相同（`.ollama` / `.lmstudio` /
//! `.cache/huggingface`），不拆两栏（硬约束 9：两边一致的落到 `win_rel`
//! 兜底 arm）。探测型：一个来源都不存在时整个分组不进侧栏（同 toolchain）。

use crate::scan::dirsize::{dir_size, file_size};
use crate::scan::repos::{human_bytes, mtime_secs};
use crate::scan::{AssetGroup, Field, Item, MetricValue, Scanner};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct LocalModelsScanner {
    home: PathBuf,
}

impl LocalModelsScanner {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }

    /// 探测型：三个来源目录一个都不存在就不进侧栏（硬约束 8）。
    pub fn exists(&self) -> bool {
        source_roots(&self.home).iter().any(|r| r.exists())
    }
}

/// 三个来源的根目录。都是 dot-dir，两边平台一致（见模块注释的平台节）。
fn source_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".ollama/models"),
        home.join(".lmstudio/models"),
        home.join(".cache/huggingface/hub"),
    ]
}

fn local_model_fields() -> Vec<Field> {
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
    ]
}

/// 时间指标：读不到修改时间就明说「未知」，不编一个 0（0 会被显示成 1970）。
fn mtime_metric(p: &Path) -> MetricValue {
    match mtime_secs(p) {
        Some(ts) => MetricValue::Int(ts),
        None => MetricValue::text("未知"),
    }
}

impl Scanner for LocalModelsScanner {
    fn id(&self) -> &'static str {
        "local_models"
    }

    fn label(&self) -> &'static str {
        "本地模型"
    }

    fn scan(&self) -> AssetGroup {
        let mut items: Vec<Item> = Vec::new();
        items.extend(ollama_items(&self.home.join(".ollama/models")));
        items.extend(lmstudio_items(&self.home.join(".lmstudio")));
        items.extend(huggingface_items(&self.home.join(".cache/huggingface/hub")));

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

        let count = items.len();
        let total: u64 = items
            .iter()
            .filter_map(|it| match it.metrics.get("size") {
                Some(MetricValue::Bytes(v)) => Some(*v),
                _ => None,
            })
            .sum();

        let notes = vec![
            "模型是资产不是缓存：删一个要重新下载（几十 GB 起步），所以本组永不标可回收".into(),
            "Ollama 按 manifest 解析每个模型引用的权重层；权重文件缺失时只计磁盘上存在的部分，缺失层数写在明细里".into(),
            "同一模型装了多个 tag 时体积按引用各计一次；删掉单个 tag 实际腾出多少看明细的「共享层」一行（只在真有共享时出现）".into(),
            "LM Studio 只扫 models/ 与 hub/models/——~/.lmstudio 下的会话、日志与凭据目录不是模型资产，本工具不枚举".into(),
            "HuggingFace 按模型目录整体计（snapshots 是指向 blobs 的软链，不重复计）".into(),
        ];

        AssetGroup {
            id: self.id().into(),
            label: self.label().into(),
            summary: format!("{} 个模型 · 合计 {}", count, human_bytes(total)),
            fields: local_model_fields(),
            items,
            notes: Some(notes),
            error: None,
            supports_clean: false,
        }
    }
}

// ---- Ollama ----

/// Ollama 的行：`manifests/` 下每个文件是一个模型 tag 的层清单。
///
/// 模型名取路径最后两段（`…/library/qwen3/14b` → `qwen3:14b`）——自定义
/// registry 的布局同形，只是前缀不同，取最后两段对两种都对。
/// `Item.name` 拼成 `Ollama · qwen3:14b`：分节前缀与 `group_key` 一致是全库
/// 约定（两边契约都断言它，前端行名靠剥掉这个前缀显示）。
///
/// **两遍读**：先把全部 manifest 的层清单读出来，数每个 blob 被几个 manifest
/// 引用（blobs 池按内容寻址，没有「模型边界」——同一模型的不同 tag、不同名字
/// 的模型，digest 相同就是共用），再逐行算体积与共享账（P3）。manifest 都是
/// 小 JSON，两遍读的代价可忽略。
fn ollama_items(models_dir: &Path) -> Vec<Item> {
    let manifests = models_dir.join("manifests");
    if !manifests.exists() {
        return vec![];
    }
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&manifests, &mut files);
    files.sort();

    let parsed: Vec<Option<Vec<String>>> = files
        .iter()
        .map(|m| {
            std::fs::read_to_string(m)
                .ok()
                .and_then(|r| parse_manifest_layers(&r))
        })
        .collect();
    let mut refs: HashMap<&str, usize> = HashMap::new();
    for layers in parsed.iter().flatten() {
        for d in layers {
            *refs.entry(d.as_str()).or_default() += 1;
        }
    }

    let blobs = models_dir.join("blobs");
    files
        .iter()
        .zip(parsed.iter())
        .filter_map(|(m, layers)| ollama_item(m, layers.as_deref()?, &blobs, &refs))
        .collect()
}

/// manifest JSON → 它引用的 digest 列表（`sha256:<hex>` 原样）。解析失败
/// （不是 JSON / 没有 layers）给 `None`——整行跳过：读不出层清单就答不了
/// 「这个模型占多少」，硬报一个 0 是撒谎（原行为，抽出来好喂样本）。
fn parse_manifest_layers(raw: &str) -> Option<Vec<String>> {
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    let layers = parsed.get("layers")?.as_array()?;
    Some(
        layers
            .iter()
            .filter_map(|l| l.get("digest").and_then(|d| d.as_str()).map(String::from))
            .collect(),
    )
}

/// 递归收集文件（不跟随软链——权重池里的软链不该顺着跑出扫描根）。
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            collect_files(&e.path(), out);
        } else if ft.is_file() {
            out.push(e.path());
        }
    }
}

/// 一个 manifest → 一行。层清单由调用方解析好传入（[`parse_manifest_layers`]）。
///
/// 共享账（P3）：`refs` 是「每个 digest 被几个 manifest 引用」。`size` 照旧
/// 按引用计（共享层在引用它的每个 tag 行上各计一次，与 S4 的「跨行共享各算
/// 各的」同一条口径——它是「这个模型攒出来要多大」的答案）；「删掉这个 tag
/// 实际腾出多少」是另一个问题，答案只写在 detail 的共享层一行里——只在真有
/// 共享时出现（全部层独占时答案就是 size 本身，不用再说一遍）。
fn ollama_item(
    manifest: &Path,
    digests: &[String],
    blobs_dir: &Path,
    refs: &HashMap<&str, usize>,
) -> Option<Item> {
    let name = manifest
        .parent()
        .and_then(|p| p.file_name())
        .zip(manifest.file_name())
        .map(|(model, tag)| format!("{}:{}", model.to_string_lossy(), tag.to_string_lossy()))?;

    let mut size: u64 = 0;
    let mut missing = 0usize;
    let mut shared_count = 0usize;
    let mut shared_bytes: u64 = 0;
    for digest in digests {
        // digest 形如 `sha256:<hex>`，blob 文件名是 `sha256-<hex>`
        let blob = blobs_dir.join(digest.replace(':', "-"));
        if !blob.exists() {
            missing += 1;
            continue;
        }
        let bytes = file_size(&blob);
        size += bytes;
        if refs.get(digest.as_str()).copied().unwrap_or(1) > 1 {
            shared_count += 1;
            shared_bytes += bytes;
        }
    }

    let mut detail: HashMap<String, String> = HashMap::new();
    detail.insert("0. 路径".into(), manifest.display().to_string());
    detail.insert("1. 权重层".into(), format!("{} 个", digests.len()));
    if missing > 0 {
        detail.insert(
            "2. 缺失层".into(),
            format!("{missing} 个（权重文件不在磁盘上，未计入体积）"),
        );
    }
    if shared_count > 0 {
        detail.insert(
            "3. 共享层".into(),
            format!(
                "{} 个层被其它模型引用（{}）——删这个模型实际腾出 {}",
                shared_count,
                human_bytes(shared_bytes),
                human_bytes(size - shared_bytes)
            ),
        );
    }

    Some(Item {
        id: format!("model:ollama:{}", manifest.display()),
        tags: vec![],
        name: format!("Ollama · {name}"),
        group_key: Some("Ollama".into()),
        path: manifest.display().to_string(),
        metrics: [
            ("size".into(), MetricValue::Bytes(size)),
            ("last_modified".into(), mtime_metric(manifest)),
        ]
        .into_iter()
        .collect(),
        detail: Some(detail),
        artifact_entries: vec![],
        cleanable: false,
        reclaimable: None,
    })
}

// ---- LM Studio ----

/// `models/<发布者>/<模型>` 一个目录一个模型；`hub/models/` 是下载暂存。
fn lmstudio_items(root: &Path) -> Vec<Item> {
    let mut items = Vec::new();
    items.extend(publisher_model_items(&root.join("models"), false));
    items.extend(publisher_model_items(&root.join("hub/models"), true));
    items
}

/// 两层目录结构：`<发布者>/<模型>`。发布者目录读不动 / 没有子目录就整体算一行。
fn publisher_model_items(root: &Path, staging: bool) -> Vec<Item> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut out = Vec::new();
    for pub_entry in rd.flatten() {
        let pub_path = pub_entry.path();
        let Ok(ft) = pub_entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let publisher = pub_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let Ok(models) = std::fs::read_dir(&pub_path) else {
            out.push(model_item(&pub_path, &publisher, staging));
            continue;
        };
        let mut subs: Vec<PathBuf> = models
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        if subs.is_empty() {
            out.push(model_item(&pub_path, &publisher, staging));
            continue;
        }
        subs.sort();
        for m in subs {
            out.push(model_item(&m, &publisher, staging));
        }
    }
    out
}

fn model_item(dir: &Path, publisher: &str, staging: bool) -> Item {
    let model = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let size = dir_size(dir, &std::collections::HashSet::new());

    let mut detail: HashMap<String, String> = HashMap::new();
    detail.insert("0. 路径".into(), dir.display().to_string());
    detail.insert("1. 发布者".into(), publisher.into());
    if staging {
        detail.insert(
            "2. 状态".into(),
            "下载暂存：清单与缩略图可能在、权重没下完".into(),
        );
    }
    // 目录里没有文件 = 权重不在磁盘上（LM Studio 会把「知道有这个模型」也
    // 留一个空目录）。照实报 0 会让人以为它不占地方，明细里说清楚。
    if size == Some(0) {
        detail.insert("3. 权重".into(), "不在磁盘上（目录为空）".into());
    }

    // 体积未知（遍历超时）就**不放这个指标**——界面显示 `—`；放 0 等于说它不占地方
    let mut metrics: HashMap<String, MetricValue> = HashMap::new();
    if let Some(v) = size {
        metrics.insert("size".into(), MetricValue::Bytes(v));
    }
    metrics.insert("last_modified".into(), mtime_metric(dir));

    Item {
        id: format!("model:lmstudio:{}", dir.display()),
        tags: vec![],
        name: format!("LM Studio · {model}"),
        group_key: Some("LM Studio".into()),
        path: dir.display().to_string(),
        metrics,
        detail: Some(detail),
        artifact_entries: vec![],
        cleanable: false,
        reclaimable: None,
    }
}

// ---- HuggingFace ----

/// `hub/models--<org>--<name>` 一个目录一个模型。名字里的 `--` 是转义的分隔
/// （org/name 里的 `-` 不转义），反除非 org 或 name 自己带 `--`。
fn huggingface_items(hub: &Path) -> Vec<Item> {
    let Ok(rd) = std::fs::read_dir(hub) else {
        return vec![];
    };
    rd.flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let dir = e.path();
            let raw = dir.file_name()?.to_string_lossy().into_owned();
            let rest = raw.strip_prefix("models--")?;
            let (org, name) = rest.split_once("--")?;
            let size = dir_size(&dir, &std::collections::HashSet::new());

            let mut metrics: HashMap<String, MetricValue> = HashMap::new();
            if let Some(v) = size {
                metrics.insert("size".into(), MetricValue::Bytes(v));
            }
            metrics.insert("last_modified".into(), mtime_metric(&dir));

            Some(Item {
                id: format!("model:huggingface:{}", dir.display()),
                tags: vec![],
                name: format!("HuggingFace · {org}/{name}"),
                group_key: Some("HuggingFace".into()),
                path: dir.display().to_string(),
                metrics,
                detail: Some(
                    [
                        ("0. 路径".to_string(), dir.display().to_string()),
                        ("1. 组织".to_string(), org.to_string()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                artifact_entries: vec![],
                cleanable: false,
                reclaimable: None,
            })
        })
        .collect()
}

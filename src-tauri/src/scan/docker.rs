//! Docker 资产扫描。
//!
//! 为什么单独做一组：本机 Docker Desktop 的数据目录实测 10.5 GB，
//! 和 Git 仓库、AI 工具数据是同一量级的本机资产，但它**不在文件系统里**——
//! 镜像层和卷都在 Linux 虚拟机内部，`du` 根本看不到，只能问 docker 自己。
//!
//! # 三个数字对不上，别当成 bug
//!
//! - `docker system df` 报镜像 7.5 GB：虚拟大小，共享层只算一次
//! - 明细行里的体积：每个镜像各自的虚拟大小，共享层被重复计算，各行之和**大于**上面的合计
//! - 桌面端目录实际占用 10.5 GB：虚拟机里还有系统盘与日志，比前两项都大
//!
//! 三个都给出来、口径写进 notes。少给一个，用户自己 `docker system df` 一对就会发现对不上，
//! 然后怀疑是扫错了。
//!
//! # 为什么不能只写 `Command::new("docker")`
//!
//! GUI 应用从 Finder 启动时 PATH 是 `/usr/bin:/bin:/usr/sbin:/sbin`，
//! 而 docker 装在 `/opt/homebrew/bin`。终端里 `npm run tauri dev` 找得到，
//! 打包后双击打开就找不到——这类"开发时好好的、上线后失效"的问题最难查，所以显式兜底。
//!
//! # 只读
//!
//! 全部走 `docker system df` / `image inspect` / `volume inspect`，没有任何写操作，
//! 也不注册清理命令（见 `mod.rs` 里清理入口的说明）。

use crate::scan::repos::dir_size_fast;
use crate::scan::{
    AssetGroup, DetailRow, Field, Item, MetricValue, Scanner, Tag, Text, TextArg, field_text,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct DockerScanner {
    /// Docker Desktop 的数据目录，没装桌面端则为 None
    desktop: Option<PathBuf>,
    /// docker 命令行，找不到则为 None
    bin: Option<PathBuf>,
}

impl DockerScanner {
    pub fn new(home: PathBuf) -> Self {
        // 只认 Docker Desktop 这一个位置。docker 的数据不在 home 的 dotfile 里，
        // 没有像 AI 工具那样的白名单可列——要么装了桌面端，要么没装。
        let desktop = home.join("Library/Containers/com.docker.docker");
        Self {
            desktop: if desktop.exists() {
                Some(desktop)
            } else {
                None
            },
            bin: find_docker(),
        }
    }

    /// 两个都找不到就别在侧栏里占位，给没装 docker 的用户一个空分组是噪音。
    pub fn present(&self) -> bool {
        self.bin.is_some() || self.desktop.is_some()
    }

    /// 守护进程不可用（或压根没装 docker）时的分组：仍然把磁盘占用报出来，
    /// 那正是没启动 Docker Desktop 的人最想知道的一件事。
    fn unavailable(&self, reason: Text, disk: Option<u64>) -> AssetGroup {
        let mut notes: Vec<Text> = Vec::new();
        if let Some(d) = disk {
            notes
                .push(Text::fmt("data.note.dockerDesktopDiskIdle").arg("bytes", TextArg::bytes(d)));
        }
        notes.push(Text::key("data.note.dockerStartHint"));
        AssetGroup {
            id: self.id().into(),
            label: self.label(),
            summary: reason.clone(),
            fields: docker_fields(),
            items: vec![],
            notes: Some(notes),
            error: Some(reason),
            supports_clean: false,
        }
    }
}

/// 先按 PATH 找，再兜底常见安装位置。
///
/// PATH 优先是因为用户可能用 nix / asdf 装在别处——PATH 里的那份是他自己认可的。
fn find_docker() -> Option<PathBuf> {
    const CANDIDATES: [&str; 4] = [
        "/opt/homebrew/bin/docker", // Apple 芯片上的 Homebrew
        "/usr/local/bin/docker",    // Intel Mac 上的 Homebrew
        "/usr/bin/docker",          // 系统包管理器装的位置
        "/Applications/Docker.app/Contents/Resources/bin/docker",
    ];

    let mut candidates: Vec<PathBuf> = std::env::var("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("docker"))
                .collect()
        })
        .unwrap_or_default();
    candidates.extend(CANDIDATES.iter().map(PathBuf::from));

    candidates.into_iter().find(|p| p.is_file())
}

/// 跑一次 docker 子进程。失败时把 stderr 首行带回去——
/// 「Cannot connect to the Docker daemon」这类信息直接给用户看比自己翻译准确。
fn run(bin: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let first = err.lines().next().unwrap_or("").trim();
        return Err(if first.is_empty() {
            format!("退出码 {:?}", out.status.code())
        } else {
            first.to_string()
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ============================================================================
// 解析。全部是纯函数，测试直接喂真实输出，不需要真跑 docker。
// ============================================================================

/// docker 的 human 体积是 **1000 进制**（kB/MB/GB），不是 1024。
/// 按 1024 解析会让 1.12GB 差出 8%。
pub(crate) fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() || s == "N/A" || s == "-" {
        return None;
    }
    let cut = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(cut);
    let mult: u64 = match unit.trim() {
        "B" | "b" | "" => 1,
        "kB" | "KB" | "kb" => 1_000,
        "MB" | "mb" => 1_000_000,
        "GB" | "gb" => 1_000_000_000,
        "TB" | "tb" => 1_000_000_000_000,
        _ => return None,
    };
    let v: f64 = num.parse().ok()?;
    Some((v * mult as f64).round() as u64)
}

/// `2026-10-01T04:14:50Z` 与 `2026-09-30 02:11:52 +0800 CST` 两种都吃：
/// inspect 给前者，`image ls` 给后者。
///
/// 手写而不引 chrono 是因为只要一个函数，多一个依赖不划算；
/// 日期转天数用 Hinnant 的 days_from_civil，避免自己算闰年出错。
pub(crate) fn parse_docker_time(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let date = &s[0..10];
    let time = &s[11..19];
    if s.as_bytes()[10] != b'T' && s.as_bytes()[10] != b' ' {
        return None;
    }

    let p = |r: std::ops::Range<usize>, src: &str| -> Option<i64> { src.get(r)?.parse().ok() };
    let y = p(0..4, date)?;
    let m = p(5..7, date)?;
    let d = p(8..10, date)?;
    let h = p(0..2, time)?;
    let mi = p(3..5, time)?;
    let sec = p(6..8, time)?;

    // 时区偏移：Z 结尾是 UTC，其余找 ±HHMM（`+0800 CST` 里的 CST 只是注释，忽略）。
    // 拿到的是 UTC 秒数，所以减偏移。
    let rest = &s[19..];
    // 用 contains 而不是 starts_with：纳秒精度的时间是 `...:52.517606715Z`，
    // 秒后面还挂着小数，Z 不在开头。
    let offset = if rest.contains('Z') {
        0
    } else {
        let sign = rest.find(['+', '-'])?;
        let digits = &rest[sign + 1..];
        let hh: i64 = digits.get(0..2)?.parse().ok()?;
        let mm: i64 = digits.get(2..4).and_then(|x| x.parse().ok()).unwrap_or(0);
        let total = hh * 3600 + mm * 60;
        if rest.as_bytes()[sign] == b'-' {
            total
        } else {
            -total
        }
    };

    Some(days_from_civil(y, m, d) * 86400 + h * 3600 + mi * 60 + sec + offset)
}

/// 公历日期 → 距 1970-01-01 的天数（Howard Hinnant 的算法，含负数年份）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - (m <= 2) as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + (if m > 2 { -3 } else { 9 })) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub(crate) struct ImageRow {
    pub(crate) repo: String,
    pub(crate) tag: String,
    pub(crate) id: String,
    /// 虚拟大小（含共享层）
    pub(crate) size: u64,
    pub(crate) shared: u64,
    /// 独占层大小——删掉这个镜像真正能回收的量
    pub(crate) unique: u64,
    pub(crate) containers: u32,
}

pub(crate) struct VolumeRow {
    pub(crate) name: String,
    pub(crate) links: u32,
    pub(crate) size: u64,
}

/// `docker system df -v` 的镜像行。
///
/// 左三列（REPOSITORY TAG IMAGE ID）和右四列（SIZE SHARED SIZE UNIQUE SIZE CONTAINERS）
/// 位置固定，中间 CREATED 的词数不定（"11 days ago" 三个词，"Less than a second ago" 五个），
/// 所以从两头夹着取，不去数中间的列。
pub(crate) fn parse_image_line(line: &str) -> Option<ImageRow> {
    let t: Vec<&str> = line.split_whitespace().collect();
    // 3 + 4 = 7 是最少列数，要求 8 是因为 CREATED 至少占一个词——
    // 少于 8 列说明这段格式变了，宁可跳过也不要填错列。
    if t.len() < 8 {
        return None;
    }
    let n = t.len();
    Some(ImageRow {
        repo: t[0].into(),
        tag: t[1].into(),
        id: t[2].into(),
        containers: t[n - 1].parse().ok()?,
        unique: parse_size(t[n - 2])?,
        shared: parse_size(t[n - 3]).unwrap_or(0),
        size: parse_size(t[n - 4])?,
    })
}

/// 卷段每行固定三列：VOLUME NAME / LINKS / SIZE。卷名不含空格。
pub(crate) fn parse_volume_line(line: &str) -> Option<VolumeRow> {
    let t: Vec<&str> = line.split_whitespace().collect();
    if t.len() != 3 {
        return None;
    }
    Some(VolumeRow {
        name: t[0].into(),
        links: t[1].parse().ok()?,
        size: parse_size(t[2])?,
    })
}

#[derive(PartialEq)]
enum Section {
    None,
    Images,
    Containers,
    Volumes,
    Done,
}

/// 按段落标题切分 `docker system df -v`。
///
/// 容器段只用来计数，不生成明细行——它的 COMMAND 列被截断且含空格、
/// STATUS 词数也不定，解析出来的行可靠性没有保证；而容器可写层一共才几十 MB，
/// 不值得为它冒填错列的风险。返回第三个值是跳过的行数，写进 notes 让人知道有东西没解析出来。
pub(crate) fn parse_verbose(out: &str) -> (Vec<ImageRow>, Vec<VolumeRow>, usize) {
    let mut images: Vec<ImageRow> = Vec::new();
    let mut volumes: Vec<VolumeRow> = Vec::new();
    let mut skipped = 0usize;
    let mut section = Section::None;

    for line in out.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("Images space usage") {
            section = Section::Images;
            continue;
        }
        if t.starts_with("Containers space usage") {
            section = Section::Containers;
            continue;
        }
        if t.starts_with("Local Volumes space usage") {
            section = Section::Volumes;
            continue;
        }
        if t.starts_with("Build cache usage") {
            section = Section::Done;
            continue;
        }
        // 列名行
        if t.starts_with("REPOSITORY")
            || t.starts_with("CONTAINER ID")
            || t.starts_with("VOLUME NAME")
            || t.starts_with("CACHE ID")
        {
            continue;
        }

        match section {
            Section::Images => match parse_image_line(t) {
                Some(r) => images.push(r),
                None => skipped += 1,
            },
            Section::Volumes => match parse_volume_line(t) {
                Some(r) => volumes.push(r),
                None => skipped += 1,
            },
            _ => {}
        }
    }

    (images, volumes, skipped)
}

/// `docker system df --format '{{json .}}'` 的四类汇总。
#[derive(Debug, serde::Deserialize)]
struct DfRow {
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "TotalCount")]
    total: String,
    #[serde(rename = "Size")]
    size: String,
}

fn parse_df_rows(out: &str) -> Vec<DfRow> {
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<DfRow>(l).ok())
        .collect()
}

/// 镜像创建时间。`df -v` 只给"11 days ago"这种相对描述，拿不到时间戳，
/// 而这一列要能排序、要能跟别的分组一样显示"多久之前"，所以补一次调用拿绝对时间。
fn image_created_at(bin: &Path, images: &[ImageRow]) -> HashMap<String, i64> {
    let mut map = HashMap::new();
    if images.is_empty() {
        return map;
    }
    let mut args: Vec<&str> = vec!["image", "inspect", "--format", "{{.Id}}|{{.Created}}"];
    args.extend(images.iter().map(|i| i.id.as_str()));
    let Ok(out) = run(bin, &args) else {
        return map;
    };
    for line in out.lines() {
        let Some((id, created)) = line.split_once('|') else {
            continue;
        };
        // inspect 给的是 sha256: 开头的完整 ID，明细表里是 12 位短 ID
        let short = id.strip_prefix("sha256:").unwrap_or(id);
        let short = if short.len() > 12 {
            &short[..12]
        } else {
            short
        };
        if let Some(ts) = parse_docker_time(created) {
            map.insert(short.to_string(), ts);
        }
    }
    map
}

/// 卷的创建时间与挂载点。挂载点是虚拟机内路径，本机访问不到，
/// 但放进 detail 能回答"这卷原来挂在哪"。
fn volume_info(bin: &Path, volumes: &[VolumeRow]) -> HashMap<String, (i64, String)> {
    let mut map = HashMap::new();
    if volumes.is_empty() {
        return map;
    }
    let mut args: Vec<&str> = vec![
        "volume",
        "inspect",
        "--format",
        "{{.Name}}|{{.CreatedAt}}|{{.Mountpoint}}",
    ];
    args.extend(volumes.iter().map(|v| v.name.as_str()));
    let Ok(out) = run(bin, &args) else {
        return map;
    };
    for line in out.lines() {
        let mut parts = line.splitn(3, '|');
        let (Some(name), Some(created), Some(mount)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if let Some(ts) = parse_docker_time(created) {
            map.insert(name.to_string(), (ts, mount.to_string()));
        }
    }
    map
}

impl Scanner for DockerScanner {
    fn id(&self) -> &'static str {
        "docker"
    }

    fn scan(&self) -> AssetGroup {
        // 这一项不依赖守护进程：没启动 Docker Desktop 也能看出它吃了多少磁盘
        let disk = self.desktop.as_deref().and_then(dir_size_fast);

        let Some(bin) = self.bin.as_deref() else {
            return self.unavailable(Text::key("data.error.dockerCliMissing"), disk);
        };

        let df = match run(bin, &["system", "df", "--format", "{{json .}}"]) {
            Ok(s) => s,
            Err(e) => {
                return self.unavailable(
                    Text::fmt("data.error.dockerQuery").arg("error", e.to_string()),
                    disk,
                );
            }
        };
        let rows = parse_df_rows(&df);
        let verbose = run(bin, &["system", "df", "-v"]).unwrap_or_default();
        let (mut images, mut volumes, skipped) = parse_verbose(&verbose);

        let created = image_created_at(bin, &images);
        let vol_info = volume_info(bin, &volumes);

        images.sort_by_key(|a| std::cmp::Reverse(a.size));
        volumes.sort_by_key(|a| std::cmp::Reverse(a.size));

        let mut items: Vec<Item> = Vec::new();

        for im in &images {
            // 悬空镜像的仓库和标签都是 <none>，显示成 `<none>:<none>` 没意义
            let label = match (im.repo.as_str(), im.tag.as_str()) {
                ("<none>", _) | (_, "<none>") => im.id.clone(),
                (r, t) => format!("{r}:{t}"),
            };
            let unused = im.containers == 0;

            let mut metrics: HashMap<String, MetricValue> = HashMap::new();
            metrics.insert("size".into(), MetricValue::Bytes(im.size));
            metrics.insert(
                "created".into(),
                match created.get(&im.id) {
                    Some(ts) => MetricValue::Int(*ts),
                    None => MetricValue::text("—"),
                },
            );
            metrics.insert("refs".into(), MetricValue::Int(im.containers as i64));

            let detail = vec![
                DetailRow::new("imageId", im.id.clone()),
                DetailRow::new("repo", im.repo.clone()),
                DetailRow::new("tag", im.tag.clone()),
                DetailRow::new(
                    "virtualSize",
                    Text::fmt("data.value.bytes").arg("bytes", TextArg::bytes(im.size)),
                ),
                DetailRow::new(
                    "sharedLayers",
                    Text::fmt("data.value.bytes").arg("bytes", TextArg::bytes(im.shared)),
                ),
                DetailRow::new(
                    "uniqueLayers",
                    Text::fmt("data.value.bytes").arg("bytes", TextArg::bytes(im.unique)),
                ),
            ];

            items.push(Item {
                id: format!("docker:image:{}", im.id),
                tags: if unused {
                    vec![Tag::new("unused")]
                } else {
                    vec![]
                },
                name: format!("镜像 · {label}"),
                label: None,
                short_label: None,
                group_label: None,
                kind_label: None,
                group_key: Some("镜像".into()),
                path: format!("docker://{label}"),
                metrics,
                detail,
                artifact_entries: vec![],
                // 回收量用独占层而不是虚拟大小：共享层删了这个镜像也还在别的镜像里，
                // 按虚拟大小报会重复计算，点一次清理发现没腾出那么多空间。
                //
                // 取 min 是因为 docker 这两列的有效位数不同（本机 redis:7.4-alpine
                // 报 SIZE 58.7MB、UNIQUE 58.71MB），会出现独占层略大于体积的情况。
                // 那是显示精度不是真实数据，但界面上「可回收」比「体积」大很怪。
                cleanable: unused,
                reclaimable: if unused {
                    Some(im.unique.min(im.size))
                } else {
                    None
                },
            });
        }

        for v in &volumes {
            let unused = v.links == 0;
            let (ts, mount) = match vol_info.get(&v.name) {
                Some((t, m)) => (Some(*t), m.clone()),
                None => (None, String::new()),
            };

            let mut metrics: HashMap<String, MetricValue> = HashMap::new();
            metrics.insert("size".into(), MetricValue::Bytes(v.size));
            metrics.insert(
                "created".into(),
                match ts {
                    Some(t) => MetricValue::Int(t),
                    None => MetricValue::text("—"),
                },
            );
            metrics.insert("refs".into(), MetricValue::Int(v.links as i64));

            let detail = vec![
                DetailRow::new("volumeName", v.name.clone()),
                DetailRow::new("mountPoint", mount),
                DetailRow::new(
                    "refCount",
                    Text::fmt("data.value.count").arg("n", TextArg::Num(v.links.into())),
                ),
            ];

            items.push(Item {
                id: format!("docker:volume:{}", v.name),
                tags: if unused {
                    vec![Tag::new("unused")]
                } else {
                    vec![]
                },
                name: format!("卷 · {}", v.name),
                label: None,
                short_label: None,
                group_label: None,
                kind_label: None,
                group_key: Some("卷".into()),
                path: format!("docker://volume/{}", v.name),
                metrics,
                detail,
                artifact_entries: vec![],
                cleanable: unused,
                reclaimable: if unused { Some(v.size) } else { None },
            });
        }

        if items.is_empty() {
            return self.unavailable(Text::key("data.error.dockerEmpty"), disk);
        }

        // 明细体积是虚拟大小（共享层重复计算），合计用 docker 自己报的总量：
        // 两个都摆出来，混用一个口径反而对不上。
        let reported_total: u64 = rows
            .iter()
            .filter(|r| r.kind == "Images" || r.kind == "Local Volumes")
            .filter_map(|r| parse_size(&r.size))
            .sum();
        let reclaimable: u64 = items.iter().filter_map(|i| i.reclaimable).sum();
        let unused_count = items.iter().filter(|i| i.cleanable).count();

        let mut notes: Vec<Text> = vec![
            Text::key("data.note.dockerInsideVm"),
            Text::key("data.note.dockerDecimalUnits"),
        ];
        if images.iter().any(|i| i.shared > 0) {
            notes.push(Text::key("data.note.dockerVirtualSize"));
        }
        let container_row = rows
            .iter()
            .find(|r| r.kind == "Containers" && r.total.parse::<u32>().is_ok_and(|n| n > 0));
        if let Some(c) = container_row {
            notes.push(
                Text::fmt("data.note.dockerContainers")
                    .arg("n", c.total.clone())
                    .arg("bytes", TextArg::bytes(parse_size(&c.size).unwrap_or(0))),
            );
        }
        let cache_bytes = rows
            .iter()
            .find(|r| r.kind == "Build Cache")
            .and_then(|r| parse_size(&r.size))
            .unwrap_or(0);
        if cache_bytes > 0 {
            notes.push(
                Text::fmt("data.note.dockerBuildCache").arg("bytes", TextArg::bytes(cache_bytes)),
            );
        }
        if let Some(d) = disk {
            notes.push(Text::fmt("data.note.dockerDesktopDisk").arg("bytes", TextArg::bytes(d)));
        }
        if skipped > 0 {
            notes.push(Text::fmt("data.note.dockerSkippedRows").arg("n", TextArg::num(skipped)));
        }

        let summary = Text::fmt("data.summary.docker")
            .arg("images", TextArg::num(images.len()))
            .arg("volumes", TextArg::num(volumes.len()))
            .arg("total", TextArg::bytes(reported_total))
            .arg("unused", TextArg::num(unused_count))
            .arg("reclaim", TextArg::bytes(reclaimable));

        AssetGroup {
            id: self.id().into(),
            label: self.label(),
            summary,
            fields: docker_fields(),
            items,
            notes: Some(notes),
            error: None,
            supports_clean: false,
        }
    }
}

/// 前端只渲染 `fields.slice(0, 3)`，第四个字段不会进表头也不会进列。
fn docker_fields() -> Vec<Field> {
    vec![
        Field {
            key: "size".into(),
            label: field_text("size"),
            numeric: Some(true),
            temporal: None,
        },
        Field {
            key: "created".into(),
            label: field_text("created"),
            numeric: Some(true),
            temporal: Some(true),
        },
        Field {
            key: "refs".into(),
            label: field_text("refs"),
            numeric: Some(true),
            temporal: None,
        },
    ]
}

// ---- 测试辅助：把纯函数暴露给测试模块 ----

#[cfg(test)]
pub(crate) fn parse_size_for_test(s: &str) -> Option<u64> {
    parse_size(s)
}

#[cfg(test)]
pub(crate) fn parse_time_for_test(s: &str) -> Option<i64> {
    parse_docker_time(s)
}

#[cfg(test)]
pub(crate) fn parse_image_line_for_test(s: &str) -> Option<ImageRow> {
    parse_image_line(s)
}

#[cfg(test)]
pub(crate) fn parse_volume_line_for_test(s: &str) -> Option<VolumeRow> {
    parse_volume_line(s)
}

#[cfg(test)]
pub(crate) fn parse_verbose_for_test(s: &str) -> (Vec<ImageRow>, Vec<VolumeRow>, usize) {
    parse_verbose(s)
}

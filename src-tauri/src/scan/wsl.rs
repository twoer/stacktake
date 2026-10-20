//! WSL 发行版的虚拟磁盘扫描（**只在 Windows 上注册**）。
//!
//! # 为什么单列一组
//!
//! Windows 上的后端开发者装几个发行版吃几十 GB 是常态，而这块盘此前完全不在账上。
//! 每个 WSL2 发行版在宿主上就是一个 `ext4.vhdx`，体积可以直接读文件元数据。
//!
//! # 位置从哪来：注册表，不是目录扫描
//!
//! `wsl --list -v` 只给 `NAME / STATE / VERSION`，**不含任何文件位置**。而且
//! vhdx 有三处可能落点：`%LOCALAPPDATA%\Packages\<包名>\LocalState`、
//! 新版 `wsl --install` 的 `%LOCALAPPDATA%\wsl\<GUID>`、以及 `wsl --import`
//! 用 `--location` 指到的任意盘（如 `D:\WSL\Ubuntu-22.04`）。目录扫描只能覆盖
//! 前两处，唯一能覆盖全部三种的是注册表：
//!
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss` 的每个 GUID 子键带
//! `DistributionName` / `BasePath` / `Version`，`ext4.vhdx` 就在 `BasePath` 下。
//!
//! # 口径：报体积，但**不标可回收**
//!
//! vhdx 是动态扩展、**只涨不缩**的：在发行版里删文件只在虚拟盘内部留空洞，
//! Windows 侧的文件尺寸纹丝不动。要把空间真正还给 Windows 得走三步：
//! 发行版内 `fstrim -av` → `wsl --shutdown` → 管理员 `Optimize-VHD -Mode Full`
//! （或 diskpart 的 `compact vdisk`）。**本工具一条都不做**——前两步要在 Linux 里
//! 执行，第三步要管理员权限且会重写整个文件。
//!
//! 于是 `reclaimable` 在这里没有诚实的填法：填 0 会让人以为「这里没什么可清的」，
//! 而这恰是最需要提醒的场景（WSL 用户最常见的困惑就是「Linux 里明明删干净了，
//! C 盘怎么还是满」）；填文件尺寸则等于报一个用户无论如何都拿不到的数字。
//!
//! 所以：**体积照实报、`cleanable` 为 false、`reclaimable` 为 None**，把机制与
//! 压缩步骤写进 `notes` 与每行的 detail。分组价值因此从「能清多少」转为
//! 「这东西怎么才能缩」——后者本身就是一个可执行的答案。
//!
//! # 可测性
//!
//! 平台是**值**不是 `#[cfg]`（硬约束 9）：本机是 macOS，`cfg` 一写 Windows 那半
//! 就永远没人验过。这里把「读注册表」做成可注入的参数（`wsl_scanners_with`），
//! 于是「Windows 平台下会不会注册」「解析出错怎么办」都能在本机断言；真正在
//! macOS 上做不到的只有一件事——**那条 `reg query` 命令在真机上跑不跑得通**。
//!
//! # 安全
//!
//! 只读注册表、只读 `ext4.vhdx` 的元数据，不进入发行版内部、不执行任何 wsl 命令、
//! 不读任何文件内容（同 `ai_tools.rs` 的姿态）。

use crate::scan::platform::Platform;
use crate::scan::repos::{dir_sizes_batch, human_bytes, mtime_secs};
use crate::scan::{AssetGroup, Field, Item, MetricValue, Scanner};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 每个 WSL2 发行版在宿主上的虚拟磁盘文件名。固定名，WSL 自己定的。
const VHDX_FILE: &str = "ext4.vhdx";

/// 注册表里 Lxss 键的完整路径。
const LXSS_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss";

/// 注册表里的一条发行版记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslDistro {
    pub name: String,
    /// 发行版目录（`ext4.vhdx` 在它下面）
    pub base_path: PathBuf,
    /// 1 或 2；0 = 注册表里没写（未识别）
    pub version: u32,
}

impl WslDistro {
    pub(crate) fn vhdx_path(&self) -> PathBuf {
        self.base_path.join(VHDX_FILE)
    }
}

// ============================================================================
// 解析（纯函数，喂样本就能测）
// ============================================================================

/// 解析 `reg query "…\Lxss" /s` 的输出。
///
/// 输出形状（同一份键族的两次出现，第一次是根键、第二次是发行版子键）：
///
/// ```text
/// HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Lxss
///     (Default)    REG_SZ    {00000000-0000-0000-0000-000000000000}
///
/// HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Lxss\{GUID}
///     State    REG_DWORD    0x1
///     DistributionName    REG_SZ    Ubuntu-22.04
///     Version    REG_DWORD    0x2
///     BasePath    REG_SZ    C:\Users\me\AppData\Local\Packages\…\LocalState
/// ```
///
/// 规则：
/// - 以 `HKEY_` 开头的行是**键**，缩进的是**值**（`名字  类型  值`，之间是空白）。
/// - 只有键名的最后一段是 `{GUID}` 的键才算发行版——根键的 `(Default)` 值不是。
/// - 值里可以有空格（BasePath 就是），所以按「前两个 token + 其余全是值」切。
/// - **没有 `BasePath` 的记录直接跳过**：定位不到 vhdx，报出来也没有意义。
pub(crate) fn parse_lxss_registry(text: &str) -> Vec<WslDistro> {
    let mut out = Vec::new();
    let mut cur: Option<HashMap<String, String>> = None;
    let mut cur_is_distro = false;

    let mut flush = |is_distro: bool, map: &mut Option<HashMap<String, String>>| {
        if is_distro {
            if let Some(d) = map.take().and_then(|v| distro_from_values(&v)) {
                out.push(d);
            }
        } else {
            map.take();
        }
    };

    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }

        if t.starts_with("HKEY_") {
            flush(cur_is_distro, &mut cur);
            cur = Some(HashMap::new());
            cur_is_distro = key_is_distro_key(t);
            continue;
        }

        if let Some(map) = cur.as_mut()
            && let Some((k, v)) = split_value_line(t)
        {
            map.insert(k.to_string(), v.to_string());
        }
    }
    flush(cur_is_distro, &mut cur);

    out
}

/// 键名的最后一段是 `{GUID}` → 这是一个发行版子键。
fn key_is_distro_key(key: &str) -> bool {
    key.rsplit('\\')
        .next()
        .is_some_and(|seg| seg.starts_with('{'))
}

/// 值行 → `(名字, 值)`。不是值行（第二段不以 `REG_` 开头）返回 `None`。
fn split_value_line(line: &str) -> Option<(&str, &str)> {
    let mut it = line.split_whitespace();
    let name = it.next()?;
    let ty = it.next()?;
    if !ty.starts_with("REG_") {
        return None;
    }
    // 值里可以有空格，按原串切：跳过「名字 + 类型」之后剩下的（trim 掉分隔空白）
    let rest = line[name.len()..].trim_start();
    let value = rest[ty.len()..].trim_start();
    Some((name, value))
}

/// 一条记录的值 → `WslDistro`。缺 `BasePath` 就返回 `None`（定位不到 vhdx）。
fn distro_from_values(v: &HashMap<String, String>) -> Option<WslDistro> {
    let base = v.get("BasePath").filter(|s| !s.is_empty())?;
    let name = v
        .get("DistributionName")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| "未命名发行版".to_string());
    Some(WslDistro {
        name,
        base_path: PathBuf::from(base),
        version: v.get("Version").and_then(|s| parse_dword(s)).unwrap_or(0),
    })
}

/// `REG_DWORD` 的值形如 `0x2`，也可能是不带前缀的十进制。
fn parse_dword(s: &str) -> Option<u32> {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

// ============================================================================
// 读取（Windows 专有；上层先判平台，非 Windows 不会走到这里）
// ============================================================================

/// `reg.exe` 的位置。System32 几乎总在系统 PATH 里，但双击启动的进程拿到的 PATH
/// 未必包含它，所以先给绝对路径、再退回裸命令名——同 `docker.rs::find_docker` 的惯例。
fn reg_exe() -> PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    let abs = Path::new(&root).join("System32").join("reg.exe");
    if abs.is_file() {
        abs
    } else {
        PathBuf::from("reg")
    }
}

/// 调一次 `reg query` 读出整个 Lxss 键（`/s` 递归到各发行版子键）。
fn read_lxss_registry() -> Result<String, String> {
    let out = Command::new(reg_exe())
        .args(["query", LXSS_KEY, "/s"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("无法调用 reg query：{e}"))?;

    if !out.status.success() {
        return Err(format!("reg query 退出码非零：{}", out.status));
    }

    // 用户名含中文时 reg 会以代码页（GBK）输出，字节不是合法 UTF-8。用 lossy 解码
    // 保住结构：解不出的 BasePath 会探不到 vhdx（该行体积缺失），不会报成错数——
    // 与「推错路径的后果是漏显示而非错数」同一个取舍。
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ============================================================================
// Scanner
// ============================================================================

pub struct WslScanner {
    distros: Vec<WslDistro>,
}

impl WslScanner {
    pub(crate) fn new(distros: Vec<WslDistro>) -> Self {
        Self { distros }
    }
}

impl Scanner for WslScanner {
    fn id(&self) -> &'static str {
        "wsl"
    }
    fn label(&self) -> &'static str {
        "WSL 发行版"
    }

    fn scan(&self) -> AssetGroup {
        let paths: Vec<PathBuf> = self.distros.iter().map(|d| d.vhdx_path()).collect();
        let sizes = dir_sizes_batch(&paths);

        let mut items: Vec<Item> = self
            .distros
            .iter()
            .map(|d| {
                let vhdx = d.vhdx_path();
                // 查不到 = 文件不存在（WSL1 没有 vhdx）或超时，一律当作「体积未知」，
                // 不能报 0——那是「不占空间」的意思，与事实相反。
                let size = sizes.get(&vhdx.to_string_lossy().to_string()).copied();
                distro_item(d, size)
            })
            .collect();

        // 体积降序，与其他体积型分组一致；体积未知的排最后
        items.sort_by_key(|i| match i.metrics.get("size") {
            Some(MetricValue::Bytes(b)) => std::cmp::Reverse(*b),
            _ => std::cmp::Reverse(0),
        });

        let total: u64 = items
            .iter()
            .filter_map(|i| match i.metrics.get("size") {
                Some(MetricValue::Bytes(b)) => Some(*b),
                _ => None,
            })
            .sum();

        AssetGroup {
            id: self.id().into(),
            label: self.label().into(),
            // 不写「可回收 0 B」：那读起来像「没什么可清的」。这里的关键信息是
            // 这些数字**够不着**，以及怎么才够得着。
            summary: format!(
                "{} 项 · 合计 {} · vhdx 只涨不缩，本组不计可回收",
                items.len(),
                human_bytes(total)
            ),
            fields: wsl_fields(),
            items,
            notes: Some(wsl_notes()),
            error: None,
            supports_clean: false,
        }
    }
}

fn distro_item(d: &WslDistro, size: Option<u64>) -> Item {
    let vhdx = d.vhdx_path();
    let version = match d.version {
        1 => "1".to_string(),
        2 => "2".to_string(),
        other => format!("未知（注册表值 {other}）"),
    };

    let mut metrics: HashMap<String, MetricValue> = HashMap::new();
    if let Some(s) = size {
        metrics.insert("size".into(), MetricValue::Bytes(s));
    }
    metrics.insert(
        "last_modified".into(),
        match mtime_secs(&vhdx) {
            Some(ts) => MetricValue::Int(ts),
            None => MetricValue::text("未知"),
        },
    );
    metrics.insert(
        "kind".into(),
        MetricValue::text(if size.is_some() {
            "虚拟磁盘"
        } else if d.version == 1 {
            "WSL1（无虚拟磁盘）"
        } else {
            "虚拟磁盘缺失"
        }),
    );

    // detail 的键名带编号前缀：HashMap 顺序不定，前端按编号剥前缀后渲染
    let mut detail: HashMap<String, String> = HashMap::new();
    detail.insert("0. 虚拟磁盘".into(), vhdx.display().to_string());
    detail.insert("1. 发行版目录".into(), d.base_path.display().to_string());
    detail.insert("2. WSL 版本".into(), version);
    detail.insert(
        "3. 体积口径".into(),
        "vhdx 文件的占用空间，不是发行版里实际用了多少：动态扩展、只涨不缩".into(),
    );
    detail.insert(
        "4. 怎么缩".into(),
        "发行版内 fstrim -av → wsl --shutdown → 管理员 Optimize-VHD -Mode Full".into(),
    );

    Item {
        id: format!("wsl:{}", d.name),
        tags: vec![], // 整组都不可回收，做成筛选标签等于没筛
        name: d.name.clone(),
        group_key: None,
        // path 给发行版目录：它一定存在，且 ext4.vhdx 就在里面。
        // 虚拟磁盘的完整路径在 detail 里（U4 的两个动作只读、不执行）。
        path: d.base_path.display().to_string(),
        metrics,
        detail: Some(detail),
        cleanable: false,
        reclaimable: None,
    }
}

/// 前端只渲染 `fields.slice(0, 3)`，这里正好三个。
fn wsl_fields() -> Vec<Field> {
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

fn wsl_notes() -> Vec<String> {
    vec![
        "体积是 ext4.vhdx 文件的占用空间，不是发行版里实际用了多少".into(),
        "动态扩展的 vhdx 只涨不缩：在发行版里删文件只在虚拟盘内部留空洞，Windows 侧文件尺寸纹丝不动——所以这里没法给出「删了能省多少」".into(),
        "要把空间还给 Windows 得三步：发行版内 sudo fstrim -av → wsl --shutdown → 管理员执行 Optimize-VHD -Mode Full（或 diskpart 的 compact vdisk）。本工具一条都不做".into(),
        "差多少属正常：实测有 60 GB 的 vhdx 里只有 12 GB 实际数据，另一例 Windows 显示 583 GB 而发行版内 df 只有 93 GB".into(),
        "位置读自注册表 Lxss 键（wsl --list -v 不给路径），不执行任何 wsl 命令，也不进入发行版内部".into(),
    ]
}

// ============================================================================
// 注册
// ============================================================================

/// 按当前平台组装 WSL 扫描器。**只在 Windows 上**：其他平台不存在 WSL，
/// 注册出来必然是空分组（硬约束 8「环境不存在就不注册」）。
pub fn wsl_scanners() -> Vec<Box<dyn Scanner>> {
    wsl_scanners_with(Platform::current(), read_lxss_registry)
}

/// 「读注册表」可注入的版本。**给测试用**——本机是 macOS，真的 `reg query`
/// 跑不起来，而「Windows 平台下会不会注册」「读到坏数据怎么办」这两条
/// 不该因此变成没人验过的分支（硬约束 9）。
pub(crate) fn wsl_scanners_with<F>(p: Platform, read: F) -> Vec<Box<dyn Scanner>>
where
    F: FnOnce() -> Result<String, String>,
{
    if !p.is_windows() {
        return Vec::new();
    }
    let Ok(text) = read() else {
        return Vec::new();
    };
    let distros = parse_lxss_registry(&text);
    if distros.is_empty() {
        return Vec::new();
    }
    vec![Box::new(WslScanner::new(distros))]
}

// ---- 测试辅助 ----

#[cfg(test)]
pub(crate) fn parse_lxss_registry_for_test(text: &str) -> Vec<WslDistro> {
    parse_lxss_registry(text)
}

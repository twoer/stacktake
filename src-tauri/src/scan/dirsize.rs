//! 目录体积统计：原生遍历，不再 shell out 到 `du`。
//!
//! 换掉 `du` 的理由很简单：Windows 没有 du，而体积统计是 repos / toolchain /
//! ai_tools / dev_env 四组共用的底座，不换就没有 Windows 版。
//!
//! ## 口径：占用空间，不是逻辑大小
//!
//! 这不是实现细节，是这个工具所有数字的含义。`du` 报的是**占用空间**，
//! 而删掉一个目录实际能回收的也正好是占用空间。APFS/HFS+ 上压缩、副本（clone）、
//! 稀疏文件都会让逻辑大小显著偏离实际占用——用逻辑大小会把「可回收」报虚，
//! 而这个工具的全部意义就是「删了能省多少」。
//!
//! - Unix：`st_blocks * 512`，与 `du` 同源，同一份数据两把尺子量出来一致。
//! - Windows：没有等价字段，退回逻辑大小（`file_size`）。**已知限制**——
//!   要精确需 `GetCompressedFileSizeW`，等 W4 的 Windows CI 就位后补。
//!
//! ## 超时
//!
//! 与 `du` 时代不同，这里的超时是**真的**：每一层递归都查截止时间，超了立刻
//! 带 `Expired` 返回，不会把活干完再判定。调用方拿到 `None` 表示「体积未知」，
//! 界面显示 `—`。
//!
//! ## 与 `du` 的差异
//!
//! 换掉 `du` 时的实测（2026-10-12，`tests::bench_native_size_vs_du`）：耗时没退
//! 反进（pnpm store 2.6s 对 du 6.4s），数值只差硬链接一处。当时**故意不去重**
//! （跨 rayon 线程共享 inode 集合要引入锁），依据是六工具链目录实测偏差仅 1.21%
//! ——这个前提在 D14 被推翻：cargo `target` 的硬链接密度极高（86k 条目 / 30k
//! inode），逐目录项累加比 du 高 **4.6 倍**，整个 repos 组账面虚高 1.45 倍。
//!
//! 现在 **Unix 上按 (dev, inode) 去重**，与 du 单次调用的语义一致。去重范围是
//! **单趟遍历**（对 repos 即单仓库 / 单条目）：跨条目共享的硬链接（如 pnpm store
//! 与项目 node_modules 共享同一批文件）各算各的——这是「按行记账」的口径，
//! 行加总因此可能大于全局唯一字节数，与 du 多参数调用不同，已记进 known-limits。
//! Windows 没有稳定的文件索引（`file_index` 未稳定），维持不去重：数字只虚不高，
//! 不会显示成错的。

use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 单个目录的统计上限。
///
/// 给得宽是有意的：超时返回 `None` = 这一项体积未知，界面上显示 `—`，
/// 等于没扫到。而实测最慢的单项（pnpm store，9.4 GB）原生遍历只要 2.6 秒
/// ——debug 构建的数字，release 更快。60 秒是安全网，不是常态约束。
/// 依据见 `tests::bench_native_size_vs_du`。
pub(crate) const SIZE_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(unix)]
fn allocated_bytes(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn allocated_bytes(meta: &fs::Metadata) -> u64 {
    meta.len()
}

/// 子目录数达到这个量才拆并行任务。小目录拆任务的开销比遍历本身还大。
const PAR_MIN_SUBDIRS: usize = 8;

/// 并行只在前几层展开。再深就是几十万个小任务，rayon 的调度成本会吃掉收益。
const PAR_MAX_DEPTH: usize = 4;

/// 走到一半超时用它区分「真的是 0 字节」和「没测出来」。
enum Step {
    Done(u64),
    Expired,
}

/// 调用方「顺带」收集感兴趣的子目录及其体积的篮子。
///
/// repos 扫描器用它在**同一次遍历**里把构建产物目录（node_modules / target…）
/// 单独摘出来计量——产物体积本来就包含在仓库总体积里，单独再走一遍树等于
/// 把最贵的活干两遍。被摘出来的子树内部不再观察（`noting: None` 递归）：
/// 产物内部的嵌套产物（pnpm 的 `.pnpm` 之类）已经在整棵子树的体积里，不再单独立账。
pub(crate) struct Noted {
    pub items: Mutex<Vec<(PathBuf, u64)>>,
}

/// 观察钩子：判据 + 收集篮。
pub(crate) type Noting<'a> = (&'a (dyn Fn(&Path) -> bool + Sync), &'a Noted);

/// 单趟遍历的 (dev, inode) 去重集。
///
/// Unix 上这是硬链接的口径修正（D14）；`nlink <= 1` 的条目不可能在本趟里
/// 再见，根本不进集合不加锁——付锁成本的只有多链接条目。Windows 没有稳定的
/// 文件索引（`MetadataExt::file_index` 未稳定），类型退化为 `()`，行为与
/// W1 时代一致：逐条目累加，数字只虚不高。
#[cfg(unix)]
type InodeSet = Mutex<HashSet<(u64, u64)>>;
#[cfg(not(unix))]
type InodeSet = ();

struct Walk<'a> {
    exclude: &'a HashSet<PathBuf>,
    deadline: Option<Instant>,
    noting: Option<Noting<'a>>,
    /// 产物子树的 quiet 递归与主干道**共享**同一把集合：硬链接跨两边时
    /// （依赖在 target 里、仓库根也留一份），只该计在先遍历到的那一处。
    seen: &'a InodeSet,
}

impl Walk<'_> {
    fn expired(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }

    /// 单个非目录条目：算它自己占的块。软链**不跟随**——跟随会顺着链接跑出
    /// 扫描根，体积也就算到别人头上（`du` 的默认行为同样不跟随）。
    /// 本趟已见过的多链接条目（硬链接）计 0——同 (dev, inode) 只计一次。
    fn leaf(&self, entry: &fs::DirEntry) -> u64 {
        let Ok(meta) = entry.metadata() else { return 0 };
        if !self.first_visit(&meta) {
            return 0;
        }
        allocated_bytes(&meta)
    }

    /// 本趟遍历是否第一次见到这个条目。
    #[cfg(unix)]
    fn first_visit(&self, meta: &fs::Metadata) -> bool {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() <= 1 {
            return true;
        }
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((meta.dev(), meta.ino()))
    }

    #[cfg(not(unix))]
    fn first_visit(&self, _meta: &fs::Metadata) -> bool {
        true
    }

    fn dir(&self, path: &Path, depth: usize) -> Step {
        if self.expired() {
            return Step::Expired;
        }

        // 目录自身的 inode 也占块，`du` 是计的，这里跟着计
        let mut total = fs::symlink_metadata(path)
            .map(|m| allocated_bytes(&m))
            .unwrap_or(0);

        let read = match fs::read_dir(path) {
            Ok(r) => r,
            // 权限不足 / 目录被删——按 0 计并继续，`du` 同样是跳过不报错
            Err(_) => return Step::Done(total),
        };

        let mut subdirs: Vec<PathBuf> = Vec::new();
        for entry in read.flatten() {
            let p = entry.path();
            if self.exclude.contains(&p) {
                continue;
            }
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if let Some((pred, noted)) = self.noting
                    && pred(&p)
                {
                    // 值得单独计量的子树：直接递归出体积，总体积照常累加，
                    // 但不再深入观察。超时与主干道同语义——整趟作废。
                    let quiet = Walk {
                        exclude: self.exclude,
                        deadline: self.deadline,
                        noting: None,
                        seen: self.seen,
                    };
                    match quiet.dir(&p, depth + 1) {
                        Step::Done(n) => {
                            total += n;
                            noted.items.lock().unwrap().push((p, n));
                        }
                        Step::Expired => return Step::Expired,
                    }
                    continue;
                }
                subdirs.push(p);
            } else {
                total += self.leaf(&entry);
            }
        }

        let step = |total: &mut u64, s: Step| -> bool {
            match s {
                Step::Done(n) => {
                    *total += n;
                    true
                }
                Step::Expired => false,
            }
        };

        if subdirs.len() >= PAR_MIN_SUBDIRS && depth < PAR_MAX_DEPTH {
            let parts: Vec<Step> = subdirs.par_iter().map(|d| self.dir(d, depth + 1)).collect();
            for s in parts {
                if !step(&mut total, s) {
                    return Step::Expired;
                }
            }
        } else {
            for d in &subdirs {
                let s = self.dir(d, depth + 1);
                if !step(&mut total, s) {
                    return Step::Expired;
                }
            }
        }

        Step::Done(total)
    }
}

/// 目录的占用空间（字节）。
///
/// - 路径不存在 → `None`
/// - 超过 [`SIZE_TIMEOUT`] → `None`（体积未知，界面显示 `—`）
/// - `exclude` 里的路径整棵子树跳过（用于嵌套仓库）
pub(crate) fn dir_size(path: &Path, exclude: &HashSet<PathBuf>) -> Option<u64> {
    dir_size_impl(path, exclude, None)
}

/// 同 [`dir_size`]，`pred` 挑中的子目录单独计量并记入 `noted`。
///
/// `pred` 在遍历中对**每个**子目录调用一次，要便宜——只做名字比对这类
/// 零 stat 判断；标记文件、祖先检查这类有代价的判断留给 pred 内部在
/// 名字命中时才做。`noted.items` 的顺序是遍历序（不稳定），调用方要
/// 确定性就自己排序。
pub(crate) fn dir_size_noting(
    path: &Path,
    exclude: &HashSet<PathBuf>,
    pred: &(dyn Fn(&Path) -> bool + Sync),
    noted: &Noted,
) -> Option<u64> {
    dir_size_impl(path, exclude, Some((pred, noted)))
}

/// Windows 上 `InodeSet` 就是 `()`，这里的 `let seen` 绑的是 unit 值——刻意的：
/// 两个平台的调用处写法必须一致，否则 `Walk { seen: ... }` 要跟着分叉。
/// clippy 的 `let_unit_value` 不懂这个意图，在 `-D warnings` 下直接判红
/// （CI 的 Windows job 首次跑到 clippy 就挂在这一行，macOS 永不触发）。
/// **别改成零大小具名 struct 来绕开它**——那样 `InodeSet::default()` 会撞上
/// `default_constructed_unit_structs`，红灯只是换了个名字。
#[allow(clippy::let_unit_value)]
fn dir_size_impl(
    path: &Path,
    exclude: &HashSet<PathBuf>,
    noting: Option<Noting<'_>>,
) -> Option<u64> {
    if !path.exists() {
        return None;
    }
    let seen = InodeSet::default();
    let walk = Walk {
        exclude,
        deadline: Some(Instant::now() + SIZE_TIMEOUT),
        noting,
        seen: &seen,
    };
    match walk.dir(path, 0) {
        Step::Done(n) => Some(n),
        Step::Expired => None,
    }
}

/// 一批目录的占用空间，key 就是传入的路径字符串。
///
/// 不做任何路径规范化：调用方拿原路径来查，命中率 100%。这是换掉 `du` 后
/// 顺带去掉的一类隐患——du 时代它回显的路径可能与传入的不一致（编码差异、
/// 重复路径去重），所以才有 `lookup_size` 那个「查不到就再跑一次」的兜底。
pub(crate) fn dir_sizes_batch(dirs: &[PathBuf]) -> HashMap<String, u64> {
    let empty = HashSet::new();
    dirs.par_iter()
        .filter_map(|d| dir_size(d, &empty).map(|n| (d.to_string_lossy().to_string(), n)))
        .collect()
}

/// 单个文件的占用空间（字节）。与 [`dir_size`] 同一口径（Unix `st_blocks * 512`）。
///
/// 给「体积不在自己目录里」的条目用：Ollama 的权重是全部模型共享的 blob 池，
/// 一个模型占多少只能按它 manifest 引用的 blob 逐个文件累加（G2）。
/// 读不到（文件不存在 / 权限）按 0 计——调用方自己知道该文件理应存在，
/// 「缺失」由它去报，这里不替它猜。
pub(crate) fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| allocated_bytes(&m)).unwrap_or(0)
}

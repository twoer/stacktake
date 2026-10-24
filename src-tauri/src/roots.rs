//! Git 仓库扫描范围：默认位置只读，用户通过原生目录选择器补充位置。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const ROOTS_FILE: &str = "repo-roots.json";

#[derive(Serialize)]
pub struct RepoRoot {
    pub path: PathBuf,
    pub available: bool,
}

#[derive(Serialize)]
pub struct RepoRoots {
    pub default_roots: Vec<PathBuf>,
    pub extra_roots: Vec<RepoRoot>,
}

#[derive(Serialize, Deserialize)]
struct Config {
    extra_roots: Vec<PathBuf>,
}

pub fn default_roots() -> Vec<PathBuf> {
    dirs::home_dir()
        .map(|home| default_roots_in(&home))
        .unwrap_or_default()
}

pub fn default_roots_in(home: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for candidate in [
        "Documents/Code",
        "Projects",
        "Developer",
        "code",
        "src",
        "workspace",
    ] {
        let p = home.join(candidate);
        if p.is_dir() {
            roots.push(p);
        }
    }
    if roots.is_empty() {
        roots.push(home.join("Documents"));
    }
    roots
}

pub fn load_extra(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let raw = match std::fs::read(dir.join(ROOTS_FILE)) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let config: Config = serde_json::from_slice(&raw)?;
    if config.extra_roots.iter().any(|p| !p.is_absolute()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "补充目录必须是绝对路径",
        ));
    }
    Ok(config.extra_roots)
}

pub fn save_extra(dir: &Path, extra_roots: Vec<PathBuf>) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let target = dir.join(ROOTS_FILE);
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&Config { extra_roots })?)?;
    std::fs::rename(tmp, target)
}

pub fn merged_roots(defaults: Vec<PathBuf>, extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for path in defaults.into_iter().chain(extra.iter().cloned()) {
        let path = dunce::canonicalize(&path).unwrap_or(path);
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
    roots
}

pub struct ScanRoots {
    pub paths: Vec<PathBuf>,
    pub extra: Vec<PathBuf>,
}

pub fn for_scan(dir: &Path) -> Result<ScanRoots, String> {
    let extra = load_extra(dir).map_err(|e| format!("无法读取 Git 仓库补充目录：{e}"))?;
    Ok(ScanRoots {
        paths: merged_roots(default_roots(), &extra),
        extra,
    })
}

pub fn view(dir: &Path) -> Result<RepoRoots, String> {
    let extra = load_extra(dir).map_err(|e| format!("无法读取 Git 仓库补充目录：{e}"))?;
    Ok(RepoRoots {
        default_roots: default_roots(),
        extra_roots: extra
            .into_iter()
            .map(|path| RepoRoot {
                available: path.is_dir(),
                path,
            })
            .collect(),
    })
}

/// 候选目录与某个已有根目录的位置关系。
#[derive(Debug, PartialEq, Eq)]
pub enum Overlap {
    /// 是同一个目录。
    Same,
    /// 候选在某个已有根**之下**，已被它覆盖。
    Inside(PathBuf),
    /// 候选**包含**某个已有根，加上它等于把那一项变冗余。
    Contains(PathBuf),
}

/// 候选目录与已有根目录重不重叠。**纯函数**：只看路径、不碰磁盘，喂样本即可测。
///
/// 只判「同一处或祖先关系」而不判任意相交：两个根目录只能是完全分开的两棵树，
/// 相交却不互为祖先的情况（软链、挂载点）在文件系统上不是这里该管的事。
///
/// `case_insensitive` 只在 Windows 上传 `true`：那里 `D:\Code` 与 `d:\code` 是同一处，
/// 逐字节比会漏。Unix 上大小写是两个不同目录，按原样比。参数显式传入而不是直接读
/// `cfg!`，是为了让这条规则在两种平台上都能被测到（硬约束 9 的同款规矩）。
pub fn overlap_of(
    existing: &[PathBuf],
    candidate: &Path,
    case_insensitive: bool,
) -> Option<Overlap> {
    let norm = |p: &Path| {
        if case_insensitive {
            PathBuf::from(p.to_string_lossy().to_lowercase())
        } else {
            p.to_path_buf()
        }
    };
    let candidate_norm = norm(candidate);
    for root in existing {
        let root_norm = norm(root);
        if root_norm == candidate_norm {
            return Some(Overlap::Same);
        }
        if candidate_norm.starts_with(&root_norm) {
            return Some(Overlap::Inside(root.clone()));
        }
        if root_norm.starts_with(&candidate_norm) {
            return Some(Overlap::Contains(root.clone()));
        }
    }
    None
}

/// 默认目录只读：跟它重叠没有「先移除那一项」这条路可走，提示必须点明这一点，
/// 否则用户会去找一个不存在的移除按钮。
fn default_root_conflict(overlap: Overlap) -> String {
    match overlap {
        Overlap::Same => "该目录已是默认扫描目录".into(),
        Overlap::Inside(root) => format!("默认扫描目录 {} 已包含它，不必再补充", root.display()),
        Overlap::Contains(root) => {
            format!(
                "它会覆盖默认扫描目录 {}，而默认目录不可移除",
                root.display()
            )
        }
    }
}

fn extra_root_conflict(overlap: Overlap) -> String {
    match overlap {
        Overlap::Same => "该目录已补充过".into(),
        Overlap::Inside(root) => format!("已补充的 {} 已包含它", root.display()),
        Overlap::Contains(root) => {
            format!("它会覆盖已补充的 {}，先移除那一项再添加", root.display())
        }
    }
}

pub fn add(dir: &Path, path: &Path) -> Result<(), String> {
    if !path.is_absolute() || !path.is_dir() {
        return Err("请选择一个存在的绝对目录".into());
    }
    let path = dunce::canonicalize(path).map_err(|e| format!("无法访问所选目录：{e}"))?;
    let mut extra = load_extra(dir).map_err(|e| format!("无法读取 Git 仓库补充目录：{e}"))?;
    let case_insensitive = cfg!(windows);
    // 重叠即拒绝，不静默剔除被覆盖的旧项：补充目录是用户显式选的位置，替他改列表
    // 等于让一次点击悄悄改写两处配置。重复范围还得把同一棵树扫两遍。
    if let Some(overlap) = overlap_of(&merged_roots(default_roots(), &[]), &path, case_insensitive)
    {
        return Err(default_root_conflict(overlap));
    }
    let extra_normalized: Vec<PathBuf> = extra
        .iter()
        .map(|p| dunce::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .collect();
    if let Some(overlap) = overlap_of(&extra_normalized, &path, case_insensitive) {
        return Err(extra_root_conflict(overlap));
    }
    extra.push(path);
    save_extra(dir, extra).map_err(|e| format!("无法保存 Git 仓库补充目录：{e}"))
}

pub fn remove(dir: &Path, path: &Path) -> Result<(), String> {
    let mut extra = load_extra(dir).map_err(|e| format!("无法读取 Git 仓库补充目录：{e}"))?;
    let before = extra.len();
    extra.retain(|p| p != path);
    if extra.len() == before {
        return Err("该目录不在补充列表中，默认目录不能移除".into());
    }
    save_extra(dir, extra).map_err(|e| format!("无法保存 Git 仓库补充目录：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "stacktake-roots-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        dunce::canonicalize(d).unwrap()
    }

    #[test]
    fn default_roots_in_filters_to_existing_and_falls_back() {
        let home = temp_dir("defaults-home");
        fs::create_dir_all(home.join("Documents/Code")).unwrap();
        fs::create_dir_all(home.join("src")).unwrap();
        fs::write(home.join("Projects"), b"not a directory").unwrap();
        assert_eq!(
            default_roots_in(&home),
            vec![home.join("Documents/Code"), home.join("src")]
        );
        let bare = temp_dir("defaults-bare");
        assert_eq!(default_roots_in(&bare), vec![bare.join("Documents")]);
    }

    #[test]
    fn extra_roots_persist_and_remove_without_deleting_files() {
        let dir = temp_dir("persist");
        let root = dir.join("code space 中文");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("keep.txt"), b"keep").unwrap();
        assert!(load_extra(&dir).unwrap().is_empty());
        add(&dir, &root).unwrap();
        assert_eq!(load_extra(&dir).unwrap(), vec![root.clone()]);
        assert!(for_scan(&dir).unwrap().paths.contains(&root));
        let shown = view(&dir).unwrap();
        assert!(shown.extra_roots[0].available);
        assert_eq!(shown.extra_roots[0].path, root);
        remove(&dir, &root).unwrap();
        assert!(load_extra(&dir).unwrap().is_empty());
        assert_eq!(fs::read(root.join("keep.txt")).unwrap(), b"keep");
    }

    #[test]
    fn extra_roots_reject_invalid_and_duplicate_choices() {
        let dir = temp_dir("validation");
        let root = dir.join("code");
        fs::create_dir(&root).unwrap();
        fs::write(dir.join("file"), b"file").unwrap();
        for invalid in [
            PathBuf::new(),
            PathBuf::from("relative"),
            dir.join("missing"),
            dir.join("file"),
        ] {
            assert!(add(&dir, &invalid).is_err());
        }
        add(&dir, &root).unwrap();
        assert!(add(&dir, &root.join(".")).is_err());
        assert!(remove(&dir, &dir.join("not-configured")).is_err());
        assert_eq!(load_extra(&dir).unwrap(), vec![root]);
    }

    #[test]
    fn unavailable_extra_roots_remain_removable() {
        let dir = temp_dir("unavailable");
        let root = dir.join("external");
        fs::create_dir(&root).unwrap();
        add(&dir, &root).unwrap();
        fs::remove_dir(&root).unwrap();
        let shown = view(&dir).unwrap();
        assert_eq!(shown.extra_roots.len(), 1);
        assert!(!shown.extra_roots[0].available);
        remove(&dir, &root).unwrap();
        assert!(load_extra(&dir).unwrap().is_empty());
    }

    #[test]
    fn extra_roots_corrupt_config_is_not_silently_overwritten() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join(ROOTS_FILE), b"invalid").unwrap();
        assert!(load_extra(&dir).is_err());
        assert!(add(&dir, &dir).is_err());
        assert_eq!(fs::read(dir.join(ROOTS_FILE)).unwrap(), b"invalid");
        fs::write(dir.join(ROOTS_FILE), br#"{"extra_roots":[""]}"#).unwrap();
        assert!(load_extra(&dir).is_err());
    }

    #[test]
    fn overlap_of_detects_same_child_and_parent_and_only_ignores_case_when_asked() {
        let root = PathBuf::from(if cfg!(windows) { "D:\\Code" } else { "/code" });
        let child = root.join("my");
        let other = PathBuf::from(if cfg!(windows) { "E:\\Code" } else { "/other" });
        // 单元素切片用 from_ref，别 clone 一个 PathBuf 只为了凑出 &[PathBuf]
        assert_eq!(
            overlap_of(std::slice::from_ref(&root), &root, false),
            Some(Overlap::Same)
        );
        assert_eq!(
            overlap_of(std::slice::from_ref(&root), &child, false),
            Some(Overlap::Inside(root.clone()))
        );
        assert_eq!(
            overlap_of(std::slice::from_ref(&child), &root, false),
            Some(Overlap::Contains(child.clone()))
        );
        assert_eq!(overlap_of(std::slice::from_ref(&other), &root, false), None);
        // 名字是前缀但不是祖先：/codebase 不是 /code 的子目录，逐字节前缀比会误判
        let sibling = PathBuf::from(if cfg!(windows) {
            "D:\\Codebase"
        } else {
            "/codebase"
        });
        assert_eq!(
            overlap_of(std::slice::from_ref(&sibling), &root, false),
            None
        );
        // 大小写只有 Windows 那边不问
        let upper = PathBuf::from(if cfg!(windows) {
            "D:\\CODE\\My"
        } else {
            "/CODE/My"
        });
        assert_eq!(
            overlap_of(std::slice::from_ref(&child), &upper, false),
            None
        );
        assert_eq!(
            overlap_of(std::slice::from_ref(&child), &upper, true),
            Some(Overlap::Same)
        );
    }

    #[test]
    fn extra_roots_reject_overlapping_directories() {
        let dir = temp_dir("overlap-child");
        let parent = dir.join("code");
        let child = parent.join("my");
        fs::create_dir_all(&child).unwrap();
        add(&dir, &parent).unwrap();
        let err = add(&dir, &child).unwrap_err();
        assert!(err.contains("已包含它"), "{err}");
        assert_eq!(load_extra(&dir).unwrap().len(), 1);

        let dir2 = temp_dir("overlap-parent");
        let parent2 = dir2.join("code");
        let child2 = parent2.join("my");
        fs::create_dir_all(&child2).unwrap();
        add(&dir2, &child2).unwrap();
        let err = add(&dir2, &parent2).unwrap_err();
        assert!(err.contains("先移除"), "{err}");
        assert_eq!(
            load_extra(&dir2).unwrap(),
            vec![dunce::canonicalize(&child2).unwrap()]
        );
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(dir2).unwrap();
    }

    #[test]
    fn merged_roots_deduplicate_normalized_paths() {
        let dir = temp_dir("merge");
        let child = dir.join("child");
        fs::create_dir(&child).unwrap();
        assert_eq!(
            merged_roots(vec![dir.clone()], &[dir.join("."), child.clone()]),
            vec![dir, child]
        );
    }
}

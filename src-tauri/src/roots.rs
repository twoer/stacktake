//! Git 仓库的扫描根目录。
//!
//! # 沿革（别照着旧文档找配置项，已经没了）
//!
//! 这里原先叫 `config.rs`，装的是「设置页增删扫描根目录」（C1 刀，D2 拍板选 A）：
//! 根目录列表落盘成 `scan-config.json`、三个 command 读写删、改根目录即废缓存。
//! 2026-10-19 由用户拍板整个删掉，本模块因此改名——只剩默认根目录这一件事，
//! 再叫 config 就是误导。
//!
//! 删掉的三条理由（也是将来要不要加回来的判据）：
//!
//! 1. **只对一组生效**：roots 的唯一消费者是 `RepoScanner`。其余五组
//!    （dev_env / toolchain / ai_tools / local_models / wsl）一律吃 `dirs::home_dir()`
//!    下的固定位置，跟这个配置无关。UI 标题写「扫描根目录」，实际只是「Git 仓库根目录」。
//! 2. **覆盖度没有被真正解决**：默认列表是六个**固定名字**的候选（见
//!    [`default_roots_in`]），漏掉的人得先知道自己该填什么路径才能补上；
//!    而设置页是手输文本框、没有目录选择器，等于把「扫哪些目录」变成了一个
//!    需要用户先有答案的问题。
//! 3. **代价可逆**：删掉只是回到「硬编码必然漏」的旧状态，不删数据、不改数据契约——
//!    但老缓存里存的是按旧范围扫出来的快照，所以 `cache::SCHEMA_VERSION` 跟着 +1
//!    （变更清单见 `cache.rs`）。
//!
//! 当初为什么做它，留在 `docs/evidence/C1.md` 与 `docs/archive/decisions/d02.md`。
//! **别因为功能删了就去改那两个文件**——它们记录的是「当时」，不是现状。
//!
//! # 留下的东西
//!
//! [`default_roots`] / [`default_roots_in`]：`~/Documents/Code`、`~/Projects` 等常见
//! 代码目录，**存在才收**；一个都不存在时兜底 `~/Documents`（不管它存不存在——
//! 空 roots 会让 `RepoScanner` 越界；它有守卫兜着，但守卫是兜底不是常态）。

use std::path::{Path, PathBuf};

/// 默认根目录：`~/Documents/Code` 等常见代码目录，**存在才收**。
///
/// 行为与从 `lib.rs` 搬来时逐字一致：一个都不存在时兜底
/// `~/Documents`（不管它是否存在——空 roots 会 panic，见 `RepoScanner` 的守卫）。
pub fn default_roots() -> Vec<PathBuf> {
    match dirs::home_dir() {
        Some(h) => default_roots_in(&h),
        None => Vec::new(),
    }
}

/// [`default_roots`] 的纯函数版：home 显式传入，测试喂假 home（AGENTS 硬约束 9
/// 的同一条规矩——断「某套布局」必须能钉住输入，不能跟着跑测试的机器走）。
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
        if p.exists() {
            roots.push(p);
        }
    }
    if roots.is_empty() {
        roots.push(home.join("Documents"));
    }
    roots
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
        d
    }

    #[test]
    fn default_roots_in_filters_to_existing_and_falls_back() {
        let home = temp_dir("defaults-home");
        fs::create_dir_all(home.join("Documents/Code")).unwrap();
        fs::create_dir_all(home.join("src")).unwrap();

        let roots = default_roots_in(&home);
        assert_eq!(
            roots,
            vec![home.join("Documents/Code"), home.join("src")],
            "只收存在的候选，按候选表顺序"
        );

        let bare = temp_dir("defaults-bare");
        assert_eq!(
            default_roots_in(&bare),
            vec![bare.join("Documents")],
            "一个都不存在时兜底 Documents——空 roots 会让 RepoScanner 越界"
        );
    }
}

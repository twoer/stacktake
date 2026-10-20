//! 平台：路径族解析的公共入口。
//!
//! ## 为什么做成值，而不是 `cfg!`
//!
//! 直觉写法是 `#[cfg(windows)]` 就地分叉。但那样一来**测试只能跑当前平台那一半**——
//! 本机是 macOS，Windows 的映射表就永远没人验过，和没写一样。
//! 把平台变成可以传进来的值，映射表就能在本机被断言（喂一个假 home + `Platform::Windows`），
//! 只有「真机上那个目录到底在不在」这一件事必须等实机。
//!
//! 所以这个模块的职责很窄：**给出跨平台的路径换算，并让它可以被测试**。

use std::path::{Path, PathBuf};

/// 运行平台。目前只区分两族：macOS/Linux 一类（`~/.cache`、`~/Library`、XDG），
/// Windows 一类（`%LOCALAPPDATA%` / `%APPDATA%`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    Mac,
    Windows,
}

impl Platform {
    pub(crate) fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Mac
        }
    }

    pub(crate) fn is_windows(self) -> bool {
        self == Self::Windows
    }
}

/// `%LOCALAPPDATA%` 相对用户主目录的位置。
///
/// 两者都在用户配置文件目录下（`C:\Users\<user>\AppData\Local`），所以现有的
/// 「相对 home 的路径」这套模型在 Windows 上照样成立，不需要引入第二套基准目录。
///
/// **下面三项逐项 `allow(dead_code)`，是判断不是偷懒**（H1）：路径解析现在走
/// `win_rel` 的字符串表，不经这三个；但留着有具体去处，不是猜的——
/// - `win_local("Programs")` 被 `tests.rs::ide_parents` 用来拼 Windows 用户级安装路径；
/// - `WIN_ROAMING`（`%APPDATA%`）是 G3「版本管理器」的 Windows 变体要用的基准
///   （`%APPDATA%\nvm`，见 `docs/backlog.md` 的 G3）。
///
/// 删了 G3 还得原样加回来，所以在有真实调用者之前先显式留着。
#[allow(dead_code)]
pub(crate) const WIN_LOCAL: &str = "AppData/Local";

/// `%APPDATA%` 相对用户主目录的位置。见上一条的说明。
#[allow(dead_code)]
pub(crate) const WIN_ROAMING: &str = "AppData/Roaming";

/// 拼一个 `%LOCALAPPDATA%\<rest>` 的 home 相对路径。
///
/// 用 `format!` 而不是字面量拼接，是为了让上表的常量成为唯一事实来源：
/// 哪天 Windows 换了布局，改这里一处。
#[allow(dead_code)]
pub(crate) fn win_local(rest: &str) -> String {
    format!("{WIN_LOCAL}/{rest}")
}

/// 在 `home` 下按平台解析一组「候选路径」，返回第一个存在的。
///
/// **多候选**是这个项目的既有惯例（见 `docker.rs::find_docker`）：第三方工具的
/// 安装位置本来就因安装器而异，写死一个路径会让一部分用户永远扫不到。
pub(crate) fn first_existing(home: &Path, candidates: &[&str]) -> Option<PathBuf> {
    candidates
        .iter()
        .map(|rel| home.join(rel))
        .find(|p| p.exists())
}

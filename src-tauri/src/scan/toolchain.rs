//! 工具链与包管理器缓存扫描。
//!
//! # 为什么单列一组
//!
//! 本机实测 35.9 GB 散落在三十来个目录里：pnpm store 9.4 GB、npm 5.5 GB、
//! uv 3.5 GB、Playwright 浏览器 3.3 GB、nvm 的 9 个 Node 版本合计 3.1 GB、
//! Homebrew 750 MB……不存在误删业务数据的路径。
//!
//! # 边界：什么算可回收（S3 起，两条而不是一条）
//!
//! - **可回收**：纯下载/依赖缓存**且没有官方清理命令**。删除的代价是下次使用时
//!   重新下载，功能无损。S3 起本组开放了清理入口（走废纸篓，与 repos 同构）。
//! - **不可回收（一）已安装产物**：Rustup 工具链、Cargo/Go 已装工具、pnpm 自带的
//!   Node。删了丢功能，留哪个版本是主动选择，与 JDK 同口径。
//! - **不可回收（二）有官方清理命令的缓存**：npm / pnpm / uv / pip / Homebrew /
//!   Go / Deno / NuGet / Composer / SwiftPM / CocoaPods 这 14 条。搬走整个目录也
//!   安全，但下次要完整重建（pnpm store 几个 GB），而官方命令只动真正多余的部分
//!   （`pnpm store prune` 只删没被引用的包）。**命令写在每行 detail 的「清理方式」里，
//!   人自己跑——本工具不代跑用户级命令**（D11 拍板：代跑要新增一类执行子进程的能力，
//!   不在 V1/V2 第一刀里开这个口子）。
//!
//! **为什么第二类反而不算可回收**：`cleanable` 这个字段的含义是「本工具点下去真能删掉」，
//! 不是「删了会不会丢功能」。报一个自己做不到的数字，比少报几个 GB 更糟——
//! 那会让账面与实际对不上，而用户没有任何提示能看出差在哪。
//! 代价是可回收口径收窄（见 [`cache_is_cleanable`]），这是**账面变小**，方向与 S2
//! 收紧 repos 判据时一致。
//!
//! **加了白名单条目之后**，先判断它属上面三类里的哪一类；若它有官方清理命令，
//! `toolchain_sources_with_official_cmd_are_not_cleanable` 那个计数断言会红——
//! 那是提醒你重新判一遍，不是让你把断言改松。
//!
//! # 与 Java 分组刻意不重叠
//!
//! `~/.m2` 与 `~/.gradle` 归 Java 分组，这里不重复统计，否则总量守恒会被双计。
//!
//! # 白名单而非 glob
//!
//! `~/Library/Caches` 下还住着 LarkShell、微信开发者工具、Espressif 等应用缓存，
//! 性质不同（异质、边界要逐个确认），只认下面列死的开发工具缓存目录。
//! 同 dev_env：目录不存在行就不出现，一个都没有时整个分组不进侧栏。
//!
//! # G4 补进来的语言缓存：本机一条都不装
//!
//! NuGet / Composer / Coursier / SwiftPM / CocoaPods / Deno 这七条**在本机都不存在**
//! （2026-10-13 勘测），所以它们的效果只能在别人的机器上看到，本机 dump 仍是 36 条。
//! 这不是实现没做完——它们的价值按定义就在「别的开发者装了而本机没装」的那些机器上。
//!
//! **代价是路径只能靠测试钉，不能靠真机数据验**：`windows_cache_locations_are_pinned`
//! 与 `g4_windows_layout_is_found_but_not_on_mac` 断言映射表本身，
//! `absent_language_caches_stay_off_the_ledger` 断言它们在本机一条都不出现。
//! 路径全部来自各家官方文档（已在 `note` 里标明依据），**但没有一条经过实机核对**。
//!
//! **G4 顺带修掉一处真 bug**：Deno 那条原先写 `rel: ".cache/deno"`，那是
//! **Linux** 的位置——本工具只出 macOS 与 Windows 两族，等于这条在两个平台上
//! 都探不到。本机没装 Deno 所以一直没暴露。现按官方文档改为 macOS
//! `~/Library/Caches/deno` / Windows `%LOCALAPPDATA%\deno`，并补上官方清理命令
//! `deno clean`（原先写「无官方清理命令」也是错的）。
//! **教训**：白名单里的路径若只在本机装了的工具上验过，另一平台写错是查不出来的；
//! 每加一条都要两侧都给位置（硬约束 9）。
//!
//! # 版本管理器（G3）：逐版本出行，不是一个目录一行
//!
//! 版本管理器与上面那类缓存源**结构不同**，所以单独一套：
//!
//! - `CacheSource` 是「一个白名单路径 = 一行」，因为缓存的价值在「这个目录多大、
//!   能不能清」。
//! - 版本管理器的价值恰恰在**多版本并存的清单**——`~/.nvm/versions/node` 下装了
//!   9 个 Node 版本，合成一行报「2.7 GB」就把唯一有用的信息埋掉了。台账要回答的
//!   是「装了哪几个版本、哪个最占地方」，所以**每个版本目录一行**。
//! - 全部**不可回收**：删一个版本是主动选择（可能还有项目在用它），与 Rustup 工具链
//!   同口径。`toolchain_installed_artifacts_are_not_cleanable` 守着这条。
//!
//! **本机实测的两个边界**（2026-10-13，macOS）：
//! 1. `~/.nvm/versions/node` 下 9 个版本从 v12.18.0 到 v22.19.0，其中 v22.19.0
//!    单个 2.0 GB——多版本并存的代价是**平时看不见的**（PATH 里只有一个生效）。
//! 2. `~/.nvm/.cache/bin` 是**下载下来的 tar.xz**（9 个版本一一对应，167 MB），
//!    装完就是残留、删了安全。它跟已装版本是两回事：前者可回收，后者不可。
//!    这个缓存不进版本清单（不是版本），按普通 `CacheSource` 单独成行。
//!
//! # 安全约束
//!
//! 同 `ai_tools.rs`：只用 `metadata()` 与 `read_dir`（文件名与元数据），不读任何文件内容。
//! `toolchain_source_never_reads_file_contents` 从源码层守着这条。

use crate::scan::platform::Platform;
use crate::scan::repos::{dir_sizes_batch, lookup_size, mtime_secs};
use crate::scan::{
    AssetGroup, CleanFailure, CleanPlan, CleanResult, Field, Item, MetricValue, Scanner, Tag, Text,
    TextArg, field_text, kind_text,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 一个版本管理器的静态描述。
///
/// 与 `CacheSource` 分开的原因见模块注释的「版本管理器」节：这里描述的是
/// **版本根目录**（其下每个子目录是一个已装版本），不是一个缓存目录。
struct VersionManager {
    /// 分节名。同一管理器的所有版本行共用一个小节，前端据此渲染小节与组合计。
    section: &'static str,
    /// 管理器名。`Item.name` 拼成 `section · <版本名>`，`group_key` 给 section。
    ///
    /// **行名是版本目录名而不是管理器名**——`v22.19.0` 这种名字在筛选和搜索里
    /// 比重复十遍「nvm」有用，管理器名由小节标题承担。
    _manager: &'static str,
    /// 版本根目录，相对 home，多个候选按顺序取第一个存在的。
    ///
    /// **给候选列表而不是单个路径**（既有惯例，见 `platform::first_existing`）：
    /// 版本管理器的安装位置因安装器而异，写死一个会让一部分用户永远扫不到。
    mac_rels: &'static [&'static str],
    win_rels: &'static [&'static str],
    /// 说明。进每行的 detail——版本管理器的清理方式是「卸掉不用的版本」，
    /// 各家命令不同，与其在这里猜，不如说明白让人自己判断。
    note: &'static str,
}

impl VersionManager {
    /// 该平台下的候选列表。
    fn rels(&self, p: Platform) -> &'static [&'static str] {
        if p.is_windows() {
            self.win_rels
        } else {
            self.mac_rels
        }
    }

    /// 找到这个管理器在本机的版本根目录。没装就没有——`None` 不是「空目录」。
    fn root(&self, home: &Path, p: Platform) -> Option<PathBuf> {
        self.rels(p)
            .iter()
            .map(|rel| home.join(rel))
            .find(|d| d.is_dir())
    }
}

/// 版本管理器白名单。
///
/// **只列「一个根目录下有多个版本目录」的管理器**。以下两种情况刻意不在这里：
/// - 版本数恒为 1 的（Homebrew 装的 Python、rustup 的 stable）——那不是「并存」，
///   已经在 Rust / Homebrew 那两个分节里以「已安装」口径报过了。
/// - 用户目录不是「版本目录集合」的（`~/.nodenv` 之类）：判据是子目录本身就是
///   一个可运行的版本，不做启发式（看目录里有没有 `bin/`），那种猜法在布局变动时
///   会静默给出错误的版本清单。
fn version_managers() -> Vec<VersionManager> {
    vec![
        VersionManager {
            section: "nvm",
            _manager: "nvm",
            // nvm 官方布局：已装版本在 versions/node/<版本>/ ✔
            mac_rels: &[".nvm/versions/node"],
            // Windows 上的 nvm 是另一个实现（coreybutler/nvm-windows），
            // NVM_HOME 默认 %APPDATA%\nvm，版本目录直接在它下面 ✔
            win_rels: &["AppData/Roaming/nvm"],
            note: "逐个版本目录。用 `nvm uninstall <版本>` 卸——直接删目录会让 nvm 的版本列表和 alias 残留",
        },
        VersionManager {
            section: "fnm",
            _manager: "fnm",
            // FNM_DIR 未设时新版默认 ~/.local/share/fnm（XDG），旧版 ~/.fnm。
            // 两个都列，装哪个版本都能扫到。
            mac_rels: &[".local/share/fnm/node-versions", ".fnm/node-versions"],
            // fnm 官方文档：Windows 默认 %LOCALAPPDATA%\fnm ✔
            win_rels: &["AppData/Local/fnm/node-versions"],
            note: "逐个版本目录。用 `fnm uninstall <版本>` 卸",
        },
        VersionManager {
            section: "volta",
            _manager: "volta",
            // volta 按语言分层：tools/image/node/<版本>/。语言是已知的一层，
            // 所以这里给「语言 + 通配」的候选，实际扫描按一层子目录展开。
            mac_rels: &[
                ".volta/tools/image/node",
                ".volta/tools/image/python",
                ".volta/tools/image/ruby",
            ],
            win_rels: &[
                "AppData/Local/volta/tools/image/node",
                "AppData/Local/volta/tools/image/python",
                "AppData/Local/volta/tools/image/ruby",
            ],
            note: "逐个版本目录。用 `volta uninstall node@<版本>` 卸",
        },
        VersionManager {
            section: "pyenv",
            _manager: "pyenv",
            mac_rels: &[".pyenv/versions"],
            // pyenv-win（Windows 的实现）同样落在 ~/.pyenv ✔
            win_rels: &[".pyenv/versions"],
            note: "逐个版本目录。用 `pyenv uninstall <版本>` 卸",
        },
        VersionManager {
            section: "conda",
            _manager: "conda",
            // conda 的「版本」是环境（env），装在三个可能的位置：官方安装器默认
            // ~/miniconda3，Anaconda 发行版 ~/anaconda3，手动装 ~/conda。
            mac_rels: &["miniconda3/envs", "anaconda3/envs", "conda/envs"],
            win_rels: &["miniconda3/envs", "anaconda3/envs", "conda/envs"],
            note: "每个子目录是一个 conda 环境。用 `conda env remove -n <名字>` 卸",
        },
        VersionManager {
            section: "rbenv",
            _manager: "rbenv",
            mac_rels: &[".rbenv/versions"],
            win_rels: &[".rbenv/versions"],
            note: "逐个版本目录。用 `rbenv uninstall <版本>` 卸",
        },
        VersionManager {
            section: "asdf",
            _manager: "asdf",
            // asdf 按语言分层：installs/<语言>/<版本>/。这里给的是各语言的根，
            // 逐版本展开时把语言带上（行名 `python 3.12.1`），否则多个语言的
            // 同名版本会看不出是谁的。
            mac_rels: &[
                ".asdf/installs/nodejs",
                ".asdf/installs/python",
                ".asdf/installs/ruby",
                ".asdf/installs/golang",
            ],
            win_rels: &[
                ".asdf/installs/nodejs",
                ".asdf/installs/python",
                ".asdf/installs/ruby",
                ".asdf/installs/golang",
            ],
            note: "逐个版本目录。用 `asdf uninstall <语言> <版本>` 卸",
        },
    ]
}

/// 一类缓存的静态描述。路径在 `resolve` 时按 home 拼出，方便测试喂假 home。
struct CacheSource {
    /// 分节前缀。`Item.name` 拼成 `section · short`，前端据此渲染小节与组合计。
    section: &'static str,
    /// 行名（不含前缀）
    short: &'static str,
    /// 类别列
    kind: &'static str,
    /// 相对 home 的路径
    rel: &'static str,
    /// 这是不是「删了按需重下也不损失什么」的纯缓存。
    ///
    /// **不是所有纯缓存都标 `cleanable`**——还要看有没有官方清理命令，见
    /// [`cache_is_cleanable`]。
    cache: bool,
    /// 官方清理命令；没有留空
    cmd: &'static str,
    note: &'static str,
    /// **逐子目录成行**（P1）：true 时本源不产生整目录行，改为每个子目录一行
    /// （DerivedData 按项目、设备符号按 iOS 版本、运行时按版本）。
    ///
    /// 与 G3 版本清单同一条逻辑：合成一行只剩一个总数，「哪个项目最占」全看不见。
    /// 子行的 `cleanable` 继承 `cache_is_cleanable`；[`clean_verdict`] 同步放行
    /// 「可列出且可清理」源的子目录、拒整目录——**删除面对齐账面**（硬约束 4）。
    list_children: bool,
}

/// 这个缓存本工具有没有删除实现，因而能不能标 `cleanable`。
///
/// **S3 起的口径**（D11 拍板选 A）：`cleanable` 的含义从「删了按需重下」收窄成
/// **「本工具点下去能真的删掉它」**。两者不是一回事——
///
/// | 情况 | 条数（2026-10-13 实测） | 合计 | 怎么处置 |
/// |---|---|---|---|
/// | 纯缓存 + 无官方命令 | 17 | **10.32 GB** | 标 `cleanable`，走废纸篓（与 repos 同构） |
/// | 纯缓存 + 有官方命令 | 6 | 20.95 GB | **不标 `cleanable`**，detail 里指给用户命令 |
/// | 已安装产物（`KIND_INSTALLED`） | 13 | — | 永不标 `cleanable`（G3 立的口径） |
///
/// **为什么有官方命令反而不标**：`pnpm store prune` 只删没被引用的包，把整个 store
/// 目录搬进废纸篓虽然也安全，但下次要完整重建（几百 MB 到几个 GB 的重新下载），
/// 而官方命令只动真正多余的部分——**代跑它更精确，不代跑它就不能假装自己能做**。
/// 代价是那 20.95 GB 账面不再计入「可回收」：这是**账面变小**，方向与 S2 收紧
/// repos 判据时一致（报不满比报虚好）。命令不白给，它们照旧显示在每行的
/// detail「清理方式」里，人自己跑。
fn cache_is_cleanable(src: &CacheSource) -> bool {
    src.cache && src.cmd.is_empty()
}

/// 已安装产物的类别名。测试与汇总都按这个串判定「不可回收」。
const KIND_INSTALLED: &str = "已安装";

/// 分节标题的展示名。`section` 本身是筛选与 `group_key` 的稳定取值，
/// 这里只决定怎么显示——品牌名（nvm / Rust / .NET…）不翻译。
fn section_text(section: &str) -> Text {
    match section {
        "Node 生态" => Text::key("data.section.node"),
        "AI 运行时" => Text::key("data.section.aiRuntime"),
        "其它" => Text::key("data.section.other"),
        "模拟器" => Text::key("data.section.simulators"),
        _ => Text::raw(section),
    }
}

/// 行名后半截（`section · short`）。品牌与产品名保持原样，中文描述走
/// `data.item.*`；**新增白名单条目时在这里补一条**，漏了会回落成中文。
fn short_text(short: &str) -> Text {
    let key = match short {
        "nvm 下载缓存" => "nvmDownloadCache",
        "pnpm 内容存储" => "pnpmStore",
        "pnpm 的 Node" => "pnpmNode",
        "node-gyp 头文件" => "nodeGypHeaders",
        "Playwright 浏览器" => "playwrightBrowsers",
        "Puppeteer 浏览器" => "puppeteerBrowsers",
        "Cypress 二进制" => "cypressBinary",
        "Electron（XDG）" => "electronXdg",
        "TypeScript 版本" => "typescriptVersions",
        "Rustup 工具链" => "rustupToolchains",
        "Rustup 下载缓存" => "rustupDownloads",
        "Cargo 依赖缓存" => "cargoRegistry",
        "Cargo Git 依赖" => "cargoGitDeps",
        "Cargo 已装工具" => "cargoInstalledTools",
        "模块缓存" => "goModuleCache",
        "构建缓存" => "goBuildCache",
        "已装工具" => "goInstalledTools",
        "下载缓存" => "brewDownloads",
        "vcpkg 二进制缓存" => "vcpkgBinaryCache",
        "Prisma 引擎" => "prismaEngines",
        "NuGet 全局包" => "nugetGlobalPackages",
        "NuGet HTTP 缓存" => "nugetHttpCache",
        "Coursier（旧版）" => "coursierLegacy",
        "iOS 设备符号" => "iosDeviceSymbols",
        "模拟器运行时" => "simulatorRuntimes",
        _ => return Text::raw(short),
    };
    Text::key(format!("data.item.{key}"))
}

fn toolchain_fields() -> Vec<Field> {
    vec![
        Field {
            key: "size".into(),
            label: field_text("size"),
            numeric: Some(true),
            temporal: None,
        },
        Field {
            key: "last_modified".into(),
            label: field_text("lastModified"),
            numeric: Some(true),
            temporal: Some(true),
        },
        Field {
            key: "kind".into(),
            label: field_text("kind"),
            numeric: None,
            temporal: None,
        },
    ]
}

/// macOS 路径 → Windows 路径。
///
/// 只有**两边不同**的才需要出现在这里。dot-dir 类（`.cargo` / `.rustup` / `go` /
/// `.bun` 等）两个平台用的是同一个相对路径，走最后一个 arm 兜底。
/// 返回 `None` 表示这个平台没有这一项（Windows 上没有 Homebrew）。
///
/// **置信度**：标 ✔ 的来自工具官方文档或已核实的约定；其余是按
/// `%LOCALAPPDATA%` 下的惯例布局推出来的，**待 Windows 实机核对**。
/// 推错的后果是探不到 → 该行不出现（漏显示），不是显示成错的数——这个方向的
/// 错误可以接受，这也是「探不到就不出分组/不出行」这个既有设计的价值所在。
fn win_rel(rel: &'static str) -> Option<&'static str> {
    match rel {
        // npm 官方文档：Windows 的 cache 是 %LocalAppData%\npm-cache，**不是 ~/.npm** ✔
        ".npm" => Some("AppData/Local/npm-cache"),
        // pnpm v7+ 把 store 挪到 LOCALAPPDATA 下
        "Library/pnpm/store" => Some("AppData/Local/pnpm/store"),
        "Library/pnpm/nodejs" => Some("AppData/Local/pnpm/nodejs"),
        "Library/Caches/ms-playwright" => Some("AppData/Local/ms-playwright"), // ✔
        "Library/Caches/electron" => Some("AppData/Local/electron/Cache"),     // ✔
        "Library/Caches/electron-builder" => Some("AppData/Local/electron-builder/Cache"),
        "Library/Caches/Cypress" => Some("AppData/Local/Cypress/Cache"),
        "Library/Caches/pip" => Some("AppData/Local/pip/Cache"), // ✔
        "Library/Caches/Yarn" => Some("AppData/Local/Yarn/Cache"),
        "Library/Caches/node-gyp" => Some("AppData/Local/node-gyp/Cache"),
        "Library/Caches/typescript" => Some("AppData/Local/typescript"),
        ".cache/uv" => Some("AppData/Local/uv/cache"),
        // G4 修正：原先这里写 `.cache/deno` 配 mac 侧的 `.cache/deno`——那是
        // **Linux** 的位置，两个平台都探不到。Deno 官方文档：Windows 默认
        // %LOCALAPPDATA%\deno；macOS 默认 ~/Library/Caches/deno ✔
        "Library/Caches/deno" => Some("AppData/Local/deno"),
        // G4：NuGet http-cache。官方文档：Windows %localappdata%\NuGet\v3-cache ✔
        // （global-packages 那条是 ~/.nuget/packages，两平台一致，走兜底 arm）
        ".local/share/NuGet/v3-cache" => Some("AppData/Local/NuGet/v3-cache"),
        // G4：Composer cache-dir。官方 config 文档：Windows %LOCALAPPDATA%\Composer ✔
        "Library/Caches/composer" => Some("AppData/Local/Composer"),
        // G4：Coursier。官方文档：macOS ~/Library/Caches/Coursier/v1（**大写 C**）、
        // Windows %LOCALAPPDATA%\Coursier\Cache ✔
        "Library/Caches/Coursier" => Some("AppData/Local/Coursier/Cache"),
        // G4：SwiftPM。Windows 上 swift package 的缓存位置随工具链版本变化，
        // 这里给的是 Swift 官方工具链在 Windows 下的默认布局
        "Library/Caches/org.swift.swiftpm" => Some("AppData/Local/org.swift.swiftpm"),
        // CocoaPods 依赖 Xcode 的 Ruby 运行时，只存在于 macOS（Windows 上没有
        // 可用的 CocoaPods）。显式 None 而不是靠兜底 arm 漏出去。
        "Library/Caches/CocoaPods" => None,
        // GOCACHE 在 Windows 上是 %LOCALAPPDATA%\go-build ✔
        ".cache/go-build" => Some("AppData/Local/go-build"),
        // Xcode 与模拟器只在 macOS（P1）：四条全是 macOS 专有布局，显式 None——
        // 别落兜底 arm 去探一个 Windows 上必然不存在的路径
        "Library/Developer/Xcode/DerivedData" => None,
        "Library/Developer/Xcode/iOS DeviceSupport" => None,
        "Library/Developer/Xcode/Archives" => None,
        "Library/Developer/CoreSimulator/Profiles/Runtimes" => None,
        // Windows 没有 Homebrew
        "Library/Caches/Homebrew" => None,
        other => {
            // `Library/` 是 macOS 专有布局，任何一条都不能靠兜底漏到 Windows 去
            // ——那会静默地去探一个必然不存在的路径，而且没人会发现。
            debug_assert!(
                !other.starts_with("Library/"),
                "「{other}」是 macOS 专有布局，必须在 win_rel 里显式给出 Windows 位置（或显式 None）"
            );
            Some(other)
        }
    }
}

/// 按平台解析一个缓存源的 home 相对路径。`None` = 这个平台没有这一项。
fn resolve_rel(rel: &'static str, p: Platform) -> Option<&'static str> {
    if p.is_windows() {
        win_rel(rel)
    } else {
        Some(rel)
    }
}

fn sources() -> Vec<CacheSource> {
    vec![
        // ---- Node 生态 ----
        CacheSource {
            section: "Node 生态",
            short: "npm",
            kind: "下载缓存",
            rel: ".npm",
            cache: true,
            cmd: "npm cache clean --force",
            note: "该命令清 _cacache；_npx 等子目录可直接删除，下次使用重新下载",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "nvm 下载缓存",
            kind: "下载缓存",
            rel: ".nvm/.cache",
            cache: true,
            cmd: "",
            note: "nvm 下载的 node tar.xz 暂存，装完即是残留；已装版本在 versions/ 下，与这里无关",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "pnpm 内容存储",
            kind: "依赖缓存",
            rel: "Library/pnpm/store",
            cache: true,
            cmd: "pnpm store prune",
            note: "prune 只移除没有被任何项目引用的包；整个 store 删除也安全，按需重建",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "pnpm 的 Node",
            kind: KIND_INSTALLED,
            rel: "Library/pnpm/nodejs",
            cache: false,
            cmd: "",
            note: "pnpm 自行管理的 Node 二进制，删了依赖它的命令不可用",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "bun",
            kind: "下载缓存",
            rel: ".bun/install/cache",
            cache: true,
            cmd: "",
            note: "删除后按需重新下载",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "node-gyp 头文件",
            kind: "构建缓存",
            rel: "Library/Caches/node-gyp",
            cache: true,
            cmd: "",
            note: "各版本 Node 的头文件，编译原生插件时按版本下载",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "Playwright 浏览器",
            kind: "下载缓存",
            rel: "Library/Caches/ms-playwright",
            cache: true,
            cmd: "",
            note: "多版本并存是不同项目按需下载的结果，删除后对应项目下次运行重新下载",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "Puppeteer 浏览器",
            kind: "下载缓存",
            rel: ".cache/puppeteer",
            cache: true,
            cmd: "",
            note: "删除后按需重新下载",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "Cypress 二进制",
            kind: "下载缓存",
            rel: "Library/Caches/Cypress",
            cache: true,
            cmd: "",
            note: "删除后下次运行重新下载",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "Electron",
            kind: "下载缓存",
            rel: "Library/Caches/electron",
            cache: true,
            cmd: "",
            note: "打包用的 Electron zip，删除后重新下载",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "Electron（XDG）",
            kind: "下载缓存",
            rel: ".cache/electron",
            cache: true,
            cmd: "",
            note: "部分工具按 XDG 规范另存的一份，与 Library/Caches/electron 同源",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "Electron Builder",
            kind: "下载缓存",
            rel: "Library/Caches/electron-builder",
            cache: true,
            cmd: "",
            note: "dmg / nsis / wine 等打包辅助工具，删除后重新下载",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "TypeScript 版本",
            kind: "下载缓存",
            rel: "Library/Caches/typescript",
            cache: true,
            cmd: "",
            note: "编辑器按版本下载的 tsserver，删除后重新下载",
            list_children: false,
        },
        CacheSource {
            section: "Node 生态",
            short: "Yarn",
            kind: "下载缓存",
            rel: "Library/Caches/Yarn",
            cache: true,
            cmd: "yarn cache clean",
            note: "Yarn 1 的全局缓存目录",
            list_children: false,
        },
        // ---- Python ----
        CacheSource {
            section: "Python",
            short: "uv",
            kind: "依赖缓存",
            rel: ".cache/uv",
            cache: true,
            cmd: "uv cache clean",
            note: "下载的 wheel 与解包缓存",
            list_children: false,
        },
        CacheSource {
            section: "Python",
            short: "pip",
            kind: "下载缓存",
            rel: "Library/Caches/pip",
            cache: true,
            cmd: "pip cache purge",
            note: "HTTP 缓存与 wheel",
            list_children: false,
        },
        // ---- Rust ----
        CacheSource {
            section: "Rust",
            short: "Rustup 工具链",
            kind: KIND_INSTALLED,
            rel: ".rustup/toolchains",
            cache: false,
            cmd: "",
            note: "已安装的工具链版本，装几个是主动选择，同 JDK 口径",
            list_children: false,
        },
        CacheSource {
            section: "Rust",
            short: "Rustup 下载缓存",
            kind: "下载缓存",
            rel: ".rustup/downloads",
            cache: true,
            cmd: "",
            note: "安装过程的暂存文件，装完即是残留",
            list_children: false,
        },
        CacheSource {
            section: "Rust",
            short: "Cargo 依赖缓存",
            kind: "依赖缓存",
            rel: ".cargo/registry",
            cache: true,
            cmd: "",
            note: "crates.io 索引与包缓存；无官方 prune 命令，可直接删除，下次构建重新下载",
            list_children: false,
        },
        CacheSource {
            section: "Rust",
            short: "Cargo Git 依赖",
            kind: "依赖缓存",
            rel: ".cargo/git",
            cache: true,
            cmd: "",
            note: "git 依赖的检出缓存，删除后重新克隆",
            list_children: false,
        },
        CacheSource {
            section: "Rust",
            short: "Cargo 已装工具",
            kind: KIND_INSTALLED,
            rel: ".cargo/bin",
            cache: false,
            cmd: "",
            note: "cargo install 装的二进制，删了命令就没了",
            list_children: false,
        },
        // ---- Go ----
        CacheSource {
            section: "Go",
            short: "模块缓存",
            kind: "依赖缓存",
            rel: "go/pkg/mod",
            cache: true,
            cmd: "go clean -modcache",
            note: "下载的模块源码",
            list_children: false,
        },
        CacheSource {
            section: "Go",
            short: "构建缓存",
            kind: "构建缓存",
            rel: ".cache/go-build",
            cache: true,
            cmd: "go clean -cache",
            note: "编译中间产物，删除后重新编译，耗时但不丢数据",
            list_children: false,
        },
        CacheSource {
            section: "Go",
            short: "已装工具",
            kind: KIND_INSTALLED,
            rel: "go/bin",
            cache: false,
            cmd: "",
            note: "go install 装的二进制，删了命令就没了",
            list_children: false,
        },
        // ---- Homebrew ----
        CacheSource {
            section: "Homebrew",
            short: "下载缓存",
            kind: "下载缓存",
            rel: "Library/Caches/Homebrew",
            cache: true,
            cmd: "brew cleanup --prune=all",
            note: "bottle 与 API 缓存；Backup 子目录是旧版本备份，cleanup 会一并处理",
            list_children: false,
        },
        // ---- AI 运行时 ----
        CacheSource {
            section: "AI 运行时",
            short: "Codex",
            kind: "下载缓存",
            rel: ".cache/codex-runtimes",
            cache: true,
            cmd: "",
            note: "按版本下载的运行时，删除后重新下载",
            list_children: false,
        },
        CacheSource {
            section: "AI 运行时",
            short: "acode",
            kind: "下载缓存",
            rel: ".cache/acode-runtimes",
            cache: true,
            cmd: "",
            note: "按版本下载的运行时，删除后重新下载",
            list_children: false,
        },
        // ---- 其它 ----
        CacheSource {
            section: "其它",
            short: "vcpkg 二进制缓存",
            kind: "构建缓存",
            rel: ".cache/vcpkg",
            cache: true,
            cmd: "",
            note: "删除后相关包重新编译，耗时但不丢数据",
            list_children: false,
        },
        CacheSource {
            section: "其它",
            short: "Prisma 引擎",
            kind: "下载缓存",
            rel: ".cache/prisma",
            cache: true,
            cmd: "",
            note: "下载的查询引擎，删除后重新下载",
            list_children: false,
        },
        CacheSource {
            section: "其它",
            short: "Deno",
            kind: "依赖缓存",
            // G4 修正：原先写 `.cache/deno`，那是 **Linux** 的位置。本工具只出 macOS
            // 与 Windows 两族，等于这条在两个平台上都探不到——只是本机没装 Deno，
            // 漏显示看不出来。Deno 官方文档给的默认位置（DENO_DIR 未设时）：
            // macOS `~/Library/Caches/deno`、Windows `%LOCALAPPDATA%\deno`、
            // Linux `$XDG_CACHE_HOME/deno`。走 `Library/` 前缀落到 `win_rel` 兜底
            // arm 会静默去探 `~/Library/Caches/deno`，所以必须显式给 Windows 位置。
            rel: "Library/Caches/deno",
            cache: true,
            // 官方文档：deno clean 清整个 DENO_DIR，里面全部可重新拉取或重新生成
            cmd: "deno clean",
            note: "远程模块与编译产物，按需重新下载。注意 DENO_DIR 里还有 kv / webstorage 这类应用数据，删之前先确认没在用",
            list_children: false,
        },
        // ---- G4：语言缓存补齐 ----
        // 全部是「纯下载缓存，删了重下」。这一组的价值在**跨语言覆盖**：
        // 本机一条都不装（见模块注释的覆盖说明），所以它们的效果在别的机器上验证。
        CacheSource {
            section: ".NET",
            short: "NuGet 全局包",
            kind: "依赖缓存",
            // 官方文档：global-packages 在 Windows 是 %userprofile%\.nuget\packages、
            // Mac/Linux 是 ~/.nuget/packages——**两边一致**，落兜底 arm ✔
            rel: ".nuget/packages",
            cache: true,
            cmd: "dotnet nuget locals global-packages --clear",
            note: "按包 id + 版本展开存放，所有项目共享。删掉后各项目重新 restore 下载；NUGET_PACKAGES 环境变量可改这个位置",
            list_children: false,
        },
        CacheSource {
            section: ".NET",
            short: "NuGet HTTP 缓存",
            kind: "下载缓存",
            // 官方文档：http-cache 在 Windows 是 %localappdata%\NuGet\v3-cache、
            // Mac/Linux 是 ~/.local/share/NuGet/v3-cache
            rel: ".local/share/NuGet/v3-cache",
            cache: true,
            cmd: "dotnet nuget locals http-cache --clear",
            note: "未展开的原始下载（.dat），有 30 分钟过期时间",
            list_children: false,
        },
        CacheSource {
            section: "PHP",
            short: "Composer",
            kind: "依赖缓存",
            // 官方 config 文档的 cache-dir 默认值：Windows %LOCALAPPDATA%\\Composer、
            // macOS ~/Library/Caches/composer ✔。$COMPOSER_HOME/cache 是
            // COMPOSER_HOME 被改过时的落点，那种情况认不出来。
            rel: "Library/Caches/composer",
            cache: true,
            cmd: "composer clear-cache",
            note: "下载的 zip（files）、仓库元信息（repo）、VCS 检出（vcs）。项目里的 vendor/ 不在这个目录下",
            list_children: false,
        },
        CacheSource {
            section: "JVM",
            short: "Coursier",
            kind: "依赖缓存",
            // 官方文档：macOS ~/Library/Caches/Coursier/v1（**大写 C**）、
            // Windows %LOCALAPPDATA%\\Coursier\\Cache\v1。老版本（1.0.0-RC12-1 之前，
            // 2017-10 之前用过）在 ~/.coursier/cache/v1，而**机器上第一个跑过的版本
            // 决定后面用哪个**——所以两个都要探。
            rel: "Library/Caches/Coursier",
            cache: true,
            cmd: "",
            note: "Scala / sbt 用的构件缓存，按 URL 组织。v1 子目录是版本号",
            list_children: false,
        },
        CacheSource {
            section: "JVM",
            short: "Coursier（旧版）",
            kind: "依赖缓存",
            rel: ".coursier/cache",
            cache: true,
            cmd: "",
            note: "2017 年 10 月之前的 coursier 默认位置。机器上第一个跑过的 coursier 版本决定实际用哪个，新旧两个目录都在说明曾经用过老版本",
            list_children: false,
        },
        CacheSource {
            section: "Swift",
            short: "SwiftPM",
            kind: "依赖缓存",
            // Xcode 与命令行 swift build 共用的缓存。**刻意不扫 ~/.swiftpm**——
            // 它的 cache 子目录是指向这里的软链（configuration / security 另有所指），
            // 扫两边会双计。`dir_size` 不跟随软链，所以扫本目录已足够。
            rel: "Library/Caches/org.swift.swiftpm",
            cache: true,
            cmd: "swift package purge-cache",
            note: "依赖检出与仓库缓存，重新解析会重新下载。Xcode 工程内的 SourcePackages 在 DerivedData 下，不在这里",
            list_children: false,
        },
        CacheSource {
            section: "Swift",
            short: "CocoaPods",
            kind: "依赖缓存",
            rel: "Library/Caches/CocoaPods",
            cache: true,
            cmd: "pod cache clean --all",
            note: "下载的 pod；重新 pod install 会再拉。**刻意不扫 ~/.cocoapods/repos**——那是 spec 仓库，pod repo update 才能恢复，代价与其它条目不同",
            list_children: false,
        },
        // ---- Xcode 系（P1）：Xcode 与模拟器只在 macOS。竞品依据：DevCleaner for
        // Xcode 与 mac-cleanup 都把构建缓存类当「删了安全（可再生）」的标准类目。
        // 归 toolchain（构建缓存性质）不进 dev_env 的 iOS 组（那边是「装没装」的探测语义）。
        CacheSource {
            section: "Xcode",
            short: "Xcode DerivedData",
            kind: "构建缓存",
            rel: "Library/Developer/Xcode/DerivedData",
            cache: true,
            cmd: "",
            note: "每个子目录是一个项目的构建索引与中间产物，删了下次构建重建；有进行中的构建时先等完成",
            list_children: true,
        },
        CacheSource {
            section: "Xcode",
            short: "iOS 设备符号",
            kind: "设备符号",
            rel: "Library/Developer/Xcode/iOS DeviceSupport",
            cache: true,
            cmd: "",
            note: "连接对应版本真机时从设备提取的系统符号，按 iOS 版本成行；删了下次连接该版本设备时重新生成",
            list_children: true,
        },
        CacheSource {
            section: "Xcode",
            short: "Xcode Archives",
            kind: "打包产物",
            rel: "Library/Developer/Xcode/Archives",
            cache: false,
            cmd: "",
            note: "上传 App Store 的打包归档（含 dSYM 符号），删了丢历史归档——是产物不是缓存，永不计入可回收",
            list_children: false,
        },
        CacheSource {
            section: "模拟器",
            short: "模拟器运行时",
            kind: "模拟器运行时",
            rel: "Library/Developer/CoreSimulator/Profiles/Runtimes",
            cache: true,
            // 有官方命令 → 不标 cleanable（S3 口径）：simctl 按运行时 ID 精确删除
            // 并正确注销，比搬走整个目录干净
            cmd: "xcrun simctl runtime delete <运行时ID>",
            note: "下载的模拟器运行时，按版本成行；删了要用 Xcode 重新下载——官方命令按运行时 ID 精确删除，本工具不代跑",
            list_children: true,
        },
    ]
}

pub struct ToolchainScanner {
    home: PathBuf,
    platform: Platform,
}

impl ToolchainScanner {
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            platform: Platform::current(),
        }
    }

    /// 指定平台构造。**只给测试用**——映射表要能在本机被断言，
    /// 不能只测当前平台那一半（本机是 macOS，那样 Windows 侧等于没测）。
    ///
    /// `#[cfg(test)]` 而不是 `allow(dead_code)`：生产代码只用 `new()`，
    /// 这个方法在非测试构建里既不该存在也不该被警告。
    #[cfg(test)]
    pub(crate) fn on(home: PathBuf, platform: Platform) -> Self {
        Self { home, platform }
    }

    /// 一个缓存目录都不存在就不该出现在侧栏。
    ///
    /// **版本管理器也算存在条件之一**（G3）：装了 nvm 就说明这台机器上有多版本
    /// 并存的问题，即使所有缓存目录都被清干净了，这一组仍该出现。
    pub fn exists(&self) -> bool {
        self.exists_on(self.platform)
    }

    /// 某个平台下是否有任何一个白名单目录存在。
    pub(crate) fn exists_on(&self, p: Platform) -> bool {
        sources()
            .iter()
            .any(|s| resolve_rel(s.rel, p).is_some_and(|r| self.home.join(r).is_dir()))
            || self.version_items_on(p).next().is_some()
    }

    /// 某个平台下实际存在的 (缓存源, 绝对路径)。不存在的整行消失（没装这个工具）。
    fn present_on(&self, p: Platform) -> Vec<(CacheSource, PathBuf)> {
        sources()
            .into_iter()
            .filter_map(|s| {
                let rel = resolve_rel(s.rel, p)?;
                let path = self.home.join(rel);
                path.is_dir().then_some((s, path))
            })
            .collect()
    }

    /// 版本管理器的行：**每个已装版本一行**。
    ///
    /// 返回迭代器而不是 `Vec` 是因为 `exists_on` 只想知道「有没有」，
    /// 而 `scan` 要全部——一次实现两处用，`next()` 短路即可。
    fn version_items_on(&self, p: Platform) -> impl Iterator<Item = Item> + use<'_> {
        let mut found: Vec<Item> = Vec::new();
        for vm in version_managers() {
            // 候选里同一管理器可能有多个根都存在（如 conda 的 miniconda3 与
            // anaconda3）。逐个展开，不取第一个——取第一个会让装了 Anaconda 的
            // 机器上看不到 miniconda3 里的环境。
            for rel in vm.rels(p) {
                let dir = self.home.join(rel);
                if !dir.is_dir() {
                    continue;
                }
                found.extend(version_rows(&vm, &dir));
            }
        }
        found.into_iter()
    }

    /// 清理缓存目录（S3）。与 `clean_repo` 同构：逐项复核 → `trash::delete` → 汇总。
    ///
    /// **复核的方向与 repos 相反，别照搬那条的直觉**：repos 要查「这个仓库还有没有
    /// 未推送 / 未提交改动」——外部状态可能变，所以要重跑 git。这里要查的是
    /// **「这条现在还标不标 `cleanable`」**——它由白名单（`cache` 与 `cmd` 两个字段）
    /// 静态决定，**外部无法改变**，所以不该信前端传来的 id 直接删，而是拿路径回到
    /// 白名单里查一遍：不在白名单、或者在白名单但不可清理（已装产物 / 有官方命令），
    /// 一律拒绝。
    ///
    /// **为什么不信任前端**：前端门控（`canClean`）只是渲染入口，用户改不了 UI 但
    /// 能改缓存文件、也能从别的客户端调这个 command。「点得动才删得了」不该由
    /// 前端说了算——S2 那条「账面可回收不许含点下去会被拒的条目」是同一个道理的两面：
    /// 账本要对齐真删，真删要对齐白名单。
    pub fn clean_cache(&self, plan: &CleanPlan) -> CleanResult {
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
            let Some(path) = parse_cache_id(id) else {
                result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: "条目 id 不合法".into(),
                });
                continue;
            };

            // 回到白名单复核：这条现在还能不能删（纯函数，喂假 home 可断言）
            match clean_verdict(&self.home, self.platform, &path) {
                CleanVerdict::Allow => {}
                CleanVerdict::Deny(reason) => {
                    result.failed.push(CleanFailure {
                        id: id.clone(),
                        reason,
                    });
                    continue;
                }
            }

            // 再确认它现在真的还在（白名单命中但目录没了 = 缓存已过期）
            let size = crate::scan::dirsize::dir_size(&path, &HashSet::new()).unwrap_or(0);

            match trash::delete(&path) {
                Ok(()) => {
                    result.succeeded.push(id.clone());
                    result.freed_bytes += size;
                }
                Err(e) => result.failed.push(CleanFailure {
                    id: id.clone(),
                    reason: e.to_string(),
                }),
            }
        }

        result
    }
}

/// `clean_cache` 的白名单复核，抽成纯函数便于喂假 home 断言（硬约束 4：账面
/// 与删除判据逐条对齐——账面列了什么，删除面就收什么）。
///
/// 三条规则：
/// - **精确命中**白名单且是「无官方命令的纯缓存」→ 放行；但**可列出子目录的源
///   例外**——账面只列子行，整目录入口等于比账面删得更多（用户界面上根本没有
///   这个按钮，能从别处递进来的调用也不该比界面能得更多），拒。
/// - **子目录命中**：父目录恰好是「可列出且可清理」的白名单源 → 放行（P1 的
///   逐项目 / 逐版本行就长这样）；其余一切子路径（含不可列出源的子路径、
///   不可清理源的子路径）按原有口径拒。
/// - 都没命中 → 「不在缓存白名单内」。
enum CleanVerdict {
    Allow,
    Deny(String),
}

fn clean_verdict(home: &Path, platform: Platform, path: &Path) -> CleanVerdict {
    let deny_uncleanable = |src: &CacheSource| {
        if src.cmd.is_empty() {
            format!("{}（{}）不提供清理入口", src.short, src.kind)
        } else {
            format!("{} 有官方清理命令，请自己运行：{}", src.short, src.cmd)
        }
    };

    let all = sources();
    let sources: Vec<(&CacheSource, PathBuf)> = all
        .iter()
        .filter_map(|s| resolve_rel(s.rel, platform).map(|rel| (s, home.join(rel))))
        .collect();

    if let Some((src, _)) = sources.iter().find(|(_, p)| p.as_path() == path) {
        return if !cache_is_cleanable(src) {
            CleanVerdict::Deny(deny_uncleanable(src))
        } else if src.list_children {
            CleanVerdict::Deny(format!(
                "{} 按子目录逐项清理（账面即按子目录成行），不提供整目录删除入口",
                src.short
            ))
        } else {
            CleanVerdict::Allow
        };
    }

    if let Some(parent) = path.parent()
        && let Some((src, _)) = sources.iter().find(|(s, p)| s.list_children && p == parent)
    {
        // 走到这里的父目录一定是可列出源；可不可清理再分一层
        return if cache_is_cleanable(src) {
            CleanVerdict::Allow
        } else {
            CleanVerdict::Deny(deny_uncleanable(src))
        };
    }

    CleanVerdict::Deny("不在缓存白名单内".into())
}

/// `Item.id` → 路径。**去掉 `toolchain:` 前缀**，剩下的才是路径。
///
/// 做成纯函数是为了能被断言，且因为**它现在是唯一的入参校验**：`clean_repo` 那边
/// 历史上直接 `PathBuf::from(id)` 整个当路径用，在只有一种 id 形态时恰好能跑，
/// 多一种形态后就成了「拿 id 当路径」——`toolchain:/Users/…` 会被当成一个
/// 以 `toolchain:` 开头的相对路径，`trash::delete` 拿它去删会得到一个费解的错误。
///
/// 三组 id 形态不同（repos 裸路径 / toolchain 带前缀 / ai_tools 的
/// `ai:<类别键>:<根路径>` 走自己的 `parse_ai_id`），所以**这不是通用反序列化**，
/// 只是本组的去前缀。前缀对不上、或去完是空的，一律返回 `None`——让服务端拒绝
/// 比让它去猜一个路径安全。
///
/// **空结果必须拒**：`strip_prefix` 去掉前缀后给的是 `Some("")`，
/// `PathBuf::from("")` 是**空路径**，而空路径在文件系统语义上等于「当前工作目录」——
/// 它过不了 `is_dir` 之类的检查时看起来像「目录不存在」，但真到别的调用
/// （复制、打开、递归删除）上，空路径的行为完全不是「不存在的目录」。
/// 这条由 `toolchain_id_parsing_rejects_foreign_forms` 钉住（它真的先红过：
/// 写用例时按直觉断言空串会被拒，编译运行才暴露 `strip_prefix` 的这个行为）。
fn parse_cache_id(id: &str) -> Option<PathBuf> {
    let rest = id.strip_prefix("toolchain:")?;
    if rest.is_empty() {
        return None;
    }
    Some(PathBuf::from(rest))
}

/// 版本根目录下的每个子目录 = 一个已装版本 = 一行。
///
/// **只列目录，不列文件**：`read_dir` 里混着的 `.DS_Store`、`.nvmrc` 这类不是版本。
fn version_rows(vm: &VersionManager, root: &Path) -> Vec<Item> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    // 目录遍历顺序不保证稳定，排序让输出与体积排序叠加时仍然确定
    dirs.sort();

    let sizes = dir_sizes_batch(&dirs);
    dirs.iter()
        .map(|d| version_item(vm, d, lookup_size(&sizes, d), mtime_secs(d)))
        .collect()
}

/// 一个已装版本 → 一行。
///
/// **永不可回收**：删一个版本是主动选择（可能还有项目 pin 在这个版本上），
/// 与 Rustup 工具链、pnpm 的 Node 同口径。`cleanable` 恒 false、
/// `reclaimable` 恒 `None`——不给「可回收 2.7 GB」这种会误导的数字。
fn version_item(vm: &VersionManager, dir: &Path, size: u64, mtime: Option<i64>) -> Item {
    let version = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut metrics: HashMap<String, MetricValue> = HashMap::new();
    metrics.insert("size".into(), MetricValue::Bytes(size));
    metrics.insert(
        "last_modified".into(),
        match mtime {
            Some(ts) => MetricValue::Int(ts),
            None => MetricValue::key("data.value.unknown"),
        },
    );
    // kind 用既有分类串，前端「类别」维度与口径测试都认这一套
    metrics.insert("kind".into(), MetricValue::text(KIND_INSTALLED));

    let mut detail: HashMap<String, String> = HashMap::new();
    detail.insert("0. 路径".into(), dir.display().to_string());
    detail.insert("1. 版本管理器".into(), vm.section.into());
    detail.insert("2. 类别".into(), KIND_INSTALLED.into());
    detail.insert("3. 说明".into(), vm.note.into());

    Item {
        id: format!("toolchain:version:{}", dir.display()),
        // 标签只放筛选真正需要的。「可清理」没有（永不可回收），而版本号不该做标签——
        // 它是行名的一部分（`nvm · v22.19.0`），再挂一个标签就是同一信息显示两遍。
        tags: vec![],
        name: format!("{} · {}", vm.section, version),
        label: None,
        short_label: Some(Text::raw(version)),
        group_label: Some(section_text(vm.section)),
        group_key: Some(vm.section.into()),
        path: dir.display().to_string(),
        metrics,
        kind_label: Some(kind_text(KIND_INSTALLED)),
        detail: Some(detail),
        artifact_entries: vec![],
        cleanable: false,
        reclaimable: None,
    }
}

/// 可列出源（P1）的行：**每个子目录一行**。DerivedData 按项目、设备符号按 iOS
/// 版本、运行时按版本——合成一行只剩一个总数，「哪个项目最占、哪个版本最旧」
/// 全看不见（与 G3 版本清单同一条逻辑）。
///
/// **只列目录，不列文件**：DerivedData 下混着的 `.DS_Store` 之类不是项目。
fn child_rows(src: &CacheSource, root: &Path) -> Vec<Item> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    // 目录遍历顺序不保证稳定，排序让输出与体积排序叠加时仍然确定
    dirs.sort();

    let sizes = dir_sizes_batch(&dirs);
    dirs.iter()
        .map(|d| child_item(src, d, lookup_size(&sizes, d), mtime_secs(d)))
        .collect()
}

/// 可列出源的子目录行。与 [`source_item`] 同构，差别只有三点：行名取子目录名、
/// id 直接指向子目录、`cleanable` 继承父源的口径（含「有官方命令不标」那半）。
fn child_item(src: &CacheSource, dir: &Path, size: u64, mtime: Option<i64>) -> Item {
    let short = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut metrics: HashMap<String, MetricValue> = HashMap::new();
    metrics.insert("size".into(), MetricValue::Bytes(size));
    metrics.insert(
        "last_modified".into(),
        match mtime {
            Some(ts) => MetricValue::Int(ts),
            None => MetricValue::key("data.value.unknown"),
        },
    );
    metrics.insert("kind".into(), MetricValue::text(src.kind));

    let cleanable = cache_is_cleanable(src);

    let mut detail: HashMap<String, String> = HashMap::new();
    detail.insert("0. 路径".into(), dir.display().to_string());
    detail.insert("1. 类别".into(), format!("{}（{}）", src.kind, src.short));
    detail.insert(
        "2. 清理方式".into(),
        if cleanable {
            "直接删除目录（移入废纸篓）".into()
        } else if !src.cmd.is_empty() {
            format!("用官方命令清理：{}", src.cmd)
        } else {
            "不可清理".into()
        },
    );
    detail.insert("3. 说明".into(), src.note.into());

    Item {
        id: format!("toolchain:{}", dir.display()),
        tags: if cleanable {
            vec![Tag::new("cleanable")]
        } else {
            vec![]
        },
        name: format!("{} · {}", src.section, short),
        label: None,
        short_label: Some(Text::raw(short)),
        group_label: Some(section_text(src.section)),
        group_key: Some(src.section.into()),
        path: dir.display().to_string(),
        metrics,
        kind_label: Some(kind_text(src.kind)),
        detail: Some(detail),
        artifact_entries: vec![],
        cleanable,
        reclaimable: if cleanable { Some(size) } else { None },
    }
}

impl Scanner for ToolchainScanner {
    fn id(&self) -> &'static str {
        "toolchain"
    }
    /// S3 起返回 `true`——`clean_cache` 已实现，17 条无官方命令的纯缓存有删除入口。
    ///
    /// **注意它不等于「本组所有 `cleanable` 的行都能删」**：有官方命令的那 6 条
    /// `cleanable` 恒 false（见 [`cache_is_cleanable`]）。前端按分组决定「这一组
    /// 要不要渲染清理入口」，按行 `item.cleanable` 决定「这一行给不给按钮」——
    /// 两个粒度，别混。
    fn supports_clean(&self) -> bool {
        true
    }

    fn scan(&self) -> AssetGroup {
        // 存在的目录才成行；不存在的（没装这个工具）整行消失
        let present = self.present_on(self.platform);

        // 整目录成行的源：一次批量拿全部体积（并行遍历每个目录，见 scan/dirsize.rs）。
        // 可列出子目录的源（P1）不在这里——它们的体积落在各子行上，父目录本身不成行。
        let mut aggregate = Vec::new();
        let mut listing = Vec::new();
        for (s, p) in &present {
            if s.list_children {
                listing.push((s, p.clone()));
            } else {
                aggregate.push((s, p.clone()));
            }
        }

        let paths: Vec<PathBuf> = aggregate.iter().map(|(_, p)| p.clone()).collect();
        let sizes = dir_sizes_batch(&paths);

        let mut items: Vec<Item> = aggregate
            .iter()
            .map(|(s, p)| source_item(s, p, lookup_size(&sizes, p), mtime_secs(p)))
            .collect();

        // 可列出子目录的源（P1）：逐子目录成行。父目录为空时零行——
        // 不出一个 0 B 的聚合行（硬约束 8 的行级版本：没内容就不占视线）。
        for (s, p) in listing {
            items.extend(child_rows(s, &p));
        }

        // 版本管理器（G3）：逐版本出行，接在缓存行之后，同一批按体积降序排。
        // 这一段是本刀新增的第二个数据来源——它们不是缓存，是已装的版本。
        items.extend(self.version_items_on(self.platform));

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

        let (count, total, cleanable_count, reclaimable) = summarize(&items);

        let mut notes: Vec<Text> = vec![
            Text::key("data.note.toolchain.noContent"),
            // 口径两条（分节标题「可回收」列的数按这个来，别写错）：
            // ① 纯下载/依赖缓存**且没有官方清理命令**的才可回收——本工具的「清理」
            //    按钮只对这类开放。② 有官方命令的（npm / pnpm / uv / pip / Homebrew /
            //    Go …）不算，命令写在每行明细里由你自己跑：搬走整个目录虽然安全，
            //    下次要完整重建，而官方命令只动真正多余的部分。
            Text::key("data.note.toolchain.reclaimRule"),
            Text::key("data.note.toolchain.installedNotReclaimable"),
            Text::key("data.note.toolchain.waitBuilds"),
        ];
        // Xcode 系（P1）在场时才说：Archives 不是缓存这件事必须摆在明面上，
        // 别让人按「工具链缓存分组」的直觉把它当成可清理
        if present
            .iter()
            .any(|(s, _)| s.section == "Xcode" || s.section == "模拟器")
        {
            notes.push(Text::key("data.note.toolchain.xcodeArchives"));
        }
        // 版本管理器（nvm / fnm / volta / …）逐版本成行：多版本并存是主动选择，
        // 全部不可回收，卸掉要用各家自己的 uninstall 命令（见每行明细）。
        // 措辞按实际扫到了几个管理器给——只装一个就说「们」是套话。
        let managers: Vec<&str> = {
            let mut v: Vec<&str> = version_managers()
                .iter()
                .filter(|vm| vm.root(&self.home, self.platform).is_some())
                .map(|vm| vm.section)
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        if !managers.is_empty() {
            notes.push(
                Text::fmt("data.note.toolchain.versionManagers")
                    .arg("managers", Text::raw(managers.join(" / "))),
            );
            notes.push(Text::key(
                "data.note.toolchain.versionManagersNotReclaimable",
            ));
        }
        if items.is_empty() {
            notes.push(Text::key("data.note.toolchain.empty"));
        }

        AssetGroup {
            id: self.id().into(),
            label: self.label(),
            summary: Text::fmt("data.summary.toolchain")
                .arg("count", TextArg::num(count))
                .arg("total", TextArg::bytes(total))
                .arg("clean", TextArg::num(cleanable_count))
                .arg("reclaim", TextArg::bytes(reclaimable)),
            fields: toolchain_fields(),
            items,
            notes: Some(notes),
            error: None,
            supports_clean: true,
        }
    }
}

fn source_item(src: &CacheSource, path: &Path, size: u64, mtime: Option<i64>) -> Item {
    let mut metrics: HashMap<String, MetricValue> = HashMap::new();
    metrics.insert("size".into(), MetricValue::Bytes(size));
    metrics.insert(
        "last_modified".into(),
        match mtime {
            Some(ts) => MetricValue::Int(ts),
            None => MetricValue::key("data.value.unknown"),
        },
    );
    metrics.insert("kind".into(), MetricValue::text(src.kind));

    let cleanable = cache_is_cleanable(src);

    // detail 是 HashMap，键序不确定，靠编号前缀保证前端渲染顺序（前端会剥掉前缀）
    let mut detail: HashMap<String, String> = HashMap::new();
    detail.insert("0. 路径".into(), path.display().to_string());
    detail.insert("1. 类别".into(), src.kind.into());
    // 清理方式分三档说（S3 起）：本工具能删 / 该用它自己的命令 / 两者都不行。
    // 中间那档是「有官方命令但本工具不代跑」——命令要指给人，不能只写
    // 「直接删除目录」，否则用户会以为搬走目录等价于官方清理（pnpm store
    // 会因此要完整重建，见 cache_is_cleanable 的注释）。
    detail.insert(
        "2. 清理方式".into(),
        if cleanable {
            "直接删除目录（移入废纸篓）".into()
        } else if !src.cmd.is_empty() {
            format!("用官方命令清理：{}", src.cmd)
        } else {
            "不可清理".into()
        },
    );
    detail.insert("3. 说明".into(), src.note.into());

    Item {
        id: format!("toolchain:{}", path.display()),
        tags: if cleanable {
            vec![Tag::new("cleanable")]
        } else {
            vec![]
        },
        name: format!("{} · {}", src.section, src.short),
        label: None,
        short_label: Some(short_text(src.short)),
        group_label: Some(section_text(src.section)),
        group_key: Some(src.section.into()),
        path: path.display().to_string(),
        metrics,
        kind_label: Some(kind_text(src.kind)),
        detail: Some(detail),
        artifact_entries: vec![],
        cleanable,
        reclaimable: if cleanable { Some(size) } else { None },
    }
}

/// 汇总口径与 Java / dev_env 分组保持一致：合计、可回收项数、可回收体积。
fn summarize(items: &[Item]) -> (usize, u64, usize, u64) {
    let total: u64 = items
        .iter()
        .filter_map(|i| match i.metrics.get("size") {
            Some(MetricValue::Bytes(b)) => Some(*b),
            _ => None,
        })
        .sum();
    let reclaimable: u64 = items.iter().filter_map(|i| i.reclaimable).sum();
    let cleanable_count = items.iter().filter(|i| i.cleanable).count();
    (items.len(), total, cleanable_count, reclaimable)
}

// ---- 测试辅助：把纯函数暴露给 tests.rs，避免重复实现 ----

/// 白名单的原始行，**不做平台解析**。
///
/// 可回收边界是数据自身的属性，与跑在哪个平台上无关。所以口径测试必须走这里，
/// 不能走 `sources_on_for_test`——后者在 Windows 上会滤掉 Homebrew 那一条
/// （`win_rel` 显式返回 `None`），「Homebrew 下载缓存可回收」这条断言就永远验不到，
/// 而它在 macOS 上是验得到的。**测试的覆盖面不该取决于跑测试的机器。**
#[cfg(test)]
pub(crate) fn kinds_for_test() -> Vec<(String, String, String, bool)> {
    sources()
        .into_iter()
        .map(|s| {
            (
                s.section.to_string(),
                s.short.to_string(),
                s.kind.to_string(),
                // 返回**最终**的 cleanable（`cache && cmd.is_empty()`），不是「是不是缓存」——
                // 口径测试要断的是这个值，否则测试与运行时各说各话，测了个假的。
                cache_is_cleanable(&s),
            )
        })
        .collect()
}

/// 指定平台的白名单展开。Windows 那半边靠它在本机被断言。
#[cfg(test)]
pub(crate) fn sources_on_for_test(
    home: &Path,
    p: Platform,
) -> Vec<(String, String, PathBuf, bool)> {
    sources()
        .into_iter()
        .filter_map(|s| {
            let rel = resolve_rel(s.rel, p)?;
            Some((
                s.section.to_string(),
                s.short.to_string(),
                home.join(rel),
                cache_is_cleanable(&s),
            ))
        })
        .collect()
}

/// 逐条给出「短名 → 有没有官方命令」，用来断言「有官方命令的一律不标 cleanable」
/// 这条 S3 立的口径。
#[cfg(test)]
pub(crate) fn cleanable_and_cmd_for_test() -> Vec<(String, bool, &'static str)> {
    sources()
        .into_iter()
        .map(|s| (s.short.to_string(), cache_is_cleanable(&s), s.cmd))
        .collect()
}

/// `parse_cache_id` 的测试出口。**它是 `clean_cache` 唯一的入参校验**，
/// 错一个字符的后果是把 `toolchain:` 当成路径的一部分去删东西。
#[cfg(test)]
pub(crate) fn parse_cache_id_for_test(id: &str) -> Option<PathBuf> {
    parse_cache_id(id)
}

/// `clean_verdict` 的测试出口：`Allow` → `Ok(())`，`Deny(reason)` → `Err(reason)`。
/// 喂假 home 就能断言「哪类路径放行、哪类拒绝、拒绝理由是什么」。
#[cfg(test)]
pub(crate) fn clean_verdict_for_test(home: &Path, p: Platform, path: &Path) -> Result<(), String> {
    match clean_verdict(home, p, path) {
        CleanVerdict::Allow => Ok(()),
        CleanVerdict::Deny(reason) => Err(reason),
    }
}

#[cfg(test)]
pub(crate) fn resolve_rel_for_test(rel: &'static str, p: Platform) -> Option<&'static str> {
    resolve_rel(rel, p)
}

/// 白名单里所有的 mac 侧相对路径。用来守住「新增条目别忘了 Windows」。
#[cfg(test)]
pub(crate) fn all_rels_for_test() -> Vec<&'static str> {
    sources().into_iter().map(|s| s.rel).collect()
}

#[cfg(test)]
pub(crate) fn summarize_for_test(items: &[Item]) -> (usize, u64, usize, u64) {
    summarize(items)
}

/// 版本管理器白名单展开成可断言的形状：
/// `(管理器, 该平台的候选相对路径列表)`。
///
/// 给测试用：Windows 那半边的映射在本机（macOS）也要能断言——路径解析做成值之后
/// 这件事是免费的，`cfg` 一写就没了（硬约束 9）。
#[cfg(test)]
pub(crate) fn version_managers_for_test(p: Platform) -> Vec<(String, Vec<&'static str>)> {
    version_managers()
        .into_iter()
        .map(|vm| (vm.section.to_string(), vm.rels(p).to_vec()))
        .collect()
}

/// 某个管理器在给定 home 下的版本目录行。走真实扫描逻辑，测试不重新实现一遍。
#[cfg(test)]
pub(crate) fn version_items_for_test(home: &Path, p: Platform) -> Vec<Item> {
    ToolchainScanner {
        home: home.to_path_buf(),
        platform: p,
    }
    .version_items_on(p)
    .collect()
}

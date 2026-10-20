# AGENTS.md 附录——约束的来历与实测时间线

> 本文件收 [AGENTS.md](../AGENTS.md) 瘦身（2026-10-17）时挪出来的叙事：事故背景、实测时间线、范例细节。**规则以 AGENTS.md 为准**，这里只解释它们从哪来。
> 内容原样保留、不再更新——数字是当时快照（H4 立的判断线：记录历史的不改，改掉是伪造记录）。
>
> 一条后来者须知：本文件与 `docs/archive/*`、`docs/evidence/*` 里提到的 `prototypes/*.html`（UI 迭代过程稿）**已随开源清理移出仓库**——那些引用是当时的真实状态，按上面的判断线保留原样。UI 密度的当前权威值是生产代码 `src/views/Assets.vue`，不是任何过程稿。

## 硬约束 1：约 26 秒的实测时间线

全量扫描实测**约 26 秒**（U3-④ 重测：`cargo run --example dump`，2026-10-13，debug 构建 7 个分组并行；release 更快），瓶颈全在目录体积统计——Git 仓库侧最重（237 个仓库）、工具链缓存次之（36 个条目 = 27 个缓存目录 + 9 个 nvm 版本目录，pnpm store 一个 9.4 GB 就够慢，单项原生遍历 2.6 秒，见 `tests::bench_native_size_vs_du`），git 子进程只占 0.14 秒，读缓存 5 毫秒。

> **H3 之后本机是 5 个分组**（SSH 与 Docker 移出注册）。2026-10-13 已复测：5 组 / 339 条 / **26264 ms**，与上面那个 26 秒一致——那两组一个 0 B、一个只有 notes 没有 `items`，对耗时不构成量级影响。**别把它当成「5 组才是 26 秒」**。G1 收口时再测（ai_tools 并入编辑器数据后 5 组 / 349 条）：**24991 ms**，条目多了 10 条而耗时略降——量级不变。G2 收口再测（6 组 / 357 条）：**29400 / 24930 ms** 两次；G3 收口再测（6 组 / 367 条）：**25606 ms**；H6 收口再测（同样 6 组 / 367 条，一条数据没变）：**31973 ms**。**所以「26 秒」是个量级、不是定值——同一份快照在 25~32 秒之间跑过。** H6 那次偏慢不代表分组变重了（条目数完全一样），别据此推断「加了分组就会更慢」——那要控制变量实测。

> 这个 26 秒替换掉了 `du` 时代的 54 秒。W1 换成原生遍历后总时长没退反进，单项对照见 `tests::bench_native_size_vs_du`（pnpm store 原生 2.6 秒 vs du 6.4 秒）。

## 硬约束 3：H2 勘误

AGENTS 原先写的是「私钥文件本身只用来取创建时间和权限位」，与实现对不上（实现会读前 64/512 字节判断格式与加密），H2 已勘误对齐。同句错话当时有三个副本（AGENTS、`ssh.rs` 模块注释、README 精度补充），口径统一为「读头但不读内容」。

## 硬约束 4：判据收窄的代价

S3 收窄 `cleanable` 口径的代价是**账面 toolchain 可回收从 31.27 GB 降到约 10 GB**（2026-10-13 实测），方向与 S2 收紧 repos 判据一致（报不满比报虚好）。`@{u}...HEAD` 两列赋反的事故：S2 收口时抓到「只是落后（别人推了新提交）的仓库被当成『有未推送』误拒，真有未推送提交的反而不被拦」——开门即可被删，方向反了比不判更危险。

## 硬约束 9：三类坏法的具体案例

- **绑错平台 → 假红**：`toolchain_sources_resolve_from_home` 断言 `npm → .npm`（macOS 布局），走 `Platform::current()` 在 Windows runner 上必红。改法：`sources_on_for_test(&fake, Platform::Mac)`、`ToolchainScanner::on(root, Platform::Mac)`。
- **绑错平台 → 静默漏验**（更坏）：`dev_env_probe_hides_absent_environments` 原先只读 `/Applications`，Windows 上 read_dir 失败 → `has_ide` 恒 false → 三条断言全被跳过，**测试照样显示 ok 但等于没跑**。补了 `tests.rs::ide_parents`（按平台给 IDE 位置，Windows 侧含 `%ProgramFiles%` 与 `%LOCALAPPDATA%\Programs`）。
- **口径不该经平台解析**：`toolchain_installed_artifacts_are_not_cleanable` 原走 `sources_for_test`，后者在 Windows 上滤掉 Homebrew（`win_rel` 返回 `None`），「Homebrew 下载缓存可回收」就只在 macOS 上验得到。改走 `kinds_for_test()`（不解析平台）。
- **「本机根本跑不了」的探测**：W3 的 WSL 读注册表要 `reg query`，macOS 上连这个命令都没有。做法是把「读」做成可注入参数（`wsl::wsl_scanners_with(platform, read)`）——「Windows 平台下会不会注册」「读到坏数据怎么办」都在本机被断言，真正留给 Windows 实机的只剩一条「那条命令跑不跑得通」。「本机真实状态」断言可以跟 `current()` 走的范例：`toolchain_registered_when_present` 拿真实 home 与 `scanners()` 比一致。

## 硬约束 11：版本行的事例数字

本机 v22.19.0 单个 2.0 GB、v12.18.0 停在 2025-02 占 81 MB、9 个版本合计 3.1 GB、`~/.nvm/.cache` 下载暂存 167 MB（2026-10-13 实测）。合成一行这些全看不见。

## 架构：并行的实测细节

实测两个体积统计密集型扫描器并发会互相拖慢（pnpm store 单个目录原生遍历 2.6 秒，与仓库扫描并发时谁都快不起来），APFS 元数据操作在设备层排队。体积统计的 `du` 对照细节在 `tests::bench_native_size_vs_du`（`#[ignore]`，手跑）。

## 已知限制：Windows CI 核不了什么的原文

「Windows CI 绿了」守的是：`#[cfg(not(unix))]` 分支（ssh 权限位、体积的 `file_size` 兜底）此前从未被编译过，以及只在 Windows 上才暴露的语义差异（如 `cache::save` 的 rename 覆盖靠 `MOVEFILE_REPLACE_EXISTING`）。这两类问题 CI 能抓，路径对不对它抓不到——runner 上没有 Android Studio / DevEco Studio / VS Code，探不到就是探不到。

## 测试思路范例（原「常用命令」段的完整版）

- **Docker**：6 个用例里 5 个是纯解析测试（喂真实 CLI 输出，不真跑 docker），守护进程没启动也能过——只有 `docker_scan_returns_group_when_daemon_runs` 会真调 docker，且 daemon 不可用时自己跳过。
- **工具链**：7 个用例全部喂假 home（临时目录），通过与否不取决于本机装没装某个工具。
- **WSL**（最极端）：本机（macOS）根本不注册，所以「读注册表」走注入（`wsl_scanners_with`）、解析喂 `reg query` 样本，四条用例覆盖「解析对不对 / 缺 BasePath 怎么办 / 哪些平台该注册 / 扫出来的形状与可回收口径」——「本机不注册」不等于「没测」。
- **体积统计**：`dirsize_matches_independent_block_sum` / `dirsize_excludes_nested_subtree` / `dirsize_does_not_follow_symlinks` 喂临时目录，且自写独立实现（`tree_bytes`）当基准。
- **反向示例**：扫真实数据的用例（如 `ai_tools_sizes_stay_within_root`）本质是拿两次测量比大小，被测目录被写入时会偶发假红——容差按「数据是活的」给。
- **du 作为 oracle 的两处是刻意的**：`tests.rs` 里 `tree_bytes` 与 `bench_native_size_vs_du` 用 `du -sk` 当独立参照物——测试要自备与被测代码无关的基准，那里的 `du` 不是实现残留。

## 测试基建坑（S2 收口时踩的，写用例前看一眼）

- `GIT_*_DATE` 不吃 "400 days ago"（要绝对 Unix 时间戳；且判据读 `%ct` 提交者日期，只改 author date 不够）。
- 只有 `branch.*.remote` 而没有 `[remote "origin"]` 段时 `@{u}` 解析失败（fatal: not stored as a remote-tracking branch），必须真 `git remote add`。

## harness 接入初期抓到的问题（docs/harness-and-loop.md 的姊妹记录）

改名后 target/ 陈旧产物（`cargo clean -p tauri` 修）、clippy 9 警告清零后才上 `-D warnings`、repos 声明 5 个字段但前端只渲染 3 个（契约测试首跑抓到）、`SCHEMA_VERSION >= 2` 常量断言是死代码（换成 `scanner_registration_is_pinned` 集合哨兵）、测试硬依赖本机 JDK（改注册状态与探测一致）、私钥内容闸第一跑误伤自家测试针串（收紧到完整 PEM 甲胄头）。细节见 `docs/harness-and-loop.md`「这次接入时抓到的问题」。

## 文档漂移治理的判断线（H4/H6/H7 沉淀，写文档前想清楚）

- **代码注释与现行文档里「描述现状」的话必须准确；「记录历史」的文档保留原样。** 设计记录（`docs/ai-tools-scanner-plan.md`，已由 H5 加现状声明）、证据存档（`docs/evidence/*`）、原型内嵌 dump、backlog/本附录里已拍板决策的历史引文，出现旧值是把当时的实测当记录，改掉反而是伪造记录。
- **带出处的具体值只在 README「扫描策略」写一次**，代码注释里不重复数字——重复数字是漂移的源头（H4）。
- **注释里的数不是一律要改，是要逐个判它是「描述现状」还是「记录当时」**；修一处不能只修被点到的那处，同句同类的数一起核（H7）。
- **凡「N 个条目 / X GB」这种由代码结构决定的数**，加条目的刀就该顺手核，专门的文档刀只能收拾已经漂了的（H6）。
- **lint 集合随工具链版本漂移**是「本地绿 CI 红」的真因之一（H8 勘测：CI stable 1.99 有 `unnecessary_sort_by`，本地 1.93 没有）。

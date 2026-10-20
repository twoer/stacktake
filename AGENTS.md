# AGENTS.md

给在这个仓库里干活的 AI / 协作者看的。写的是踩过坑才定下来的约束，不是风格偏好。每条约束只留规则与一句话为什么；完整的事故背景与实测时间线在 [docs/agents-appendix.md](./docs/agents-appendix.md)——翻历史去那边，**别把叙事加回本文件**（它是每个会话都要支付的上下文成本）。`docs/backlog.md`、`docs/archive/`、`docs/evidence/` 是本机开发工作流文件，不随仓库分发，缺它们不影响构建与门禁。

## 项目是什么

Stacktake —— 本机资产清点台账。Tauri 2 + Vue 3 + Tailwind v4 的**双平台**（macOS + Windows）桌面看板，清点本机开发资产（Git 仓库、工具链缓存、编辑器与 AI 工具数据、语言环境…）。除 repos / toolchain / ai_tools 三组的清理入口（移入废纸篓、可恢复，见硬约束 4）外全程只读。

选 Tauri 而不是 Web 的理由：**不监听端口、不是 HTTP 服务、没有跨域攻击面**。任何引入网络监听的改动都违背这个前提。

## 架构

```
Scanner trait → AssetGroup → scan-cache.json → tauri command → Vue 表格
```

- `src-tauri/src/scan/mod.rs` 是枢纽：`Scanner` trait + 数据契约（`AssetGroup` / `Item` / `MetricValue` / `Field`）。
- 前端只认 `AssetGroup`。**新增资产类型只需实现 `Scanner` 并注册进 `lib.rs::scanners()`，前端不用改**——但有三条前端约束：
  - `fields` **最多三个**：`Assets.vue` 里是 `fields.slice(0, 3)`，第四个字段不进表头也不进列。
  - `Item.detail` 是 `HashMap<String, String>`，序列化顺序不确定、前端按对象键序渲染——要排序就键名前缀编号（`"1. logs"`）。
  - **分节与筛选读 `Item.group_key`，不解析名字**。`Some(值)` = 本条属于某维度取值（同值做小节标题、显示小计，行名剥 ` · ` 前缀），`None` = 本组不分节。维度名由前端按分组声明（`Assets.vue` 的 `PREFIX_LABEL`），只有真有 ≥2 个取值的维度才出筛选按钮。给了 `group_key` 的名字必须以 `key · ` 开头（两边契约都断言）。标签只放筛选真正需要的（如「可清理」）——类别、工具名别做成标签，行里重复显示挤爆单行 tab。
- `src/lib/types.ts` 是 Rust 结构的镜像，改 Rust 结构必须同步。**字段名逐字一致（snake_case，没有 `rename_all`）**——多单词字段写驼峰 vue-tsc 抓不到（类型是自己声明的，G1 踩过）。
- `examples/dump.rs` 走 `scanners()`，**不要另写一份列表**（早先另写过一份，新增 Scanner 后示例与 app 对不上）。它输出的分组与 app 一致：`scanners()` + 现算总览。app 真实路径是 `run_full_scan` → `scanners_with_roots(roots::default_roots())`，与 `scanners()` 同源，所以示例与 app 永远一致（不像 C1 时代还有「配没配过」的岔路）。
- 现有扫描器：`repos` / `toolchain`（包管理器与工具链缓存）/ `ai_tools`（label「**编辑器与 AI 工具**」）/ `local_models`（Ollama / LM Studio / HuggingFace）/ `dev_env`（Java 与移动端）/ `wsl`（发行版虚拟磁盘，**仅 Windows 注册**）。**跟环境装没装有关的（dev_env、toolchain、local_models、wsl）走「不存在就不注册」**，见硬约束 8。**`ssh` 与 `docker` 已实现但未注册**（D6 拍板不进 V1，H3 移出注册），两个文件刻意保留在树里（重新注册只是一行），**别顺手加回来**。**`java` / `harmony` 暂时隐藏**（H14，2026-10-20）：`dev_env` 照常构造与测试，`lib.rs` 注册时按 `HIDDEN_GROUPS` 撤下；恢复 = 清名单 + 改 `scanner_registration_is_pinned` 两条断言 + SCHEMA_VERSION +1。
- **`scanners()` 注册顺序 = 侧栏顺序 = `scan::GROUP_ORDER` 固定语义序**（D13）：repos → java → ios → android → harmony → toolchain → ai_tools → local_models → wsl（源码 → 环境 → 缓存 → 工具 → 模型 → 虚拟机）。**不按体积排序**——顺序随数据跳会丢失空间记忆。`with_overview` 两条路径（扫完 / 读缓存）都按它重排并插入总览，**缓存里的旧顺序也显示为新序**。未知 id 排最后；新增分组须登记 `GROUP_ORDER`（`scanner_registration_follows_group_order` 守着）。总览**不在** `scanners()`——由 `scan::synthesize_overview` 现算（纯函数，不碰磁盘），行顺序同序。**缓存里不存总览**：否则总览与分组会对不上。总览行 id 是 `overview:<分组id>`、path 是 `overview://<分组id>`（不是宿主机路径，`isLocalPath()` 天然不给动作）。
- `run_full_scan` 用 rayon **并行**跑各扫描器（`Scanner: Send + Sync` 为此加）。但并行只能收回一部分：体积统计密集型扫描器并发会互相拖慢（APFS 元数据操作在设备层排队），别指望并行把总时间压回单组之和以下。
- **「Git 仓库」的扫描根目录固定，不可配置**：`roots.rs` 的 `default_roots()` = `~/Documents/Code`、`~/Projects`、`~/Developer`、`~/code`、`~/src`、`~/workspace`（**存在才收**，一个都没有时兜底 `~/Documents`）。`RepoScanner` 有空 roots 守卫（拿不到 home 时报「未配置根目录」不 panic）。**这里曾经可配置**（C1 刀 / D2 拍板选 A：设置页增删、落盘 `scan-config.json`、三个 command、改根目录即废缓存），2026-10-19 由用户拍板整个删掉，模块随之从 `config.rs` 改名 `roots.rs`。删的三条理由与「将来要不要加回来」的判据都写在 `roots.rs` 的模块注释里——**想加回来看那段，别照着旧文档照搬**。已知代价：硬编码的六个候选名必然漏掉不在这个命名习惯里的代码目录（外置盘、公司强制路径），这个代价是被接受了的，不是没发现。设置页（`views/Settings.vue`，路由 `/settings`）现在只有「缓存」与「更新」两节。
- **前端形态（原型 v3 为基准）**：侧栏通高（`titleBarStyle: "Overlay"` + `hiddenTitle`，空区与工具栏挂 `data-tauri-drag-region`），主区「列表 + 右侧 252px 检查器」主从结构（窗口 <1040 默认收起），工具栏承载维度筛选（弹出菜单带计数）、搜索、外观切换、重扫与「数据更新于」。窗口 1180×760 / minWidth 880。动作面：访达中显示在检查器动作条；复制路径只在行右键菜单（2026-10-19 从检查器动作条收掉，别两边各挂一个）。

## 硬约束

### 1. 别给启动流程加自动扫描

全量扫描瓶颈全在目录体积统计，实测**约 26 秒量级**（同一份快照在 25~32 秒间跑过，差异来自文件系统缓存冷热与机器负载；带出处的值只在 README「扫描策略」写一次，实测时间线见 appendix）。数据变化极慢——两百多个仓库里近 7 天有提交的只有个位数。

策略：**有缓存直接返回，不做任何后台扫描；只有无缓存（首次运行）才真扫；之后靠用户点「重扫」。** 曾经有过「先出缓存 + 后台自动重扫」的版本，是**刻意删掉**的，不要加回来——它换不来新鲜度，却让每次打开都啃几十秒磁盘。

### 2. `MetricValue` 必须带类型标签

形式是 `{"t":"b"|"i"|"s"|"z", "v":...}`，**不要改成 `#[serde(untagged)]`**——untagged 时代 `Int(1700000000)` 与 `Bytes(1700000000)` 在 JSON 里长得一样，缓存读回来时间戳被显示成「1.7 GB」。

连带规则：**改 `AssetGroup` / `MetricValue` 结构时 `cache::SCHEMA_VERSION` 必须 +1**（否则老缓存静默错位）；**增删 Scanner 也要 +1**——启动是「有缓存直接返回」，版本不变老用户拿到缺分组的旧快照，界面上没有任何提示让他点「重扫」。

### 3. 绝不读私钥内容

**指纹**只能走 `ssh-keygen -lf <私钥>`（有 `.pub` 时直接读那个）——`ssh-keygen` 只读公钥部分，带 passphrase 的 key 也能取到，不需要解密。

**私钥文件会被读两次，但只读文件头、内容不出进程**：

| 读什么 | 读多少 | 干什么用 |
|---|---|---|
| `looks_like_private_key` | 前 **64** 字节 | 判断是不是私钥（`-----BEGIN` / `PuTTY-User-Key-File` 开头），好把 `.pub`、`known_hosts`、`config`、`authorized_keys` 排除掉 |
| `KeyMeta.encrypted` | 前 **512** 字节 | 找 `ENCRYPTED` / `Proc-Type: 4,ENCRYPTED`，只用于「加密 / 无密码」标签 |

两次都是 `File::open` + 定长缓冲 + `from_utf8_lossy` 后**只做子串判断**：内容不落缓存、不进 `detail`、不传前端、不出进程；其余字段走 `metadata()`。**别觉得「读头」是多余之举、删掉更安全——删了才是错**：没有 64 字节判断，`~/.ssh` 下每个文件都会当密钥列出；没有 512 字节判断，「加密/无密码」标签永远是错的。这条约束要的是**读头但不读内容**，不是「什么都不读」。（例外：`~/.ssh/config` 会被**整份**读进来——判断每把 key 有没有被引用必须看内容；它不是密钥材料，也不进缓存。）

`examples/dump.rs` 与 `tests.rs::cache_never_contains_private_key_material` 都有泄漏自检，别削弱。**H3 之后这条守着的是未注册的代码**——SSH 分组已移出侧栏，这个工具因此不接触任何凭据；`ssh.rs` 保留在树里，约束与两条自检守「万一加回来」。

### 4. 清理入口只开 repos / toolchain / ai_tools 三组，执行必须人显式发起

`trash::delete`、`RepoScanner::clean_repo` / `ToolchainScanner::clean_cache` / `AiToolsScanner::clean_ai`、`CleanPlan` / `CleanResult` 经 `clean_items` command 接入前端。开启三条件已满足：前端二次确认 UI（检查器动作条 + 确认弹窗，列明路径/体积/废纸篓可恢复）+ 服务端 `confirmed` 字段校验（缺字段反序列化即拒，显式 `false` 再拒）+ 废纸篓兜底（`trash::delete`，绝不硬删）。

- **只有这三组接了删除**（`supports_clean()` 返回 `true`）。其它 Scanner **别覆盖 `supports_clean()`**：没有删除实现却返回 `true`，等于宣称一个点不动的能力。
- **repos 组内有两种删除，复核是两条路**（P2 起）：整仓（`clean_repo`，查未推送 / 未提交改动）与构建产物（`clean_artifacts`，按 `CleanPlan.scope` 派发——清产物不动源码与 `.git`，所以**不查** git 状态；藏有 `.git` 的产物目录整目录拒）。产物判据 `repos::is_artifact_dir_under` 是纯「名字白名单 + 兄弟项目标记」规则，**跨平台一致、没有 win_rel 之分**；账面（`metrics.artifacts` / detail「构建产物」/ 标签「有产物」）与删除复核共用它，别只改一头。白名单加条目时 `artifact_whitelist_is_pinned` 会红。
- **ai_tools 的一行 = 一个「工具 × 类别」**（B1）：`clean_ai` 只接受 `cleanable` 类别（日志 / 会话 / 缓存），根路径必须命中**本次** `tools_for` 白名单，并在点击那一刻重读目录、按文件名重新分类，只删当前仍属该类别的一级子项（软链跳过）。**会话删了不会再生**——确认弹窗必须单独把这条写在明处，别套缓存组的文案。
- **前端别写死分组 id**：能力由后端以 `AssetGroup.supports_clean` 下发，`canClean` 判它。**能力的唯一真相在 Rust 侧**（S2 之前写死 `id === "repos"`，加第二个分组就漏改）。
- **`cleanable` 的含义是「本工具点下去真能删掉」**（S3 收窄，不再是「删了按需重下」）。toolchain 里有官方清理命令的（pnpm store prune / npm cache clean --force / uv / pip / brew cleanup / go clean 等）**一律不标 `cleanable`**——命令显示在 detail 的「清理方式」里让人自己跑，本工具不代跑用户级命令。搬走整个缓存也安全，但 `pnpm store prune` 只删没被引用的包，精确得多；**账面变小是方向正确的**（报不满比报虚好）。判据 `cache_is_cleanable(src) = src.cache && src.cmd.is_empty()`，由 `toolchain_sources_with_official_cmd_are_not_cleanable` 守着（**新增白名单条目时它会红，别把断言改松**）。ai_tools 同口径：技能 / 索引 / 配置 / 运行时 / 未知**一律不标**，由 `ai_tools_clean_flags_follow_category` 守着。
- **账面与删除判据必须逐条对齐**：标了可清理的，不许是点下去会被拒的。repos 查外部 git 状态（会变），toolchain / ai_tools 查白名单与类别（静态 + 点击时复核）——但**「点得动才删得了」同样不该由前端说了算**：`clean_cache` / `clean_ai` **不信任前端传来的 id**，回到白名单 / 当前目录重查一遍，不在白名单或不可清理一律拒。`repos_clean_candidate_mirrors_deletion_judgment` 守 repos 半边，`toolchain_cleanable_rows_all_have_a_deletion_path` 与 `ai_tools_cleanable_rows_all_have_a_deletion_path` 守另两组。
- **`Item.id` 三组形态不同**：repos 是裸路径、toolchain 是 `toolchain:` 前缀、ai_tools 是 `ai:<类别键>:<工具根路径>`（类别键 ASCII，路径在第二个冒号之后，Windows 盘符不受影响）。`parse_cache_id` 去完前缀是空串必须拒——`PathBuf::from("")` 在文件系统语义上等于当前工作目录（这条写用例时真红过）。
- **执行清理必须人显式发起**：属暂停条件里的破坏性操作，门禁只验代码与确认流，**绝不真删**——测试不覆盖 `trash::delete` 成功路径（只验拒绝路径，真删留给真人）。`@{u}...HEAD` 的 `--left-right` 两列曾赋反（落后当未推送、未推送放行），已修并由 `repos_scan_reports_unpushed_direction_correctly` 钉住方向——**方向反了比不判更危险**。
- **前端无自动化行为测试**：只有 `vue-tsc` + 构建 + 代码审查。`canClean` 判据这类逻辑错 vue-tsc 抓不到。

### 5. AI 工具目录只读文件名，不读任何文件内容

`ai_tools.rs` 里只允许 `metadata()` 和目录遍历（`scan::dirsize` 的原生遍历），不允许 `fs::read` / `read_to_string` / `File::open`。凭据藏在哪根本枚举不完——`auth.json`、`config.toml`、`settings.json` 里都可能写着 key；逐个文件做内容过滤是堵不完的漏洞，不读内容则从根本上消除了这条路径：凭据压根不进入数据流，不是「读出来再抹掉」。`tests.rs::ai_tools_source_never_reads_file_contents` 用 `include_str!` 从源码层面守着（匹配带左括号的 `fs::read(` 以避开 `read_dir`）——比事后检查扫描结果更硬，挡得住后来者加一行读取。改这条测试前先想清楚。

### 6. 镜像源不能删

`src-tauri/.cargo/config.toml` 配了 rsproxy 稀疏索引（crates.io 直连在国内超时），`src-tauri/.gitignore` 里有对应 `!.cargo/` 反向规则。两条都得留着。

### 7. 自定义 button 的 cursor 和 focus 环在 base 层统一兜底

Tailwind v4 的 preflight 不给 `<button>` 设 `cursor: pointer`，而这里到处是没走 `Button` 组件的裸 `<button>`（侧栏项、排序表头、筛选 pill、每一行）。`style.css` 的 `@layer base` 对 `button:not(:disabled)` 统一挂 cursor 和 `focus-visible:ring-1 ring-ring`。**别挪进组件**：散在各组件一定漏，且 `Button` 组件自带焦点环，重复语义会打架。新增交互元素的默认假设是「已经有手型光标和焦点环了」。

### 8. 环境没装，就别注册这个 Scanner

`dev_env::dev_env_scanners()`、`wsl::wsl_scanners()`、`toolchain::ToolchainScanner::exists()` 各自判断环境存在与否，**不存在就不 push 进 `scanners()`**。WSL 还多一层平台判断——只在 Windows，其他平台直接返回空（不是「探不到」，是「不存在这东西」）。理由：侧栏不该出现 0 条目的分组，空 tab 只会让人以为工具坏了；等装上，分组自己会冒出来。新增这类 Scanner 照抄这个套路，**不要**返回 `items` 为空的分组再指望前端过滤。（Docker 原先也在这条里，已随 D6/H3 变成「未注册」而非「条件注册」，别按这条加回来。）

### 9. 跨平台路径：平台是**值**，不是 `cfg`；新增路径必须同时给出两侧

`scan/platform.rs` 定义 `Platform`，路径解析全部走「传入平台」而不是 `#[cfg(windows)]` 就地分叉。为什么：`cfg` 一写，测试就只能跑当前平台那一半；做成值之后，喂一个假 home + `Platform::Windows` 就能在 macOS 上断言整条映射链路。新增缓存源/探测位置时的硬规矩：

- 白名单条目两边一致的（`.cargo`、`.rustup` 这类 dot-dir）不用写两遍，落 `win_rel` 兜底 arm。
- **`Library/` 开头是 macOS 专有布局，必须在 `win_rel` 显式给出 Windows 位置或显式 `None`**（Homebrew 就是 `None`）。兜底 arm 有 `debug_assert` 拦着，`every_whitelisted_cache_rel_resolves_on_windows` 会把全部条目跑一遍触发它——漏写的后果是静默去探一个必然不存在的路径。
- 第三方工具的安装目录**给候选列表**（`platform::first_existing`），别写死一个——安装器不同路径就不同。`locations`（「扫了哪些位置」那行）也要按平台给。

**推错 Windows 路径的后果是可接受的**：探不到 → 该行/该分组不出现（漏显示），不会显示成错的数。这正是硬约束 8 的价值。但**别把没核过的路径当成已核实**——见「已知限制」。

**测试的覆盖面不该取决于跑测试的机器**。`Platform::current()` 在测试里等于把断言绑到跑测试的这台机器，两类坏法：**绑错平台 → 假红**（断言 macOS 布局的测试在 Windows runner 上必红——**断言哪套布局就显式钉哪个平台**）；**绑错平台 → 静默漏验**（更坏：read_dir 失败 → 断言全被跳过、测试照样显示 ok）。**口径不该经平台解析**的走 `kinds_for_test()`；**「本机根本跑不了」的探测**把「读」做成可注入参数（`wsl::wsl_scanners_with(platform, read)`），解析做成纯函数——**别因为『本机跑不了』就写成 `#[cfg(windows)]`**：那等于把整个分组交给一条从没被编译过的分支。区分标准：**「本机真实状态」的断言可以跟 `current()` 走，「某套布局/某条口径」的断言必须显式钉平台**。（各坏法的具体测试名与事故见 appendix。）

### 10. 前端动作只有「揭示位置」与「复制路径」，别加「打开」

动作面就两个：`reveal_path`（访达 / 资源管理器里选中它）与复制路径，都不写盘、不执行任何东西。**不做「打开」**——打开一个路径会执行它（`.app` / `.command` / 安装包都会被跑起来），与硬约束 5 是同一条理由；也别加「在终端中打开」「用默认程序打开」这类变体，它们都是执行。两个必须保留的守卫：

- **`Item.path` 不都是宿主机路径**：docker 的是 `docker://<镜像名>` 与 `docker://volume/<卷名>`（虚拟机里的东西，宿主上没有对应位置）。前端渲染动作的条件是 `isLocalPath()`（`/` 开头或 Windows 盘符）；Rust 侧 `reveal_path` 再判一次**存在性**——缓存会过期（条目被删、外置盘没挂上），后端判过才能给出「路径已不存在，重扫一次」这句人话。docker 分组已移出 V1，守卫要留着（`docker.rs` 文件仍在）。
- **不做分组级「打开根目录」**：只有 ai_tools 有单一根，toolchain 与 repos 的条目分散在不同父目录，做出来是不一致的半截功能。

入口两处：揭示位置在检查器动作条，复制路径只在行右键菜单（同一个 `openMenu()`）。菜单自己画的（两项不值得拉菜单库），收起条件是点别处 / 滚动 / Esc / 改窗口大小。

### 11. 版本管理器的行**永不标可回收**，也别改成「一个目录一行」

G3 起 toolchain 组里有版本管理器（nvm / fnm / volta / pyenv / conda / rbenv / asdf）的**逐个已装版本**。**逐版本成行是这套数据唯一的价值**：合成一行只剩一个总数，「9 个 Node 版本里哪个最占地方、哪些停了三年还留着」全看不见。**别给版本行填 `reclaimable`**——不是保守，是**算不出来**：要判某个版本能不能卸，得读遍各仓库的 `.nvmrc` / `.node-version` / `engines` / `.python-version` 再求交集，本工具不读文件内容、也没有跨仓库语义解析。报一个数会让人照着删掉还在用的版本——从「报不满」变成「报错」。`installed_versions_are_never_cleanable` 守着 `cleanable=false` 与 `reclaimable=None` 两条。**版本行与缓存行是两类东西，别合并**：`~/.nvm/.cache`（下载暂存，可回收）与 `~/.nvm/versions`（已装，不可回收）路径相邻、性质相反。

## 已知限制

逐条细节（路径映射、体积口径、各分组边界、WSL、PATH 兜底）在 [docs/known-limits.md](./docs/known-limits.md)——**改对应模块前必读**。最容易踩的结论：

- 「Git 仓库」的扫描根目录固定六个候选名，不可配；环境变量改位置（`NVM_DIR` 等）一概认不出来。
- Windows 路径多为按惯例推导，**CI 绿 ≠ 路径核对过**；推错只漏显示、不出错数。
- AI 工具与工具链缓存都是白名单，不是 glob；`vendor` / Ruby `vendor/bundle` 刻意不算构建产物。
- 体积口径是「占用空间」（Unix `st_blocks * 512`，Windows 暂退回逻辑大小），**单趟遍历内硬链接按 (dev, inode) 去重**（S4；跨行共享的硬链接各算各的，行加总可以大于全局唯一字节数）；不 shell out 到 `du`；60 秒超时是真超时。
- 总占用与可回收**有意报不满**；带出处的值只在 README「扫描策略」写一次，且随数据增长。
- 版本行永不标可回收；WSL 报体积但永不标可回收；Docker 未注册，其约 10.5 GB 永久不在账上。
- 调外部命令前先想 PATH（双击 app 只有 `/usr/bin:/bin:/usr/sbin:/sbin`）。

## 验证回路（harness 与 loop）

验证是工程设施，不是事后补的测试。**改什么就跑什么，红不往下走**。完整说明在 `docs/harness-and-loop.md`。

| 档位 | 命令 | 内容 | 耗时 |
|---|---|---|---|
| 内环 | `npm run verify:quick` | rustfmt + clippy（`-D warnings`）+ vue-tsc + 契约校验器自检 + 文档门禁 | 约 10 秒 |
| 外环 | `npm run verify` | 内环 + 测试（有 nextest 用 nextest，否则 `cargo test`）+ `vite build` | 约 20 秒 |
| 评估 | `npm run verify:full` | 外环 + 全量扫描 + 快照契约校验 | 约 45 秒 |

内环秒级、改完就跑，任何一层红就停；外环提交前跑，各层跑完再汇总；评估档改了扫描逻辑再跑。**非 TTY 且非 CI 时静音**：每层输出写入 `<git-dir>/verify-logs/<mode>.log`，终端只打 PASS 行与失败尾巴（H10）；需要细节看日志。nextest 只报失败，没装时自动回退 `cargo test --quiet`，不影响正确性。

**文档门禁（H13）**：`scripts/check-docs.mjs` 查常驻文档字符预算、archive 索引完整性（每刀/每决策有索引行、无孤儿/陈旧 ID）、活文档死链，自带坏样本自检；内环与 CI 都跑。预算超了要改数字并说明理由，不是删检查。

**两个去重机制（2026-10-17 起）**：
- **验证戳**：verify.sh 外环/评估档**全绿且工作区干净**时，往 `<git-dir>/gate-stamp` 写「树 hash + 档位」。pre-push 看到要推的每个引用都是同一棵树，就跳过本地重跑（远端 CI 仍全量跑）——外环档的刀不再「收口闸跑一遍、push 又跑一遍」。树变了、戳档位不是外环/评估档、删分支、空输入，一律保守回退真跑。
- **push 按 Track 攒批**，别每刀一推：每次 push 远端 CI 都全量跑，攒批减少重复；pre-push 有验证戳兜底，外环约 20 秒。

CI（`.github/workflows/ci.yml`）两个 job：**macOS** 跑外环全套 + 契约/文档门禁；**Windows** 只跑 `cargo fmt` / `clippy` / `test`，守 `#[cfg(not(unix))]` 分支有人编译、Windows 特有语义有人抓，**核不了路径对不对**。细节（含 Windows job 删 `.cargo/config.toml` 的原因）见 harness 文档。

**前端契约两边守**：`tests.rs::assert_frontend_contract` + `scripts/check-snapshot-contract.mjs`（自带坏样本自检）；不变量清单见 harness 文档。

按改动类型还要额外做的事：

- 改 Rust 数据结构 → `cache::SCHEMA_VERSION` +1，同步 `src/lib/types.ts`
- 新增 Scanner → 注册进 `lib.rs::scanners()`；把 id 登记进 `scan::GROUP_ORDER`（漏了会排在最后，`scanner_registration_follows_group_order` 会红）；更新 `scanner_registration_is_pinned` 的无条件名单（哨兵，不更新会绊倒你）；`every_scanner_survives_empty_home` 的构造列表加上它；SCHEMA_VERSION +1
- 新增测试 → 解析逻辑做成纯函数喂样本，**别让测试的通过与否取决于本机装没装某个软件**；断言某套布局或口径时**显式钉 `Platform::Mac` / `Platform::Windows`**（硬约束 9）

钩子（`core.hooksPath = scripts/hooks`，已装）：**pre-commit** = 私钥/证书双形态拦截 + 暂存 .sh 语法预检 + 内环（纯 .md 提交跳过）；**pre-push** = 外环（新鲜验证戳则跳过）。钩子是纵深，不替代双闸，也拦不住绕过验证直接报告完成。

## 工作节拍（给 agent 的硬规矩）

回路不是事后补救，是开发过程本身。**每一轮改动的完成定义是「绿了再报告」，不是「改完了」**：

1. 动手前先定这一轮对应哪层验证（看上表）：小改动内环、功能完成外环、扫描逻辑变更评估档。刀进行中即按此节拍迭代；收口由「工作循环」的双闸把关。
2. 改完**当场**跑那一层。红了就地修，修完重跑；同一层连续两轮修不下来，停下来报告卡点——不要带着红往下做新功能，也不准用弱化测试的方式变绿（删断言、放宽容差、`#[allow]` 兜底都算）。
3. 报告里必须带上当次验证的实际结果（哪几层、PASS/FAIL），不允许只描述改了什么。
4. 能先写测试的先写测试：解析逻辑、口径边界都值得让回路从「改完再验」变成「验着改」。
5. 钩子与 CI 是机器层的兜底，但拦不住没提交的半成品、也拦不住「绕过验证直接报告完成」——第 2、3 条是 agent 自己的责任。

## 上下文纪律（省 token）

- 查历史与决策先 `grep` 索引 `docs/backlog-archive.md`（每刀/每决策一行），再读 `docs/archive/` 对应文件、用 `sed -n 'a,bp'` 只读命中区间；**别整读** archive 正文与 `tests.rs`（上千行）。
- 验证/测试的成功日志不往上下文里粘：`npm run verify` 默认静音，先看 PASS 行与失败尾巴；要细节再读 `<git-dir>/verify-logs/`。
- 报告只给命令、时间、关键数字与结论，不复制完整输出。

## 工作循环（刀 / 闸 / 门禁）

目的：agent 连续执行、人不再逐轮敲「继续」，同时每一步可机器校验。**工作单元是「刀」**。

- **取刀清单**：`docs/backlog.md` 是跨会话的唯一真相，**只装活的**——未收口的刀、Track 级门禁、当前待确认、未挂刀的遗留。已收口的刀（含收口说明）、已拍板的决策、历史勘测的**索引**在 `docs/backlog-archive.md`，正文按 Track / 决策号在 `docs/archive/`。每把刀带描述、门禁档位、状态勾选、commit 号回填。新刀先登记再开工；【待确认】拍板前不得开工。
- **刀的完成定义 = 收口闸通过 + commit 落盘 + backlog 勾选回填 commit 号**。收口 commit 同时把「当前持刀」清空。
- **两个闸门（`scripts/verify-cut.sh`）**：
  - **取刀闸** `scripts/verify-cut.sh --pre [刀号]`——刀存在且 open、按序（文件序中先于它的正式刀已收口）、持刀锁空闲、已勾选条目回填真实存在的 commit、门禁档位可读且合法、Track 无未拍板的开工前置、工作区干净。任一不过即拒绝开工，**不得绕过**。
  - **收口闸** `scripts/verify-cut.sh [--cut 刀号]`——**脏态（有未提交变更）直接列出待提交清单并停，不烧档位验证**；干净态跑门1（按该刀登记的档位完整跑 `verify.sh`）+ 门3 硬校验（已勾选 + 回填真实 commit + 持刀锁已清空 + 证据存档在位）。想在提交前验证，直接跑 verify.sh 对应层——工作节拍本来就要求改完当场跑。
- **刀与刀之间连续执行**：收口后跑取刀闸取下一把未收口的刀继续，不要停下来等待确认。
- **仅在以下情况暂停**：①破坏性操作——任何触碰废纸篓/删除/扫描之外的写（执行清理必须人显式发起，门禁只验代码与确认流）；②需要拍板的【待确认】；③连续两轮修不绿同一门禁；④需要外部凭据或用户操作；⑤口径/契约变更（SCHEMA_VERSION、可回收边界、fields 约束）——可以做，但报告里必须单独说明理由。
- **【待确认】三段模板**：①阻塞点（等谁、拍什么）②可选项 + 各自后果 + 默认建议 ③拍板后影响的刀序。让人 30 秒拍完。
- **backlog 正式刀跑空时：勘测自动、下刀等人**——勘测下一阶段并挂【待确认】，然后停下等拍板，不得自行新增刀并直接开工。
- **证据落盘**：评估档的刀收口时把 `npm run verify:full` 输出存档到该刀登记的 `证据:` 路径（命令 + 时间 + 结果摘录）。机器取证是硬门，存档是人的底稿。
- **backlog 维护纪律**：完成即勾选并附 commit 号；持刀锁开工登记、收刀清空；`verify-cut.sh` 自身的修改独立 commit，不与刀的代码混提。**归档纪律（2026-10-17 起）**：已收口刀与已拍板决策及时搬去 `docs/archive/`（Track 文件 / `decisions/dNN.md`），并在 `docs/backlog-archive.md` 索引加一行；backlog 只留活内容——文档是每个会话都要反复支付的上下文成本（首归档 88 KB → 6 KB；H12 再把 archive 正文拆出，索引 4.5k 字符），别让它只增不减。

## 常用命令

```bash
npm install && npm run tauri dev          # 开发
npm run verify:quick                      # 内环：改完就跑（秒级）
npm run verify                            # 外环：提交前
npm run verify:full                       # 评估档：全量扫描 + 契约校验
cd src-tauri && cargo test                # 只跑 Rust 测试（本机真实数据，只读不写）；只跑某组 cargo test <关键字>
cd src-tauri && cargo run --example dump  # 扫一次写缓存再读回，输出 JSON 供人工核对
```

测试目前 114 passed + 2 ignored（约 6 秒；ignored 的是对照 bench `bench_native_size_vs_du` 与真实数据守恒检查 `repo_nesting_real_data_stays_conserved`）。**用例数随刀增长，改了扫描逻辑顺手更新这个数**——它过期时最容易骗人。**扫描逻辑的测试优先喂临时目录、不依赖本机装了什么**，且**测试要自己写一份独立实现当基准**（`tree_bytes`），别拿被测代码当 oracle——两边一起错会一起绿；反过来，扫真实数据的用例容差要按「数据是活的」给，不能用它证明精度。各 Scanner 的测试思路范例（docker 喂真实 CLI 输出、toolchain 全喂假 home、WSL 读取走注入——「本机不注册」不等于「没测」）见 appendix。

## 代码风格

- Rust 注释解释**为什么**，不解释代码在做什么。现有的决策注释（启动策略、untagged 的坑）是范例。
- 前端 UI 走紧凑路线：小字号、无横向滚动条、不显示冗余信息。主从结构（列表 + 检查器）替掉了「行内展开」，工具栏的维度筛选是弹出菜单不是一排 pill——**别把筛选塞回标签行**。侧栏 208px / 检查器 252px / 行高 26px（分组视图；总览分组行放大一档）/ 控件 24px 是定下的密度，改前先看 `src/views/Assets.vue`。视觉身份「纸账本」（U14/U15）：配色只出自 `style.css` token（暖纸/墨，组件不挑 Tailwind 色号），数字用正文字体 tabular（mono 只给分支/路径），绿只经 `--reclaim` 表「可回收」；行界靠 1px 行线，**不画行背景条、不做药丸标签、侧栏不放图标、UI 不自我解释**——都是生成式界面指纹，删过别加回来。动配色先跑 `node scripts/contrast.mjs`；改 UI 用 `scripts/render-preview.mjs` 截图自验。
- **不留模块级 `#![allow(dead_code)]`**。它连整棵子树的死代码警告一起咽（`ssh.rs::_unused` 曾在里面躺几个月，clippy 对整棵 `scan/` 瞎）。要留一个暂时没有调用者的项，就在**那一项**上加 `#[allow(dead_code)]` 并写清为什么（`platform.rs` 的 `WIN_LOCAL` / `WIN_ROAMING` 是范例）。
- **只在测试里用的辅助一律 `#[cfg(test)]`**，别用 `allow(dead_code)` 兜（`ToolchainScanner::on` 是范例）。
- 删一个函数后记得看它的 `use`：`cargo clippy --all-targets` 会报出失效的 import，但只在**重新编译**那个 crate 时才报。
- **文档、证据、代码注释与测试夹具不写本机可识别的真实名**（仓库名、用户名、公司缩写、个人路径），用 `repo-a` 这类占位；体积与版本号可留。H15 开源前清过一遍，别再写回来。

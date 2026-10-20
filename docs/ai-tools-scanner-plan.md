# AI 工具目录 Scanner 实施计划

给 Stacktake 增加一个 `AiToolsScanner`，清点本机所有 AI 编码工具的数据目录。

文档里的数字都是本机实测（2026-10-11，当晚复核过一遍），不是估算。

> **现状声明（2026-10-13 补，backlog D8 拍板选 A）**：本文是 **2026-10-11 `du` 时代**的设计记录，**历史正文原样保留、不改写**。其中 9 处 `du` 表述（文首优先级论述、「安全红线」第 1 节两处、「实施步骤」的软链与 `dir_size_fast` 段、「测试」的体积守恒条、「性能」表两行、「验收标准」末条）已被 **W1**（commit `f076253`，`du` 换成 `scan/dirsize.rs` 原生遍历）取代，**别照着它们写新代码**：
>
> - 「安全红线」第 1 节那句「只允许 `metadata` 和 `du` 子进程」与现行约束**正相反**。现行权威是 AGENTS.md 硬约束 5 与 `ai_tools.rs` 模块注释：只允许 `metadata()` 与 `read_dir`，整棵树已无 `du` 子进程，`tests.rs::ai_tools_source_never_reads_file_contents` 从源码层守着。
> - 「实施步骤」里 `repos.rs::dir_size_fast` 那段「20 秒假超时、不会杀进程」同属被 W1 修掉的旧状态（现为每层查截止、60 秒真超时）。该函数还在（`dirsize` 的薄封装，`ai_tools.rs` 仍在用），只有超时那句是错的。
> - 性能与条目数是当天快照：「AI 工具数据」组名 G1 已改为「编辑器与 AI 工具」（条目 49 → 59），「`~/Library/Application Support/` 第一版不扫」这个决定也已被 G1 推翻（VS Code / JetBrains 数据已并入该组）。
>
> 仍照旧有效的部分：按语义分类（不按工具）、只读文件名不读内容、配置与未知类别永久不可清理——这三条与现行实现一致。

## 为什么先做这个

本机装了 9 个 AI 编码工具（共 10 个路径），它们的数据目录合计 **15.2 GB**，比 Git 仓库侧更有清理价值，但完全没有人管：

| 工具 | 路径 | 体积 |
| --- | --- | --- |
| WorkBuddy | `~/.workbuddy` | 5.9 GB |
| Codex | `~/.codex` | 4.6 GB |
| Windsurf | `~/.windsurf` | 1.2 GB |
| Qoder | `~/.qoder` | 1.2 GB |
| Trae | `~/.trae` | 863 MB |
| opencode | `~/.local/share/opencode` | 981 MB |
| Cursor | `~/.cursor` | 433 MB |
| Claude | `~/.claude` | 61 MB |
| Gemini | `~/.gemini` | 44 KB |

另外 opencode 的配置在 `~/.config/opencode`（6.3 MB），数据在数据目录，两个都要看。

按之前定的优先级，这个排第一是因为它和现有 `RepoScanner` 完全同构——读目录、`du`、打标签，改造成本最低。Docker 需要走 CLI 拿分层占用（`du` 拿不到镜像共享层），MCP 配置要做跨文件关联改动数据模型，都排在后面。

## 现状：钱花在哪了

逐个一级子目录拆开，15.5 GB 里绝大部分是历史数据：

| 构成 | 体积 | 属于 |
| --- | --- | --- |
| 日志 / 追踪 | 2.7 GB | WorkBuddy（logs 1.9G + traces 801M） |
| 会话历史 | 2.5 GB | Codex（sessions 1.6G + archived_sessions 831M） |
| 编辑器扩展 | 1.2 GB | Windsurf extensions |
| 日志 | 898 MB | opencode log |
| 扩展 / 技能 | 2.4 GB | Trae 863M、Qoder 791M、Cursor 426M |
| 运行时二进制 | 946 MB | WorkBuddy binaries |
| SQLite 数据库 | 1.3 GB | Codex logs_2.sqlite 838M、thread_history 418M |

值得注意的两点：

- **Codex 的日志是以 SQLite 形式存的**（`logs_2.sqlite` 838 MB、`thread_history_1.sqlite` 418 MB），不是普通文本文件。这意味着「清日志」不等于删 `.log` 文件。
- **一级散文件占 1.3 GB**，其中单个文件就有 838 MB。只扫子目录会漏掉这部分，Codex 的体积会少近 1 GB。

## 核心决策：按语义分类，不按工具

九个工具的目录命名毫无共性——同一个东西，WorkBuddy 叫 `logs`，opencode 叫 `log`；会话历史有的叫 `sessions`，有的叫 `projects`，有的叫 `file-history`。

所以不能按工具建模（每个工具写一套字段），也不能按目录名硬套（会漏）。折中办法是抽一层**语义类别**：

| 类别 | 典型目录名 | 能否清理 | 理由 |
| --- | --- | --- | --- |
| 日志 | `logs` `traces` `debug` `*-otel` `*.log` `history.jsonl` | 可 | 纯历史，工具下次启动会重建 |
| 会话 | `sessions` `projects` `file-history` `memory` | 可（提示隐私） | 含对话内容与代码片段 |
| 缓存 | `cache` `snapshots` `paste-cache` `tool-output` `blobs` `tmp` | 可 | 可从源重建 |
| 技能 | `plugins` `skills` `extensions` `commands` `agents` | 可（需重装）· 第一版不开放 | 删了要重新安装，但不是数据 |
| 索引 | `*.sqlite` `*.db` `*-wal` `*-shm` | 谨慎 | 是数据库，直接删可能让工具状态错乱 |
| 配置 | `config.*` `settings.*` `*.json` `*.toml` `*.md` `auth.*` | **不可** | 含 MCP 配置与凭据 |
| 运行时 | `bin` `binaries` `app` `entry` `models` | 不可 | 删了工具跑不起来 |
| 未知 | 兜底 | **不可** | 保守默认 |

分类决定可清理性，工具只作为归属标签。这样做的好处是筛选「可清理」能跨工具一次看全，而不是在每个工具下面分别判断。

### 分类器实测覆盖率

实现完跑了一遍本机（`cargo run --example dump`），落库 **49 个条目**（工具 × 类别），合计 15.2 GB：

| 构成 | 体积 | 可清理 |
| --- | --- | --- |
| 日志 | 3.8 GB | 是 |
| 会话 | 2.8 GB | 是（提示隐私） |
| 缓存 | 386 MB | 是 |
| 技能 | 2.9 GB | 否，删了要重装 |
| 运行时 | 1.5 GB | 否 |
| 索引 | 1.6 GB | 否，数据库文件 |
| 配置 | 0.7 MB | 否，含凭据 |
| 未知 | 1.4 GB（77 项） | 否 |

**可回收 6.95 GB**。写计划时预估的是 10.6 GB，那是按「技能类也可清理」算的；实施时定了技能类第一版不开放，所以降到这里。

未识别里最大的几块，是规则迭代的候选：

| 未识别目录 | 体积 | 应该是 |
| --- | --- | --- |
| `~/.workbuddy/edge-ksb-profile` | 519 MB | 浏览器 profile，缓存 |
| `~/.workbuddy/workspace` | 374 MB | 工作区临时数据 |
| `~/.workbuddy/file-tree-manifests` | 123 MB | 缓存 |
| `~/.codex/computer-use` | 70 MB | 日志 |

计划里原本列在这里的 `~/.codex/sqlite`（293 MB）和 `~/.codex/.tmp`（256 MB）已被第一版规则覆盖——目录名精确匹配到「索引」和「缓存」，不用等后续迭代。

覆盖率不必追求完美。**未知一律不可清理**，所以漏识别的代价只是少显示一块可回收空间，不会误删。规则可以后续迭代补。

## 数据模型

### Item 粒度：工具 × 类别

不是每个工具一行，也不是每个子目录一行，而是两者的组合。

对比三种粒度：

| 粒度 | 条目数 | 问题 |
| --- | --- | --- |
| 每个工具 | 10 行 | 只看到「WorkBuddy 5.9 GB」，不知道哪块能删，不敢动手 |
| 每个子目录 | ~250 行 | 太碎，大量几 KB 的配置文件淹没真正的大块 |
| **工具 × 类别** | ~45 行 | 直接呈现「WorkBuddy 日志 2.7 GB 可清理」 |

第三种粒度刚好落在决策点上——用户要决定的是「删不删这一块」，而不是「删不删这个工具」。

### 字段定义

沿用现有 `AssetGroup` / `Item`，数据结构不用动。但有三处前端约束必须先知道，否则做了也看不见。

`fields`——**最多三个**。`Assets.vue` 里是 `fields.slice(0, 3)`，第四个字段不会进表头也不会进列。所以只留这三个：

| key | 标签 | 类型 | 说明 |
| --- | --- | --- | --- |
| `size` | 体积 | Bytes | 该工具该类别下所有条目的体积之和 |
| `last_activity` | 最后活动 | Int (temporal) | 条目 mtime 的最大值 |
| `kind` | 类别 | Text | 日志 / 会话 / 缓存 / 技能 / 索引 / 配置 / 运行时 / 未知 |

`name`：`工具名 · 类别`，例如 `WorkBuddy · 日志`。前端检测到 ` · ` 分隔符就按前缀分节渲染：工具名做小节标题（右侧显示该工具合计体积），行名只显示类别。第一版是每行重复工具名的平铺，截图看了一眼就改了——8 行「WorkBuddy ·」是纯噪音。

`tags`：只有可清理时的 `可清理` 一个。类别不做成标签——分节行名已经显示了，重复；工具名更不能做，9 个工具加 8 个类别十几颗 pill 会挤爆单行 tab。

`detail`：构成清单。`Item.detail` 是 `HashMap<String, String>`，Rust 序列化顺序不确定，前端按对象键序直接渲染，**做不到按体积降序**。最小改法是键名前缀化，排序在生成时做完：

```
1. logs        1.9 GB
2. traces      801 MB
3. ai-otel     12 MB
条目            12
```

条目数（该类别聚合了几个子目录或散文件）也塞进 detail，占一行。要改成真正有序就得把 `detail` 换成 `Vec`，那是动 `Item` 结构，得连带 `SCHEMA_VERSION` +1，第一版不值得。

`reclaimable` / `cleanable`：类别可清理时置位。

### 散文件怎么办

一级散文件（不是目录）同样要统计，否则和工具总量对不上。按文件名和扩展名分类：

- `logs_2.sqlite`、`thread_history_1.sqlite` → 索引类
- `*.json`、`config.toml`、`auth.json` → 配置类
- `history.jsonl` → 日志类
- `*.tmp-*`、`.bak` → 缓存类

这些文件汇总到所属工具的对应类别里，不单独占一行。

## 安全红线

这部分比功能本身重要。

### 1. 只读文件名，不读任何文件内容

这是整个 Scanner 最重要的约束，也是它比「过滤敏感词」更稳的地方：

- 我们**只**做 `du`（拿体积）和 `stat`（拿 mtime、判断是不是目录）
- 分类依据是**文件名**，不是文件内容
- 因此凭据根本不进入数据流——不是「读出来再抹掉」，而是压根不读

这一点很关键，因为凭据藏在哪根本枚举不完。本机已知的就包括 `~/.local/share/opencode/auth.json`、`~/.claude/config.json`、`~/.gemini/settings.json`，而 Codex 的 `config.toml` 里也可能写着 key。逐个文件做内容过滤是堵不完的漏洞；不读内容则从根本上消除了这条路径。

对应到实现：`AiToolsScanner` 里不允许出现 `fs::read` / `read_to_string`，只允许 `metadata` 和 `du` 子进程。

### 2. 配置和未知类别永久不可清理

`cleanable` 只对日志、会话、缓存三类置位（技能类见文末「已定的两个决定」）。索引类体积大（Codex 1.3 GB），但删数据库文件可能损坏工具状态，先标「谨慎」不开放清理。

索引类要在 `notes` 里点破一层：`~/.codex/logs_2.sqlite` 名字带 `logs`，内容是日志，载体却是 SQLite。用户看到 838 MB 不能删会困惑，得写明「名义是日志，实为数据库文件，删了可能损坏工具状态」。

**不要覆盖 `Scanner::supports_clean()`**。它默认返回 `false`，`RepoScanner` 也没覆盖——这个标志是给前端判断「有没有清理入口」用的，而现在根本没有 clean command 注册，返回 `true` 等于宣称有这个能力却点不动。照抄 `RepoScanner`：标志留 `false`，只在 `Item` 上置 `cleanable` / `reclaimable`，前端已有的「可回收 X · 清理入口待启用」那行会自己出现。

### 3. 会话类要显式提示隐私

会话数据含完整对话记录和代码片段。删掉不可逆，而且用户可能想保留。所以 `notes` 里要写清楚，不能和其他缓存类混为一谈。

### 4. 清理走废纸篓

沿用 `trash::delete`，不 `rm -rf`。这条和 `RepoScanner` 一致。

### 5. 工具清单是白名单

只在已知的 10 个路径里扫，不 glob `~/.*`。否则会误伤 `.ssh`、`.aws` 这类同结构的敏感目录。

## 实施步骤

### 第 1 步：`src-tauri/src/scan/ai_tools.rs`

新增文件，实现 `Scanner` trait。

```
struct AiToolsScanner { tools: Vec<Tool> }
struct Tool { label: String, root: PathBuf }
```

主要函数：

- `known_tools()` — 返回工具清单，路径不存在就跳过
- `classify(name: &str) -> Category` — 纯函数，**无条件覆盖测试**
- `scan_tool(tool) -> Vec<Entry>` — 遍历一级条目，跳过符号链接
- `aggregate(entries) -> Vec<Item>` — 按 (工具, 类别) 聚合成 Item

注意两点：

- `entry.file_type()` 要判断 symlink：`.tmp`、`.bin` 这类隐藏软链如果当目录 `du`，会跟着遍历到别的地方去。
- 体积统计复用 `repos.rs::dir_size_fast`（改成 `pub(crate)` 共用，别复制一份）。它那个 20 秒「超时」不是真超时——`du` 跑完之后才看 elapsed，不会杀进程。AI 工具目录层级浅，用不上，但别指望它兜底。

### 第 2 步：注册

`lib.rs::scanners()` 里加一行：

```rust
v.push(Box::new(AiToolsScanner::new()));
```

顺序放在 SSH 和 Git 仓库之后。前端不用改——前提是上文那三条前端约束（字段 ≤ 3、detail 键名前缀、tags 不放工具名）都遵守了。

### 第 3 步：缓存版本 +1

不做这一步，新分组就是隐形的。

`bootstrap` 的逻辑是「有缓存直接返回，不扫描」。老用户启动后拿到的是只含 SSH 和 Git 仓库两个分组的旧快照，「AI 工具数据」不会出现，界面上也没有任何提示告诉他们该点「重扫」。

所以 `cache::SCHEMA_VERSION` 从 1 改到 2。老缓存版本不匹配 → 读不出来 → 走首扫路径 → 一次性多花约 36 秒，换一个新分组一定可见。这代价只付一次，之后恢复正常吃缓存。

`AGENTS.md` 里写的是「改 `AssetGroup` 结构时 +1」。这次属于分组集合变了，不是结构变更，但失效机制是同一个，一样要用。

### 第 4 步：测试

`scan/tests.rs` 里加（`cargo test` 目前 14 个用例，跑的都是本机真实数据）：

1. **分类器单元测试**（重点）— fixture 覆盖各类别典型名字，断言未知名字落到 `未知`
2. **凭据不泄漏** — 两层断言，见下
3. **体积守恒** — 各类别体积之和 ≤ 工具根目录实际大小，防止重复计算。必须用 `<=` 并留缺口：符号链接被跳过、`du` 的块对齐都会让各类别之和小于根目录实测值，写成相等会假红
4. **符号链接安全** — 遇到软链目录不跟随
5. **清理标志** — 配置类、未知类、技能类 `cleanable` 必须为 false

第 2 条分两层：

- 结果层：扫描结果序列化后的 JSON 里不应包含 `PRIVATE KEY`、`api_key`、`token=` 一类片段。按设计不可能出现，但能挡住后来者不小心引入文件读取。
- 源码层：用 `include_str!("ai_tools.rs")` 把源文件读进来，断言里面不出现 `fs::read` / `read_to_string` / `File::open`。这条比结果层更硬——它守住的是「压根不打开文件」这个设计约束本身，而不是事后检查漏没漏。

### 第 5 步：文档

更新 `README.md` 的「做什么」和「已知限制」，补上这一节的性能数据与分类规则。

## 性能

已实测（`cargo run --example dump`），成本远低于 Git 仓库侧：

| 操作 | 耗时 |
| --- | --- |
| AI 侧：遍历一级条目 + `du`（并行 12） | 约 2.7 秒 |
| 对照：Git 仓库侧 `du` 238 个仓库 | 约 33 秒 |

原因是 AI 数据目录虽然大，但层级浅、不涉及 `node_modules` 那种百万级文件树。

用 `rayon` 并行、并行度默认取 CPU 核数（本机 12）即可，不需要为它单独调参。

加进去之后全量扫描从 33.5 秒变成 **36.2 秒**，可接受。结合上一轮改的「默认吃缓存」策略，这个成本只在首次运行和手动重扫时才付出。

## 已定的两个决定

**1. 技能类第一版不开放清理**

本机技能加起来 2.4 GB（Windsurf 1.2G、Trae 863M、Qoder 791M、Cursor 426M）。语义上它确实可清理，但删了要重装，用户未必知道自己在删什么。

所以分类照做，`cleanable` 置 false，`notes` 里写「可重装，清理入口启用后再开放」。等清理功能真正上线时再决定放不放开。

**2. `~/Library/Application Support/` 第一版不扫**

Cursor、Windsurf 这类 Electron 应用通常会把缓存放在那里，但本机 `~/.windsurf` 只有 `extensions` 和 `argv.json` 两个条目，主体数据在别处。

范围先收窄到 `~/` 下的点目录加 `~/.local/share/opencode`。跑起来看结果再决定要不要扩。

## 验收标准

- 10 个工具全部出现在侧栏的「AI 工具数据」分组里
- 把 `SCHEMA_VERSION` 改回 1 造一份旧缓存放进应用数据目录，启动后分组照样出现（验证第 3 步真的生效）
- 体积与 `du -sk` 实测值一致（误差在 du 精度内）
- 筛选「可清理」的总量与分类器实测一致。当前 6.95 GB（49 个条目、合计 15.2 GB），这个数会随分类规则迭代变化，别写成硬断言
- 配置类（`~/.codex/config.toml`、`~/.gemini/settings.json` 所在类别）、技能类、未知类都不出现在可清理结果中
- 表头只有三列，条目数在 detail 里，构成清单按体积降序
- `cargo test` 全过，凭据不泄漏的两层测试都通过

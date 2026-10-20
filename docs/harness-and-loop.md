# Harness 与 Loop Engineering

这个仓库把「验证」当成工程设施来建，不是写完代码再补测试。两份东西：

- **Harness（验证脚手架）**：一条命令 `scripts/verify.sh`，分层跑完所有检查；契约不变量由测试和脚本双重守着，换手、改结构、加扫描器都不会静默漂移。
- **Loop engineering（循环工程）**：规定「改什么就跑什么」的回路，让每一轮改动在最短时间内得到确定反馈。红不往下走。

## 三层回路

| 档位 | 命令 | 内容 | 耗时 | 什么时候跑 |
|---|---|---|---|---|
| 内环 | `npm run verify:quick` | rustfmt + clippy（`-D warnings`）+ vue-tsc + 契约校验器自检 + 文档门禁 | 约 10 秒 | 每次改完代码 |
| 外环 | `npm run verify` | 内环 + 测试（有 `cargo-nextest` 用 nextest，否则 `cargo test`）+ `vite build` | 约 20 秒 | 提交前、合入前 |
| 评估 | `npm run verify:full` | 外环 + 全量扫描 + 快照契约校验 | 约 45 秒 | 改了扫描逻辑、准备发布 |

外环各层全部跑完再汇总失败信息——已经花了十几秒，一次拿全比逐层刹车划算。内环相反，任何一层红就停，不浪费后面的时间。

**静音输出（H10，2026-10-17）**：非 TTY 且非 CI 时（agent 调用、重定向到文件），每层完整输出写入 `<git-dir>/verify-logs/<mode>.log`，终端只打 `PASS <层> (<耗时>)`；失败时打该层末尾 30 行 + 日志路径，退出码语义不变。TTY 与 CI 保持流式（人要看进度、CI 要完整日志）。nextest 加 `--status-level fail --final-status-level fail` 只报失败，无 nextest 时 `cargo test --quiet`。

CI（`.github/workflows/ci.yml`）两个 job：**macOS（全量门禁）**跑外环全套 + 契约/文档门禁——前端那套只在这里跑，干净环境下真实数据类测试自动跳过（各扫描器有 `present()` / `exists()` 守卫），纯函数与契约测试全量执行；**Windows（Rust 侧）**只跑 `cargo fmt` / `clippy` / `test`——它守的是 `#[cfg(not(unix))]` 分支有人编译过、只在 Windows 上暴露的语义差异有人抓（如 `cache::save` 的 rename 覆盖靠 `MOVEFILE_REPLACE_EXISTING`，Unix 上写十遍也验不到），**核不了路径对不对**。Windows job 会先 `rm` 掉 `src-tauri/.cargo/config.toml` 的工作区副本——rsproxy 给国内开发机用（AGENTS 硬约束 6，仓库那份不能删），美国 runner 走它反而慢。

## 契约：两边守同一份

前端只认 `AssetGroup`，渲染假设写在 `src/lib/types.ts` 和 `Assets.vue` 里。这些假设现在是收口的契约，Rust 和 Node 各守一半：

| 不变量 | Rust 侧 | Node 侧 |
|---|---|---|
| `fields` 最多三个（`Assets.vue` 是 `slice(0, 3)`） | `assert_frontend_contract` | `check-snapshot-contract.mjs` |
| `MetricValue` 带 `t` 标签且与值类型匹配 | serde 往返测试 | 同左（JSON 层） |
| `reclaimable` 必须标 `cleanable` 且不超过 `size` | 同左 | 同左 |
| 名称带 ` · ` 时分节前缀非空 | 同左 | 同左 |
| 有 `error` 就没有明细行 | 同左 | 同左 |
| `detail` 是字符串到字符串的映射 | 同左 | 同左 |

Node 侧多一样本事：直接验 dump 出来的真实 JSON，能抓住 Rust 类型系统看不见的序列化问题。它自带 `--self-test`（12 个故意构造的坏样本，每个都必须被抓住），每次内环都跑——校验器自己挂了比没有校验器更危险。

```bash
node scripts/check-snapshot-contract.mjs <snapshot.json>   # 校验 dump 输出或缓存文件
node scripts/check-snapshot-contract.mjs --self-test       # 自检
```

软警告（不挡通过）：声明了字段但所有条目都没有对应指标（该列会空白）、metrics 为空的条目。`--strict` 可把警告升级为失败。

## 文档门禁（H13）

`scripts/check-docs.mjs` 把「文档也是每会话成本」落成机器检查，和契约校验器同一个模式：

- **预算**：`AGENTS.md` ≤17k 字符、`docs/backlog.md` 与 `docs/backlog-archive.md`（索引）各 ≤8k。预算是天花板不是目标值；要加内容先改数字并说明理由，别删检查。
- **索引完整性**：每把已收口刀 / 每个决策在 `docs/backlog-archive.md` 有一行；索引链接可达；`docs/archive/` 无孤儿文件、无重复 ID、无陈旧 ID。归档时漏加索引会被它抓住。
- **死链**：AGENTS / README / backlog / 索引 / harness / known-limits 里的相对链接必须可达。
- **自检**：12 个坏样本（超预算、漏索引、断链、孤儿、重复、陈旧……）每次先跑；校验器自己挂了比没有更危险。

接在内环与 macOS CI。新增 Track 字母时不需要改脚本——它从 archive 正文收集刀号，只要求索引段里每条以 `**刀号` 开头。

## 回路表：改什么就跑什么

| 改动 | 要跑 | 还要做 |
|---|---|---|
| Rust 扫描逻辑 | 内环 + `cargo test <关键字>` | 解析逻辑做成纯函数喂样本，别让测试取决于本机装没装某个软件 |
| Rust 数据结构（`AssetGroup` / `MetricValue`） | 外环 | `cache::SCHEMA_VERSION` +1；同步 `src/lib/types.ts` |
| 增删 Scanner | 评估档 | 注册进 `lib.rs::scanners()`；`scanner_registration_is_pinned` 的名单同步；SCHEMA_VERSION +1；`every_scanner_survives_empty_home` 的构造列表同步。**档位是 评估档而不是外环**：增删都会改变快照的分组集合，而「组数与条目数对不对」只有全量扫描 + 契约校验拿得到（H3 移除两个 Scanner 时，`344 → 339` 这个数就是评估档给的；登记成外环会漏掉这层核对） |
| 前端组件 | 内环 + `npm run build` | — |
| 准备发布 / 大改扫描器后 | 评估档 | 人工核对 dump 的 JSON |
| 环境相关行为（PATH、daemon 探测） | 外环 | 第三方 CLI 兜底候选路径，见 `AGENTS.md` 硬约束 |

## git 钩子（可选）

```bash
bash scripts/hooks/install.sh      # pre-commit 跑内环，pre-push 跑外环
bash scripts/hooks/install.sh --uninstall
```

用 `core.hooksPath` 指向 `scripts/hooks`，不碰 `.git/hooks`。单次绕过：`--no-verify`。
pre-commit 做三件事：私钥/证书双形态拦截（文件名 + PEM 甲胄头，本项目安全身份唯一缺的门）、暂存 `.sh` 语法预检、内环。**纯 `.md` 提交跳过内环**（2026-10-17 起）——fmt/clippy/tsc 验不到文档，而流程 commit（登记/回填/文档）每刀至少两个，别为纯 docs 改动等一轮构建；任何非 `.md` 文件入暂存照常全跑。

**验证戳（2026-10-17 起）**：verify.sh 外环/评估档全绿且工作区干净时，往 `<git-dir>/gate-stamp` 写「树 hash + 档位」；pre-push 看到要推的每个引用都是同一棵树就跳过本地重跑（远端 CI 仍全量跑）。这是「外环档的刀收口闸跑一遍、push 又跑一遍」的去重：收口闸跑完立刻 push 时，外环一秒都不用再等。push 按 Track 攒批效果更好——戳只在「最后一次 commit 之后没再动过」时命中。

## 工作循环（刀 / 闸 / 门禁）

参照另一个本机工作区的成熟契约适配，2026-10-12 接入。上面三层回路管「一次改动怎么验」，这一层管「多刀之间怎么连续推进」——agent 收口即取下一刀，人不再逐轮敲「继续」。

- **`docs/backlog.md` 是跨会话唯一真相**：每把刀带描述、门禁档位（`内环`/`外环`/`评估档`）、状态勾选、commit 号回填；顶部一行持刀锁。【待确认】区放决策请求（三段模板：阻塞点 / 选项+后果+默认建议 / 拍板后刀序）。
- **双闸 `scripts/verify-cut.sh`**：
  - 取刀闸 `--pre [刀号]`：刀 open、按序、锁空闲、已勾选的刀回填真实 commit、**门禁档位可读且合法**、**所在 Track 没有「开工前置」的未拍板门禁**、工作区干净。不过即拒绝开工。
  - 收口闸 `[--cut 刀号]`：**门3 先判工作区**——脏则列待提交清单并停（不烧档位验证；想在提交前验证就直接跑 `verify.sh` 对应层）；干净则跑门1（按该刀登记的档位完整跑 `verify.sh`）+ 硬校验「已勾选 + 回填真实 + 锁已清空 + 证据存档在位」。
- **两条容易被忽略的登记规则**（都由真实事故换来的，写在 `docs/backlog.md` 文件头）：
  - `门禁:` / `证据:` 必须写在**条目首行**——脚本只读第一行，写在续行等于没登记。
  - 整个 Track 被未拍板决策挡住时，在 Track 头下写 `> 开工前置：【待确认】Dn`。**只写散文说明不够**：脚本认文件序，不认「这节被挡着」，裸 `--pre` 会把它取走并登记锁（2026-10-13 实测）。
- **一刀完成 = 收口闸绿 + commit 落盘 + backlog 勾选回填**。收口 commit 同时清空持刀锁。
- **暂停条件**（只有这些能拦住连续执行）：破坏性操作（清理入口属此类，执行必须人显式发起，门禁绝不真删）、【待确认】未拍板、连续两轮修不绿、需要外部凭据、口径/契约变更（可做但报告须单独说明理由）。

日常节奏：`scripts/verify-cut.sh --pre` 取刀 → 按工作节拍迭代 → 改完跑对应档位 → commit（勾选+回填+清锁）→ `--cut <刀号>` 收口 → 无暂停条件即取下一刀。

## 这次接入时抓到的问题

按时间顺序，都是回路自己发现的，不是人眼看出来的：

1. **改名后 build 是坏的**。目录从 `tally-tauri` 改成 `stacktake`，`target/` 里 tauri 构建脚本的产物还指着旧绝对路径，`cargo build` / `cargo test` 全部报 `failed to read plugin permissions`。修复：`cargo clean -p tauri`。这类陈旧产物只有真的去 build 才会暴露——第一层回路就撞上了。
2. **clippy 9 个警告**。全部清零后 `-D warnings` 才成为门禁，否则门禁形同虚设。
3. **repos 分组声明了 5 个字段**，第 4、5 个（状态、远端）从来没渲染过。`assert_frontend_contract` 第一跑就抓住。已裁剪到三个，`dirty` 指标留在 metrics 里供 JSON 消费。
4. **常量断言是死代码**。原测试 `SCHEMA_VERSION >= 2` 编译期定型、运行时永真，换成 `scanner_registration_is_pinned` 集合哨兵——新增 Scanner 会真的把它绊倒。
5. **测试依赖本机装了 JDK**。`dev_env_scanner_registered_when_present` 原本硬断言 java 已注册，CI 干净环境必红，改成注册状态与探测结果一致。
6. **私钥闸误伤自己人**。pre-commit 的私钥内容闸第一跑就拒了 `tests.rs`——仓库的泄漏检测针串（`"BEGIN OPENSSH PRIVATE KEY"`）全是裸词。收紧到完整 PEM 甲胄头（前后短划）后误伤排除，用假密钥实测仍能拦截。教训：秘密扫描的 pattern 要精确到「只有真密钥才长这样」，否则第一个被拦的就是自己的测试。

## 已知边界

- 外环的测试约 10 秒（2026-10-17 H9 后实测：nextest 5.4–9.2s，外环全程 20.1s）。此前一个扫真实 `~/Documents/Code` 的嵌套仓库测试独占 124 秒（外环全程 132.7s），而 CI 上该路径不存在、一直静默跳过——只惩罚本机、不提供跨机覆盖。H9 改成临时目录夹具（精确断言扣减与守恒），真实数据版退为 `#[ignore]` 手跑。nextest 的收益是失败隔离与单测计时，提速是个位数百分比；并行扫描只在 APFS 上收回一部分收益，这是已知限制，不是回归。
- 评估档会真扫全机（约 25 秒，全程约 45 秒），会读 `~/Documents/Code` 等目录的元数据。只读，不写入。
- 契约校验的是数据形状，不是数据正确性。体积对不对、去重准不准，仍然靠各扫描器自己的守恒测试和人工核对 dump。

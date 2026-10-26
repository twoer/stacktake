//! 扫描器集成测试。
//!
//! 跑法：cd src-tauri && cargo test
//!
//! 这些测试只读本机真实数据，不做任何写入或删除。

#[cfg(test)]
mod harness {
    use crate::cache;
    #[cfg(not(unix))]
    use crate::scan::ai_tools::{AiToolsScanner, Category, classify};
    #[cfg(unix)]
    use crate::scan::ai_tools::{AiToolsScanner, Category, classify, collect_entries_for_test};
    use crate::scan::ai_tools::{ai_id, clean_candidates, parse_ai_id};
    use crate::scan::dev_env::{
        DevEnvScanner, JavaScanner, dev_env_scanners, gradle_version_for_test,
        jdk_version_for_test, maven_rows_for_test, version_major_for_test,
    };
    use crate::scan::dirsize;
    use crate::scan::docker::{
        DockerScanner, parse_image_line_for_test, parse_size_for_test, parse_time_for_test,
        parse_verbose_for_test, parse_volume_line_for_test,
    };
    use crate::scan::local_models::LocalModelsScanner;
    use crate::scan::platform::{self, Platform};
    use crate::scan::repos::{
        RepoScanner, artifact_rules_for_test, find_artifact_dirs, human_bytes,
        is_artifact_dir_under, resolve_artifact_target,
    };
    use crate::scan::ssh::SshScanner;
    use crate::scan::ssh::{
        identity_matches_key_for_test, looks_like_private_key_for_test, parse_ssh_config_for_test,
    };
    use crate::scan::toolchain::{
        ToolchainScanner, all_rels_for_test, clean_verdict_for_test, cleanable_and_cmd_for_test,
        kinds_for_test, note_keys_for_test, parse_cache_id_for_test, resolve_rel_for_test,
        sources_on_for_test, summarize_for_test, version_items_for_test, version_managers_for_test,
    };
    use crate::scan::wsl::{
        WslDistro, WslScanner, parse_lxss_registry_for_test, wsl_scanners_with,
    };
    use crate::scan::{
        AssetGroup, DetailRow, Field, Item, MetricValue, Scanner, Tag, Text, TextArg, detail_text,
    };
    use crate::scan::{CleanPlan, CleanScope, OVERVIEW_ID, synthesize_overview};
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};

    /// `Text` 里的文案键（`Key` 直接是键，`Fmt` 在 `k`）；`Raw` 没有键。
    /// 断言「后端下发的是键而不是某一种语言的成品」就用它——那是切语言
    /// 不重扫的前提，成品文案一旦进缓存就会在别的语言里露出中文。
    fn text_key(t: &Text) -> Option<&str> {
        match t {
            Text::Key(k) => Some(k.as_str()),
            Text::Fmt(f) => Some(f.k.as_str()),
            Text::Raw(_) => None,
        }
    }

    /// 展示文案非空（三种形态各自的文案字段都得有内容）。
    fn text_present(t: &Text) -> bool {
        match t {
            Text::Raw(s) | Text::Key(s) => !s.is_empty(),
            Text::Fmt(f) => !f.k.is_empty(),
        }
    }

    /// Raw 文案的裸字串（路径 / 命令 / 标识这类语言无关的值）。
    fn text_raw(t: &Text) -> Option<&str> {
        match t {
            Text::Raw(s) => Some(s),
            _ => None,
        }
    }

    /// `Fmt` 文案的某个参数。
    fn text_arg<'a>(t: &'a Text, name: &str) -> Option<&'a TextArg> {
        match t {
            Text::Fmt(f) => f.a.get(name),
            _ => None,
        }
    }

    /// 明细行按标签键取值（标签统一是 `data.detail.<key>`）。
    fn detail_value<'a>(it: &'a Item, key: &str) -> Option<&'a Text> {
        let label = detail_text(key);
        it.detail
            .iter()
            .find(|r| r.label == label)
            .map(|r| &r.value)
    }

    #[test]
    fn ssh_scan_finds_keys_without_reading_secrets() {
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".ssh");
        if !dir.exists() {
            return;
        }

        let g = SshScanner::new(dir).scan();

        // 不该出现的内容：私钥正文
        let json = serde_json::to_string(&g).unwrap();
        assert!(
            !json.contains("BEGIN OPENSSH PRIVATE KEY"),
            "序列化结果里泄漏了私钥正文"
        );
        assert!(
            !json.contains("BEGIN RSA PRIVATE KEY"),
            "序列化结果里泄漏了 RSA 私钥正文"
        );

        // 该有的：指纹
        if !g.items.is_empty() {
            let has_fp = g.items.iter().any(|i| {
                i.detail
                    .iter()
                    .any(|r| text_raw(&r.value).is_some_and(|v| v.starts_with("SHA256:")))
            });
            assert!(has_fp, "没有任何条目带 SHA256 指纹");
        }
    }

    /// 嵌套仓库的扣减口径：父仓库体积**精确等于**自身树减去最浅一层子仓库。
    ///
    /// 喂临时目录而不是真实 `~/Documents/Code`：真实数据版曾占外环 124 秒
    /// （本机 238 个仓库、还要再走一遍 `tree_bytes`），在 CI 上却因路径不存在
    /// 直接 return——只惩罚本机、不提供覆盖。口径本身不依赖数据规模，几个固定
    /// 大小的仓库足以钉死「扣减多少、扣给谁、总和守恒」。
    ///
    /// `tree_bytes` 是独立实现（不复用 dirsize），当基准用；两边都按占用块计。
    #[test]
    fn repo_scan_handles_nesting_without_double_counting() {
        let root = temp_dir("repos-nesting");
        let parent = root.join("parent");
        let child_a = parent.join("child-a");
        let grandchild = child_a.join("grandchild");
        let child_b = parent.join("child-b");

        // `.git` 只要存在就算仓库；伪造的空仓库会让 git 子命令失败，
        // 但分支/提交/未推送各字段都有失败兜底，不影响体积口径。
        for repo in [&parent, &child_a, &grandchild, &child_b] {
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(repo.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
        }
        let write = |p: &Path, n: usize| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![b'x'; n]).unwrap();
        };
        write(&parent.join("src/main.rs"), 8192);
        write(&parent.join("docs/notes.md"), 2048);
        write(&parent.join("plain/data.bin"), 3072);
        write(&child_a.join("a.txt"), 1024);
        write(&grandchild.join("deep.txt"), 512);
        write(&child_b.join("b.txt"), 4096);

        let g = RepoScanner::new(vec![root.clone()]).scan();
        assert_eq!(g.items.len(), 4, "应识别 4 个仓库");

        let size = |p: &Path| -> u64 {
            let item = g
                .items
                .iter()
                .find(|i| Path::new(&i.path) == p)
                .unwrap_or_else(|| panic!("缺少仓库条目 {}", p.display()));
            size_of(item).unwrap_or_else(|| panic!("仓库 {} 体积缺失", p.display()))
        };

        // 父仓库扣掉 child-a / child-b 两棵最浅子树，剩下的才是它自己的
        assert_eq!(
            size(&parent),
            tree_bytes(&parent) - tree_bytes(&child_a) - tree_bytes(&child_b),
            "父仓库扣减后体积不对"
        );
        // child-a 只扣 grandchild；grandchild 已整棵包含在 child-a 的扣减里，不能再扣一次
        assert_eq!(
            size(&child_a),
            tree_bytes(&child_a) - tree_bytes(&grandchild),
            "中间层仓库扣减后体积不对"
        );
        // 叶子仓库没有被扣的对象，应等于整棵子树
        assert_eq!(
            size(&grandchild),
            tree_bytes(&grandchild),
            "孙仓库体积应完整"
        );
        assert_eq!(size(&child_b), tree_bytes(&child_b), "子仓库体积应完整");

        // 守恒：四个仓库之和恰好等于根目录树——扣减没丢数、也没重复计算
        let total = size(&parent) + size(&child_a) + size(&grandchild) + size(&child_b);
        assert_eq!(total, tree_bytes(&root), "各仓库体积之和应等于根目录树");

        assert!(size(&parent) > 0, "父仓库扣减后不应为零");
        assert!(
            size(&parent) < tree_bytes(&parent),
            "父仓库未扣减子仓库，嵌套口径失效"
        );

        let notes = g.notes.unwrap_or_default();
        assert!(
            notes.contains(&Text::fmt("data.note.reposNested").arg("count", TextArg::num(3))),
            "notes 应报告嵌套数: {notes:?}"
        );
    }

    /// 真实数据守恒检查（手跑，不进外环）：
    /// `cargo test --lib repo_nesting_real_data -- --ignored --nocapture`
    ///
    /// 夹具版钉的是口径；这一条只验「本机真实目录上也没有重复计算」。
    /// 它要跑两遍全树（扫描 + `tree_bytes`），本机 238 个仓库时约 124 秒，
    /// 而 CI 上 `~/Documents/Code` 不存在、一直是静默跳过——慢且不提供覆盖。
    #[test]
    #[ignore]
    fn repo_nesting_real_data_stays_conserved() {
        let Some(home) = dirs::home_dir() else { return };
        let code = home.join("Documents/Code");
        if !code.exists() {
            return;
        }

        let g = RepoScanner::new(vec![code.clone()]).scan();
        assert!(!g.items.is_empty(), "应至少找到一个仓库");

        // 正确的断言不是「父 ≥ 子」——扣减后父仓库的体积是**自身文件减去所有子仓库**，
        // 剩下的那部分与子仓库的完整体积没有任何包含关系。父仓库扣掉子仓库 `repo-g` 后剩 1.1G，
        // 而 `repo-g` 自身有 1.45G，两者同时成立。
        //
        // 真正该保证的是**总量守恒**：所有顶层仓库的体积之和不应超过扫描根目录的实际大小，
        // 否则说明有重复计算或漏算。逐组验证「父 + 子 > 父」即可。
        let mut items: Vec<_> = g.items.iter().collect();
        items.sort_by_key(|i| i.path.len());

        let mut checked = 0usize;
        for (i, parent) in items.iter().enumerate() {
            let p_path = std::path::Path::new(&parent.path);
            let Some(p_size) = size_of(parent) else {
                continue;
            };
            for child in &items[i + 1..] {
                let c_path = std::path::Path::new(&child.path);
                if !c_path.starts_with(p_path) || c_path == p_path {
                    continue;
                }
                let Some(c_size) = size_of(child) else {
                    continue;
                };
                checked += 1;
                // 父仓库扣减后是「剩余」，加上被扣掉的子仓库体积，
                // 应当不超过父仓库的原始大小（这里用子仓库体积做下界校验：
                // 扣减量不能超过总量，否则是过度扣减）。
                assert!(
                    c_size > 0 && p_size > 0,
                    "父仓库 {} 与子仓库 {} 体积为零，扣减异常",
                    parent.name,
                    child.name
                );
            }
        }

        assert!(
            checked > 0,
            "本机应至少存在一组嵌套仓库，测试才有效（当前 {} 组）",
            checked
        );

        // 守恒校验：所有仓库的体积之和必须 ≤ 扫描根目录的实际大小。
        //
        // 扣减逻辑保证 `my`（7.1G，含 30 个子仓库共 5.5G）报 1.1G，
        // 加上子仓库的 5.5G 正好等于 7.1G。若扣减失效，这个和会远超根目录实际占用。
        let total_reported: u64 = g.items.iter().filter_map(size_of).sum();
        let root_bytes = tree_bytes(code.as_path());

        assert!(
            total_reported <= root_bytes,
            "各仓库体积之和 {} 超过扫描根目录实际大小 {}，存在重复计算",
            human_bytes(total_reported),
            human_bytes(root_bytes)
        );
    }

    /// 用 `du -sk` 取目录大小（KB），失败返回 0。
    fn du_kb(path: &Path) -> u64 {
        let out = std::process::Command::new("du")
            .arg("-sk")
            .arg(path)
            .stdin(std::process::Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0),
            _ => 0,
        }
    }

    /// 原生遍历 vs 系统 `du` 的**数值与耗时**对照。
    ///
    /// 不参与常规回归——慢，而且依赖系统 du（Windows 上没有）。
    /// 手动跑：`cargo test --lib bench_native_size_vs_du -- --ignored --nocapture`
    ///
    /// 2026-10-12 去 du 化时的实测结论（debug 构建，release 更快）：
    /// **耗时没退反进**——pnpm store 2.6s vs du 6.4s，ms-playwright 持平，
    /// Homebrew 小目录略慢（98ms vs 19ms，绝对值可忽略）。
    /// **数值**：当时原生比 du 高 1.21%（`.npm` 的硬链接，du 去重而原生不去重）。
    /// S4 起原生也按 (dev, inode) 去重，这一处差异应当消失——这个 bench 因此
    /// 变成去重口径的**真机对照**：再跑出系统性偏高就说明去重破了。
    #[test]
    #[ignore]
    fn bench_native_size_vs_du() {
        let Some(home) = dirs::home_dir() else { return };
        let targets: Vec<PathBuf> = [
            "Library/pnpm/store",
            "Library/Caches/ms-playwright",
            ".npm",
            ".cargo/registry",
            ".m2/repository",
            "Library/Caches/Homebrew",
        ]
        .iter()
        .map(|r| home.join(r))
        .filter(|p| p.is_dir())
        .collect();

        println!(
            "\n{:<34} {:>12} {:>12} {:>8} {:>9}",
            "目录", "du", "原生", "比值", "原生耗时"
        );
        let mut worst_ratio = 0.0f64;
        for t in &targets {
            let t0 = std::time::Instant::now();
            let native = crate::scan::dirsize::dir_size(t, &HashSet::new());
            let native_ms = t0.elapsed().as_millis();

            let t1 = std::time::Instant::now();
            let du = du_kb(t) * 1024;
            let du_ms = t1.elapsed().as_millis();

            let n = native.unwrap_or(0);
            let ratio = if du > 0 { n as f64 / du as f64 } else { 0.0 };
            worst_ratio = worst_ratio.max((ratio - 1.0).abs());
            println!(
                "{:<34} {:>12} {:>12} {:>8.4} {:>6}ms (du {}ms)",
                t.strip_prefix(&home).unwrap_or(t).display(),
                human_bytes(du),
                human_bytes(n),
                ratio,
                native_ms,
                du_ms
            );
        }
        println!("\n最大口径偏差 {:.2}%\n", worst_ratio * 100.0);
    }

    #[test]
    fn human_bytes_reads_naturally() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(1536 * 1024), "1.5 MB");
    }

    #[test]
    fn ssh_config_parser_groups_identity_per_host() {
        let fixture = "\
Host alpha beta
  HostName 10.0.0.1
  IdentityFile ~/.ssh/alpha_key

Host gamma
  IdentityFile ~/.ssh/gamma_key
";
        let parsed = parse_ssh_config_for_test(fixture);
        let alpha: Vec<_> = parsed.iter().filter(|(h, _)| h == "alpha").collect();
        assert_eq!(alpha.len(), 1, "alpha 应有一条 IdentityFile 记录");
        assert!(alpha[0].1.contains("alpha_key"));
        assert!(
            parsed.iter().any(|(h, _)| h == "beta"),
            "beta 与 alpha 共享同一块，应一并记录"
        );
    }

    #[test]
    fn private_key_detection_by_magic_bytes() {
        let p = PathBuf::from("/nonexistent/key");
        // 文件不存在时应安全返回 false，不 panic
        assert!(!looks_like_private_key_for_test(&p));
    }

    // ========================================================================
    // S1: 引用匹配是路径级相等，不是文件名子串
    // ========================================================================

    #[test]
    fn ssh_reference_matching_rejects_prefix_and_suffix_names() {
        let ssh_dir = PathBuf::from("/home/tester/.ssh");
        let home = PathBuf::from("/home/tester");
        let key = ssh_dir.join("id_rsa");
        let old = ssh_dir.join("id_rsa_old");

        // 误标方向：config 指向 id_rsa_old，不能把 id_rsa 算成已引用
        // （旧实现 id.contains(name) 在这里必然误判）
        assert!(!identity_matches_key_for_test(
            "~/.ssh/id_rsa_old",
            &key,
            &ssh_dir,
            Some(&home)
        ));
        // 漏标方向：反过来也不能把 id_rsa_old 算成被 id_rsa 引用
        assert!(!identity_matches_key_for_test(
            "~/.ssh/id_rsa",
            &old,
            &ssh_dir,
            Some(&home)
        ));
        // 正对照：同名同目录必须匹配——别把修正做成谁也不引用
        assert!(identity_matches_key_for_test(
            "~/.ssh/id_rsa",
            &key,
            &ssh_dir,
            Some(&home)
        ));
    }

    #[test]
    fn ssh_reference_matching_resolves_path_forms() {
        let ssh_dir = PathBuf::from("/home/tester/.ssh");
        let home = PathBuf::from("/home/tester");
        let key = ssh_dir.join("id_ed25519");

        // 同名不同目录：别处的同名文件不算引用本机这把
        assert!(!identity_matches_key_for_test(
            "/etc/ssh/keys/id_ed25519",
            &key,
            &ssh_dir,
            Some(&home)
        ));
        assert!(!identity_matches_key_for_test(
            "/home/other/.ssh/id_ed25519",
            &key,
            &ssh_dir,
            Some(&home)
        ));

        // 相对路径按 .ssh 目录拼齐后相等，算引用（man ssh_config 语义）
        assert!(identity_matches_key_for_test(
            "id_ed25519",
            &key,
            &ssh_dir,
            Some(&home)
        ));

        // ~ 展开 + .. 规范化后相等，算引用
        assert!(identity_matches_key_for_test(
            "~/.ssh/../.ssh/id_ed25519",
            &key,
            &ssh_dir,
            Some(&home)
        ));

        // %d / %u 令牌展开后相等，算引用
        assert!(identity_matches_key_for_test(
            "%d/.ssh/id_ed25519",
            &key,
            &ssh_dir,
            Some(&home)
        ));
        assert!(identity_matches_key_for_test(
            "%d/.ssh/%u_key",
            &ssh_dir.join("tester_key"),
            &ssh_dir,
            Some(&home)
        ));

        // ~user 与他人 home 本地展开不了：不匹配，也不误标
        assert!(!identity_matches_key_for_test(
            "~other/.ssh/id_ed25519",
            &key,
            &ssh_dir,
            Some(&home)
        ));

        // home 未知时 ~ 形态解析不了：不匹配，而不是猜一个目录
        assert!(!identity_matches_key_for_test(
            "~/.ssh/id_ed25519",
            &key,
            &ssh_dir,
            None
        ));
    }

    #[test]
    fn ssh_scan_marks_reference_by_path_not_substring() {
        let dir = temp_dir("ssh-ref");
        let ssh = dir.join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();

        // 假私钥只靠开头甲胄被识别；字符串拼出来，源码里不留真甲胄
        // （pre-commit 内容闸拦的就是那个形态）
        let head = format!("-----BEGIN {} PRIVATE KEY-----", "OPENSSH");
        for name in ["id_rsa", "id_rsa_old", "id_ed25519"] {
            std::fs::write(ssh.join(name), format!("{head}\nnot-a-real-key\n")).unwrap();
        }
        let old_abs = ssh.join("id_rsa_old").display().to_string();
        std::fs::write(
            ssh.join("config"),
            format!(
                "Host prod\n  IdentityFile {old_abs}\n\nHost home\n  IdentityFile id_ed25519\n"
            ),
        )
        .unwrap();

        let g = SshScanner::new(ssh).scan();
        let tags_of = |name: &str| {
            g.items
                .iter()
                .find(|i| i.name == name)
                .map(|i| {
                    i.tags
                        .iter()
                        .map(|t| t.key.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_else(|| panic!("扫描结果里缺少条目 {name}"))
        };
        // 绝对路径命中 id_rsa_old
        assert!(
            tags_of("id_rsa_old").contains("referenced"),
            "id_rsa_old 应被 prod 引用: {}",
            tags_of("id_rsa_old")
        );
        // 相对路径按 .ssh 拼齐命中 id_ed25519
        assert!(
            tags_of("id_ed25519").contains("referenced"),
            "id_ed25519 应被 home 引用: {}",
            tags_of("id_ed25519")
        );
        // 关键回归：id_rsa 只是 id_rsa_old 的前缀，旧实现会把它误标成已引用
        assert!(
            tags_of("id_rsa").contains("unreferenced"),
            "id_rsa 未被引用却被误标: {}",
            tags_of("id_rsa")
        );
    }

    fn size_of(item: &Item) -> Option<u64> {
        match item.metrics.get("size") {
            Some(MetricValue::Bytes(b)) => Some(*b),
            _ => None,
        }
    }

    // ========================================================================
    // 缓存
    // ========================================================================

    /// 每个测试目录一个独立路径。
    ///
    /// 只拼 `process::id()` 不够——`cargo test` 把所有测试跑在**同一进程的多个线程**里，
    /// 同 tag 的两条测试会拿到同一个目录，而本函数开头就 `remove_dir_all`：一条正在
    /// 扫描，另一条把它的数据删了，于是出现「单独跑绿、并发跑红」的假失败
    /// （`displayed_text_is_a_key_not_finished_chinese` 与
    /// `every_displayed_key_has_a_translation_in_all_five_catalogs` 共用
    /// `display_smoke_groups`，就撞在这里）。加一个进程内原子序号，每次调用都唯一。
    fn temp_dir(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "stacktake-cache-test-{}-{}-{}",
            tag,
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 单个 inode 占的块数（字节）。Unix 走 `st_blocks`，其他平台退回逻辑大小。
    fn block_bytes(m: &std::fs::Metadata) -> u64 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            m.blocks() * 512
        }
        #[cfg(not(unix))]
        {
            m.len()
        }
    }

    /// 递归累加一棵目录树的占用块数。
    ///
    /// **故意不复用 `dirsize::dir_size`**：拿被测代码当基准等于循环论证——
    /// 实现里少算一层目录、少跟一次软链，两边一起错，测试照样是绿的。
    /// 这里写得笨一点没关系。软链不跟随（`symlink_metadata` 给的是软链自身，
    /// `is_dir()` 为假，自然不会递归进去）。
    fn tree_bytes(path: &Path) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return 0;
        };
        let mut total = block_bytes(&meta);
        if !meta.is_dir() {
            return total;
        }
        let Ok(rd) = std::fs::read_dir(path) else {
            return total;
        };
        for e in rd.flatten() {
            total += tree_bytes(&e.path());
        }
        total
    }

    /// 原生遍历的基准正确性：与独立累加逐字节相等。
    ///
    /// 喂临时目录，**不依赖本机装了什么**——这正是换掉 `du` 之前写不出来的一类测试
    /// （Windows 上没有 du，凡是拿 du 当基准的用例在那边必然红）。
    #[test]
    fn dirsize_matches_independent_block_sum() {
        let dir = temp_dir("dirsize-basic");
        std::fs::write(dir.join("a.bin"), vec![0u8; 8192]).unwrap();
        std::fs::create_dir_all(dir.join("sub/deep")).unwrap();
        std::fs::write(dir.join("sub/b.bin"), vec![0u8; 4096]).unwrap();
        std::fs::write(dir.join("sub/deep/c.bin"), vec![0u8; 1024]).unwrap();

        let got = dirsize::dir_size(&dir, &HashSet::new()).unwrap();
        assert_eq!(got, tree_bytes(&dir), "原生遍历与独立累加不一致");

        // 不存在的路径给 None 而不是 0：0 在界面上是「确实是空的」，两回事
        assert!(
            dirsize::dir_size(&dir.join("nope"), &HashSet::new()).is_none(),
            "不存在的路径应返回 None"
        );
    }

    /// 去重口径的独立基准：同一 (dev, inode) 只计一次，目录 inode 各算各的
    /// （目录不可能硬链接）。**与 `tree_bytes` 同级的不复用被测代码**——
    /// 这就是 `du` 单目录调用的语义。Windows 无稳定文件索引，不编译。
    #[cfg(unix)]
    fn tree_bytes_dedup(path: &Path) -> u64 {
        use std::collections::HashSet as INodeSet;
        use std::os::unix::fs::MetadataExt;
        fn walk(p: &Path, seen: &mut INodeSet<(u64, u64)>) -> u64 {
            let Ok(meta) = std::fs::symlink_metadata(p) else {
                return 0;
            };
            if !meta.is_dir() {
                // 多链接条目查重；单链接（nlink <= 1）不可能在本趟里再见
                if meta.nlink() > 1 && !seen.insert((meta.dev(), meta.ino())) {
                    return 0;
                }
                return block_bytes(&meta);
            }
            let mut total = block_bytes(&meta);
            let Ok(rd) = std::fs::read_dir(p) else {
                return total;
            };
            for e in rd.flatten() {
                total += walk(&e.path(), seen);
            }
            total
        }
        let mut seen = INodeSet::new();
        walk(path, &mut seen)
    }

    /// 硬链接去重（D14 / S4）：同一趟遍历里同 (dev, inode) 只计一次，与
    /// 「唯一 inode 字节和」的独立基准逐字节相等。真实世界里的受害者的样子：
    /// cargo `target`（86k 条目 / 30k inode，逐条目累加比 du 高 4.6 倍）。
    #[cfg(unix)]
    #[test]
    fn dirsize_dedups_hardlinks_within_one_walk() {
        use std::fs::hard_link;
        let dir = temp_dir("dirsize-hardlink");
        std::fs::write(dir.join("a.bin"), vec![0u8; 8192]).unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/b.bin"), vec![0u8; 4096]).unwrap();
        // c.bin 与 a.bin 同一 inode：逐条目累加计两次，去重后只计一次
        hard_link(dir.join("a.bin"), dir.join("sub/c.bin")).unwrap();

        let got = dirsize::dir_size(&dir, &HashSet::new()).unwrap();
        assert_eq!(got, tree_bytes_dedup(&dir), "去重后应等于唯一 inode 字节和");
        assert!(
            got < tree_bytes(&dir),
            "含硬链接的树，去重结果必须严格小于逐条目累加"
        );
    }

    /// noting 的产物子树（quiet 递归）与主干道**共享同一把去重集**：硬链接
    /// 跨产物目录内外时只计在先遍历到的一处——仓库总体积与产物子树体积各自
    /// 都等于唯一 inode 字节和。这条对应真实场景：依赖文件在 target 里、
    /// 仓库根也留一份的 cargo 布局。
    #[cfg(unix)]
    #[test]
    fn dirsize_noting_dedup_covers_quiet_subtree() {
        use std::sync::Mutex;
        let dir = temp_dir("dirsize-noting-hardlink");
        std::fs::create_dir_all(dir.join("deps")).unwrap();
        std::fs::write(dir.join("src.bin"), vec![0u8; 8192]).unwrap();
        std::fs::write(dir.join("deps/lib.bin"), vec![0u8; 4096]).unwrap();
        std::fs::hard_link(dir.join("src.bin"), dir.join("deps/dup.bin")).unwrap();

        let noted = dirsize::Noted {
            items: Mutex::new(Vec::new()),
        };
        let pred = |p: &Path| p.file_name().is_some_and(|n| n == "deps");
        let total = dirsize::dir_size_noting(&dir, &HashSet::new(), &pred, &noted).unwrap();

        assert_eq!(
            total,
            tree_bytes_dedup(&dir),
            "总体积应等于唯一 inode 字节和"
        );
        let items = noted.items.lock().unwrap();
        assert_eq!(items.len(), 1, "应恰好记下 deps 一个产物目录");
        // dup.bin 与主道的 src.bin 同一 inode，这份字节记在谁头上取决于目录
        // 遍历序（deps 先见 → 记在 deps；src.bin 先见 → deps 里计 0）——两种
        // 都合法；**各计一次以上才是错**，那由上面的 total 断言守着
        let dup = block_bytes(&std::fs::symlink_metadata(dir.join("deps/dup.bin")).unwrap());
        let with_dup = tree_bytes_dedup(&dir.join("deps"));
        assert!(
            items[0].1 == with_dup || items[0].1 == with_dup - dup,
            "产物子树计量应为两种遍历序之一：{} vs {} / {}",
            items[0].1,
            with_dup,
            with_dup - dup
        );
    }

    /// 排除集合是**整棵子树剪枝**，而且一次遍历就够。
    ///
    /// du 时代这里要算两遍再相减（先整棵、再逐个嵌套目录），原生遍历能按路径
    /// 直接不进去——嵌套仓库的体积统计因此少走一趟。
    #[test]
    fn dirsize_excludes_nested_subtree() {
        let dir = temp_dir("dirsize-exclude");
        std::fs::create_dir_all(dir.join("parent/child")).unwrap();
        std::fs::write(dir.join("parent/keep.bin"), vec![0u8; 2048]).unwrap();
        std::fs::write(dir.join("parent/child/drop.bin"), vec![0u8; 65536]).unwrap();

        let parent = dir.join("parent");
        let child = parent.join("child");
        let all = dirsize::dir_size(&parent, &HashSet::new()).unwrap();

        let mut ex = HashSet::new();
        ex.insert(child.clone());
        let excluded = dirsize::dir_size(&parent, &ex).unwrap();

        assert!(
            excluded < all,
            "排除子目录后体积应减少：{} vs {}",
            excluded,
            all
        );
        assert_eq!(
            excluded,
            tree_bytes(&parent) - tree_bytes(&child),
            "排除后的体积应等于「整棵减掉被排除的子树」"
        );
    }

    /// 软链不跟随：链到外面的目录不该把外面的体积算进来。
    #[cfg(unix)]
    #[test]
    fn dirsize_does_not_follow_symlinks() {
        let dir = temp_dir("dirsize-symlink");
        std::fs::create_dir_all(dir.join("real")).unwrap();
        std::fs::write(dir.join("real/small.bin"), vec![0u8; 1024]).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        std::fs::write(dir.join("outside/big.bin"), vec![0u8; 1 << 20]).unwrap();
        std::os::unix::fs::symlink(dir.join("outside"), dir.join("real/link")).unwrap();

        let real = dir.join("real");
        let got = dirsize::dir_size(&real, &HashSet::new()).unwrap();
        assert!(got < (1 << 20), "软链被跟随了，算进来 {} 字节", got);
        assert_eq!(got, tree_bytes(&real));
    }

    /// IDE 本体可能所在的父目录，按当前平台给。
    ///
    /// macOS 是 `/Applications`；Windows 上装在 `Program Files` 与
    /// `%LOCALAPPDATA%\Programs`（用户级安装走后者，DevEco Studio 尤其常见）。
    ///
    /// 写成「返回当前平台该看的目录」而不是「返回两个平台的目录」是有意的：
    /// 在 macOS 上不该因为 `C:\Program Files` 不存在而改变结论，反之亦然。
    /// 这个函数服务的是「本机真装了没有」这个判断，本机是什么平台就只看什么。
    fn ide_parents() -> Vec<PathBuf> {
        if !Platform::current().is_windows() {
            let apps = PathBuf::from("/Applications");
            return apps.is_dir().then_some(apps).into_iter().collect();
        }

        let mut dirs: Vec<PathBuf> = Vec::new();
        // 系统级安装。`ProgramFiles` 由环境变量给（通常含空格，如
        // `C:\Program Files`）；拿不到时退回无盘符的相对路径，至少不 panic。
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            dirs.push(PathBuf::from(&pf));
            dirs.push(PathBuf::from(&pf).join("Program Files (x86)"));
        }
        // 用户级安装走 `%LOCALAPPDATA%\Programs`，相对 home 拼（见 platform::WIN_LOCAL）
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(platform::win_local("Programs")));
        }
        dirs.retain(|d| d.is_dir());
        dirs
    }

    fn sample_group() -> AssetGroup {
        AssetGroup {
            id: "repos".into(),
            label: "Git 仓库".into(),
            summary: "2 个仓库 · 合计 1.5 GB".into(),
            fields: vec![Field {
                key: "size".into(),
                label: Text::raw("体积"),
                numeric: Some(true),
                temporal: None,
            }],
            items: vec![Item {
                id: "/tmp/a".into(),
                tags: vec![Tag::new("large"), Tag::new("stale")],
                name: "a".into(),
                label: None,
                short_label: None,
                group_label: None,
                kind_label: None,
                group_key: None,
                path: "/tmp/a".into(),
                metrics: HashMap::from([
                    ("size".into(), MetricValue::Bytes(1_500_000_000)),
                    ("last_commit".into(), MetricValue::Int(1_700_000_000)),
                    ("branch".into(), MetricValue::Text("main".into())),
                ]),
                detail: vec![DetailRow::new("remote", "git@github.com:x/y.git")],
                cleanable: true,
                artifact_entries: vec![],
                reclaimable: Some(1_000_000_000),
            }],
            notes: Some(vec![Text::raw("提示：测试")]),
            error: None,
            supports_clean: true,
        }
    }

    /// 核心往返：写进去能原样读出来，且类型（Bytes / Int / Text）不能串味。
    /// 缓存是 untagged enum 反序列化，错配会静默变成另一个类型。
    #[test]
    fn cache_round_trip_preserves_metric_types() {
        let dir = temp_dir("roundtrip");
        let original = vec![sample_group()];

        cache::save(&dir, &original).expect("写入缓存失败");
        let got = cache::load(&dir).expect("读取缓存失败");

        assert_eq!(got.groups.len(), 1);
        assert_eq!(got.groups[0].id, "repos");
        assert_eq!(got.groups[0].items.len(), 1);

        let item = &got.groups[0].items[0];
        // 类型必须对得上：size 仍是 Bytes，不是 Int
        assert!(
            matches!(item.metrics.get("size"), Some(MetricValue::Bytes(b)) if *b == 1_500_000_000),
            "size 应还原为 Bytes，实际 {:?}",
            item.metrics.get("size")
        );
        assert!(
            matches!(
                item.metrics.get("last_commit"),
                Some(MetricValue::Int(1_700_000_000))
            ),
            "last_commit 应还原为 Int，实际 {:?}",
            item.metrics.get("last_commit")
        );
        assert!(
            matches!(item.metrics.get("branch"), Some(MetricValue::Text(s)) if s == "main"),
            "branch 应还原为 Text，实际 {:?}",
            item.metrics.get("branch")
        );
        assert_eq!(item.tags, vec![Tag::new("large"), Tag::new("stale")]);
        assert_eq!(
            detail_value(item, "remote").and_then(text_raw),
            Some("git@github.com:x/y.git")
        );
        assert!(item.cleanable);
        assert_eq!(item.reclaimable, Some(1_000_000_000));
        assert_eq!(
            got.groups[0].notes.as_ref().unwrap()[0],
            Text::raw("提示：测试")
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 缓存读不出来时必须返回 None，绝不能让启动失败。
    #[test]
    fn cache_misses_are_silent() {
        let dir = temp_dir("miss");

        // 目录空 → 无缓存
        assert!(cache::load(&dir).is_none());

        // 目录不存在 → 无缓存，不能 panic
        let gone = dir.join("nope");
        assert!(cache::load(&gone).is_none());

        // 坏 JSON → 无缓存，不能 panic
        std::fs::write(dir.join("scan-cache.json"), "{ 这不是 json").unwrap();
        assert!(cache::load(&dir).is_none());

        // 空 groups → 视为无缓存（首次扫描失败 shouldn留下空壳）
        cache::save(&dir, &[]).unwrap();
        assert!(cache::load(&dir).is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// schema 版本不匹配必须丢弃旧缓存——这是字段变更后的唯一失效机制。
    #[test]
    fn cache_rejects_schema_mismatch() {
        let dir = temp_dir("schema");
        cache::save(&dir, &[sample_group()]).unwrap();

        // 读得到（版本一致）
        assert!(cache::load(&dir).is_some());

        let p = cache::cache_path(&dir);
        let mut old: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        // 17 仍是截断文本契约（detail 还是字串映射），不能用默认空数组冒充完整产物列表。
        old["groups"][0]["items"][0]["metrics"]["artifacts"] =
            serde_json::json!({"t": "b", "v": 4096});
        old["groups"][0]["items"][0]["detail"] = serde_json::json!({"产物明细": "target · 4 KB"});
        for schema in [0, 17] {
            old["schema"] = schema.into();
            std::fs::write(&p, serde_json::to_string(&old).unwrap()).unwrap();
            assert!(cache::load(&dir).is_none(), "旧 schema {schema} 必须丢弃");
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn artifact_entries_json_and_cache_round_trip_preserves_paths() {
        let dir = temp_dir("artifact-path-roundtrip");
        // 换行在 Windows 文件名中不合法；只验证内存与缓存契约，不创建这些目录。
        let paths = [
            "with space/node_modules",
            "中文/target",
            "pkg · app/dist",
            r"packages\app\target",
            "line\nbreak/node_modules",
        ];
        let entries: Vec<_> = paths.iter().enumerate().map(|(i, path)| {
            serde_json::json!({"relative_path": path, "size_bytes": 1001 + i as u64})
        }).collect();
        let mut json = serde_json::to_value(sample_group()).unwrap();
        json["items"][0]["artifact_entries"] = serde_json::json!(entries);
        json["items"][0]["metrics"]["artifacts"] = serde_json::json!({"t": "b", "v": 5015});
        let group: AssetGroup =
            serde_json::from_str(&serde_json::to_string(&json).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&group).unwrap(),
            json,
            "JSON 路径与原始字节必须无损"
        );
        assert_frontend_contract(std::slice::from_ref(&group));
        cache::save(&dir, &[group]).unwrap();
        let got = cache::load(&dir).unwrap();
        assert_eq!(serde_json::to_value(&got.groups[0]).unwrap(), json);
        assert_frontend_contract(&got.groups);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn artifact_entries_empty_field_is_optional() {
        let dir = temp_dir("artifact-empty");
        let group = sample_group();
        let mut json = serde_json::to_value(&group).unwrap();
        assert!(json["items"][0].get("artifact_entries").is_none());
        let missing: AssetGroup = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&missing).unwrap(), json);
        json["items"][0]["artifact_entries"] = serde_json::json!([]);
        let explicit: AssetGroup = serde_json::from_value(json).unwrap();
        assert_eq!(
            serde_json::to_value(&explicit).unwrap(),
            serde_json::to_value(&group).unwrap()
        );
        cache::save(&dir, &[explicit]).unwrap();
        let got = cache::load(&dir).unwrap();
        assert!(
            serde_json::to_value(&got.groups[0]).unwrap()["items"][0]
                .get("artifact_entries")
                .is_none()
        );
        assert_frontend_contract(&got.groups);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn changed_extra_roots_invalidate_only_cached_repos_without_auto_scan() {
        let dir = temp_dir("cache-root-change");
        let root = dir.join("code");
        std::fs::create_dir(&root).unwrap();
        let mut other = sample_group();
        other.id = "toolchain".into();
        cache::save(&dir, &[sample_group(), other]).unwrap();
        let original = cache::load(&dir).unwrap();
        crate::roots::add(&dir, &root).unwrap();
        let changed = cache::load(&dir).unwrap();
        assert_eq!(cache::decide_boot(Some(&dir)), cache::Boot::UseCache);
        assert!(changed.groups[0].items.is_empty());
        assert!(changed.groups[0].error.is_some());
        assert!(!changed.groups[0].supports_clean);
        assert_eq!(changed.group_saved_at, original.group_saved_at);
        assert_eq!(
            serde_json::to_value(&changed.groups[1]).unwrap(),
            serde_json::to_value(&original.groups[1]).unwrap()
        );
        cache::save_with_times(
            &dir,
            &original.groups,
            &original.group_saved_at,
            &crate::roots::load_extra(&dir).unwrap(),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(cache::load(&dir).unwrap().groups[0].items.len(), 1);
        crate::roots::remove(&dir, &crate::roots::load_extra(&dir).unwrap()[0]).unwrap();
        assert!(cache::load(&dir).unwrap().groups[0].items.is_empty());
        assert_eq!(cache::decide_boot(Some(&dir)), cache::Boot::UseCache);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cache_fingerprint_uses_scanned_roots_not_later_config() {
        let dir = temp_dir("root-snapshot-race");
        let scanned = crate::roots::for_scan(&dir).unwrap();
        let root = dir.join("code");
        std::fs::create_dir(&root).unwrap();
        crate::roots::add(&dir, &root).unwrap();
        cache::save_with_times(
            &dir,
            &[sample_group()],
            &Default::default(),
            &scanned.extra,
            &Default::default(),
        )
        .unwrap();
        let raw = std::fs::read(cache::cache_path(&dir)).unwrap();
        let saved: cache::CacheEnvelope = serde_json::from_slice(&raw).unwrap();
        assert!(saved.extra_repo_roots.is_empty());
        assert!(cache::load(&dir).unwrap().groups[0].items.is_empty());
        assert_eq!(cache::decide_boot(Some(&dir)), cache::Boot::UseCache);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_extra_roots_preserve_unrelated_cached_groups() {
        let dir = temp_dir("corrupt-roots-cache");
        let mut other = sample_group();
        other.id = "toolchain".into();
        cache::save(&dir, &[sample_group(), other.clone()]).unwrap();
        std::fs::write(dir.join("repo-roots.json"), b"broken").unwrap();
        let loaded = cache::load(&dir).unwrap();
        assert!(loaded.groups[0].items.is_empty());
        assert_eq!(
            loaded.groups[0].error.as_ref().and_then(text_key),
            Some("data.error.repoRootsRead"),
            "补充目录读取失败应下发文案键，让界面按当前语言显示"
        );
        assert!(!loaded.groups[0].supports_clean);
        assert_eq!(
            serde_json::to_value(&loaded.groups[1]).unwrap(),
            serde_json::to_value(other).unwrap()
        );
        assert_eq!(cache::decide_boot(Some(&dir)), cache::Boot::UseCache);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clearing_scan_cache_keeps_extra_roots() {
        let dir = temp_dir("keep-root-config");
        let root = dir.join("code");
        std::fs::create_dir(&root).unwrap();
        crate::roots::add(&dir, &root).unwrap();
        cache::save(&dir, &[sample_group()]).unwrap();
        cache::remove(&dir);
        assert!(cache::load(&dir).is_none());
        assert_eq!(
            crate::roots::load_extra(&dir).unwrap(),
            vec![dunce::canonicalize(root).unwrap()]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// remove 之后读不到。
    #[test]
    fn cache_remove_clears_snapshot() {
        let dir = temp_dir("remove");
        cache::save(&dir, &[sample_group()]).unwrap();
        assert!(cache::load(&dir).is_some());

        cache::remove(&dir);
        assert!(cache::load(&dir).is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// **第二次写必须成功，且写进去的是新内容。**
    ///
    /// 这不是边角情况——「重扫」是主路径：用户点一次就调一次 `save`，
    /// 而目标文件（上一份缓存）必然已经存在。`save` 走「临时文件 + rename」，
    /// rename 在 Windows 上靠 `MOVEFILE_REPLACE_EXISTING` 覆盖已存在的目标，
    /// Unix 上则是 `rename(2)` 直接覆盖。两边语义一致，但这个跨平台差异
    /// 只有 Windows CI 才验得到——本机是 macOS，写十条也看不出差别。
    ///
    /// 若这条在 Windows 上红了，症状是**重扫后数据不更新**（`save` 静默失败，
    /// 因为 `lib.rs::run_full_scan` 用 `let _ =` 忽略了写缓存的错误）。
    #[test]
    fn cache_overwrites_previous_snapshot() {
        let dir = temp_dir("rewrite");

        let mut first = sample_group();
        first.id = "first".into();
        cache::save(&dir, &[first]).expect("首次写入失败");

        // 换一份完全不同的内容写回去
        let mut second = sample_group();
        second.id = "second".into();
        second.items[0].reclaimable = Some(2_000_000_000);
        cache::save(&dir, &[second]).expect("覆盖写入失败（rename 不能覆盖已存在文件？）");

        let got = cache::load(&dir).expect("覆盖后读不到缓存");
        assert_eq!(got.groups.len(), 1);
        assert_eq!(
            got.groups[0].id, "second",
            "读到的还是上一份缓存——rename 没覆盖成功"
        );
        assert_eq!(got.groups[0].items[0].reclaimable, Some(2_000_000_000));

        // 临时文件不该被留下（否则第二次扫描前目录里堆着一堆 .json.tmp）
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.ends_with(".tmp"))
                    .collect()
            })
            .unwrap_or_default();
        assert!(leftovers.is_empty(), "留下了临时文件：{leftovers:?}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 逐组时间戳往返：`save_with_times` 写进映射的值要原样读回来；映射里缺的
    /// 组按本次写入时间记——单组重扫路径给「刚扫完的组」记时靠的就是这个语义。
    #[test]
    fn cache_round_trip_preserves_group_saved_at() {
        let dir = temp_dir("group-times");
        let a = overview_fixture("repos", "Git 仓库", 1, 0);
        let b = overview_fixture("toolchain", "工具链缓存", 2, 0);

        let times = std::collections::BTreeMap::from([("repos".to_string(), 1_000_000)]);
        cache::save_with_times(
            &dir,
            &[a.clone(), b.clone()],
            &times,
            &[],
            &Default::default(),
        )
        .unwrap();
        let got = cache::load(&dir).expect("读缓存失败");
        assert_eq!(
            got.group_saved_at.get("repos"),
            Some(&1_000_000),
            "映射里的值要原样往返"
        );
        let toolchain_at = *got
            .group_saved_at
            .get("toolchain")
            .expect("不在映射里的组也要有记录");
        assert!(
            toolchain_at >= 1_000_000,
            "缺的组按本次写入时间记，不该比映射里的值还旧：{toolchain_at}"
        );

        // 全量 save（不带映射）：所有组记同一时刻
        cache::save(&dir, &[a, b]).unwrap();
        let got = cache::load(&dir).unwrap();
        let all: Vec<i64> = got.group_saved_at.values().copied().collect();
        assert!(
            all.iter().all(|t| *t == all[0]),
            "全量 save 后各组时间应一致：{all:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- 上一版基线（L1 / D18）----

    /// rotate(Full)：基线整体换成旧当前值——逐条、逐组合计、逐组时间都来自
    /// 旧快照；旧基线里的死组随之一并丢弃（比较对象始终是「上一次扫描」）。
    #[test]
    fn rotate_full_takes_old_current_as_baseline() {
        let repos = overview_fixture("repos", "Git 仓库", 3_000, 1_000);
        let toolchain = overview_fixture("toolchain", "工具链缓存", 2_000, 0);
        let times = std::collections::BTreeMap::from([
            ("repos".to_string(), 1_000),
            ("toolchain".to_string(), 2_000),
        ]);
        let prev_old = cache::PrevLedger {
            group_saved_at: std::collections::BTreeMap::from([("wsl".to_string(), 9_000)]),
            item_sizes: std::collections::BTreeMap::from([(
                "wsl".to_string(),
                std::collections::BTreeMap::from([("wsl:deb".to_string(), 5_000)]),
            )]),
            group_totals: std::collections::BTreeMap::from([("wsl".to_string(), 5_000)]),
        };

        let next = cache::rotate(
            &prev_old,
            &[repos, toolchain],
            &times,
            &cache::RotateBasis::Full,
        );

        assert!(!next.is_empty());
        assert_eq!(next.group_totals.get("repos"), Some(&3_000));
        assert_eq!(next.group_totals.get("toolchain"), Some(&2_000));
        assert_eq!(next.group_saved_at.get("repos"), Some(&1_000));
        assert_eq!(
            next.item_sizes.get("repos").and_then(|m| m.get("repos:a")),
            Some(&3_000)
        );
        assert!(
            !next.group_totals.contains_key("wsl") && !next.item_sizes.contains_key("wsl"),
            "旧基线里的死组不得残留：{next:?}"
        );
    }

    /// rotate(Refresh)：只有被重扫的组基线换成旧当前值，其余组沿用旧基线——
    /// 单组重扫不该动别的组的「上次」，否则清完 toolchain 会把 repos 的差额
    /// 也一起抹掉。
    #[test]
    fn rotate_refresh_swaps_one_group_and_keeps_others() {
        let repos = overview_fixture("repos", "Git 仓库", 4_000, 0);
        let toolchain = overview_fixture("toolchain", "工具链缓存", 2_000, 0);
        let times = std::collections::BTreeMap::from([
            ("repos".to_string(), 1_000),
            ("toolchain".to_string(), 2_000),
        ]);
        let prev_old = cache::PrevLedger {
            group_saved_at: std::collections::BTreeMap::from([
                ("repos".to_string(), 500),
                ("toolchain".to_string(), 2_000),
            ]),
            item_sizes: std::collections::BTreeMap::from([
                (
                    "repos".to_string(),
                    std::collections::BTreeMap::from([("repos:a".to_string(), 3_000)]),
                ),
                (
                    "toolchain".to_string(),
                    std::collections::BTreeMap::from([("toolchain:a".to_string(), 2_000)]),
                ),
            ]),
            group_totals: std::collections::BTreeMap::from([
                ("repos".to_string(), 3_000),
                ("toolchain".to_string(), 2_000),
            ]),
        };

        let next = cache::rotate(
            &prev_old,
            &[repos, toolchain],
            &times,
            &cache::RotateBasis::Refresh("repos".to_string()),
        );

        assert_eq!(
            next.group_totals.get("repos"),
            Some(&4_000),
            "该组基线 = 旧当前值"
        );
        assert_eq!(next.group_saved_at.get("repos"), Some(&1_000));
        assert_eq!(
            next.item_sizes.get("repos").and_then(|m| m.get("repos:a")),
            Some(&4_000)
        );
        assert_eq!(
            next.group_totals.get("toolchain"),
            Some(&2_000),
            "其余组沿用旧基线"
        );
        assert_eq!(next.group_saved_at.get("toolchain"), Some(&2_000));
    }

    /// rotate(Refresh) 对「旧当前为空或缺席」按 Drop 处理：空基线会让界面把整组
    /// 当前值显示成「较上次 +全组」。补充目录变更后 load 会清空 repos，那种
    /// 单组重扫正落在这里——跨范围的对比从根上就不该出现。
    #[test]
    fn rotate_refresh_with_empty_or_absent_old_group_leaves_no_baseline() {
        let mut json =
            serde_json::to_value(overview_fixture("repos", "Git 仓库", 3_000, 0)).unwrap();
        json["items"] = serde_json::json!([]);
        let empty_repos: AssetGroup =
            serde_json::from_str(&serde_json::to_string(&json).unwrap()).unwrap();
        let prev_old = cache::PrevLedger {
            group_saved_at: std::collections::BTreeMap::from([("repos".to_string(), 500)]),
            item_sizes: std::collections::BTreeMap::from([(
                "repos".to_string(),
                std::collections::BTreeMap::from([("repos:a".to_string(), 3_000)]),
            )]),
            group_totals: std::collections::BTreeMap::from([("repos".to_string(), 3_000)]),
        };

        let empty = cache::rotate(
            &prev_old,
            std::slice::from_ref(&empty_repos),
            &std::collections::BTreeMap::new(),
            &cache::RotateBasis::Refresh("repos".to_string()),
        );
        assert!(empty.is_empty(), "旧当前为空的组不留基线：{empty:?}");

        let absent = cache::rotate(
            &prev_old,
            &[],
            &std::collections::BTreeMap::new(),
            &cache::RotateBasis::Refresh("repos".to_string()),
        );
        assert!(
            absent.is_empty(),
            "旧当前里没有的组同样不留基线：{absent:?}"
        );
    }

    /// rotate(Drop)：摘组时该组基线全部删除，死 id 不永久留在映射里；
    /// 其余组原样保留。
    #[test]
    fn rotate_drop_removes_the_group_baseline() {
        let repos = overview_fixture("repos", "Git 仓库", 3_000, 0);
        let toolchain = overview_fixture("toolchain", "工具链缓存", 2_000, 0);
        let prev_old = cache::rotate(
            &cache::PrevLedger::default(),
            &[repos, toolchain],
            &std::collections::BTreeMap::new(),
            &cache::RotateBasis::Full,
        );

        let next = cache::rotate(
            &prev_old,
            &[],
            &std::collections::BTreeMap::new(),
            &cache::RotateBasis::Drop("toolchain".to_string()),
        );

        assert!(!next.group_totals.contains_key("toolchain"));
        assert!(!next.item_sizes.contains_key("toolchain"));
        assert!(!next.group_saved_at.contains_key("toolchain"));
        assert_eq!(
            next.group_totals.get("repos"),
            Some(&3_000),
            "别的组不受牵连"
        );
    }

    /// 只有 `size` 为 Bytes 的条目进基线：repos 遍历超时写 `Text("—")`、
    /// wsl / local_models 体积未知时不写 size——这些行没有可比体积，记进去
    /// 只能靠 0 兜底，会把超时行显示成假负差额。
    #[test]
    fn rotate_records_only_byte_sized_items() {
        let mut json =
            serde_json::to_value(overview_fixture("repos", "Git 仓库", 1_000, 0)).unwrap();
        // 超时行：size 是 Text（前端 itemSize 对它返回 0，差额判定必须更严）
        json["items"][0]["metrics"]["size"] = serde_json::json!({"t": "s", "v": "—"});
        // 体积未知：干脆不写 size
        json["items"][1]["metrics"]
            .as_object_mut()
            .unwrap()
            .remove("size");
        let group: AssetGroup =
            serde_json::from_str(&serde_json::to_string(&json).unwrap()).unwrap();

        let next = cache::rotate(
            &cache::PrevLedger::default(),
            &[group],
            &std::collections::BTreeMap::new(),
            &cache::RotateBasis::Full,
        );

        let sizes = next.item_sizes.get("repos").unwrap();
        assert!(sizes.is_empty(), "非 Bytes 的 size 不进基线：{sizes:?}");
        assert_eq!(
            next.group_totals.get("repos"),
            Some(&0),
            "合计按 total_size 口径：非 Bytes 按 0"
        );
    }

    /// 基线往返：save 带上的 prev 要原样读回来；空基线不写进信封（键都不出现），
    /// 读回来是空——「无基线」与「基线为空」是同一个合法态。
    #[test]
    fn cache_round_trip_preserves_prev_ledger() {
        let dir = temp_dir("prev-ledger");
        let groups = vec![
            overview_fixture("repos", "Git 仓库", 3_000, 0),
            overview_fixture("toolchain", "工具链缓存", 2_000, 0),
        ];
        let times = std::collections::BTreeMap::from([
            ("repos".to_string(), 1_000),
            ("toolchain".to_string(), 2_000),
        ]);
        let prev = cache::rotate(
            &cache::PrevLedger::default(),
            &groups,
            &times,
            &cache::RotateBasis::Full,
        );

        cache::save_with_times(&dir, &groups, &times, &[], &prev).unwrap();
        let got = cache::load(&dir).expect("读缓存失败");
        assert_eq!(got.prev, prev, "基线要原样往返");

        cache::save_with_times(&dir, &groups, &times, &[], &cache::PrevLedger::default()).unwrap();
        let raw = std::fs::read_to_string(cache::cache_path(&dir)).unwrap();
        assert!(!raw.contains("\"prev\""), "空基线不该写进信封");
        let got = cache::load(&dir).unwrap();
        assert!(got.prev.is_empty(), "读回来的空基线是合法态");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 缓存里绝不能出现私钥内容。这是最重要的安全边界：
    /// 缓存文件是明文 JSON 落盘，等于把扫描结果多复制了一份到磁盘。
    #[test]
    fn cache_never_contains_private_key_material() {
        let dir = temp_dir("secret");
        cache::save(&dir, &[sample_group()]).unwrap();

        let raw = std::fs::read_to_string(cache::cache_path(&dir)).unwrap();
        for needle in [
            "BEGIN OPENSSH PRIVATE KEY",
            "BEGIN RSA PRIVATE KEY",
            "BEGIN EC PRIVATE KEY",
            "PRIVATE KEY-----",
        ] {
            assert!(
                !raw.contains(needle),
                "缓存文件里出现了私钥片段：{}",
                needle
            );
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 回归测试：`MetricValue` 曾经用 `#[serde(untagged)]`，导致 `Int(1700000000)`
    /// 反序列化时变成 `Bytes(1700000000)`——JSON 里两者都是裸数字，无法区分。
    /// 表现为界面把「最后提交」显示成 `1.7 GB`，而且不报任何错。
    ///
    /// 这条测试直接断言两种数字类型不会互相串味。
    #[test]
    fn int_and_bytes_stay_distinct_through_serde() {
        let dir = temp_dir("types");
        let mut g = sample_group();
        g.items[0]
            .metrics
            .insert("size".into(), MetricValue::Bytes(42));
        g.items[0]
            .metrics
            .insert("last_commit".into(), MetricValue::Int(42));
        g.items[0]
            .metrics
            .insert("negative".into(), MetricValue::Int(-1));
        g.items[0]
            .metrics
            .insert("flag".into(), MetricValue::Bool(true));
        g.items[0]
            .metrics
            .insert("label".into(), MetricValue::Text("hi".into()));

        cache::save(&dir, &[g]).unwrap();
        let got = cache::load(&dir).unwrap();
        let m = &got.groups[0].items[0].metrics;

        assert!(
            matches!(m.get("size"), Some(MetricValue::Bytes(42))),
            "Bytes(42) 串味成 {:?}",
            m.get("size")
        );
        assert!(
            matches!(m.get("last_commit"), Some(MetricValue::Int(42))),
            "Int(42) 串味成 {:?}",
            m.get("last_commit")
        );
        // 负数只有 Int 能表示，若串成 u64 就会失败
        assert!(
            matches!(m.get("negative"), Some(MetricValue::Int(-1))),
            "Int(-1) 串味成 {:?}",
            m.get("negative")
        );
        assert!(
            matches!(m.get("flag"), Some(MetricValue::Bool(true))),
            "Bool 串味成 {:?}",
            m.get("flag")
        );
        assert!(
            matches!(m.get("label"), Some(MetricValue::Text(s)) if s == "hi"),
            "Text 串味成 {:?}",
            m.get("label")
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ========================================================================
    // 启动策略：默认吃缓存，不扫描
    //
    // 早先的版本是「先出缓存 + 后台自动重扫」。去掉自动刷新是刻意的：
    // 全量扫描实测约 26 秒（2026-10-13 重测），而数据变化极慢，
    // 每次打开都后台啃磁盘没有收益。
    //
    // 这几条测试守住的核心行为：**有缓存时绝不触发扫描**。
    // ========================================================================

    /// 有缓存 → 走缓存。这是默认路径，也是启动快的原因。
    #[test]
    fn boot_uses_cache_when_snapshot_exists() {
        let dir = temp_dir("boot-cached");
        assert_eq!(
            cache::decide_boot(Some(&dir)),
            cache::Boot::NeedScan,
            "空目录应判定为需要扫描"
        );

        cache::save(&dir, &[sample_group()]).unwrap();
        assert_eq!(
            cache::decide_boot(Some(&dir)),
            cache::Boot::UseCache,
            "有缓存时必须走缓存，不能触发扫描"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 没缓存 → 才允许扫。涵盖首次运行、目录不存在、缓存被清空三种情况。
    #[test]
    fn boot_scans_only_without_cache() {
        let dir = temp_dir("boot-nocache");

        // 目录存在但没文件
        assert_eq!(cache::decide_boot(Some(&dir)), cache::Boot::NeedScan);

        // 目录根本不存在（首次运行的真实情况）
        assert_eq!(
            cache::decide_boot(Some(&dir.join("never-created"))),
            cache::Boot::NeedScan
        );

        // 拿不到数据目录（AppHandle 出错）
        assert_eq!(
            cache::decide_boot(None),
            cache::Boot::NeedScan,
            "拿不到数据目录时必须能扫，不能直接返回空"
        );

        // 清掉缓存后应退回扫描
        cache::save(&dir, &[sample_group()]).unwrap();
        assert_eq!(cache::decide_boot(Some(&dir)), cache::Boot::UseCache);
        cache::remove(&dir);
        assert_eq!(
            cache::decide_boot(Some(&dir)),
            cache::Boot::NeedScan,
            "清空缓存后必须重新扫描"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 走缓存的时间成本。这是「启动不扫描」这个决定的收益量化指标。
    ///
    /// 实测读缓存毫秒级、全量扫描约 26 秒（2026-10-13，`cargo run --example dump`），
    /// 差 4 个数量级。具体数字只写在 README / AGENTS 的耗时段（带出处），
    /// 代码注释里不重复——重复的那几处正是前一次漂移的来源。
    ///
    /// 取多次读取的最小值而不是单次：nextest 把它与做体积统计的重测试并行跑，
    /// 磁盘争用只会把单次读数往上推、不会往下拉，最小值才是滤掉争用后的真实成本。
    /// 阈值不是调节的旋钮——代码真变慢时每一次都慢，最小值一样红。
    #[test]
    fn reading_cache_is_effectively_instant() {
        let dir = temp_dir("boot-speed");
        // 造一个接近本机真实体量的快照：本机 237 个仓库（2026-10-13 实测）
        let template = sample_group();
        let groups: Vec<AssetGroup> = (0..237)
            .map(|i| {
                let mut g = template.clone();
                g.id = format!("g{}", i);
                g.items[0].id = format!("/tmp/repo-{}", i);
                g.items[0].name = format!("repo-{}", i);
                g
            })
            .collect();
        cache::save(&dir, &groups).unwrap();

        let fastest = (0..5)
            .map(|_| {
                let start = std::time::Instant::now();
                let snap = cache::load(&dir).expect("读缓存失败");
                let elapsed = start.elapsed();
                assert_eq!(snap.groups.len(), 237);
                elapsed
            })
            .min()
            .unwrap();

        assert!(
            fastest < std::time::Duration::from_millis(200),
            "读缓存最快一次也要 {:?}，超过 200ms 就失去了「秒开」的意义",
            fastest
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ========================================================================
    // AI 工具数据目录
    //
    // 这个扫描器扫的是 9 个 AI 编码工具的数据目录，里面躺着 `auth.json`、
    // `config.toml` 这类凭据文件。核心约束是「只读文件名，不读文件内容」——
    // 凭据压根不进入数据流，不是读出来再抹掉。前两条测试守着这条。
    // ========================================================================

    const ADDITIONAL_AI_ROOTS: &[(&str, Option<&str>, &str)] = &[
        ("DeepSeek Harness", Some(".dsh"), ".dsh"),
        (
            "DeepSeek Harness 桌面版",
            Some("Library/Application Support/@deepseek-ai/dsh-desktop"),
            "AppData/Roaming/@deepseek-ai/dsh-desktop",
        ),
        (
            "Warp",
            Some(
                "Library/Group Containers/2BBY89MBSN.dev.warp/Library/Application Support/dev.warp.Warp-Stable",
            ),
            "AppData/Local/warp/Warp/data",
        ),
        ("Warp 配置", Some(".warp"), "AppData/Roaming/warp/Warp/data"),
        ("Warp 本地配置", None, "AppData/Local/warp/Warp/config"),
        ("OpenClaw", Some(".openclaw"), ".openclaw"),
        ("ZCode", Some(".zcode"), ".zcode"),
        (
            "ZCode 桌面版",
            Some("Library/Application Support/ZCode"),
            "AppData/Roaming/ZCode",
        ),
        ("灵码", Some(".lingma"), ".lingma"),
        ("灵码 本地数据", None, "AppData/Local/.lingma"),
        ("Qwen Agent", None, ".qwen-agent"),
        ("Qoder CLI", None, ".qoder-cli"),
        (
            "Qoder 桌面版",
            Some("Library/Application Support/com.qoder.app.stable"),
            "AppData/Roaming/com.qoder.app.stable",
        ),
        ("Qoder IDE", None, "AppData/Roaming/Qoder"),
    ];

    #[test]
    fn ai_tools_additional_roots_are_platform_scoped_and_read_only() {
        let home = temp_dir("ai_additional_roots");
        let content = "private-fixture-content-not-for-output";
        for (_, mac, win) in ADDITIONAL_AI_ROOTS {
            for rel in mac.iter().copied().chain(std::iter::once(*win)) {
                let root = home.join(rel);
                for name in ["logs", "sessions", "cache"] {
                    std::fs::create_dir_all(root.join(name)).unwrap();
                    std::fs::write(root.join(name).join("probe.txt"), content).unwrap();
                }
                std::fs::write(root.join("settings.json"), content).unwrap();
            }
        }
        std::fs::create_dir_all(home.join(".unlisted-agent/logs")).unwrap();

        for platform in [Platform::Mac, Platform::Windows] {
            let expected: HashMap<PathBuf, &str> = ADDITIONAL_AI_ROOTS
                .iter()
                .filter_map(|(label, mac, win)| {
                    let rel = if platform == Platform::Windows {
                        Some(*win)
                    } else {
                        *mac
                    };
                    rel.map(|rel| (home.join(rel), *label))
                })
                .collect();
            let g = AiToolsScanner::on(&home, platform).scan();
            assert_eq!(g.items.len(), expected.len() * 4, "{platform:?}");
            let roots: HashSet<PathBuf> =
                g.items.iter().map(|it| PathBuf::from(&it.path)).collect();
            assert_eq!(roots, expected.keys().cloned().collect(), "{platform:?}");
            assert_eq!(
                g.items
                    .iter()
                    .map(|it| &it.id)
                    .collect::<HashSet<_>>()
                    .len(),
                g.items.len()
            );
            assert!(!serde_json::to_string(&g).unwrap().contains(content));

            for it in &g.items {
                let root = PathBuf::from(&it.path);
                assert_eq!(it.group_key.as_deref(), Some(expected[&root]));
                assert!(!it.cleanable, "{} 只能统计", it.id);
                assert!(it.reclaimable.is_none(), "{} 不应报可回收量", it.id);
                assert!(!it.tags.iter().any(|t| t.key == "cleanable"));
                let (cat, parsed_root) = parse_ai_id(&it.id).unwrap();
                assert_eq!(parsed_root, root);
                let child = match cat {
                    Category::Log => "logs",
                    Category::Session => "sessions",
                    Category::Cache => "cache",
                    Category::Config => "settings.json",
                    other => panic!("意外类别 {other:?}"),
                };
                assert!(
                    matches!(it.metrics.get("size"), Some(MetricValue::Bytes(b)) if *b == tree_bytes(&root.join(child)))
                );
            }
        }
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ai_tools_additional_roots_reject_confirmed_cleaning() {
        let home = temp_dir("ai_additional_clean_reject");
        for (_, mac, win) in ADDITIONAL_AI_ROOTS {
            for rel in mac.iter().copied().chain(std::iter::once(*win)) {
                for name in ["logs", "sessions", "cache"] {
                    std::fs::create_dir_all(home.join(rel).join(name)).unwrap();
                }
            }
        }
        for platform in [Platform::Mac, Platform::Windows] {
            let scanner = AiToolsScanner::on(&home, platform);
            for (_, mac, win) in ADDITIONAL_AI_ROOTS {
                let rel = if platform == Platform::Windows {
                    Some(*win)
                } else {
                    *mac
                };
                let Some(rel) = rel else { continue };
                let root = home.join(rel);
                let ids: Vec<_> = [Category::Log, Category::Session, Category::Cache]
                    .into_iter()
                    .map(|cat| ai_id(&root, cat))
                    .collect();
                let result = scanner.clean_ai(&CleanPlan {
                    group_id: "ai_tools".into(),
                    item_ids: ids.clone(),
                    confirmed: true,
                    scope: CleanScope::Repo,
                    artifact_targets: None,
                });
                assert!(result.succeeded.is_empty());
                assert_eq!(result.freed_bytes, 0);
                assert_eq!(result.failed.len(), ids.len());
                for (failure, id) in result.failed.iter().zip(&ids) {
                    assert_eq!(&failure.id, id);
                    assert_eq!(
                        text_key(&failure.reason),
                        Some("data.cleanFail.readOnlyRoot")
                    );
                }
                for name in ["logs", "sessions", "cache"] {
                    assert!(root.join(name).is_dir());
                }
            }
        }
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ai_tools_additional_roots_require_existing_directories() {
        let home = temp_dir("ai_additional_missing");
        for platform in [Platform::Mac, Platform::Windows] {
            assert!(AiToolsScanner::on(&home, platform).scan().items.is_empty());
        }
        for (_, mac, win) in ADDITIONAL_AI_ROOTS {
            for rel in mac.iter().copied().chain(std::iter::once(*win)) {
                let root = home.join(rel);
                std::fs::create_dir_all(root.parent().unwrap()).unwrap();
                std::fs::write(root, "not a directory").unwrap();
            }
        }
        for platform in [Platform::Mac, Platform::Windows] {
            let scanner = AiToolsScanner::on(&home, platform);
            assert!(scanner.scan().items.is_empty());
            for (_, mac, win) in ADDITIONAL_AI_ROOTS {
                let rel = if platform == Platform::Windows {
                    Some(*win)
                } else {
                    *mac
                };
                let Some(rel) = rel else { continue };
                let result = scanner.clean_ai(&CleanPlan {
                    group_id: "ai_tools".into(),
                    item_ids: vec![ai_id(&home.join(rel), Category::Log)],
                    confirmed: true,
                    scope: CleanScope::Repo,
                    artifact_targets: None,
                });
                assert_eq!(result.failed.len(), 1);
                assert_eq!(
                    text_key(&result.failed[0].reason),
                    Some("data.cleanFail.notInAiWhitelist")
                );
            }
        }
        std::fs::remove_dir_all(home).unwrap();
    }

    /// 分类器是纯函数，用 fixture 全覆盖。
    ///
    /// 判错方向的代价不对称：把配置判成缓存会误删凭据，把缓存判成未知只是少显示
    /// 一块可回收空间。所以未知名字必须落到 `未知`——那个类别一律不开放清理。
    #[test]
    fn ai_tools_classifier_covers_known_names() {
        let cases: &[(&str, Category)] = &[
            ("logs", Category::Log),
            ("log", Category::Log),
            ("traces", Category::Log),
            ("debug", Category::Log),
            ("history.jsonl", Category::Log),
            ("session.log", Category::Log),
            ("ai-otel", Category::Log),
            ("sessions", Category::Session),
            ("projects", Category::Session),
            ("file-history", Category::Session),
            ("memory", Category::Session),
            ("cache", Category::Cache),
            ("snapshots", Category::Cache),
            ("paste-cache", Category::Cache),
            ("tool-output", Category::Cache),
            (".tmp", Category::Cache),
            ("foo.bak", Category::Cache),
            ("plugins", Category::Skill),
            ("skills", Category::Skill),
            ("extensions", Category::Skill),
            ("commands", Category::Skill),
            ("agents", Category::Skill),
            ("sqlite", Category::Index),
            ("logs_2.sqlite", Category::Index),
            ("thread_history_1.sqlite", Category::Index),
            ("config", Category::Config),
            ("auth.json", Category::Config),
            ("config.toml", Category::Config),
            ("settings.json", Category::Config),
            ("README.md", Category::Config),
            ("bin", Category::Runtime),
            ("binaries", Category::Runtime),
            ("models", Category::Runtime),
            // 未识别的一律落 Unknown：宁可少显示，不可误删
            ("edge-ksb-profile", Category::Unknown),
            ("workspace", Category::Unknown),
            ("file-tree-manifests", Category::Unknown),
            ("computer-use", Category::Unknown),
            ("whatever", Category::Unknown),
        ];

        for (name, want) in cases {
            assert_eq!(classify(name), *want, "classify({:?}) 判错", name);
        }
    }

    /// 结果层：扫描结果里不该出现任何凭据片段。
    #[test]
    fn ai_tools_scan_never_exposes_credentials() {
        let Some(home) = dirs::home_dir() else { return };
        let g = AiToolsScanner::new(home).scan();
        if g.items.is_empty() {
            return;
        }

        let json = serde_json::to_string(&g).unwrap();
        for needle in ["PRIVATE KEY", "api_key", "token=", "BEGIN RSA"] {
            assert!(
                !json.contains(needle),
                "扫描结果里出现了凭据片段：{}",
                needle
            );
        }
    }

    /// 源码层：比结果层更硬。
    ///
    /// 结果层只能证明「这次没漏」，挡不住后来者加一行文件读取。这条直接守住
    /// 「压根不打开文件」这个设计约束本身。
    ///
    /// needle 带左括号是为了避开 `read_dir`——遍历目录是允许的，读文件内容不是。
    #[test]
    fn ai_tools_source_never_reads_file_contents() {
        let src = include_str!("ai_tools.rs");
        for needle in ["fs::read(", "read_to_string(", "File::open(", "BufReader"] {
            assert!(
                !src.contains(needle),
                "ai_tools.rs 里出现了文件读取调用：{}",
                needle
            );
        }
    }

    /// 体积守恒：各类别之和不应超过工具根目录的实测大小。
    ///
    /// **容差是给「被测数据是活的」，不是给块对齐的。** 原生遍历没有取整误差，
    /// 而这个测试扫的是用户正在用的目录（日志在写、缓存在清）。先量各类别、
    /// 再量根目录，中间隔了几百毫秒——缓存被清掉一点，根目录就会偏小。
    /// 实测这条在整机忙碌时偶发假红，就是被这个抖动量绊的（Windows 实机上 `.workbuddy`
    /// 8 GB、日志一直在写，撞过 8.5 GB 报值 vs 8.2 GB 实测，差 3.7%）。所以容差给了
    /// 5% + 16 MB：够吞掉活跃日志目录的一次轮转，远够不着「真重复计算」那种成倍差距。
    ///
    /// 所以这里的定位是**粗筛**：真出现重复计算会差出成倍的体量（2x），
    /// 2% 的容差拦不住它。逐字节的精确性由 `dirsize_matches_independent_block_sum`
    /// 与 `dirsize_excludes_nested_subtree` 用临时目录守着，那两个不依赖机器状态。
    #[test]
    fn ai_tools_sizes_stay_within_root() {
        let Some(home) = dirs::home_dir() else { return };
        let g = AiToolsScanner::new(home).scan();
        if g.items.is_empty() {
            return;
        }

        let mut by_root: HashMap<&str, u64> = HashMap::new();
        for it in &g.items {
            if let Some(MetricValue::Bytes(b)) = it.metrics.get("size") {
                *by_root.entry(it.path.as_str()).or_default() += *b;
            }
        }
        assert!(!by_root.is_empty(), "没有解析出任何工具根目录");

        for (root, reported) in by_root {
            let actual = tree_bytes(Path::new(root));
            // 容差按「这个目录有多活跃」给，不是按块对齐给：日志类目录可以一边被扫
            // 一边被写、还会在扫完之后轮转变小，实测 8 GB 的 `.workbuddy` 撞过 300 MB
            // 偏差（≈3.7%）。重测一遍基准救不了——那时目录是真的小了，不是量抖了。
            // 这条的定位是**粗筛**：真重复计算会差出成倍体量，5% 拦不住也不该拦。
            let slack = actual / 20 + 16 * 1024 * 1024;
            assert!(
                reported <= actual + slack,
                "{} 各类别之和 {} 超过实测 {}（容差 {}），存在重复计算",
                root,
                human_bytes(reported),
                human_bytes(actual),
                human_bytes(slack)
            );
        }
    }

    /// 清理标志严格跟随语义类别：配置、未知、技能、索引一律不可清理。
    #[test]
    fn ai_tools_clean_flags_follow_category() {
        for (c, want) in [
            (Category::Log, true),
            (Category::Session, true),
            (Category::Cache, true),
            (Category::Skill, false),
            (Category::Index, false),
            (Category::Config, false),
            (Category::Runtime, false),
            (Category::Unknown, false),
        ] {
            assert_eq!(c.cleanable(), want, "{:?} 的 cleanable 应为 {}", c, want);
        }

        // 真实扫描出来的条目也必须遵守，不能只靠单测
        let Some(home) = dirs::home_dir() else { return };
        let g = AiToolsScanner::new(home).scan();
        for it in &g.items {
            let kind = match it.metrics.get("kind") {
                Some(MetricValue::Text(s)) => s.clone(),
                _ => continue,
            };
            if ["配置", "未知", "技能", "索引"].contains(&kind.as_str()) {
                assert!(
                    !it.cleanable,
                    "{} 属于{}类，不该标记为可清理",
                    it.name, kind
                );
                assert!(it.reclaimable.is_none(), "{} 不该有可回收体积", it.name);
            }
        }
    }

    /// 符号链接不跟随。软链目录要是按普通目录累加体积，会顺着它遍历到别的地方去，
    /// 体积和归属都是错的。
    #[cfg(unix)]
    #[test]
    fn ai_tools_scan_skips_symlinks() {
        let dir = temp_dir("ai-symlink");
        std::fs::create_dir_all(dir.join("real")).unwrap();
        std::fs::write(dir.join("real/a.log"), "x").unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        std::fs::write(dir.join("outside/big.log"), "y").unwrap();
        std::os::unix::fs::symlink(dir.join("outside"), dir.join("link")).unwrap();
        // 名字像日志的软链也不能进清理候选：软链按名字会分类成 Log，
        // 删它等于删了链接（或顺着删到别处），都不该发生
        std::os::unix::fs::symlink(dir.join("outside/big.log"), dir.join("linked.log")).unwrap();

        let (entries, skipped) = collect_entries_for_test(&dir);

        assert_eq!(skipped, 2, "软链应被跳过并计数");
        assert!(
            entries.iter().all(|e| e.name != "link"),
            "软链不该出现在条目里：{:?}",
            entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>()
        );
        assert!(
            clean_candidates(&dir, Category::Log)
                .iter()
                .all(|e| e.name != "linked.log"),
            "名字像日志的软链不该成为清理候选"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- ai_tools 清理入口（B1） ----

    /// 类别键是 id 的稳定部分，必须与展示用的中文标签解耦。
    #[test]
    fn ai_tools_category_keys_are_unique_and_round_trip() {
        for c in [
            Category::Log,
            Category::Session,
            Category::Cache,
            Category::Skill,
            Category::Index,
            Category::Config,
            Category::Runtime,
            Category::Unknown,
        ] {
            assert_eq!(Category::from_key(c.key()), Some(c), "{:?} 的键往返失败", c);
        }
        for bad in ["", "logs", "LOG", "unknown ", "ai", "log:"] {
            assert_eq!(
                Category::from_key(bad),
                None,
                "{:?} 不是合法类别键，该被拒绝",
                bad
            );
        }
    }

    /// id 必须能还原「工具根 + 类别」，且 Windows 盘符的冒号不参与分割。
    #[test]
    fn ai_tools_id_round_trips_through_root_and_category() {
        let mac_root = Path::new("/Users/x/.claude");
        let id = ai_id(mac_root, Category::Log);
        assert_eq!(id, "ai:log:/Users/x/.claude");
        assert_eq!(
            parse_ai_id(&id),
            Some((Category::Log, mac_root.to_path_buf()))
        );

        let win = r"ai:cache:C:\Users\x\AppData\Roaming\Code";
        assert_eq!(
            parse_ai_id(win),
            Some((
                Category::Cache,
                PathBuf::from(r"C:\Users\x\AppData\Roaming\Code")
            )),
            "Windows 盘符冒号之后整段都该是路径"
        );
    }

    /// 非本组形态一律拒，不猜路径（与 toolchain 的 `parse_cache_id` 同一条纪律）。
    #[test]
    fn ai_tools_id_parsing_rejects_foreign_forms() {
        for id in [
            "",
            "/Users/x/.claude", // repos 的裸路径
            "toolchain:/x",
            "ai:",
            "ai:log",
            "ai:log:",          // 类别后为空
            "ai::/Users/x",     // 类别键为空
            "ai:logs:/Users/x", // 标签不是键
            "ai:LOG:/Users/x",  // 键大小写敏感
        ] {
            assert_eq!(
                parse_ai_id(id),
                None,
                "{} 不是 ai_tools 的 id 形态，该被拒绝",
                id
            );
        }
    }

    /// 清理候选 = 点击那一刻重新分类后、仍属于该类别的一级子项。
    /// 配置 / 未知 / 其他类别都不能混进来。
    #[test]
    fn ai_tools_clean_candidates_follow_current_names() {
        let root = temp_dir("ai_clean_candidates");
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(root.join("app.log"), "x").unwrap();
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("auth.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("whatever")).unwrap();

        let names = |cat: Category| {
            let mut v: Vec<String> = clean_candidates(&root, cat)
                .into_iter()
                .map(|e| e.name)
                .collect();
            v.sort();
            v
        };

        assert_eq!(names(Category::Log), ["app.log", "logs"]);
        assert!(
            !names(Category::Log)
                .iter()
                .any(|n| n == "config" || n == "auth.json"),
            "日志候选里不该混进配置类条目"
        );
        assert_eq!(names(Category::Session), ["sessions"]);

        std::fs::remove_dir_all(&root).ok();
    }

    /// 拒绝路径全覆盖：全部在 `trash::delete` 之前返回，目录必须原样还在。
    /// **不测成功路径**——真删留给真人（AGENTS 硬约束 4）。
    #[test]
    fn ai_tools_clean_rejects_before_touching_trash() {
        let root = temp_dir("ai_clean_reject");
        let home = root.join("home");
        let tool_root = home.join(".claude");
        std::fs::create_dir_all(tool_root.join("logs")).unwrap();
        std::fs::create_dir_all(tool_root.join("sessions")).unwrap();
        std::fs::create_dir_all(tool_root.join("config")).unwrap();
        let scanner = AiToolsScanner::new(home.clone());

        let plan = |ids: Vec<String>, confirmed: bool| CleanPlan {
            group_id: "ai_tools".into(),
            item_ids: ids,
            confirmed,
            scope: CleanScope::Repo,
            artifact_targets: None,
        };

        let foreign_root = root.join("foreign-tool");
        std::fs::create_dir_all(foreign_root.join("logs")).unwrap();

        let cases: Vec<(Vec<String>, bool, &str)> = vec![
            (
                vec![ai_id(&tool_root, Category::Log)],
                false,
                "data.cleanFail.unconfirmed",
            ),
            (vec!["".into()], true, "data.cleanFail.invalidId"),
            (
                vec!["toolchain:/x".into()],
                true,
                "data.cleanFail.invalidId",
            ),
            // 解析合法但根不在本次白名单内（绝对路径，跨平台可构造）
            (
                vec![ai_id(&foreign_root, Category::Log)],
                true,
                "data.cleanFail.notInAiWhitelist",
            ),
            // 相对路径先被拒（不依赖白名单）
            (
                vec!["ai:log:relative/logs".into()],
                true,
                "data.cleanFail.notAbsolute",
            ),
            (
                vec![ai_id(&tool_root, Category::Config)],
                true,
                "data.cleanFail.categoryLocked",
            ),
            (
                vec![ai_id(&tool_root, Category::Cache)],
                true,
                "data.cleanFail.noCleanableEntries",
            ),
        ];

        for (ids, confirmed, want) in cases {
            let r = scanner.clean_ai(&plan(ids, confirmed));
            assert!(
                r.succeeded.is_empty(),
                "拒绝路径不该有成功项：{:?}",
                r.succeeded
            );
            assert_eq!(r.freed_bytes, 0, "拒绝路径不该计腾出空间");
            assert!(
                r.failed.iter().any(|f| text_key(&f.reason) == Some(want)),
                "期望拒绝原因 {:?}，实际 {:?}",
                want,
                r.failed
            );
        }

        // 所有拒绝都发生在删除之前
        assert!(tool_root.join("logs").is_dir());
        assert!(tool_root.join("sessions").is_dir());
        assert!(tool_root.join("config").is_dir());

        std::fs::remove_dir_all(&root).ok();
    }

    /// 账面纪律：标了 `cleanable` 的 ai_tools 行，id 必须能解析回
    /// 「可清理类别 + 本次白名单里的工具根」；组级能力标志必须为 true。
    #[test]
    fn ai_tools_cleanable_rows_all_have_a_deletion_path() {
        let root = temp_dir("ai_clean_rows");
        let home = root.join("home");
        let tool_root = home.join(".claude");
        std::fs::create_dir_all(tool_root.join("logs")).unwrap();
        std::fs::create_dir_all(tool_root.join("sessions")).unwrap();
        std::fs::create_dir_all(tool_root.join("config")).unwrap();

        let g = AiToolsScanner::new(home.clone()).scan();
        assert!(g.supports_clean, "ai_tools 有 clean_ai 实现，应声明能力");

        let mut cleanable = 0;
        for it in &g.items {
            let (cat, path) =
                parse_ai_id(&it.id).unwrap_or_else(|| panic!("ai_tools 的 id 应能解析：{}", it.id));
            assert_eq!(path, tool_root, "id 里的根路径应是本次扫描的工具根");
            assert_eq!(
                cat.cleanable(),
                it.cleanable,
                "类别可清理性与行标志不一致：{}",
                it.id
            );
            if it.cleanable {
                cleanable += 1;
                assert!(it.reclaimable.is_some(), "可清理行应有可回收量");
            }
        }
        assert_eq!(cleanable, 2, "logs 与 sessions 两行可清理");

        std::fs::remove_dir_all(&root).ok();
    }

    // ========================================================================
    // 扫描根目录（`roots` 模块）
    // ========================================================================

    /// 空根目录不能 panic：`dirs::home_dir()` 拿不到时 `default_roots()` 返回空，
    /// `RepoScanner` 就会拿到它。原来的 `WalkBuilder::new(&self.roots[0])` 会越界。
    ///
    /// 这条曾经还有第二个触发路径——用户在设置里清空全部根目录（C1）。设置页
    /// 2026-10-19 删掉了，空 roots 现在只剩 home 拿不到这一条路，守卫照旧留着。
    #[test]
    fn repo_scan_with_no_roots_reports_instead_of_panicking() {
        let g = RepoScanner::new(vec![]).scan();
        assert_eq!(g.summary, Text::key("data.summary.reposNoRoots"));
        assert!(g.items.is_empty());
        assert_eq!(g.error, None, "这是配置状态不是扫描失败");
    }

    #[test]
    fn repo_scan_deduplicates_overlapping_roots() {
        let dir = temp_dir("overlapping-roots");
        let child = dir.join("code");
        std::fs::create_dir_all(child.join("repo-a/.git")).unwrap();
        std::fs::create_dir_all(child.join("repo-b/.git")).unwrap();
        let baseline = RepoScanner::new(vec![child.clone()]).scan();
        let overlap = RepoScanner::new(vec![dir.clone(), child.clone(), child]).scan();
        let mut ids: Vec<_> = overlap.items.iter().map(|i| &i.id).collect();
        assert_eq!(ids.len(), 2);
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 2);
        assert_eq!(overlap.summary, baseline.summary);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scanners_with_roots_scans_only_the_given_roots() {
        let root = temp_dir("config-roots");
        std::fs::create_dir_all(root.join("alpha/.git")).unwrap();
        std::fs::create_dir_all(root.join("beta/.git")).unwrap();
        std::fs::create_dir_all(root.join("elsewhere")).unwrap(); // 不是仓库

        let scanners = crate::scanners_with_roots(vec![root.clone()]);
        let repos = scanners
            .iter()
            .find(|s| s.id() == "repos")
            .expect("repos 未注册");
        let g = repos.scan();

        let names: Vec<&str> = g.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names.len(), 2, "只该找到两个假仓库：{:?}", names);
        assert!(
            names.contains(&"alpha") && names.contains(&"beta"),
            "{:?}",
            names
        );

        // 空 roots → 同一个分组报「未配置根目录」而不是 panic
        let empty = crate::scanners_with_roots(vec![]);
        let repos_empty = empty
            .iter()
            .find(|s| s.id() == "repos")
            .expect("repos 未注册");
        assert_eq!(
            repos_empty.scan().summary,
            Text::key("data.summary.reposNoRoots"),
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// 新增 Scanner 的哨兵：把新 id 加进下面的名单，并给 `cache::SCHEMA_VERSION` +1。
    ///
    /// 启动逻辑是「有缓存直接返回，不扫描」，版本不变的话老用户拿到的是缺分组的
    /// 旧快照，界面上也没有任何提示告诉他该点「重扫」——新功能等于白做。
    ///
    /// 这里刻意用集合比对，而不是 `SCHEMA_VERSION >= 2` 那种常量断言：
    /// 后者编译期就定型，运行时永远为真，挡不住任何人。
    #[test]
    fn scanner_registration_is_pinned() {
        if dirs::home_dir().is_none() {
            return; // 拿不到 home 时 scanners() 接近空，无从比对
        }

        // 随环境条件注册的分组（装了才出现），先从 id 列表里剔掉。
        // wsl 是「装了 WSL 发行版才出现」，且只在 Windows 上——同 dev_env 一类。
        // ssh 与 docker 已随 H3 移出 scanners()（D6：两个分组都不进 V1），不在这两张名单里。
        // local_models（G2）同 toolchain：三个来源目录一个都不存在时不进侧栏。
        let conditional = [
            "toolchain",
            "java",
            "ios",
            "android",
            "harmony",
            "wsl",
            "local_models",
        ];
        let ids: Vec<&str> = crate::scanners().iter().map(|s| s.id()).collect();
        let unconditional: Vec<&str> = ids
            .iter()
            .copied()
            .filter(|id| !conditional.contains(id))
            .collect();

        assert_eq!(
            unconditional,
            ["repos", "ai_tools"],
            "无条件注册的扫描器集合或顺序变了：{:?}。新增 Scanner 时同步更新这里，并 SCHEMA_VERSION +1。顺序 = 侧栏顺序 = 固定语义序（D13 拍板）",
            ids
        );
        assert!(ids.contains(&"ai_tools"), "AI 工具扫描器未注册：{:?}", ids);
        assert!(ids.contains(&"repos"), "仓库扫描器未注册：{:?}", ids);

        // H14（2026-10-20 用户拍板）：java 与 harmony 暂时隐藏——不是「没装上」，
        // 是「装了也不给」。两头都钉：名单断言让本机没装这两组的机器也验得到
        // （缺席断言在那里会静默失真），缺席断言守真实注册结果。
        assert_eq!(
            crate::HIDDEN_GROUPS,
            ["java", "harmony"],
            "暂时隐藏的分组名单变了——拍板项，不是顺手改的地方：恢复/增删都连本断言与 SCHEMA_VERSION 一起动"
        );
        for hidden in crate::HIDDEN_GROUPS {
            assert!(
                !ids.contains(hidden),
                "{hidden} 应处于暂时隐藏状态（H14），却出现在注册结果里：{:?}",
                ids
            );
        }
    }

    /// D13 哨兵：注册顺序必须是 `GROUP_ORDER` 的子序列。
    ///
    /// 条件分组（toolchain / dev_env 四组 / local_models / wsl）本机不一定都注册，
    /// 所以不比对完整列表，只验实际出现的分组没有乱序、也没有漏登记。
    #[test]
    fn scanner_registration_follows_group_order() {
        if dirs::home_dir().is_none() {
            return;
        }
        let ids: Vec<&str> = crate::scanners().iter().map(|s| s.id()).collect();
        let mut rest = crate::scan::GROUP_ORDER;
        for id in ids {
            let pos = rest
                .iter()
                .position(|known| *known == id)
                .unwrap_or_else(|| panic!("{id} 不在 GROUP_ORDER 中，或注册顺序与它不符"));
            rest = &rest[pos + 1..];
        }
    }

    // ---- Docker ----
    //
    // 解析类测试全部喂 `docker system df -v` 的真实输出，不真跑 docker：
    // 守护进程没启动时这些测试也得能过。

    #[test]
    fn docker_parse_size_uses_si_units() {
        let p = parse_size_for_test;
        assert_eq!(p("0B"), Some(0));
        assert_eq!(p("201kB"), Some(201_000));
        assert_eq!(p("37.5MB"), Some(37_500_000));
        assert_eq!(p("1.12GB"), Some(1_120_000_000));
        assert_eq!(p("1.388MB"), Some(1_388_000));
        // docker 用 1000 进制，按 1024 解析会让 1GB 差出 7%
        assert_eq!(p("1GB"), Some(1_000_000_000));
        assert_eq!(p("N/A"), None);
        assert_eq!(p(""), None);
    }

    #[test]
    fn docker_parse_time_handles_both_formats() {
        let p = parse_time_for_test;
        // inspect 给的 UTC（卷）
        let a = p("2026-10-01T04:14:50Z").expect("RFC3339 解析失败");
        // image ls 给的带时区偏移
        let b = p("2026-09-30 02:11:52 +0800 CST").expect("带偏移的时间解析失败");
        // 同一个时刻的两种写法必须算出同一个 unix 秒
        let c = p("2026-09-29T18:11:52Z").expect("UTC 解析失败");
        assert_eq!(b, c, "+0800 的 02:11:52 应等于 UTC 的 前一天 18:11:52");
        assert!(a > 0 && b > 0);
        // 纳秒精度（inspect 的 Created 带小数）
        assert!(p("2026-09-29T18:11:52.517606715Z").is_some());
        assert_eq!(p("not a time"), None);
    }

    #[test]
    fn docker_parse_image_line_picks_fixed_columns() {
        // CREATED 的词数不定，这两种都得解析出同样的右侧四列
        let a = parse_image_line_for_test(
            "mysql                 8               6ea90827b110   11 days ago     1.12GB    0B            1.116GB       1",
        )
        .expect("三词 CREATED 的行解析失败");
        assert_eq!(a.repo, "mysql");
        assert_eq!(a.tag, "8");
        assert_eq!(a.id, "6ea90827b110");
        assert_eq!(a.size, 1_120_000_000);
        assert_eq!(a.shared, 0);
        assert_eq!(a.unique, 1_116_000_000);
        assert_eq!(a.containers, 1);

        let b = parse_image_line_for_test(
            "maven                 3.8-openjdk-8   ff18d86faefa   4 years ago     814MB     560.4MB       253.7MB       0",
        )
        .expect("共享层非零的行解析失败");
        assert_eq!(b.shared, 560_400_000);
        assert_eq!(b.unique, 253_700_000);
        assert_eq!(b.containers, 0);

        // 列数不足（格式变了）必须跳过而不是填错列
        assert!(parse_image_line_for_test("mysql 8 6ea90827b110 1.12GB").is_none());
    }

    #[test]
    fn docker_parse_volume_line() {
        let v = parse_volume_line_for_test(
            "repo-a-web_mysql-data                         1         278MB",
        )
        .expect("卷行解析失败");
        assert_eq!(v.name, "repo-a-web_mysql-data");
        assert_eq!(v.links, 1);
        assert_eq!(v.size, 278_000_000);

        // 卷段固定三列，多一列（卷名里出现空格）就跳过
        assert!(parse_volume_line_for_test("a b 1 278MB").is_none());
    }

    #[test]
    fn docker_parse_verbose_splits_sections_and_ignores_containers() {
        let sample = "\
Images space usage:

REPOSITORY            TAG             IMAGE ID       CREATED         SIZE      SHARED SIZE   UNIQUE SIZE   CONTAINERS
mysql                 8               6ea90827b110   11 days ago     1.12GB    0B            1.116GB       1
redis                 6               e7b96daa9a18   3 weeks ago     176MB     0B            175.8MB       1

Containers space usage:

CONTAINER ID   IMAGE                        COMMAND                  LOCAL VOLUMES   SIZE      CREATED       STATUS                      NAMES
35ed745d4d34   eclipse-temurin:21-jre       \"java -Xms256m -Xmx7…\"   0               201kB     2 days ago    Exited (143) 40 hours ago   rent-server-new

Local Volumes space usage:

VOLUME NAME                                                        LINKS     SIZE
repo-a-web_mysql-data                                         1         278MB
c0b54f5cc0d8cd4067dd8543cb3a7ab5e526c1589e18715d88b1d68bbd156f4fa   0         246.1MB

Build cache usage: 0B

CACHE ID   CACHE TYPE   SIZE      CREATED   LAST USED   USAGE     SHARED
";
        let (images, volumes, skipped) = parse_verbose_for_test(sample);
        assert_eq!(images.len(), 2, "镜像段应解析出 2 行");
        assert_eq!(volumes.len(), 2, "卷段应解析出 2 行");
        // 容器段与构建缓存段不做明细，也不该被计成「未解析」
        assert_eq!(skipped, 0, "容器段的行不该算进未解析：{skipped}");
        assert!(volumes.iter().any(|v| v.links == 0), "应识别出未被引用的卷");
    }

    /// 集成：本机装了 docker 就真扫一次，核对分组形状。
    /// 守护进程没启动时 scan() 会走 error 分支，这时不做断言——
    /// 上面那些纯函数测试已经覆盖了解析逻辑。
    #[test]
    fn docker_scan_returns_group_when_daemon_runs() {
        let Some(home) = dirs::home_dir() else { return };
        let s = DockerScanner::new(home);
        if !s.present() {
            return;
        }
        let g = s.scan();
        assert_eq!(g.id, "docker");
        // 前端只渲染前三个字段
        assert!(g.fields.len() <= 3, "字段最多三个：{:?}", g.fields.len());
        if g.error.is_some() {
            assert!(g.items.is_empty(), "出错时不该给出明细行");
            return;
        }
        assert!(!g.items.is_empty());
        for it in &g.items {
            assert!(it.id.starts_with("docker:"), "id 前缀不对：{}", it.id);
            // 分节渲染依赖这个分隔符：所有行都得带，否则整组退化成单节
            assert!(it.name.contains(" · "), "名称缺少分节分隔符：{}", it.name);
            assert!(
                it.metrics.contains_key("size") && it.metrics.contains_key("refs"),
                "缺少指标字段：{}",
                it.id
            );
            // 回收量只给未被引用的条目，且不含镜像的共享层
            if let Some(r) = it.reclaimable {
                assert!(it.cleanable, "有回收量就该标 cleanable：{}", it.id);
                let size = match it.metrics.get("size") {
                    Some(MetricValue::Bytes(b)) => *b,
                    _ => panic!("size 不是字节类型：{}", it.id),
                };
                assert!(r <= size, "可回收 {} 大于体积 {}：{}", r, size, it.id);
            }
        }
    }

    // ---- Java / iOS / Android / 鸿蒙 ----
    //
    // 移动端三个在本机都没装，测试必须能在这两种情况下都成立：
    // 「没装就不出现」和「装了就给出合法分组」都要覆盖。

    #[test]
    fn dev_env_version_parsers_read_directory_names() {
        assert_eq!(gradle_version_for_test("gradle-7.4.1-bin"), "7.4.1");
        assert_eq!(gradle_version_for_test("gradle-8.14.3-all"), "8.14.3");
        assert_eq!(jdk_version_for_test("jdk-17.jdk"), "17");
        assert_eq!(jdk_version_for_test("temurin-21.jdk"), "21");
        assert_eq!(jdk_version_for_test("zulu-17.0.9.jdk"), "17.0.9");

        // 主版本号决定谁被当成「在用」而保留
        assert_eq!(version_major_for_test("8.14.3"), 8);
        assert_eq!(version_major_for_test("7.4.1"), 7);
        assert_eq!(version_major_for_test(""), 0);
    }

    /// Maven 拆分落在二级 groupId 上：一级太粗（`org` 单独 907 MB），三级太细（上千个）。
    #[test]
    fn maven_rows_split_at_second_level() {
        let root = temp_dir("maven");
        let repo = root.join("repository");
        for d in ["org/apache/maven", "com/alibaba", "io/netty"] {
            std::fs::create_dir_all(repo.join(d)).unwrap();
        }
        // 体积靠真实字节撑起来，du 才有非零输出
        std::fs::write(repo.join("org/apache/maven/a.jar"), vec![0u8; 8192]).unwrap();
        std::fs::write(repo.join("com/alibaba/b.jar"), vec![0u8; 1024]).unwrap();

        let rows = maven_rows_for_test(&repo);
        let names: Vec<&str> = rows.iter().map(|(n, _)| n.as_str()).collect();

        assert_eq!(rows.len(), 3, "只应有二级目录：{:?}", names);
        assert!(!names.contains(&"org"), "一级前缀不该单独成行：{:?}", names);
        // Windows 的目录遍历给出 `\` 分隔符，groupId 断言按 Path 分量比较
        // （Windows 上对两种分隔符不敏感，Unix 上两边本来就一致）
        let is_group = |row: &str, group: &str| Path::new(row) == Path::new(group);
        assert!(
            names.iter().any(|n| is_group(n, "org/apache")),
            "缺少二级 groupId：{:?}",
            names
        );

        // 降序：大的在前，表格第一屏就是最该看的
        assert!(
            rows.windows(2).all(|w| w[0].1 >= w[1].1),
            "未按体积降序：{:?}",
            rows
        );

        // 只断言放了文件的两个：空目录（io/netty）本来就该是 0。
        // 这里不断言具体字节数——占用块数受块大小与文件系统影响，
        // 写死数字会在别的机器上假红。
        let size_of_row = |n: &str| {
            rows.iter()
                .find(|(k, _)| Path::new(k) == Path::new(n))
                .map(|(_, s)| *s)
        };
        assert!(
            size_of_row("org/apache").unwrap() > 0,
            "有文件却报 0：{:?}",
            rows
        );
        assert!(
            size_of_row("com/alibaba").unwrap() > 0,
            "有文件却报 0：{:?}",
            rows
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn java_scan_keeps_maven_out_of_reclaimable() {
        let Some(home) = dirs::home_dir() else { return };
        let s = JavaScanner::new(home);
        if !s.exists() {
            return;
        }

        let g = s.scan();
        assert_eq!(g.id, "java");
        assert!(g.fields.len() <= 3, "字段最多三个：{}", g.fields.len());
        assert!(!g.items.is_empty(), "Java 环境存在却扫不出条目");

        for it in &g.items {
            assert!(
                it.metrics.contains_key("size")
                    && it.metrics.contains_key("last_modified")
                    && it.metrics.contains_key("kind"),
                "缺少指标字段：{}",
                it.id
            );
            // 分节渲染依赖这个分隔符
            assert!(it.name.contains(" · "), "名称缺少分节分隔符：{}", it.name);
            if let Some(r) = it.reclaimable {
                assert!(it.cleanable, "有回收量就该标 cleanable：{}", it.id);
                assert!(r <= size_of(it).unwrap_or(0), "回收量大于体积：{}", it.id);
            }
        }

        // Maven 缓存删了要联网重拉，用户明确决定不计入可回收
        for it in g.items.iter().filter(|i| i.id.starts_with("java:mvn:")) {
            assert!(!it.cleanable, "Maven 不该标可回收：{}", it.id);
            assert!(it.reclaimable.is_none(), "Maven 不该给回收量：{}", it.id);
        }

        // JDK 多版本是主动选择，不是残留
        for it in g.items.iter().filter(|i| i.id.starts_with("java:jdk:")) {
            assert!(!it.cleanable, "JDK 不该标可回收：{}", it.id);
        }

        // Gradle 分发版：有多个时只保留最新的那个不计入可回收
        let dists: Vec<&Item> = g
            .items
            .iter()
            .filter(|i| i.id.starts_with("java:gradle:dist:"))
            .collect();
        if dists.len() > 1 {
            assert_eq!(
                dists.iter().filter(|i| !i.cleanable).count(),
                1,
                "多个分发版时应只保留一个不算可回收"
            );
        }
    }

    /// 探测型扫描器：环境不存在就不该出现在侧栏。
    ///
    /// 用一个空目录当 home，移动端三个（路径全在 home 下）都必须消失。
    /// JDK 装在系统目录、与 home 无关，所以 java 仍可能出现——这是有意为之。
    #[test]
    fn dev_env_probe_hides_absent_environments() {
        let root = temp_dir("dev_env_empty");
        let ids: Vec<&str> = dev_env_scanners(&root).iter().map(|s| s.id()).collect();

        // IDE 本体装在系统目录里（Xcode 装满约 12 GB，是 iOS 环境的大头），
        // 空 home 拦不住它——本机真装了对应 IDE 就跳过那条断言。
        //
        // **按平台给候选目录**：只写 `/Applications` 的话，Windows 上 read_dir 失败 →
        // has_ide 恒为 false → 三条断言全被跳过，这条测试在 Windows 上等于没跑。
        // 覆盖面的塌陷是静默的——它照样显示 ok。
        let has_ide = |prefix: &str| {
            ide_parents()
                .iter()
                .any(|dir| match std::fs::read_dir(dir) {
                    Ok(d) => d
                        .flatten()
                        .any(|e| e.file_name().to_string_lossy().starts_with(prefix)),
                    Err(_) => false,
                })
        };

        if !has_ide("Xcode") {
            assert!(!ids.contains(&"ios"), "空 home 下 iOS 不该出现：{:?}", ids);
        }
        if !has_ide("Android Studio") {
            assert!(
                !ids.contains(&"android"),
                "空 home 下 Android 不该出现：{:?}",
                ids
            );
        }
        if !has_ide("DevEco") {
            assert!(
                !ids.contains(&"harmony"),
                "空 home 下鸿蒙不该出现：{:?}",
                ids
            );
        }

        let envs = [
            ("Xcode", DevEnvScanner::ios(&root, Platform::current())),
            (
                "Android Studio",
                DevEnvScanner::android(&root, Platform::current()),
            ),
            ("DevEco", DevEnvScanner::harmony(&root, Platform::current())),
        ];
        for (ide, env) in &envs {
            if !has_ide(ide) {
                assert!(!env.any_present(), "空 home 下不该探测到 {} 环境", env.id());
            }
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 同 ai_tools：`dev_env.rs` 只读文件名与体积，不读文件内容。
    /// JDK 里有 cacerts，Android SDK 里可能有 debug keystore，读内容就有泄漏路径。
    #[test]
    fn dev_env_source_never_reads_file_contents() {
        let src = include_str!("dev_env.rs");
        for needle in ["fs::read(", "read_to_string(", "File::open(", "BufReader"] {
            assert!(
                !src.contains(needle),
                "dev_env.rs 里出现了文件读取调用：{}",
                needle
            );
        }
    }

    #[test]
    fn dev_env_scanner_registered_when_present() {
        let Some(home) = dirs::home_dir() else { return };
        let ids: Vec<&str> = crate::scanners().iter().map(|s| s.id()).collect();

        // 注册状态必须跟探测结果一致——装了就注册、没装就不注册。不能写成「java 必须
        // 已注册」：那会让测试的通过与否取决于机器上装没装 JDK，CI 的干净环境直接红。
        // 同 docker / toolchain 的口径。H14 起多一层：`HIDDEN_GROUPS` 里的分组装了也
        // 不注册（暂时隐藏），期望值因此是「探测到 且 不在隐藏名单」——本机装了 Java
        // 与 DevEco，这条就是「隐藏真的生效了」的实机证据。
        let expect_registered =
            |id: &str, present: bool| present && !crate::HIDDEN_GROUPS.contains(&id);

        let java_present = JavaScanner::new(home.clone()).exists();
        assert_eq!(
            ids.contains(&"java"),
            expect_registered("java", java_present),
            "Java 的注册状态与「探测到 且 未隐藏」不一致（探测到={java_present}）：{:?}",
            ids
        );

        // 移动端没装就不该出现在侧栏；装了则要给出合法分组（harmony 在隐藏名单里，装了也不给）
        for (id, env) in [
            ("ios", DevEnvScanner::ios(&home, Platform::current())),
            (
                "android",
                DevEnvScanner::android(&home, Platform::current()),
            ),
            (
                "harmony",
                DevEnvScanner::harmony(&home, Platform::current()),
            ),
        ] {
            let present = env.any_present();
            assert_eq!(
                ids.contains(&id),
                expect_registered(id, present),
                "{} 的注册状态与「探测到 且 未隐藏」不一致（探测到={}）：{:?}",
                id,
                present,
                ids
            );
        }
    }

    // ========================================================================
    // 工具链缓存
    // ========================================================================

    /// 路径解析是纯函数：喂假 home，出来的路径必须全部落在它下面。
    /// 这条守着「不读真实 home」——解析阶段碰不到真实目录。
    ///
    /// **显式钉 `Platform::Mac`**：下面抽查的三条都是 macOS 布局
    /// （`.npm` / `Library/pnpm/store` / `.cache/uv`）。若跟着 `Platform::current()` 走，
    /// 这条在 Windows runner 上会拿 Windows 布局去比 macOS 的期望值，必红。
    /// 路径断言必须自己声明在断哪个平台——这与「平台是传进来的值」是同一条道理。
    #[test]
    fn toolchain_sources_resolve_from_home() {
        let fake = PathBuf::from("/fake/home");
        let rows = sources_on_for_test(&fake, Platform::Mac);

        assert!(!rows.is_empty(), "源列表不该为空");
        for (section, short, path, _) in &rows {
            assert!(path.starts_with(&fake), "路径跑出了假 home：{:?}", path);
            assert!(!section.is_empty() && !short.is_empty(), "分节或行名为空");
        }

        // 抽查几个已知映射，防止手滑改错相对路径
        let by_short: HashMap<&str, &Path> = rows
            .iter()
            .map(|(_, short, path, _)| (short.as_str(), path.as_path()))
            .collect();
        assert_eq!(
            by_short.get("npm").copied(),
            Some(fake.join(".npm").as_path())
        );
        assert_eq!(
            by_short.get("pnpm 内容存储").copied(),
            Some(fake.join("Library/pnpm/store").as_path())
        );
        assert_eq!(
            by_short.get("uv").copied(),
            Some(fake.join(".cache/uv").as_path())
        );

        // 与 Java 分组刻意不重叠：.m2 和 .gradle 归 Java，这里不能出现
        assert!(
            !rows
                .iter()
                .any(|(_, _, p, _)| p.ends_with(".m2") || p.ends_with(".gradle")),
            "与 Java 分组重叠了"
        );
    }

    // ========================================================================
    // 平台路径（W2）
    //
    // 这一组的全部意义是：**本机是 macOS，但 Windows 那半边必须有人守**。
    // 所以平台是传进来的值（`Platform::Windows`），不是 `cfg`——`cfg` 一写，
    // 测试就只能跑当前平台那一半，另一侧和没写一样。
    // 「真机上那个目录到底在不在」只能实机核；映射表本身在这里守。
    // ========================================================================

    /// 白名单里每一条都要能在 Windows 上解析出来。
    ///
    /// `Library/` 是 macOS 专有布局。任何一条这类路径若没在 `win_rel` 里显式
    /// 给出 Windows 位置（或显式声明「Windows 没有」），就会落到兜底 arm——
    /// 那等于静默地去探一个必然不存在的路径，而且永远没人会发现。
    /// 兜底 arm 里有 `debug_assert`，这里把全部条目跑一遍触发它。
    #[test]
    fn every_whitelisted_cache_rel_resolves_on_windows() {
        let rels = all_rels_for_test();
        assert!(!rels.is_empty(), "白名单不该为空");

        for rel in &rels {
            let _ = resolve_rel_for_test(rel, Platform::Windows);
        }

        let unhandled: Vec<&str> = rels
            .iter()
            .copied()
            .filter(|r| {
                r.starts_with("Library/") && resolve_rel_for_test(r, Platform::Windows) == Some(r)
            })
            .collect();
        assert!(
            unhandled.is_empty(),
            "这些 macOS 专有路径没给 Windows 位置：{unhandled:?}"
        );
    }

    /// 官方文档写明的几个 Windows 位置逐个钉住。
    /// 只放**有把握**的那几条——钉错了会在实机上静默探不到。
    #[test]
    fn windows_cache_locations_are_pinned() {
        use crate::scan::platform::Platform::{Mac, Windows};
        let cases: Vec<(&'static str, Option<&'static str>, Option<&'static str>)> = vec![
            // npm 官方文档：Windows 是 %LocalAppData%\npm-cache，**不是 ~/.npm**
            (".npm", Some(".npm"), Some("AppData/Local/npm-cache")),
            (
                "Library/Caches/pip",
                Some("Library/Caches/pip"),
                Some("AppData/Local/pip/Cache"),
            ),
            (
                "Library/Caches/electron",
                Some("Library/Caches/electron"),
                Some("AppData/Local/electron/Cache"),
            ),
            (
                "Library/Caches/ms-playwright",
                Some("Library/Caches/ms-playwright"),
                Some("AppData/Local/ms-playwright"),
            ),
            // dot-dir 类跨平台通用，不该被改动
            (
                ".cargo/registry",
                Some(".cargo/registry"),
                Some(".cargo/registry"),
            ),
            (
                ".rustup/toolchains",
                Some(".rustup/toolchains"),
                Some(".rustup/toolchains"),
            ),
            ("go/pkg/mod", Some("go/pkg/mod"), Some("go/pkg/mod")),
            // GOCACHE 在 Windows 上是 %LOCALAPPDATA%\go-build
            (
                ".cache/go-build",
                Some(".cache/go-build"),
                Some("AppData/Local/go-build"),
            ),
            // Windows 没有 Homebrew
            (
                "Library/Caches/Homebrew",
                Some("Library/Caches/Homebrew"),
                None,
            ),
            // P1：Xcode 与模拟器只在 macOS，四条显式 None
            (
                "Library/Developer/Xcode/DerivedData",
                Some("Library/Developer/Xcode/DerivedData"),
                None,
            ),
            (
                "Library/Developer/Xcode/iOS DeviceSupport",
                Some("Library/Developer/Xcode/iOS DeviceSupport"),
                None,
            ),
            (
                "Library/Developer/Xcode/Archives",
                Some("Library/Developer/Xcode/Archives"),
                None,
            ),
            (
                "Library/Developer/CoreSimulator/Profiles/Runtimes",
                Some("Library/Developer/CoreSimulator/Profiles/Runtimes"),
                None,
            ),
            // G4：这几条原先写的是 **Linux** 的位置（`.cache/deno`），两个平台都探不到。
            // 本机没装 Deno 所以一直没人看见——这正是要钉住的原因。
            (
                "Library/Caches/deno",
                Some("Library/Caches/deno"),
                Some("AppData/Local/deno"),
            ),
            (
                "Library/Caches/composer",
                Some("Library/Caches/composer"),
                Some("AppData/Local/Composer"),
            ),
            // Coursier 的 macOS 目录名是**大写 C**（Coursier 不是 coursier）
            (
                "Library/Caches/Coursier",
                Some("Library/Caches/Coursier"),
                Some("AppData/Local/Coursier/Cache"),
            ),
            (
                ".local/share/NuGet/v3-cache",
                Some(".local/share/NuGet/v3-cache"),
                Some("AppData/Local/NuGet/v3-cache"),
            ),
            // NuGet global-packages 两平台一致（官方文档都是 ~/.nuget/packages）
            (
                ".nuget/packages",
                Some(".nuget/packages"),
                Some(".nuget/packages"),
            ),
            // G4：SwiftPM 迁到 Windows 默认布局；CocoaPods 依赖 Xcode，Windows 上没有
            (
                "Library/Caches/org.swift.swiftpm",
                Some("Library/Caches/org.swift.swiftpm"),
                Some("AppData/Local/org.swift.swiftpm"),
            ),
            (
                "Library/Caches/CocoaPods",
                Some("Library/Caches/CocoaPods"),
                None,
            ),
        ];
        for (rel, on_mac, on_win) in cases {
            assert_eq!(resolve_rel_for_test(rel, Mac), on_mac, "macOS 侧：{rel}");
            assert_eq!(
                resolve_rel_for_test(rel, Windows),
                on_win,
                "Windows 侧：{rel}"
            );
        }
    }

    /// 整条链路：假 home 里放 **Windows 布局**的目录，Windows 平台探得到、
    /// macOS 平台探不到。不依赖实机——那个 home 是我们自己造的。
    #[test]
    fn toolchain_finds_windows_layout_on_a_fake_home() {
        let home = temp_dir("win-home");
        std::fs::create_dir_all(home.join("AppData/Local/npm-cache")).unwrap();
        std::fs::create_dir_all(home.join("AppData/Local/pnpm/store")).unwrap();

        let win = ToolchainScanner::on(home.clone(), Platform::Windows);
        let mac = ToolchainScanner::on(home.clone(), Platform::Mac);

        assert!(win.exists_on(Platform::Windows), "Windows 布局没被认出来");
        assert!(
            !mac.exists_on(Platform::Mac),
            "macOS 侧不该去认 LOCALAPPDATA 下的东西"
        );

        let rows = sources_on_for_test(&home, Platform::Windows);
        let npm: Vec<&PathBuf> = rows
            .iter()
            .filter(|(_, s, _, _)| s == "npm")
            .map(|(_, _, p, _)| p)
            .collect();
        assert_eq!(npm.len(), 1, "npm 应当正好一行");
        assert!(
            npm[0].ends_with("AppData/Local/npm-cache"),
            "npm 的 Windows 路径不对：{:?}",
            npm[0]
        );
    }

    /// 鸿蒙是**倒挂**的：DevEco Studio 的主战场在 Windows，而原先只扫 macOS 路径。
    #[test]
    fn harmony_registers_on_windows_layout() {
        let home = temp_dir("harmony-win");
        std::fs::create_dir_all(home.join("AppData/Local/Programs/Huawei/DevEco Studio")).unwrap();
        std::fs::create_dir_all(home.join("AppData/Local/Huawei/Sdk")).unwrap();

        assert!(
            DevEnvScanner::harmony(&home, Platform::Windows).any_present(),
            "Windows 上的 DevEco 布局没被认出来"
        );
        assert!(
            !DevEnvScanner::harmony(&home, Platform::Mac).any_present(),
            "macOS 侧不该去认 %LOCALAPPDATA% 布局"
        );
    }

    /// iOS 组在 Windows 上不注册——Xcode 不存在，空分组不该占侧栏。
    /// 这不是特例处理，是「环境不存在就不注册」这条既有规则的自然结果。
    #[test]
    fn ios_group_is_absent_on_windows() {
        let home = temp_dir("ios-win");
        assert!(
            !DevEnvScanner::ios(&home, Platform::Windows).any_present(),
            "Windows 上不该注册 iOS 分组"
        );
    }

    /// 可回收边界：已安装的工具链与二进制一律不可回收，缓存一律可回收。
    /// 这条是口径测试——改分类时必须同步改这里，不能静默漂移。
    ///
    /// 走 `kinds_for_test`（不做平台解析）而不是 `sources_for_test`：口径是数据自身的
    /// 属性。`sources_on_for_test` 在 Windows 上会滤掉 Homebrew（`win_rel` 返回 `None`），
    /// 那条断言就只在 macOS 上验得到——**覆盖面不该取决于跑测试的机器**。
    #[test]
    fn toolchain_installed_artifacts_are_not_cleanable() {
        let rows = kinds_for_test();
        let by_short: HashMap<&str, bool> = rows
            .iter()
            .map(|(_, short, _, cleanable)| (short.as_str(), *cleanable))
            .collect();

        // 已安装产物：删了丢功能，留哪个版本是主动选择（同 JDK 口径）
        for installed in [
            "pnpm 的 Node",
            "Rustup 工具链",
            "Cargo 已装工具",
            "已装工具", // Go 组的行名，前缀由分节提供
        ] {
            assert_eq!(
                by_short.get(installed).copied(),
                Some(false),
                "{} 是已安装产物，不该标可回收",
                installed
            );
        }

        // 反例：nvm 的下载缓存是**可回收**的。它与已装版本是两回事——前者在
        // `~/.nvm/.cache`（下载下来的 tar.xz，装完即残留），后者在
        // `~/.nvm/versions`。G3 之前两者都没在账上，现在分开列。
        assert_eq!(
            by_short.get("nvm 下载缓存").copied(),
            Some(true),
            "nvm 下载缓存是下载暂存，该标可回收"
        );

        // P1：Xcode Archives 是打包产物不是缓存——删了丢历史归档，与「已安装」
        // 之外的第三种不可清理（无命令、非缓存）
        assert_eq!(
            by_short.get("Xcode Archives").copied(),
            Some(false),
            "Archives 是打包产物，不该标可回收"
        );

        // 缓存：删了按需重新下载，功能无损。
        //
        // **S3 起这条清单变短了**：有官方清理命令的那几条（npm / pnpm / uv / pip /
        // brew）不再标 `cleanable`——它们不是「不能删」，而是「本工具不代跑命令，
        // 所以不能声称自己点一下就能清掉」。见下面两条 S3 用例。
        for cache in [
            "Playwright 浏览器",
            "Rustup 下载缓存",
            "Cargo 依赖缓存",
            "nvm 下载缓存",
        ] {
            assert_eq!(
                by_short.get(cache).copied(),
                Some(true),
                "{} 是无官方命令的纯缓存，该标可清理",
                cache
            );
        }
    }

    /// S3 立的口径：**有官方清理命令的一律不标 `cleanable`**。
    ///
    /// 这条比「已装产物不可回收」更要紧——已装产物删了**丢功能**（明确不可删），
    /// 而有官方命令的缓存删了**也不丢功能**，只是下次要完整重建。容易被想成
    /// 「那它当然可以删啊」——但 `pnpm store prune` 只删没被引用的包，把整个
    /// store 目录搬走会让下次重下几个 GB。**账面标了可清理、点下去却要走一条
    /// 官方命令才能达到同等效果，那笔账就是虚的。**
    #[test]
    fn toolchain_sources_with_official_cmd_are_not_cleanable() {
        let rows = cleanable_and_cmd_for_test();

        // 白名单里带官方命令的，逐条核一次：这类改起来最容易被漏掉
        let with_cmd: Vec<&str> = rows
            .iter()
            .filter(|(_, _, cmd)| !cmd.is_empty())
            .map(|(short, _, _)| short.as_str())
            .collect();
        assert!(
            with_cmd.len() >= 6,
            "带官方命令的条目不该少于 6 条，实际 {}",
            with_cmd.len()
        );

        for (short, cleanable, cmd) in &rows {
            if !cmd.is_empty() {
                assert!(
                    !*cleanable,
                    "{} 有官方命令（{}），本工具不代跑命令，不能标可清理",
                    short, cmd
                );
            }
            // 无命令的两类：纯缓存（可清理）与已安装产物（不可清理）。
            // 这里只断前者——**已安装产物正是那种「无命令但不可清理」**，
            // 拿一条断言覆盖全体会把已装工具（pnpm 的 Node）也要求标可清理。
            // 那第三类由 `toolchain_installed_artifacts_are_not_cleanable` 守着。
        }

        // 反过来核：无官方命令的纯缓存一条都不能漏。
        // 逐条列出来是因为「漏标」是静默的——它只会让账面少几个 GB，
        // 没有任何报错提示少了。名单从 `sources()` 逐条对过（**不是照记忆写的**：
        // 少写一个断言等于少验一条，而多写一个不存在的名字会直接 panic）。
        let expected_cleanable = [
            "nvm 下载缓存",
            "bun",
            "node-gyp 头文件",
            "Playwright 浏览器",
            "Puppeteer 浏览器",
            "Cypress 二进制",
            "Electron",
            "Electron（XDG）",
            "Electron Builder",
            "TypeScript 版本",
            "Rustup 下载缓存",
            "Cargo 依赖缓存",
            "Cargo Git 依赖",
            "Codex",
            "acode",
            "vcpkg 二进制缓存",
            "Prisma 引擎",
            "Coursier",
            "Coursier（旧版）",
            // P1：可列出源的子行继承父源口径——父源是无命令纯缓存，账面（子行）可清理
            "Xcode DerivedData",
            "iOS 设备符号",
        ];
        let by_short: HashMap<&str, bool> = rows.iter().map(|(s, c, _)| (s.as_str(), *c)).collect();
        for short in expected_cleanable {
            match by_short.get(short) {
                Some(true) => {}
                other => panic!(
                    "{} 是无官方命令的纯缓存，该标可清理，实际 {:?}",
                    short, other
                ),
            }
        }

        // 总数对齐：41 条白名单 = 21 条无命令纯缓存 + 4 条已安装产物 + 15 条有命令的
        // 缓存 + 1 条打包产物（Xcode Archives，P1：无命令、非缓存、也不可清理）。
        // **四个数是双向的**：上面逐条列了 21 条可清理，若有人新加一条无命令缓存
        // 却不改这里，用例会红——这正是想要的（新增白名单条目必须过一次口径判断）。
        let cleanable_n = rows.iter().filter(|(_, c, _)| *c).count();
        let cmd_n = rows.iter().filter(|(_, _, cmd)| !cmd.is_empty()).count();
        assert_eq!(rows.len(), 41, "白名单条目数变了");
        assert_eq!(cmd_n, 15, "带官方命令的条目数变了");
        assert_eq!(
            cleanable_n, 21,
            "可清理条目数变了（新增白名单条目后要重新判断它有没有官方命令）"
        );
        assert_eq!(
            cmd_n + cleanable_n + 5,
            rows.len(),
            "三类之外还有条目：既无命令又不是纯缓存的，只该是已安装产物与打包产物"
        );
    }

    /// P1：可列出源逐子目录成行——DerivedData 按项目、设备符号按 iOS 版本。
    /// 合成一行只剩一个总数，「哪个项目最占、哪个版本最旧」全看不见（G3 同一条逻辑）。
    /// 全部喂假 home，通过与否不取决于本机装没装 Xcode。
    #[test]
    fn xcode_listable_sources_list_one_row_per_subdirectory() {
        let root = temp_dir("toolchain-xcode");
        let dd = root.join("Library/Developer/Xcode/DerivedData");
        std::fs::create_dir_all(dd.join("MyApp-abc123")).unwrap();
        std::fs::create_dir_all(dd.join("Other-def456")).unwrap();
        std::fs::write(dd.join("MyApp-abc123/index"), vec![0u8; 4096]).unwrap();
        let ds = root.join("Library/Developer/Xcode/iOS DeviceSupport");
        std::fs::create_dir_all(ds.join("14.5 (17F75)")).unwrap();

        let g = ToolchainScanner::on(root, Platform::Mac).scan();
        let names: Vec<&str> = g.items.iter().map(|i| i.name.as_str()).collect();

        assert!(
            names.contains(&"Xcode · MyApp-abc123"),
            "DerivedData 按项目成行：{:?}",
            names
        );
        assert!(
            names.contains(&"Xcode · Other-def456"),
            "空项目目录也成行（体积 0 是诚实数字）：{:?}",
            names
        );
        assert!(
            names.contains(&"Xcode · 14.5 (17F75)"),
            "设备符号按 iOS 版本成行：{:?}",
            names
        );

        for n in ["Xcode · MyApp-abc123", "Xcode · Other-def456"] {
            let it = g.items.iter().find(|i| i.name == n).unwrap();
            assert!(it.cleanable, "{} 是无命令纯缓存的子行，该标可清理", n);
            assert!(it.reclaimable.is_some(), "{} 该给可回收", n);
            assert_eq!(it.group_key.as_deref(), Some("Xcode"));
            assert!(
                it.id.starts_with("toolchain:"),
                "子行 id 必须能被 parse_cache_id 解析回路径"
            );
        }
    }

    /// P1：Archives 是打包产物、运行时删了要重下——两类都不可清理，
    /// 且运行时的行要把官方命令指给人（工具不代跑，S3 口径）。
    #[test]
    fn xcode_archives_and_runtimes_are_not_cleanable() {
        let root = temp_dir("toolchain-xcode-nc");
        let ar = root.join("Library/Developer/Xcode/Archives");
        std::fs::create_dir_all(&ar).unwrap();
        std::fs::write(ar.join("App 10-17.xcarchive"), vec![0u8; 1024]).unwrap();
        let rt = root.join("Library/Developer/CoreSimulator/Profiles/Runtimes");
        std::fs::create_dir_all(rt.join("iOS 17.4.simruntime")).unwrap();

        let g = ToolchainScanner::on(root, Platform::Mac).scan();

        let arch = g
            .items
            .iter()
            .find(|i| i.name == "Xcode · Xcode Archives")
            .expect("Archives 整目录成行");
        assert!(
            !arch.cleanable && arch.reclaimable.is_none(),
            "打包产物不可清理、不给可回收"
        );

        let rt_row = g
            .items
            .iter()
            .find(|i| i.name == "模拟器 · iOS 17.4.simruntime")
            .expect("运行时按版本成行");
        assert!(
            !rt_row.cleanable && rt_row.reclaimable.is_none(),
            "有官方命令的不标可清理（S3 口径）"
        );
        let clean_how = detail_value(rt_row, "cleanHow").expect("行明细要有「清理方式」");
        assert!(
            matches!(
                text_arg(clean_how, "cmd"),
                Some(TextArg::Raw(cmd)) if cmd.contains("xcrun simctl runtime delete")
            ),
            "行明细要指到官方命令：{:?}",
            clean_how
        );
    }

    /// P1：clean 路径的白名单复核——放行「可列出且可清理」源的子目录，
    /// 拒整目录（删除面对齐账面）、拒不可清理源的子目录、拒陌生人。
    /// 纯函数喂假 home，不真删（硬约束 4：trash 成功路径不测）。
    #[test]
    fn clean_verdict_accepts_listable_children_only() {
        let home = temp_dir("toolchain-verdict");
        let dd = home.join("Library/Developer/Xcode/DerivedData");
        std::fs::create_dir_all(dd.join("Proj")).unwrap();
        let store = home.join("Library/pnpm/store");
        std::fs::create_dir_all(&store).unwrap();
        let p = Platform::Mac;

        // 子目录命中可列出且可清理的源 → 放行
        assert!(
            clean_verdict_for_test(&home, p, &dd.join("Proj")).is_ok(),
            "可列出源的可清理子目录放行"
        );
        // 整目录命中可列出源 → 拒：账面只列子行，整目录入口比账面删得更多
        let err = clean_verdict_for_test(&home, p, &dd).unwrap_err();
        assert!(
            text_key(&err) == Some("data.cleanFail.perChildOnly"),
            "整目录删除要被拒，实际：{:?}",
            err
        );
        // 子目录命中不可列出源 → 拒（pnpm store 的孩子不在任何账面上）
        assert!(
            clean_verdict_for_test(&home, p, &store.join("v3")).is_err(),
            "非可列出源的子路径一律不在白名单"
        );
        // 精确命中无命令纯缓存 → 放行（原有口径不回退）
        assert!(
            clean_verdict_for_test(&home, p, &home.join(".nvm/.cache")).is_ok(),
            "精确命中无命令纯缓存放行"
        );
        // 精确命中已安装产物 → 拒，理由带类别
        let err = clean_verdict_for_test(&home, p, &home.join("Library/pnpm/nodejs")).unwrap_err();
        assert!(
            text_key(&err) == Some("data.cleanFail.noCleanEntry"),
            "实际：{:?}",
            err
        );
        // 陌生人 → 拒
        assert!(clean_verdict_for_test(&home, p, &home.join("somewhere/else")).is_err());
    }

    /// S3 的账本纪律：**标了 `cleanable` 的必须是「本工具点下去真能删掉」的**。
    ///
    /// 与 repos 那条 `repos_clean_candidate_mirrors_deletion_judgment` 是同一个道理的两面：
    /// 那条断的是「账面可回收不许含点下去会被拒的」，这条断的是「可清理不许含本工具
    /// 点不到的行」。两组的复核对象不同（repos 查外部 git 状态、toolchain 查白名单
    /// 与是否还有删除实现），但**账本必须等于真删**这条一样。
    #[test]
    fn toolchain_cleanable_rows_all_have_a_deletion_path() {
        // 有删除实现的标记就是 `supports_clean`；它为 true 而某行 cleanable 为 true 时，
        // 该行必须同时满足：路径是宿主机绝对路径（`toolchain:` 前缀 + 真路径），
        // 否则 clean_cache 的白名单复核会把它拒掉——那就是「账面说能删、真删被拒」。
        let g = ToolchainScanner::new(std::env::temp_dir());
        assert!(
            g.supports_clean(),
            "toolchain 有 clean_cache 实现，supports_clean 该为 true"
        );

        // 用假 home 造一份扫描结果，逐行验「cleanable ⇒ id 能解析成路径」。
        // 独立 oracle：`parse_cache_id` 是纯函数，这里只断它接受自己造出来的形态。
        for id in [
            "toolchain:/tmp/a/.npm",
            "toolchain:/Users/x/Library/pnpm/store",
        ] {
            assert_eq!(
                parse_cache_id_for_test(id),
                Some(std::path::PathBuf::from(
                    id.strip_prefix("toolchain:").unwrap()
                )),
                "toolchain 的 id 应能去掉前缀还原成路径：{}",
                id
            );
        }
    }

    /// `Item.id` → 路径的去前缀，对**非 toolchain 形态的 id 一律拒绝**。
    ///
    /// 为什么值得单独断：S3 之前 `clean_repo` 直接 `PathBuf::from(id)` 整个当路径用，
    /// 在只有一种 id 形态时恰好能跑。`clean_cache` 若也那么写，
    /// `toolchain:/Users/…` 会被当成一个以 `toolchain:` 开头的**相对路径**，
    /// `trash::delete` 拿它去删会报一个没人看得懂的错。返回 `None` 让服务端拒掉，
    /// 比让它去猜一个路径安全。
    #[test]
    fn toolchain_id_parsing_rejects_foreign_forms() {
        for id in [
            "/Users/x/repo-e",           // repos 的裸路径
            "ai:skill:/Users/x/.vscode", // ai_tools 的带前缀形态
            "toolchain",                 // 只有前缀没路径
            "",                          // 空
            "toolchain:",                // 前缀后为空
        ] {
            assert_eq!(
                parse_cache_id_for_test(id),
                None,
                "{} 不是 toolchain 的 id 形态，该被拒绝而不是猜路径",
                id
            );
        }
    }

    /// 探测语义：只建了哪几个目录，扫描结果就该只有哪几行。
    /// 全部用临时目录，不碰真实 home。
    ///
    /// 显式钉 `Platform::Mac`：造的是 `.npm` 与 `.cache/uv`，这是 macOS 布局。
    /// 跟着 `new()`（= `Platform::current()`）走的话，Windows 上会去探
    /// `AppData/Local/npm-cache`，一个都探不到，`items.len() == 2` 必红。
    #[test]
    fn toolchain_scan_skips_absent_sources() {
        let root = temp_dir("toolchain_partial");
        // 样本换成**无官方清理命令**的缓存（Playwright 浏览器 / Rustup 下载缓存）。
        // 原来这里用 `.npm` 与 `.cache/uv`——S3 起那两条都有官方命令
        // （`npm cache clean --force` / `uv cache clean`），不再标 cleanable，
        // 拿它们当「该全可回收」的样本就变成用例外行的口径去断新口径。
        std::fs::create_dir_all(root.join("Library/Caches/ms-playwright")).unwrap();
        std::fs::create_dir_all(root.join(".rustup/downloads")).unwrap();

        let g = ToolchainScanner::on(root.clone(), Platform::Mac).scan();

        assert_eq!(
            g.items.len(),
            2,
            "只该有 Playwright 浏览器与 Rustup 下载缓存两行：{:?}",
            g.items.iter().map(|i| &i.name).collect::<Vec<_>>()
        );
        assert!(
            g.items.iter().all(|i| i.cleanable),
            "两行都是无官方命令的纯缓存，该全可回收"
        );
        assert!(
            g.items.iter().all(|i| i.name.contains(" · ")),
            "名称缺少分节分隔符"
        );
        assert!(
            g.items.iter().all(|i| i.id.starts_with("toolchain:")),
            "id 前缀不对"
        );
        assert!(
            g.items
                .iter()
                .all(|i| i.tags == vec![Tag::new("cleanable")]),
            "标签只剩可清理"
        );

        let (count, total, cleanable_count, reclaimable) = summarize_for_test(&g.items);
        assert_eq!(count, 2);
        assert_eq!(cleanable_count, 2);
        assert_eq!(reclaimable, total, "全是缓存时可回收量应等于合计");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 可回收量必须把已安装产物排除在外，且不超过合计。
    /// 同样显式钉 `Platform::Mac`：造的是 `.rustup/toolchains` 与
    /// `.rustup/downloads`（后者是纯缓存且**无官方命令**，所以可回收——用
    /// `.npm` 就断了，那条有 `npm cache clean --force`，S3 起不可回收）。
    #[test]
    fn toolchain_reclaimable_excludes_installed() {
        let root = temp_dir("toolchain_mixed");
        std::fs::create_dir_all(root.join(".rustup/toolchains")).unwrap();
        std::fs::create_dir_all(root.join(".rustup/downloads")).unwrap();

        let g = ToolchainScanner::on(root.clone(), Platform::Mac).scan();

        assert_eq!(g.items.len(), 2, "该有 Rustup 工具链与下载缓存两行");
        let installed = g
            .items
            .iter()
            .find(|i| i.name.ends_with("Rustup 工具链"))
            .expect("缺少 Rustup 工具链行");
        assert!(!installed.cleanable, "已安装产物不该可回收");
        assert!(installed.reclaimable.is_none(), "已安装产物不该给回收量");
        assert!(installed.tags.is_empty(), "已安装产物不该有可清理标签");

        let (_, total, cleanable_count, reclaimable) = summarize_for_test(&g.items);
        assert_eq!(cleanable_count, 1, "只有下载缓存那一行可回收");
        assert!(reclaimable <= total, "可回收量不能超过合计");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 一个缓存目录都没有时，Scanner 不该被注册（侧栏不出现空分组）。
    #[test]
    fn toolchain_hidden_when_nothing_present() {
        let root = temp_dir("toolchain_empty");
        assert!(
            !ToolchainScanner::new(root.clone()).exists(),
            "空 home 下工具链缓存不该存在"
        );
        let g = ToolchainScanner::new(root.clone()).scan();
        assert!(g.items.is_empty(), "空 home 扫描结果该为空");
        assert!(
            g.notes
                .as_ref()
                .map(|n| n
                    .iter()
                    .any(|s| text_key(s) == Some("data.note.toolchain.empty")))
                .unwrap_or(false),
            "该提示白名单目录均不存在"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 注册状态与探测结果必须一致，否则侧栏会出现点了没数据的分组。
    #[test]
    fn toolchain_registered_when_present() {
        let Some(home) = dirs::home_dir() else { return };
        let ids: Vec<&str> = crate::scanners().iter().map(|s| s.id()).collect();
        let present = ToolchainScanner::new(home).exists();
        assert_eq!(
            ids.contains(&"toolchain"),
            present,
            "工具链缓存的注册状态与探测结果不一致：{:?}",
            ids
        );
    }

    /// 同 ai_tools / dev_env：只读文件名与体积，不读文件内容。
    #[test]
    fn toolchain_source_never_reads_file_contents() {
        let src = include_str!("toolchain.rs");
        for needle in ["fs::read(", "read_to_string(", "File::open(", "BufReader"] {
            assert!(
                !src.contains(needle),
                "toolchain.rs 里出现了文件读取调用：{}",
                needle
            );
        }
    }

    // ========================================================================
    // 版本管理器（G3）
    //
    // 这一组的判据是「一个版本管理器 = 若干版本目录 = 若干行」，
    // 与上面那批「一个目录 = 一行」的结构不同，所以单列。
    //
    // 全部喂临时目录、不依赖本机装了什么——本机只装了 nvm，靠真机数据
    // 只能验到一个管理器，其余六个就没人验过。
    // ========================================================================

    /// 逐版本出行：nvm 下三个版本就该是三行，不合成一行。
    ///
    /// **行名带版本号**（`nvm · v20.19.5`）是这一节的核心价值：合成一行报
    /// 「2.7 GB」就答不出「哪个版本最占地方」。
    #[test]
    fn nvm_reports_one_row_per_installed_version() {
        let root = temp_dir("g3_nvm_versions");
        for v in ["v18.0.0", "v20.19.5", "v22.19.0"] {
            std::fs::create_dir_all(root.join(".nvm/versions/node").join(v).join("bin")).unwrap();
        }

        let items = version_items_for_test(&root, Platform::Mac);

        assert_eq!(items.len(), 3, "三个版本就该三行：{items:?}");
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        for v in ["v18.0.0", "v20.19.5", "v22.19.0"] {
            assert!(
                names.contains(&format!("nvm · {v}").as_str()),
                "缺少版本行：{names:?}"
            );
        }
        // 分节与 group_key 一致（全库约定），前端靠它把小节标题和剥前缀对上
        for it in &items {
            assert_eq!(it.group_key.as_deref(), Some("nvm"));
            assert!(it.name.starts_with("nvm · "), "行名缺分节前缀：{}", it.name);
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 版本目录里的散落文件不是版本：`.DS_Store` / `.nvmrc` 不该各自成行。
    ///
    /// 这条守的是 `version_rows` 里「只列目录」那一句。本机 `~/.nvm/versions/node`
    /// 里就躺着一个 8 KB 的 `.DS_Store`，漏了过滤会多出一行 0 B 的假版本。
    #[test]
    fn version_rows_ignore_loose_files() {
        let root = temp_dir("g3_loose_files");
        let versions = root.join(".nvm/versions/node");
        std::fs::create_dir_all(versions.join("v20.19.5")).unwrap();
        std::fs::write(versions.join(".DS_Store"), b"x").unwrap();
        std::fs::write(versions.join(".nvmrc"), b"20").unwrap();

        let items = version_items_for_test(&root, Platform::Mac);

        assert_eq!(items.len(), 1, "只有 v20.19.5 该成行：{items:?}");
        assert!(items[0].name.ends_with("v20.19.5"));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 口径：**版本一律不可回收**。删一个版本是主动选择（可能还有项目 pin 在上面），
    /// 与 Rustup 工具链同口径。给「可回收 2.7 GB」是误导——本机 nvm 的 9 个版本里
    /// 有 4 个是两年多前装的，但没人知道哪个项目还在用。
    #[test]
    fn installed_versions_are_never_cleanable() {
        let root = temp_dir("g3_versions_not_cleanable");
        // 每个版本放一个文件：空目录的占用是 0，那样 assert!(total > 0) 红得没有意义
        for v in ["v18.0.0", "v20.19.5"] {
            std::fs::create_dir_all(root.join(".nvm/versions/node").join(v)).unwrap();
            std::fs::write(
                root.join(".nvm/versions/node").join(v).join("bin"),
                vec![0u8; 4096],
            )
            .unwrap();
        }

        let items = version_items_for_test(&root, Platform::Mac);
        assert!(!items.is_empty(), "该有版本行");
        for it in &items {
            assert!(!it.cleanable, "{} 不该可回收", it.name);
            assert!(it.reclaimable.is_none(), "{} 不该给回收量", it.name);
            assert!(
                it.tags.is_empty(),
                "{} 不该有「可清理」标签（永不可回收）",
                it.name
            );
        }

        // 汇总口径：版本行进了 items 就得计入合计，但一克回收量都不贡献
        let g = ToolchainScanner::on(root.clone(), Platform::Mac).scan();
        let (_, total, cleanable_count, reclaimable) = summarize_for_test(&g.items);
        assert!(total > 0, "版本目录该有体积");
        assert_eq!(cleanable_count, 0, "版本行一个都不该可回收");
        assert_eq!(reclaimable, 0, "版本行不该贡献任何回收量");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 体积口径：每个版本目录的体积是自己那份，**不共用父目录的数**。
    ///
    /// 这条是 G3 唯一有「算错」空间的地方——`dir_sizes_batch` 传父目录进去
    /// 的话 3 个版本会各报一遍总量，看起来全对、小计却翻三倍。用文件数当 oracle
    /// （自备，不拿被测代码当基准）：每版放 3 个已知大小的小文件，
    /// 于是「体积 > 0 且各不相同」与「小计 == 各行之和」都必须成立。
    #[test]
    fn version_rows_size_each_version_separately() {
        let root = temp_dir("g3_version_sizes");
        let versions = root.join(".nvm/versions/node");
        std::fs::create_dir_all(&versions).unwrap();
        // 各版放不同数量的 1 KB 文件
        for (v, n) in [("v18.0.0", 2usize), ("v20.19.5", 5), ("v22.19.0", 9)] {
            std::fs::create_dir_all(versions.join(v)).unwrap();
            for i in 0..n {
                std::fs::write(versions.join(v).join(format!("f{i}")), vec![0u8; 1024]).unwrap();
            }
        }

        let items = version_items_for_test(&root, Platform::Mac);
        let size_of = |name: &str| -> u64 {
            items
                .iter()
                .find(|i| i.name == name)
                .and_then(|i| match i.metrics.get("size") {
                    Some(MetricValue::Bytes(v)) => Some(*v),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("缺少 {name}"))
        };

        let small = size_of("nvm · v18.0.0");
        let mid = size_of("nvm · v20.19.5");
        let big = size_of("nvm · v22.19.0");
        assert!(
            small > 0 && small < mid && mid < big,
            "三版体积该递增：{small} {mid} {big}"
        );

        // 小计等于各行之和——若父目录被重复计，这一条会红
        let sum = small + mid + big;
        let (_, total, _, _) = summarize_for_test(&items);
        assert_eq!(
            total, sum,
            "小计应等于各版本之和（各行已含目录块开销，故只校验相等）"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 多个版本管理器同时在场，各成各的小节，互不串味。
    ///
    /// 覆盖的是「候选根逐个展开」那段：一个管理器可能有好几个候选路径
    /// （conda 的 miniconda3 / anaconda3 / conda），全都存在时都要扫到，
    /// 只取第一个会让另一种装法的机器上看不到自己的环境。
    #[test]
    fn several_version_managers_coexist() {
        let root = temp_dir("g3_multi_manager");
        std::fs::create_dir_all(root.join(".nvm/versions/node/v20.19.5")).unwrap();
        std::fs::create_dir_all(root.join(".pyenv/versions/3.11.9")).unwrap();
        std::fs::create_dir_all(root.join(".pyenv/versions/3.12.1")).unwrap();
        // conda 的第二个候选也放一个环境：只取第一个候选就会漏掉它
        std::fs::create_dir_all(root.join("anaconda3/envs/data")).unwrap();

        let items = version_items_for_test(&root, Platform::Mac);

        let sections: HashSet<&str> = items
            .iter()
            .filter_map(|i| i.group_key.as_deref())
            .collect();
        assert_eq!(
            sections,
            HashSet::from(["nvm", "pyenv", "conda"]),
            "三个管理器该各成小节：{sections:?}"
        );
        // conda 的 envs 是「环境」不是版本，行名就是环境名
        assert!(
            items.iter().any(|i| i.name == "conda · data"),
            "第二个候选根下的环境被漏了：{:?}",
            items.iter().map(|i| &i.name).collect::<Vec<_>>()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 没装版本管理器时不该出现版本行，也不该让分组凭空注册。
    #[test]
    fn absent_version_managers_add_no_rows() {
        let root = temp_dir("g3_absent");
        std::fs::create_dir_all(root.join(".npm")).unwrap(); // 只有缓存，没有版本管理器

        let g = ToolchainScanner::on(root.clone(), Platform::Mac).scan();
        assert_eq!(g.items.len(), 1, "只有 npm 一行：{g:?}");
        assert!(!g.items[0].name.contains(" · v"), "不该有版本行");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 装了版本管理器、但缓存目录全空时，分组仍该在侧栏里。
    ///
    /// 这是 G3 对 `exists_on` 的唯一改动：原来只看缓存白名单。
    /// 真实场景很常见——开发者清了所有缓存但版本还在（nvm 的 2.7 GB 不会随手删）。
    #[test]
    fn version_manager_alone_keeps_group_registered() {
        let root = temp_dir("g3_only_versions");
        std::fs::create_dir_all(root.join(".nvm/versions/node/v20.19.5")).unwrap();

        assert!(
            ToolchainScanner::on(root.clone(), Platform::Mac).exists(),
            "只装了 nvm 时分组该注册"
        );

        let g = ToolchainScanner::on(root.clone(), Platform::Mac).scan();
        assert_eq!(g.items.len(), 1);
        assert_eq!(g.items[0].name, "nvm · v20.19.5");
        assert!(
            g.notes
                .as_ref()
                .map(|n| n
                    .iter()
                    .any(|s| text_key(s) == Some("data.note.toolchain.versionManagers")))
                .unwrap_or(false),
            "notes 该说明版本管理器的口径：{:?}",
            g.notes
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 版本管理器的 Windows 侧映射在本机被断言（硬约束 9）。
    ///
    /// 平台是传进来的值，所以这张表在 macOS 上也能验。**`Library/` 一条都不能有**
    /// ——那是 macOS 专有布局，落到 Windows 上等于静默探一个必然不存在的路径。
    ///
    /// 几条有把握的单独钉住（官方文档 / 已知约定）：
    /// - nvm-windows 的 `NVM_HOME` 默认 `%APPDATA%\nvm`
    /// - fnm 的 `FNM_DIR` 在 Windows 上默认 `%LOCALAPPDATA%\fnm`
    /// - dot-dir 类（.pyenv / .rbenv / .asdf）两平台同路径
    #[test]
    fn version_manager_windows_paths_are_pinned() {
        use crate::scan::platform::Platform::{Mac, Windows};
        let rows = version_managers_for_test(Windows);
        assert!(!rows.is_empty(), "版本管理器白名单不该为空");

        for (name, rels) in &rows {
            assert!(!rels.is_empty(), "{name} 没给 Windows 路径");
            for r in rels {
                assert!(
                    !r.starts_with("Library/"),
                    "{name} 的 {r} 是 macOS 专有布局，不该出现在 Windows 侧"
                );
            }
        }

        let by_name: HashMap<&str, &Vec<&'static str>> =
            rows.iter().map(|(n, r)| (n.as_str(), r)).collect();

        // nvm-windows 与 macOS 上的 nvm 是两个不同实现，路径本来就不同
        assert!(
            by_name["nvm"].contains(&"AppData/Roaming/nvm"),
            "nvm 的 Windows 位置不对：{:?}",
            by_name["nvm"]
        );
        assert_eq!(
            version_managers_for_test(Mac)
                .into_iter()
                .find(|(n, _)| n == "nvm")
                .map(|(_, r)| r),
            Some(vec![".nvm/versions/node"]),
            "nvm 的 macOS 位置不该被 Windows 那条带偏"
        );

        assert!(
            by_name["fnm"].contains(&"AppData/Local/fnm/node-versions"),
            "fnm 的 Windows 位置不对：{:?}",
            by_name["fnm"]
        );

        // dot-dir 类两平台应当完全一致
        for dot in ["pyenv", "rbenv", "asdf"] {
            let mac: Vec<&str> = version_managers_for_test(Mac)
                .into_iter()
                .find(|(n, _)| n == dot)
                .map(|(_, r)| r)
                .unwrap();
            let win: Vec<&str> = by_name[dot].clone();
            assert_eq!(mac, win, "{dot} 是 dot-dir，两平台路径应一致");
        }
    }

    // ========================================================================
    // 语言缓存补齐（G4）
    //
    // **本机一条都不装**（Deno / NuGet / Composer / Coursier / SwiftPM /
    // CocoaPods 全无），所以这一组只能靠假 home 验证，不能靠真机数据——
    // 也就是说下面这些断言守的是「映射表本身」，不是「本机扫出来的数」。
    // 这是与 G3 版本管理器一样的处境，但更极端：G3 至少有一个（nvm）真机验过。
    // ========================================================================

    /// G4 新增的七条**全部可回收**：都是「纯下载缓存，删了按需重下」。
    ///
    /// 这条是口径测试——新增/改分类时必须同步改这里。它同时守一个反向事实：
    /// **这些条目在白名单里存在，但本机一个都不该出现在扫描结果里**
    /// （`absent_language_caches_stay_off_the_ledger`）。
    ///
    /// **S3 起这条断言翻了向**：G4 当时核的是「它们都是纯下载缓存，所以都标可回收」，
    /// 但 NuGet / Composer / Coursier / SwiftPM / CocoaPods / Deno 六条**各自都有官方
    /// 清理命令**（`dotnet nuget locals … --clear` / `composer clear-cache` /
    /// `swift package purge-cache` / `pod cache clean --all` / `deno clean`），
    /// 而 Coursier 与旧版 Coursier 没有——于是它们按「有没有官方命令」分成了两半。
    /// 这不是把 G4 推翻，是它当初问的问题在 S3 之后不够用了：G4 问「删了会不会丢功能」，
    /// S3 问的是「本工具点下去能不能真删掉」，**后者才是账面该报的东西**。
    #[test]
    fn g4_language_caches_split_by_official_cmd() {
        let rows = cleanable_and_cmd_for_test();
        let by_short: HashMap<&str, (bool, &str)> = rows
            .iter()
            .map(|(s, c, cmd)| (s.as_str(), (*c, *cmd)))
            .collect();

        // G4 收录的八条，一条都不能从白名单里消失（消失说明被删或改名了）
        for name in [
            "NuGet 全局包",
            "NuGet HTTP 缓存",
            "Composer",
            "Coursier",
            "Coursier（旧版）",
            "SwiftPM",
            "CocoaPods",
            "Deno",
        ] {
            assert!(
                by_short.contains_key(name),
                "{} 从白名单里消失了（G4 收录的八条不该被删或改名）",
                name
            );
        }

        // 有官方命令的六条：不标可清理，命令留在 detail 里让人自己跑
        for name in [
            "NuGet 全局包",
            "NuGet HTTP 缓存",
            "Composer",
            "SwiftPM",
            "CocoaPods",
            "Deno",
        ] {
            let (cleanable, cmd) = by_short[name];
            assert!(
                !cmd.is_empty(),
                "{} 该有官方清理命令（G4 补的六条都有）",
                name
            );
            assert!(
                !cleanable,
                "{} 有官方命令（{}），S3 起不标可清理",
                name, cmd
            );
        }

        // Coursier 与旧版那条没有官方命令，仍按纯缓存标可回收
        for name in ["Coursier", "Coursier（旧版）"] {
            let (cleanable, cmd) = by_short[name];
            assert!(cmd.is_empty(), "{} 没有官方清理命令", name);
            assert!(cleanable, "{} 是无官方命令的纯缓存，仍该标可清理", name);
        }
    }

    /// G4 的条目在本机**一条都不该出现**，Windows 侧也不该被 macOS 布局冒充。
    ///
    /// 这条是覆盖度的反向断言：白名单里有 7 条新条目但本机 0 条，扫描结果的
    /// 条数必须仍是 G3 收口时的那些（这里造了 `.npm` 与一个 nvm 版本做锚点）。
    /// 如果哪天某条路径碰巧在本机出现了，**这条会红——那是提醒「实现变了」，
    /// 不是失败**；确认无误后把锚点数字更新即可。
    #[test]
    fn absent_language_caches_stay_off_the_ledger() {
        let root = temp_dir("g4_absent");
        std::fs::create_dir_all(root.join(".npm")).unwrap();

        let names: Vec<String> = ToolchainScanner::on(root.clone(), Platform::Mac)
            .scan()
            .items
            .iter()
            .map(|i| i.name.clone())
            .collect();
        assert_eq!(
            names,
            vec!["Node 生态 · npm"],
            "只该有 npm 一行，实际：{names:?}"
        );

        // Windows 布局下同样只有 npm（NuGet 的 global-packages 是 dot-dir，
        // 假 home 里没建就不该出现）
        std::fs::create_dir_all(root.join("AppData/Local/npm-cache")).unwrap();
        let win_names: Vec<String> = ToolchainScanner::on(root.clone(), Platform::Windows)
            .scan()
            .items
            .iter()
            .map(|i| i.name.clone())
            .collect();
        assert_eq!(
            win_names,
            vec!["Node 生态 · npm"],
            "Windows 侧同样只有 npm，实际：{win_names:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 整条链路：假 home 里放 G4 的 **Windows 布局**目录，Windows 探得到、
    /// macOS 探不到。这与 `toolchain_finds_windows_layout_on_a_fake_home` 同形，
    /// 但专盯 G4 新增的条目——它们的路径是这一刀新写的，最需要两侧都被断言。
    #[test]
    fn g4_windows_layout_is_found_but_not_on_mac() {
        let home = temp_dir("g4-win-home");
        // NuGet global-packages 是 dot-dir（两平台同路径），所以两边都会探到——
        // 这正是它的官方位置，不该给它编一个 Windows 专有路径出来。
        std::fs::create_dir_all(home.join(".nuget/packages")).unwrap();
        // 下面几条是 Windows 专有布局
        std::fs::create_dir_all(home.join("AppData/Local/Composer")).unwrap();
        std::fs::create_dir_all(home.join("AppData/Local/Coursier/Cache/v1")).unwrap();
        std::fs::create_dir_all(home.join("AppData/Local/NuGet/v3-cache")).unwrap();
        std::fs::create_dir_all(home.join("AppData/Local/deno")).unwrap();

        let win = ToolchainScanner::on(home.clone(), Platform::Windows).scan();
        let mac = ToolchainScanner::on(home.clone(), Platform::Mac).scan();

        let win_sections: HashSet<&str> = win
            .items
            .iter()
            .filter_map(|i| i.group_key.as_deref())
            .collect();
        assert!(
            win_sections.contains(".NET") && win_sections.contains("PHP"),
            "Windows 上该认出 .NET 与 PHP：{win_sections:?}"
        );
        assert!(
            mac.items
                .iter()
                .all(|i| i.group_key.as_deref() != Some("PHP")),
            "macOS 侧不该认出 %LOCALAPPDATA%\\下的 Composer：{:?}",
            mac.items.iter().map(|i| &i.name).collect::<Vec<_>>()
        );
        // NuGet 全局包两边都在（官方位置一致），macOS 侧该看到它
        assert!(
            mac.items.iter().any(|i| i.name == ".NET · NuGet 全局包"),
            "NuGet 全局包两平台同路径，macOS 侧也该有：{:?}",
            mac.items.iter().map(|i| &i.name).collect::<Vec<_>>()
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    // ========================================================================
    // WSL 发行版（W3）
    //
    // 分组只在 Windows 上注册，而本机是 macOS——所以「读注册表」做成可注入的
    // 参数、解析逻辑做成纯函数，两条都在这里被断言。真正做不到的只有一件：
    // 那条 `reg query` 在实机上跑不跑得通（记在 AGENTS.md 已知限制里）。
    // 这与 W2 的取舍是同一条：平台是**值**，Windows 那半不该没人验过。
    // ========================================================================

    /// `reg query "…\Lxss" /s` 的真实输出形状：先根键、再每个发行版一个子键。
    const LXSS_SAMPLE: &str = r#"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Lxss
    (Default)    REG_SZ    {00000000-0000-0000-0000-000000000000}

HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Lxss\{2b1e5e4c-3a1d-4c5f-9d3a-1f2e3d4c5b6a}
    State    REG_DWORD    0x1
    DistributionName    REG_SZ    Ubuntu-22.04
    Version    REG_DWORD    0x2
    BasePath    REG_SZ    C:\Users\me\AppData\Local\Packages\CanonicalGroupLimited.Ubuntu22.04onWindows_79rhkp1fndgsc\LocalState
    Flags    REG_DWORD    0x7

HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Lxss\{aaaabbbb-1111-2222-3333-444455556666}
    DistributionName    REG_SZ    Debian Bookworm
    Version    REG_DWORD    0x2
    BasePath    REG_SZ    D:\My WSL\Debian
"#;

    #[test]
    fn wsl_parse_reads_name_basepath_version() {
        let d = parse_lxss_registry_for_test(LXSS_SAMPLE);

        // 根键的 `(Default)` 值也在 Lxss 下，但它不是发行版
        assert_eq!(d.len(), 2, "解析出的发行版数量不对：{d:?}");

        assert_eq!(d[0].name, "Ubuntu-22.04");
        assert_eq!(d[0].version, 2);
        assert!(
            d[0].base_path.to_string_lossy().ends_with("LocalState"),
            "BasePath 解析错：{:?}",
            d[0].base_path
        );

        // 名字与路径里的空格必须保住（按「前两段 + 其余全是值」切，不是 split_whitespace）
        assert_eq!(d[1].name, "Debian Bookworm");
        assert!(
            d[1].base_path.to_string_lossy().contains(r"My WSL\Debian"),
            "含空格的 BasePath 被切坏了：{:?}",
            d[1].base_path
        );
    }

    #[test]
    fn wsl_parse_skips_records_without_base_path() {
        let text = r#"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Lxss\{11111111-2222-3333-4444-555555555555}
    DistributionName    REG_SZ    Broken

HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Lxss\{66666666-7777-8888-9999-000000000000}
    DistributionName    REG_SZ    Fine
    BasePath    REG_SZ    C:\ok
"#;
        let d = parse_lxss_registry_for_test(text);
        assert_eq!(d.len(), 1, "缺 BasePath 的记录定位不到 vhdx，该整条跳过");
        assert_eq!(d[0].name, "Fine");
    }

    /// 注册条件：只 Windows、只在真有发行版时；读失败或没有发行版都不留空分组。
    #[test]
    fn wsl_registers_only_on_windows_with_distros() {
        assert!(
            wsl_scanners_with(Platform::Mac, || Ok(LXSS_SAMPLE.into())).is_empty(),
            "macOS 上不存在 WSL，不该注册"
        );

        let win = wsl_scanners_with(Platform::Windows, || Ok(LXSS_SAMPLE.into()));
        assert_eq!(win.len(), 1, "Windows 上装了发行版就该出一个分组");
        assert_eq!(win[0].id(), "wsl");

        // 没装 WSL / reg 读不到 → 不注册，而不是留一个空 tab（硬约束 8）
        assert!(
            wsl_scanners_with(Platform::Windows, || Err("reg 不可用".into())).is_empty(),
            "读注册表失败时不该注册"
        );

        let root_only = "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Lxss\n    (Default)    REG_SZ    {00000000-0000-0000-0000-000000000000}\n";
        assert!(
            wsl_scanners_with(Platform::Windows, || Ok(root_only.into())).is_empty(),
            "只有根键（没有发行版子键）时不该注册"
        );
    }

    #[test]
    fn wsl_scan_reports_vhdx_and_never_claims_reclaimable() {
        let root = temp_dir("wsl");
        let state = root.join("LocalState");
        std::fs::create_dir_all(&state).unwrap();
        // 1 MB 的假 vhdx。占用空间按块向上取整，所以只断下界——
        // 不断言等于 1_000_000，那是拿被测代码的口径当基准。
        std::fs::write(state.join("ext4.vhdx"), vec![0u8; 1_000_000]).unwrap();

        let g = WslScanner::new(vec![
            WslDistro {
                name: "Ubuntu-22.04".into(),
                base_path: state.clone(),
                version: 2,
            },
            // WSL1 没有虚拟磁盘，它的占用分散在 Windows 文件系统里
            WslDistro {
                name: "Legacy".into(),
                base_path: root.join("gone"),
                version: 1,
            },
        ])
        .scan();

        assert_eq!(g.id, "wsl");
        assert_eq!(g.items.len(), 2);
        assert_frontend_contract(std::slice::from_ref(&g));

        let ubuntu = g
            .items
            .iter()
            .find(|i| i.name == "Ubuntu-22.04")
            .expect("少了 Ubuntu 条目");
        match ubuntu.metrics.get("size") {
            Some(MetricValue::Bytes(b)) => assert!(*b >= 1_000_000, "1 MB 的 vhdx 报成了 {b}"),
            other => panic!("vhdx 体积该是字节数，实为 {other:?}"),
        }
        // 本组最核心的承诺：vhdx 只涨不缩，报出来的数拆不掉，所以永不标可回收
        assert!(!ubuntu.cleanable, "WSL 不该标可清理");
        assert!(ubuntu.reclaimable.is_none(), "WSL 不该报可回收");
        assert_eq!(
            ubuntu.path,
            state.display().to_string(),
            "path 该给发行版目录（vhdx 就在里面）"
        );
        assert_eq!(
            detail_value(ubuntu, "howToShrink").and_then(text_key),
            Some("data.value.vhdxHowToShrink"),
            "detail 里该给出压缩步骤"
        );

        // WSL1 没有 vhdx → 体积**未知**而不是 0：报 0 等于说它不占空间，与事实相反
        let legacy = g.items.iter().find(|i| i.name == "Legacy").unwrap();
        assert!(
            !legacy.metrics.contains_key("size"),
            "WSL1 没有虚拟磁盘，不该报体积"
        );
        assert!(!legacy.cleanable);

        // 体积降序：有体积的排在未知之前
        assert_eq!(g.items[0].name, "Ubuntu-22.04");

        let _ = std::fs::remove_dir_all(&root);
    }

    // ========================================================================
    // 前端契约（harness）
    //
    // `src/lib/types.ts` 是 Rust 结构的镜像，`Assets.vue` 按固定假设渲染：
    // fields 最多三个、名称带 ` ·` 分节、可回收量跟随 cleanable 且不超过体积。
    // 这些假设散在个扫描器的测试里守不全，这里收口成一份契约，
    // 冒烟测试与合成快照都过它。解析层另有
    // `scripts/check-snapshot-contract.mjs` 对 dump 的真实 JSON 做同款校验。
    // ========================================================================

    /// 前端依赖的全部不变量。新增 Scanner 的分组形状破坏这里，测试先红。
    fn assert_frontend_contract(groups: &[AssetGroup]) {
        for g in groups {
            assert!(!g.id.is_empty(), "分组 id 为空");
            assert!(text_present(&g.label), "分组 {} 的 label 为空", g.id);
            assert!(text_present(&g.summary), "分组 {} 的 summary 为空", g.id);

            // 前端 `Assets.vue` 是 `fields.slice(0, 3)`：第四个字段不会进表头
            assert!(
                g.fields.len() <= 3,
                "分组 {} 声明了 {} 个字段，超过三个",
                g.id,
                g.fields.len()
            );
            for f in &g.fields {
                assert!(
                    !f.key.is_empty() && text_present(&f.label),
                    "分组 {} 有空字段",
                    g.id
                );
            }

            // 出错时前端只显示错误条，不该出现明细行
            if g.error.is_some() {
                assert!(
                    g.items.is_empty(),
                    "分组 {} 出错时仍有 {} 条明细",
                    g.id,
                    g.items.len()
                );
            }

            for it in &g.items {
                assert!(
                    !it.id.is_empty() && !it.name.is_empty() && !it.path.is_empty(),
                    "分组 {} 有条目缺 id/name/path：{:?}",
                    g.id,
                    it.id
                );
                // 分节渲染：` · ` 前缀做小节标题，必须非空
                if let Some((prefix, _)) = it.name.split_once(" · ") {
                    assert!(!prefix.trim().is_empty(), "条目 {} 的分节前缀为空", it.name);
                }
                // group_key（G1 起）：分节与筛选的数据来源。给了就必须非空，
                // 且名字要以 `key · ` 开头——前端分节行名靠剥掉这个前缀显示，
                // 剥不掉说明两边的约定脱钩了（总览行等不分节条目没有这个字段）。
                if let Some(k) = &it.group_key {
                    assert!(!k.trim().is_empty(), "条目 {} 的 group_key 为空", it.name);
                    let prefix = format!("{k} · ");
                    assert!(
                        it.name.starts_with(&prefix),
                        "条目 {} 的 group_key 与名称前缀不一致（该以 {:?} 开头）",
                        it.name,
                        prefix
                    );
                }
                for k in it.metrics.keys() {
                    assert!(!k.is_empty(), "条目 {} 有空指标键", it.id);
                }
                let mut artifact_paths = HashSet::new();
                let mut artifact_bytes = 0u64;
                for entry in &it.artifact_entries {
                    let path = &entry.relative_path;
                    // 两侧路径语法都检查，不能让宿主平台决定契约覆盖面。
                    let drive_prefix = path.as_bytes().get(1) == Some(&b':')
                        && path.as_bytes()[0].is_ascii_alphabetic();
                    assert!(
                        !path.is_empty() && !path.starts_with(['/', '\\']) && !drive_prefix,
                        "{} 的产物路径必须非空且相对：{path:?}",
                        it.id
                    );
                    assert!(
                        !path.split(['/', '\\']).any(|part| part == ".."),
                        "{} 的产物路径包含父跳转：{path:?}",
                        it.id
                    );
                    assert!(
                        artifact_paths.insert(path),
                        "{} 的产物路径重复：{path:?}",
                        it.id
                    );
                    artifact_bytes = artifact_bytes
                        .checked_add(entry.size_bytes)
                        .expect("产物字节合计溢出");
                }
                match it.metrics.get("artifacts") {
                    Some(MetricValue::Bytes(bytes)) => {
                        assert!(
                            !it.artifact_entries.is_empty(),
                            "{} 有产物指标却缺完整列表",
                            it.id
                        );
                        assert_eq!(artifact_bytes, *bytes, "{} 产物明细与指标不一致", it.id);
                    }
                    Some(_) => panic!("{} 的 artifacts 指标不是字节", it.id),
                    None => assert!(
                        it.artifact_entries.is_empty(),
                        "{} 有产物列表却缺指标",
                        it.id
                    ),
                }
                // 可回收量必须标了 cleanable，且不能超过自身体积
                if let Some(r) = it.reclaimable {
                    assert!(it.cleanable, "{} 有回收量却未标 cleanable", it.id);
                    if let Some(MetricValue::Bytes(b)) = it.metrics.get("size") {
                        assert!(r <= *b, "{} 回收量 {} 超过体积 {}", it.id, r, b);
                    }
                }
                for r in &it.detail {
                    assert!(
                        text_present(&r.label) && text_present(&r.value),
                        "{} 的 detail 有空标签或空值",
                        it.id
                    );
                }
            }
        }
    }

    #[test]
    fn artifact_entries_contract_rejects_invalid_lists() {
        let mut valid = serde_json::to_value(sample_group()).unwrap();
        valid["items"][0]["artifact_entries"] =
            serde_json::json!([{"relative_path": "target", "size_bytes": 4096}]);
        valid["items"][0]["metrics"]["artifacts"] = serde_json::json!({"t": "b", "v": 4096});
        let group: AssetGroup = serde_json::from_value(valid.clone()).unwrap();
        assert_frontend_contract(&[group]);
        let rejects = |json: serde_json::Value| {
            let group: AssetGroup = serde_json::from_value(json).unwrap();
            assert!(
                std::panic::catch_unwind(|| assert_frontend_contract(&[group])).is_err(),
                "坏产物契约未被拒绝"
            );
        };
        for path in [
            "",
            "/target",
            r"\target",
            r"C:\target",
            "C:/target",
            "C:target",
            r"\\server\share\target",
            "../target",
            "pkg/../target",
            r"pkg\..\target",
        ] {
            let mut bad = valid.clone();
            bad["items"][0]["artifact_entries"][0]["relative_path"] = path.into();
            rejects(bad);
        }
        let mut duplicate = valid.clone();
        duplicate["items"][0]["artifact_entries"]
            .as_array_mut()
            .unwrap()
            .push(valid["items"][0]["artifact_entries"][0].clone());
        duplicate["items"][0]["metrics"]["artifacts"]["v"] = 8192.into();
        rejects(duplicate);
        let mut mismatched = valid.clone();
        mismatched["items"][0]["metrics"]["artifacts"]["v"] = 4097.into();
        rejects(mismatched);
        let mut missing = valid.clone();
        missing["items"][0]
            .as_object_mut()
            .unwrap()
            .remove("artifact_entries");
        rejects(missing);
        let mut empty = valid.clone();
        empty["items"][0]["artifact_entries"] = serde_json::json!([]);
        rejects(empty.clone());
        empty["items"][0]["metrics"]["artifacts"]["v"] = 0.into();
        rejects(empty);
        let mut no_metric = valid.clone();
        no_metric["items"][0]["metrics"]
            .as_object_mut()
            .unwrap()
            .remove("artifacts");
        rejects(no_metric);
        let mut wrong_type = valid.clone();
        wrong_type["items"][0]["metrics"]["artifacts"]["t"] = "i".into();
        rejects(wrong_type);
        valid["items"][0]["artifact_entries"][0]["size_bytes"] = 0.into();
        valid["items"][0]["metrics"]["artifacts"]["v"] = 0.into();
        assert_frontend_contract(&[serde_json::from_value(valid).unwrap()]);
    }

    /// 冒烟用的分组集合：**每个 Scanner 对着空 home 跑一遍**。
    ///
    /// 纯临时目录，毫秒级。抽成函数是因为有两条测试共用它——`every_scanner_survives_empty_home`
    /// 管「形状合法、不 panic」，`displayed_text_is_a_key_not_finished_chinese` 管
    /// 「展示文案下发的是键」。新 Scanner 若没加进下面的构造列表，两条一起漏掉它
    /// （`scanner_registration_is_pinned` 也会提醒）。
    fn empty_home_smoke_groups() -> (PathBuf, Vec<AssetGroup>) {
        let root = temp_dir("smoke");
        let empty_home = root.join("home");
        std::fs::create_dir_all(&empty_home).unwrap();

        let mut groups = vec![
            AiToolsScanner::new(empty_home.clone()).scan(),
            LocalModelsScanner::new(empty_home.clone()).scan(),
            RepoScanner::new(vec![empty_home.join("no-such-dir")]).scan(),
        ];
        for s in dev_env_scanners(&empty_home) {
            groups.push(s.scan());
        }

        // toolchain 是探测型：本机装了才进列表，空 home 下跳过，
        // 避免为了「覆盖」真去 spawn 一个不存在的 CLI。
        // ssh 与 docker 已随 H3 移出 scanners()，不再进这张冒烟表。
        let toolchain = ToolchainScanner::new(empty_home.clone());
        if toolchain.exists() {
            groups.push(toolchain.scan());
        }

        // WSL 只在 Windows 上注册，本机不会进 `scanners()`。这里喂一段合成的
        // 注册表输出，让 Windows 那半在假 home 上真跑一遍——它指向的 vhdx 不存在，
        // 正好覆盖「体积未知」那条分支（硬约束 9：另一侧不该没人验过）。
        for s in wsl_scanners_with(Platform::Windows, || Ok(LXSS_SAMPLE.into())) {
            groups.push(s.scan());
        }
        (root, groups)
    }

    /// 冒烟集 + 一份造出来的假 home，把展示文案的**行级**形态也逼出来。
    ///
    /// toolchain 是探测型，空 home 下不进冒烟集合——于是它的三种行形（整目录缓存、
    /// 可列子目录、已装版本）在别的机器上就没被查过。造一份假 home 一次把三类都
    /// 逼出来，两边平台各扫一遍（硬约束 9：覆盖面不取决于本机装了什么）。
    /// 返回（要清理的目录、分组、假 home 扫出的行数）。
    fn display_smoke_groups() -> (Vec<PathBuf>, Vec<AssetGroup>, usize) {
        let (root, mut groups) = empty_home_smoke_groups();
        let seeded = temp_dir("toolchain_display");
        for rel in [
            ".rustup/downloads",
            ".rustup/toolchains/stable-arm64-apple-darwin",
            ".cargo/registry",
            ".nvm/.cache",
            ".nvm/versions/node/v20.19.5",
            "Library/Developer/Xcode/DerivedData/projA-abcdef",
            "Library/Developer/Xcode/iOS Device Support/Devices/iPhone-17.0",
        ] {
            std::fs::create_dir_all(seeded.join(rel)).unwrap();
        }
        let mut seeded_rows = 0usize;
        for p in [Platform::Mac, Platform::Windows] {
            let g = ToolchainScanner::on(seeded.clone(), p).scan();
            seeded_rows += g.items.len();
            groups.push(g);
        }
        (vec![root, seeded], groups, seeded_rows)
    }

    /// 遍历一个分组里**所有给人看的文案位**（分组名 / 摘要 / 错误 / 字段名 / 脚注 /
    /// 行标签 / 行的四个展示字段 / 明细行的标签与值 / 指标格里的文案）。两条国际化
    /// 门禁共用这张表——漏一个展示位，那条门禁就等于没立。
    fn each_display_text(groups: &[AssetGroup], f: &mut impl FnMut(&str, &Text)) {
        for g in groups {
            f(&format!("{} 分组名", g.id), &g.label);
            f(&format!("{} 概览", g.id), &g.summary);
            if let Some(e) = &g.error {
                f(&format!("{} 错误", g.id), e);
            }
            for fld in &g.fields {
                f(&format!("{} 字段 {}", g.id, fld.key), &fld.label);
            }
            for n in g.notes.iter().flatten() {
                f(&format!("{} 脚注", g.id), n);
            }
            for it in &g.items {
                for tg in &it.tags {
                    f(&format!("{} 行标签", g.id), &tg.label);
                }
                for (slot, t) in [
                    ("label", &it.label),
                    ("short_label", &it.short_label),
                    ("group_label", &it.group_label),
                    ("kind_label", &it.kind_label),
                ] {
                    if let Some(t) = t {
                        f(&format!("{} 行的 {slot}", g.id), t);
                    }
                }
                // P2b 起明细行两边都是 Text：标签必是键，值可以是路径这类 Raw
                for r in &it.detail {
                    f(&format!("{} 明细标签", g.id), &r.label);
                    f(&format!("{} 明细值", g.id), &r.value);
                }
                // 指标格里的文案形态（`t:"k"` / `t:"f"`）：前端 `formatMetric` 同样查表，
                // 漏一条就是单元格里印着键名。`Text(String)` 是数据（分支名、`—`），不算。
                for (key, m) in &it.metrics {
                    match m {
                        MetricValue::TextKey(k) => {
                            f(&format!("{} 指标 {key}", g.id), &Text::Key(k.clone()));
                        }
                        MetricValue::TextFmt(fm) => {
                            f(&format!("{} 指标 {key}", g.id), &Text::Fmt(fm.clone()));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// 展示文案里不许出现「成品中文」——只允许文案键。
    ///
    /// 为什么单独立一条：界面切到英文后侧栏仍是中文，就是因为后端把成品串写进了
    /// `label` / `summary` / `notes` / 字段名 / 标签 / 分节名。这些位置一旦进了
    /// `scan-cache.json`，切语言只能重扫才能变——而「切语言不重扫」是整个国际化
    /// 方案的前提。所以判据不是「有没有中文」（仓库名、模型名里本来就有），
    /// 是「**展示位下发的是键还是成品**」。
    ///
    /// 覆盖的展示位：分组 `label` / `summary` / `notes` / `error`、字段名、行标签，
    /// 行的 `label` / `short_label` / `group_label` / `kind_label`，以及（P2b 起）
    /// 明细行的标签与值、指标格里的 `k` / `f` 文案。
    /// **不覆盖**：`Item.name`、`group_key`、`metrics.kind` 的原始值——它们是搜索 /
    /// 筛选 / 分节的词表，语言无关才正确。
    #[test]
    fn displayed_text_is_a_key_not_finished_chinese() {
        let (roots, groups, seeded_rows) = display_smoke_groups();
        let mut offenders: Vec<String> = vec![];

        each_display_text(&groups, &mut |where_, t| {
            check_text(&mut offenders, where_, t);
        });
        for root in &roots {
            let _ = std::fs::remove_dir_all(root);
        }
        // 假 home 一个行都没扫出来时，这条只查了分组级的文案——行级的三种行形
        // （缓存 / 子目录 / 版本）等于没验，而测试照样显示 ok。所以先钉覆盖面。
        assert!(
            seeded_rows >= 6,
            "假 home 该逼出缓存行、子目录行与版本行，实际只有 {seeded_rows} 行：白名单条目改名了？"
        );
        // 三类行形里最容易被漏掉的是版本行（`version_item` 与缓存行走的是不同的
        // 文案组装路径），所以单独钉一条：至少扫出一行「已安装」。
        let installed = groups
            .iter()
            .flat_map(|g| &g.items)
            .filter(|i| matches!(&i.kind_label, Some(Text::Key(k)) if k == "data.kind.installed"))
            .count();
        assert!(
            installed >= 1,
            "假 home 该扫出「已安装」版本行（nvm/versions/node 下那一行），实际 {installed} 行"
        );
        assert!(
            offenders.is_empty(),
            "展示位下发的是成品中文而不是文案键（切语言不会跟着变）：\n{}",
            offenders.join("\n")
        );
    }

    /// 后端下发的**每一个文案键**都得在五份语言目录里有译文。
    ///
    /// 上一条管「下发的是不是键」，这条管「键查不查得到」——两者缺一都漏：
    /// 前端 `translate()` 查不到键时**回显键名**（不抛错、不显示 undefined），
    /// 于是漏一条就是界面上出现 `data.group.repos` 这种字符串，而 Rust 侧全绿。
    /// 判据取五份目录的交集：只补 en 会让另外四种语言静默显示英文。
    ///
    /// 只查**本次扫出来的键**（方向是「代码在用 → 目录必须有」）。反向的
    /// 「目录里有 → 代码还在用」不查：目录里留着未注册分组（ssh / docker）的
    /// 文案是有意为之，加回来时不必再补。
    #[test]
    fn every_displayed_key_has_a_translation_in_all_five_catalogs() {
        const CATALOGS: [(&str, &str); 5] = [
            ("en", include_str!("../../../src/locales/en.json")),
            ("es", include_str!("../../../src/locales/es.json")),
            ("ja", include_str!("../../../src/locales/ja.json")),
            ("zh-CN", include_str!("../../../src/locales/zh-CN.json")),
            ("zh-TW", include_str!("../../../src/locales/zh-TW.json")),
        ];
        let parsed: Vec<(&str, serde_json::Value)> = CATALOGS
            .iter()
            .map(|(loc, raw)| {
                (
                    *loc,
                    serde_json::from_str(raw).expect("语言目录必须是合法 JSON"),
                )
            })
            .collect();

        let (roots, groups, _) = display_smoke_groups();
        let mut keys: Vec<String> = vec![];
        each_display_text(&groups, &mut |_, t| collect_text_keys(t, &mut keys));
        // 白名单 `note` 列的全部键按表遍历：空 home 冒烟只逼得出七八条，剩下的
        // 三十多条不在这里补上，新增条目漏译文就没人管。
        for t in note_keys_for_test() {
            collect_text_keys(&t, &mut keys);
        }
        // 冒烟扫描到不了的下发路径：清理被拒的原因（只在 `clean_*` 里出现）、
        // 未注册但留树的 docker / ssh 的组级文案、以及要有特定数据才走到的分支
        // （仓库有未推送 / 模型缺层 / LM Studio 暂存目录 / WSL 注册表值异常…）。
        // 这些键在代码里是字面量，没有表可遍历，只能登记在这里；新增一条下发
        // 路径时顺手加一行——漏登记的后果仍是「界面上印键名」，不是误报。
        const KEYS_OFF_THE_SMOKE_PATH: &[&str] = &[
            // 清理拒绝原因（repos / toolchain / ai_tools 三套实现）
            "data.cleanFail.unconfirmed",
            "data.cleanFail.unpushed",
            "data.cleanFail.pushUnknown",
            "data.cleanFail.stash",
            "data.cleanFail.dirtyFiles",
            "data.cleanFail.notAbsolute",
            "data.cleanFail.repoMissing",
            "data.cleanFail.notGitRepo",
            "data.cleanFail.artifactOutsideRepo",
            "data.cleanFail.noArtifacts",
            "data.cleanFail.artifactRecheckFailed",
            "data.cleanFail.invalidId",
            "data.cleanFail.notInCacheWhitelist",
            "data.cleanFail.noCleanEntry",
            "data.cleanFail.officialCommand",
            "data.cleanFail.perChildOnly",
            "data.cleanFail.notInAiWhitelist",
            "data.cleanFail.readOnlyRoot",
            "data.cleanFail.categoryLocked",
            "data.cleanFail.noCleanableEntries",
            "data.cleanFail.symlinkSkipped",
            "data.cleanFail.pathMissingSkipped",
            // repos：状态指标与明细里随数据出现的分支
            "data.value.pushUnknown",
            "data.value.dirtyFiles",
            "data.value.clean",
            "data.value.none",
            "data.value.bytes",
            "data.value.artifactSummary",
            "data.detail.remote",
            "data.detail.lastCommit",
            "data.detail.gitSize",
            "data.detail.artifacts",
            // toolchain：三档清理方式与类别组合（冒烟只逼出其中一两档）
            "data.value.kindOfSource",
            "data.value.cleanByTrash",
            "data.value.cleanByCommand",
            "data.value.notCleanable",
            "data.detail.manager",
            "data.detail.cleanHow",
            // ai_tools
            "data.detail.entries",
            "data.detail.clean",
            "data.value.statsOnly",
            "data.value.count",
            // local_models
            "data.detail.weightLayers",
            "data.detail.missingLayers",
            "data.detail.sharedLayers",
            "data.detail.publisher",
            "data.detail.status",
            "data.detail.weights",
            "data.detail.org",
            "data.value.layerCount",
            "data.value.missingLayers",
            "data.value.sharedLayers",
            "data.value.downloadStaging",
            "data.value.weightsNotOnDisk",
            // dev_env（java / 移动端）
            "data.detail.artifactGroups",
            "data.detail.dir",
            "data.value.mavenRest",
            "data.value.gradleDistKeep",
            "data.value.gradleDistOld",
            "data.value.gradleDaemonLogs",
            "data.value.gradleRegenerates",
            // wsl
            "data.detail.vhdx",
            "data.detail.distroDir",
            "data.detail.wslVersion",
            "data.detail.sizeSemantics",
            "data.detail.howToShrink",
            "data.value.wslVersionUnknown",
            "data.value.vhdxSizeSemantics",
            "data.value.vhdxHowToShrink",
            // docker（未注册，D6）
            "data.summary.docker",
            "data.error.dockerCliMissing",
            "data.error.dockerQuery",
            "data.error.dockerEmpty",
            "data.note.dockerDesktopDiskIdle",
            "data.note.dockerStartHint",
            "data.note.dockerInsideVm",
            "data.note.dockerDecimalUnits",
            "data.note.dockerVirtualSize",
            "data.note.dockerContainers",
            "data.note.dockerBuildCache",
            "data.note.dockerDesktopDisk",
            "data.note.dockerSkippedRows",
            "data.detail.imageId",
            "data.detail.repo",
            "data.detail.tag",
            "data.detail.virtualSize",
            "data.detail.uniqueLayers",
            "data.detail.volumeName",
            "data.detail.mountPoint",
            "data.detail.refCount",
            // ssh（未注册，D6）
            "data.summary.ssh",
            "data.note.sshDuplicates",
            "data.note.sshOrphans",
            "data.detail.duplicateOf",
            "data.detail.fingerprint",
            "data.detail.comment",
            "data.detail.permissions",
            "data.detail.pubkey",
            "data.value.unreferenced",
            "data.value.pubkeyPresent",
            "data.value.pubkeyMissing",
        ];
        keys.extend(KEYS_OFF_THE_SMOKE_PATH.iter().map(|k| k.to_string()));
        for root in &roots {
            let _ = std::fs::remove_dir_all(root);
        }
        keys.sort();
        keys.dedup();
        assert!(
            keys.len() > 40,
            "本次只收集到 {} 个文案键，覆盖面太小，这条基本等于没跑：展示位的遍历漏了？",
            keys.len()
        );

        let missing: Vec<String> = keys
            .iter()
            .flat_map(|k| {
                parsed.iter().filter_map(move |(loc, catalog)| {
                    catalog
                        .get(k)
                        .and_then(|v| v.as_str())
                        .is_none()
                        .then_some(format!("{loc} 缺 {k}"))
                })
            })
            .collect();
        assert!(
            missing.is_empty(),
            "后端下发的文案键在语言目录里没有译文，界面会直接印键名：\n{}",
            missing.join("\n")
        );
    }

    /// 收集一条 `Text` 里所有需要译文的键（含 `Fmt` 的参数，参数本身也可以是键）。
    fn collect_text_keys(t: &Text, out: &mut Vec<String>) {
        match t {
            Text::Raw(_) => {}
            Text::Key(k) => out.push(k.clone()),
            Text::Fmt(f) => {
                out.push(f.k.clone());
                for arg in f.a.values() {
                    match arg {
                        TextArg::Key(k) => out.push(k.clone()),
                        TextArg::Fmt(inner) => out.push(inner.k.clone()),
                        TextArg::Raw(_) | TextArg::Num(_) | TextArg::Bytes(_) => {}
                    }
                }
            }
        }
    }

    /// 展示位的文案里出现 CJK 就算违规。
    ///
    /// `Raw` 还合法的两种情况：**语言无关的身份串**（`JDK`、`nvm`、版本号、模型名）
    /// 与**从文件系统读来的名字**（含 CJK 的目录名本来就是数据，不是文案）。前者不含
    /// CJK；后者只在真扫到数据时才出现，空 home 冒烟集里没有——所以判据可以是
    /// 「不含 CJK 即放行」，不需要维护一张品牌名单。
    fn contains_cjk(s: &str) -> bool {
        s.chars().any(|c| c as u32 >= 0x2E80)
    }

    fn check_text(offenders: &mut Vec<String>, where_: &str, t: &Text) {
        match t {
            Text::Raw(s) => {
                if contains_cjk(s) {
                    offenders.push(format!("{where_} = {s:?}"));
                }
            }
            Text::Key(k) => {
                if contains_cjk(k) {
                    offenders.push(format!("{where_} 键名带中文：{k:?}"));
                }
            }
            // `Fmt` 的参数本该只有数字、字节与语言无关的标识符。递归一遍：
            // 成品文案混在参数里同样是「切语言不变」。
            Text::Fmt(f) => {
                if contains_cjk(&f.k) {
                    offenders.push(format!("{where_} 键名带中文：{:?}", f.k));
                }
                for (name, arg) in &f.a {
                    match arg {
                        TextArg::Raw(s) | TextArg::Key(s) => {
                            if contains_cjk(s) {
                                offenders.push(format!("{where_} 参数 {name} = {s:?}"));
                            }
                        }
                        TextArg::Fmt(inner) => {
                            let nested = Text::Fmt(inner.clone());
                            check_text(offenders, &format!("{where_} 参数 {name}"), &nested);
                        }
                        TextArg::Num(_) | TextArg::Bytes(_) => {}
                    }
                }
            }
        }
    }

    /// 冒烟：每个 Scanner 对着空 home 都能给出形状合法的分组，不 panic。
    /// 纯临时目录，毫秒级；新 Scanner 若没加进下面的构造列表，这条会漏掉它——
    /// 所以新增 Scanner 时记得同步（`scanner_registration_is_pinned` 也会提醒）。
    #[test]
    fn every_scanner_survives_empty_home() {
        let (root, groups) = empty_home_smoke_groups();

        assert!(!groups.is_empty(), "空 home 下也应有无条件注册的分组");
        assert_frontend_contract(&groups);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 合成快照的契约断言：覆盖边界形状（三字段封顶、分节名、detail 编号键、
    /// 可回收量、error 空分组），过一遍缓存往返再校验。
    /// 与冒烟测试的分工：那条管真实扫描器不 panic，这条管契约在边界形状下成立。
    #[test]
    fn synthetic_snapshot_satisfies_frontend_contract() {
        let dir = temp_dir("contract");

        let mut rich = sample_group();
        rich.id = "rich".into();
        rich.fields = vec![
            Field {
                key: "size".into(),
                label: Text::raw("体积"),
                numeric: Some(true),
                temporal: None,
            },
            Field {
                key: "last_commit".into(),
                label: Text::raw("最后提交"),
                numeric: Some(true),
                temporal: Some(true),
            },
            Field {
                key: "branch".into(),
                label: Text::raw("分支"),
                numeric: None,
                temporal: None,
            },
        ];
        rich.items[0].name = "大仓库".into(); // 不带分隔符也合法：整组单节渲染
        rich.items.push(Item {
            id: "/tmp/b".into(),
            tags: vec![Tag::new("cleanable")],
            name: "工具链 · npm 缓存".into(),
            label: None,
            short_label: None,
            group_label: None,
            kind_label: None,
            group_key: Some("工具链".into()),
            path: "/tmp/b".into(),
            metrics: HashMap::from([
                ("size".into(), MetricValue::Bytes(2_000_000_000)),
                ("last_commit".into(), MetricValue::Int(1_700_000_000)),
                ("branch".into(), MetricValue::Text("main".into())),
            ]),
            detail: vec![DetailRow::new(
                "cleanHow",
                Text::fmt("data.value.cleanByCommand").arg("cmd", "npm cache clean --force"),
            )],
            cleanable: true,
            artifact_entries: vec![],
            reclaimable: Some(1_500_000_000),
        });

        // 出错分组：有 error 就没有明细行
        let failed = AssetGroup {
            id: "broken".into(),
            label: Text::raw("坏分组"),
            summary: Text::raw("扫描失败"),
            fields: vec![],
            items: vec![],
            notes: None,
            error: Some(Text::raw("docker 未运行")),
            supports_clean: false,
        };

        cache::save(&dir, &[rich, failed]).expect("写入缓存失败");
        let got = cache::load(&dir).expect("读取缓存失败");
        assert_eq!(got.groups.len(), 2, "往返后应还是两个分组");
        assert_frontend_contract(&got.groups);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- 总览分组（G1 / D3）----

    /// 造一个带体积与可回收的分组，供总览测试用。
    fn overview_fixture(id: &str, label: &str, size: u64, reclaim: u64) -> AssetGroup {
        AssetGroup {
            id: id.into(),
            label: Text::raw(label),
            summary: Text::raw(format!("{label} 测试夹具")),
            fields: vec![Field {
                key: "size".into(),
                label: Text::raw("体积"),
                numeric: Some(true),
                temporal: None,
            }],
            items: vec![
                Item {
                    id: format!("{id}:a"),
                    tags: vec![],
                    name: "a".into(),
                    label: None,
                    short_label: None,
                    group_label: None,
                    kind_label: None,
                    group_key: None,
                    path: "/tmp/a".into(),
                    metrics: HashMap::from([("size".into(), MetricValue::Bytes(size))]),
                    detail: vec![],
                    cleanable: reclaim > 0,
                    artifact_entries: vec![],
                    reclaimable: (reclaim > 0).then_some(reclaim),
                },
                // 没标 cleanable 的回收量不计入总览——前端契约也守着这条
                Item {
                    id: format!("{id}:b"),
                    tags: vec![],
                    name: "b".into(),
                    label: None,
                    short_label: None,
                    group_label: None,
                    kind_label: None,
                    group_key: None,
                    path: "/tmp/b".into(),
                    metrics: HashMap::from([("size".into(), MetricValue::Bytes(0))]),
                    detail: vec![],
                    cleanable: false,
                    artifact_entries: vec![],
                    reclaimable: Some(999),
                },
            ],
            notes: None,
            error: None,
            supports_clean: false,
        }
    }

    #[test]
    fn overview_sums_groups_and_passes_contract() {
        let groups = vec![
            overview_fixture("repos", "Git 仓库", 3_000, 1_000),
            overview_fixture("toolchain", "工具链缓存", 2_000, 0),
        ];
        let ov = synthesize_overview(&groups);

        assert_eq!(ov.id, OVERVIEW_ID);
        assert_eq!(ov.items.len(), 2, "一行一个分组");
        assert_eq!(ov.fields.len(), 3, "体积 / 可回收 / 条目，正好三个");

        let repos = &ov.items[0];
        // 行名是**稳定身份**（分组 id，不参与展示），展示名走 `label`——
        // 与 `Item` 的通用契约同一条：搜索 / 排序读 name，渲染读 label。
        assert_eq!(repos.name, "repos", "总览行的身份就是分组 id");
        assert_eq!(
            repos.label,
            Some(Text::raw("Git 仓库")),
            "分组名进展示字段，不进 name"
        );
        match repos.metrics.get("size") {
            Some(MetricValue::Bytes(b)) => assert_eq!(*b, 3_000),
            other => panic!("size 该是字节数，实为 {other:?}"),
        }
        match repos.metrics.get("reclaimable") {
            Some(MetricValue::Bytes(b)) => assert_eq!(*b, 1_000, "只累加标了 cleanable 的"),
            other => panic!("reclaimable 该是字节数，实为 {other:?}"),
        }
        match repos.metrics.get("count") {
            Some(MetricValue::Int(n)) => assert_eq!(*n, 2),
            other => panic!("count 该是整数，实为 {other:?}"),
        }
        // 行不是资产条目：没有路径可揭示（与 docker 的 docker:// 同一类）
        assert_eq!(repos.path, "overview://repos");
        assert!(!repos.cleanable && repos.reclaimable.is_none());

        // 总览说的是「几个分组、共多少、可回收多少」：数字与字节都以带类型的
        // 参数下发，成品文案由前端按当前语言拼（es 的千分位是点、字节小数是逗号）
        assert_eq!(text_key(&ov.summary), Some("data.summary.overview"));
        if let Text::Fmt(f) = &ov.summary {
            assert_eq!(f.a.get("count"), Some(&TextArg::Num(2)), "分组数：{f:?}");
            assert_eq!(
                f.a.get("total"),
                Some(&TextArg::Bytes(5_000)),
                "合计：{f:?}"
            );
            assert_eq!(
                f.a.get("reclaim"),
                Some(&TextArg::Bytes(1_000)),
                "可回收：{f:?}"
            );
        } else {
            panic!("总览 summary 该是带参数的文案键，实为 {:?}", ov.summary);
        }

        assert_frontend_contract(std::slice::from_ref(&ov));
    }

    #[test]
    fn order_groups_uses_fixed_semantic_order() {
        let groups: Vec<AssetGroup> = crate::scan::GROUP_ORDER
            .iter()
            .rev()
            .map(|id| overview_fixture(id, id, 1, 0))
            .collect();
        let ordered = crate::scan::order_groups(groups);
        let ids: Vec<&str> = ordered.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, crate::scan::GROUP_ORDER, "乱序输入应排成固定语义序");
    }

    /// 单组重扫的合并：同 id 顶掉旧行，快照里没有的组追加在末尾
    /// （顺序由调用方过 `order_groups` 收拾，这里只验合并语义本身）。
    #[test]
    fn upsert_group_replaces_by_id_and_appends_new() {
        let snapshot = vec![
            overview_fixture("repos", "Git 仓库", 1, 0),
            overview_fixture("toolchain", "工具链缓存", 2, 0),
        ];

        let fresh = overview_fixture("repos", "Git 仓库", 100, 50);
        let merged = crate::scan::upsert_group(snapshot, fresh);
        assert_eq!(merged.len(), 2, "替换不该增加组数");
        assert_eq!(merged[0].id, "repos");
        assert_eq!(merged[0].total_size(), 100, "同 id 的旧行应被顶掉");

        let fresh = overview_fixture("wsl", "WSL", 7, 0);
        let merged = crate::scan::upsert_group(merged, fresh);
        assert_eq!(merged.len(), 3, "快照里没有的组要追加");
        assert_eq!(merged[2].id, "wsl", "新组追加在末尾");
    }

    /// 单组重扫时 id 挑不到 Scanner 的分流：已知且未隐藏的 id = 环境没了，
    /// 摘除该组（与全量重扫语义一致）；隐藏组与完全未知的 id 一律拒绝。
    #[test]
    fn missing_group_fallback_distinguishes_uninstalled_from_unknown() {
        use crate::MissingGroupFallback;
        // local_models 是探测型 Scanner：卸载了 Ollama 等全部来源后就不注册
        assert_eq!(
            crate::missing_group_fallback("local_models"),
            MissingGroupFallback::RemoveGroup
        );
        // H14 暂时隐藏的组不在侧栏，也不该被「摘除」路径碰到
        assert_eq!(
            crate::missing_group_fallback("java"),
            MissingGroupFallback::Reject
        );
        assert_eq!(
            crate::missing_group_fallback("nope"),
            MissingGroupFallback::Reject
        );
    }

    #[test]
    fn order_groups_keeps_unknown_ids_last_in_original_order() {
        let groups = vec![
            overview_fixture("zzz_b", "未知 B", 1, 0),
            overview_fixture("wsl", "WSL", 1, 0),
            overview_fixture("zzz_a", "未知 A", 1, 0),
            overview_fixture("repos", "Git 仓库", 1, 0),
        ];
        let ordered = crate::scan::order_groups(groups);
        let ids: Vec<&str> = ordered.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(
            ids,
            ["repos", "wsl", "zzz_b", "zzz_a"],
            "未知 id 排最后，且彼此保持原有相对顺序"
        );
    }

    #[test]
    fn with_overview_is_idempotent_and_front_loads() {
        // lib.rs::with_overview 是 pub 的，这里直接验三条性质：
        // 总览最前、其余按 GROUP_ORDER 重排、总览行跟随同一顺序。
        let groups = vec![
            overview_fixture("wsl", "WSL", 1_000, 0),
            overview_fixture("toolchain", "工具链缓存", 2_000, 0),
            overview_fixture("repos", "Git 仓库", 3_000, 1_000),
        ];
        let once = crate::with_overview(groups);
        assert_eq!(once.len(), 4);
        assert_eq!(once[0].id, OVERVIEW_ID, "总览排最前（默认 tab）");
        let ids: Vec<&str> = once.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, [OVERVIEW_ID, "repos", "toolchain", "wsl"]);
        let overview_rows: Vec<&str> = once[0].items.iter().map(|it| it.id.as_str()).collect();
        assert_eq!(
            overview_rows,
            ["overview:repos", "overview:toolchain", "overview:wsl"],
            "总览行顺序与侧栏一致"
        );

        let twice = crate::with_overview(once.clone());
        assert_eq!(twice.len(), 4, "幂等：再调一次不重复插入");
        assert_eq!(twice[0].id, OVERVIEW_ID);
    }

    // ---- 本地模型（G2）----

    /// 造一个 Ollama 布局：manifest 引用两个 blob，磁盘上只有一个。
    /// 返回（models_dir, manifest 路径, 存在的 blob 路径）。
    fn ollama_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let models = root.join(".ollama/models");
        let blobs = models.join("blobs");
        std::fs::create_dir_all(&blobs).unwrap();

        let existing = blobs.join("sha256-aaaa");
        std::fs::write(&existing, vec![b'x'; 1000]).unwrap();

        let manifest = models.join("manifests/registry.ollama.ai/library/foo/7b");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        // 两个层：一个 blob 在磁盘上，一个不在（模拟「模型在册、权重被删」）
        std::fs::write(
            &manifest,
            r#"{"layers":[
                {"digest":"sha256:aaaa","size":1000},
                {"digest":"sha256:bbbb","size":9999}
            ]}"#,
        )
        .unwrap();
        (models, manifest, existing)
    }

    #[test]
    fn local_models_ollama_counts_only_blobs_on_disk() {
        let root = temp_dir("models-ollama");
        let (_, manifest, existing) = ollama_fixture(&root);

        let g = LocalModelsScanner::new(root.clone()).scan();
        assert_eq!(g.id, "local_models");
        assert_eq!(g.label, Text::key("data.group.local_models"));
        assert_eq!(g.items.len(), 1, "一个 manifest 一行");

        let it = &g.items[0];
        assert_eq!(it.name, "Ollama · foo:7b", "行名 = 来源 · 模型名:tag");
        assert_eq!(it.group_key.as_deref(), Some("Ollama"));
        // 按 Path 比较而不是字符串：fixture 用 join 拼、扫描器走真实 read_dir，
        // Windows 上两边分隔符不同但分量相同（Path 相等性对分隔符不敏感）
        assert_eq!(Path::new(&it.path), manifest, "行路径应指向 manifest 文件");
        assert!(
            !it.cleanable && it.reclaimable.is_none(),
            "本组永不标可回收"
        );

        // 体积只计磁盘上存在的 blob：不拿 manifest 里的 size 报幻影
        match it.metrics.get("size") {
            Some(MetricValue::Bytes(v)) => {
                let expect = block_bytes(&std::fs::metadata(&existing).unwrap());
                assert_eq!(*v, expect, "只该计到存在的那个 blob");
            }
            other => panic!("size 该是字节数，实为 {other:?}"),
        }
        let layers = detail_value(it, "weightLayers").expect("该有权重层行");
        assert!(
            matches!(text_arg(layers, "n"), Some(TextArg::Num(2))),
            "权重层该是 2：{layers:?}"
        );
        let missing = detail_value(it, "missingLayers").expect("缺失的层要报出来");
        assert!(
            matches!(text_arg(missing, "n"), Some(TextArg::Num(1))),
            "缺失层该是 1：{missing:?}"
        );

        assert_frontend_contract(std::slice::from_ref(&g));
        std::fs::remove_dir_all(&root).ok();
    }

    /// 共享 blob 的账（P3）：两个 tag 共用一个权重层时，`size` 照旧各计一次
    /// （W1 取舍），「删掉单个 tag 实际腾出多少」落在 detail 的共享层一行；
    /// 缺失层不进共享账（不在磁盘上的东西谈不上腾出）。
    #[test]
    fn ollama_shared_blobs_reported_per_tag() {
        let root = temp_dir("models-shared");
        let models = root.join(".ollama/models");
        let blobs = models.join("blobs");
        std::fs::create_dir_all(&blobs).unwrap();

        // 共享层 4 MB（两个 tag 都引用）、各自独占层 1 MB / 2 MB、
        // 共享但缺失的层（两个 manifest 都引用、文件不在磁盘上）
        let share = blobs.join("sha256-share");
        std::fs::write(&share, vec![b'x'; 4 * 1024 * 1024]).unwrap();
        let only_a = blobs.join("sha256-only-a");
        std::fs::write(&only_a, vec![b'x'; 1024 * 1024]).unwrap();
        let only_b = blobs.join("sha256-only-b");
        std::fs::write(&only_b, vec![b'x'; 2 * 1024 * 1024]).unwrap();

        let mk = |tag: &str, body: &str| {
            let m = models.join(format!("manifests/registry.ollama.ai/library/qwen3/{tag}"));
            std::fs::create_dir_all(m.parent().unwrap()).unwrap();
            std::fs::write(&m, body).unwrap();
        };
        mk(
            "14b",
            r#"{"layers":[
                {"digest":"sha256:share","size":1},
                {"digest":"sha256:only-a","size":1},
                {"digest":"sha256:gone","size":1}
            ]}"#,
        );
        mk(
            "14b-q4",
            r#"{"layers":[
                {"digest":"sha256:share","size":1},
                {"digest":"sha256:only-b","size":1},
                {"digest":"sha256:gone","size":1}
            ]}"#,
        );

        let g = LocalModelsScanner::new(root.clone()).scan();
        assert_eq!(g.items.len(), 2, "两个 tag 两行");
        let find = |suffix: &str| {
            g.items
                .iter()
                .find(|i| i.name.ends_with(suffix))
                .unwrap_or_else(|| panic!("{suffix} 该有一行"))
        };

        let a = find("qwen3:14b");
        match a.metrics.get("size") {
            // 按引用计：共享层 + 自己的独占层（共享层在另一行也计一次）
            Some(MetricValue::Bytes(v)) => {
                let expect = block_bytes(&std::fs::metadata(&share).unwrap())
                    + block_bytes(&std::fs::metadata(&only_a).unwrap());
                assert_eq!(*v, expect, "size = 共享层 + 独占层");
            }
            other => panic!("size 该是字节数，实为 {other:?}"),
        }
        let line = detail_value(a, "sharedLayers").expect("共享的 tag 该有共享层明细");
        assert!(
            matches!(text_arg(line, "n"), Some(TextArg::Num(1))),
            "line: {line:?}"
        );
        // 腾出 = 独占层（共享层删了别人还在用，不算）
        assert!(
            matches!(text_arg(line, "freed"), Some(TextArg::Bytes(b)) if *b == 1024 * 1024),
            "腾出该是独占的 1 MB：{line:?}"
        );

        let b = find("qwen3:14b-q4");
        let line = detail_value(b, "sharedLayers").expect("q4 同样该有共享层明细");
        assert!(
            matches!(text_arg(line, "freed"), Some(TextArg::Bytes(b)) if *b == 2 * 1024 * 1024),
            "q4 腾出该是 2 MB：{line:?}"
        );

        assert_frontend_contract(std::slice::from_ref(&g));
        std::fs::remove_dir_all(&root).ok();
    }

    /// 全部层独占（只有一个 tag）时不给共享行——答案就是 size 本身，重复一遍是噪音。
    #[test]
    fn ollama_exclusive_layers_have_no_sharing_line() {
        let root = temp_dir("models-exclusive");
        ollama_fixture(&root);

        let g = LocalModelsScanner::new(root.clone()).scan();
        assert_eq!(g.items.len(), 1);
        let it = &g.items[0];
        assert!(
            detail_value(it, "sharedLayers").is_none(),
            "独占时共享行是噪音：{:?}",
            it.detail
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 坏 manifest（不是 JSON）整行跳过——重构前后行为一致，钉住。
    #[test]
    fn ollama_garbage_manifest_is_skipped_entirely() {
        let root = temp_dir("models-garbage");
        let models = root.join(".ollama/models");
        std::fs::create_dir_all(models.join("blobs")).unwrap();
        let bad = models.join("manifests/registry.ollama.ai/library/x/latest");
        std::fs::create_dir_all(bad.parent().unwrap()).unwrap();
        std::fs::write(&bad, "not json").unwrap();

        let g = LocalModelsScanner::new(root.clone()).scan();
        assert!(g.items.is_empty(), "读不出层清单的行不该出现");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn local_models_reads_lmstudio_and_hf_layouts() {
        let root = temp_dir("models-lms");

        // LM Studio：发布者/模型 两层；hub 下的算「下载暂存」
        let lms = root.join(".lmstudio/models/Qwen/Qwen3-8B-GGUF");
        std::fs::create_dir_all(&lms).unwrap();
        std::fs::write(lms.join("model.gguf"), vec![b'x'; 2000]).unwrap();
        let staging = root.join(".lmstudio/hub/models/google/gemma-4-31b");
        std::fs::create_dir_all(&staging).unwrap(); // 空目录：权重没下完

        // HuggingFace：models--<org>--<name>，blobs 里放权重
        let hf = root.join(".cache/huggingface/hub/models--org--name/blobs");
        std::fs::create_dir_all(&hf).unwrap();
        std::fs::write(hf.join("w.bin"), vec![b'x'; 3000]).unwrap();

        let g = LocalModelsScanner::new(root.clone()).scan();
        let names: Vec<&str> = g.items.iter().map(|i| i.name.as_str()).collect();
        assert!(
            names.iter().any(|n| n.ends_with("Qwen3-8B-GGUF")),
            "LM Studio 行：{names:?}"
        );
        assert!(
            names.iter().any(|n| n.ends_with("gemma-4-31b")),
            "暂存行：{names:?}"
        );
        assert!(
            names.iter().any(|n| n.ends_with("org/name")),
            "HF 行的名字要反解：{names:?}"
        );

        let by_suffix = |n: &str| g.items.iter().find(|i| i.name.ends_with(n)).unwrap();
        let real = by_suffix("Qwen3-8B-GGUF");
        assert_eq!(real.group_key.as_deref(), Some("LM Studio"));
        assert!(matches!(real.metrics.get("size"), Some(MetricValue::Bytes(v)) if *v > 0));
        assert_eq!(
            detail_value(real, "publisher").and_then(text_raw),
            Some("Qwen")
        );

        let st = by_suffix("gemma-4-31b");
        assert_eq!(
            detail_value(st, "status").and_then(text_key),
            Some("data.value.downloadStaging")
        );
        assert_eq!(
            detail_value(st, "weights").and_then(text_key),
            Some("data.value.weightsNotOnDisk")
        );

        let hf_item = by_suffix("org/name");
        assert_eq!(hf_item.group_key.as_deref(), Some("HuggingFace"));
        assert_eq!(detail_value(hf_item, "org").and_then(text_raw), Some("org"));

        assert_frontend_contract(std::slice::from_ref(&g));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn local_models_absent_everywhere_does_not_register() {
        let root = temp_dir("models-empty");
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();

        assert!(
            !LocalModelsScanner::new(home.clone()).exists(),
            "三个来源一个都没有时不该进侧栏（硬约束 8）"
        );
        // 不注册 ≠ 扫了会坏：显式扫也要给出形状合法的空分组
        let g = LocalModelsScanner::new(home).scan();
        assert!(g.items.is_empty());
        assert_eq!(text_key(&g.summary), Some("data.summary.localModels"));
        if let Text::Fmt(f) = &g.summary {
            assert_eq!(f.a.get("count"), Some(&TextArg::Num(0)), "{f:?}");
        }
        assert_frontend_contract(std::slice::from_ref(&g));

        std::fs::remove_dir_all(&root).ok();
    }

    // ---- 清理入口（S2 / D1 拍板选 A）----

    /// 显示判据必须与 `clean_repo` 真删前的复核逐条对齐（S2 修的核心）：
    /// 两条入选口径照旧，未推送 / 未提交改动 / stash 非空一票否决——账面
    /// 「可回收」不许含点下去会被拒的仓库。本机实测：收紧前 29 个里 23 个
    /// （9.41 GB）会被拒。
    #[test]
    fn repos_clean_candidate_mirrors_deletion_judgment() {
        use crate::scan::repos::is_clean_candidate;
        let big = Some(300 * 1024 * 1024);
        let stale = 400; // > 180 天
        let fresh = 10;

        // 入选：大且久未动
        assert!(is_clean_candidate(big, stale, false, 0, false));
        // 入选：从未有过提交（days = i64::MAX）且有体积
        assert!(is_clean_candidate(Some(1024), i64::MAX, false, 0, false));
        // 一票否决：有未推送提交
        assert!(
            !is_clean_candidate(big, stale, true, 0, false),
            "有未推送不该算可回收"
        );
        // 一票否决：有未提交改动
        assert!(
            !is_clean_candidate(big, stale, false, 3, false),
            "有改动不该算可回收"
        );
        // 一票否决：stash 非空（stash 本体在 .git 里，整仓删除连带丢掉）
        assert!(
            !is_clean_candidate(big, stale, false, 0, true),
            "有 stash 不该算可回收"
        );
        // 一票否决叠加在「从未提交」口径上同样成立
        assert!(!is_clean_candidate(Some(1024), i64::MAX, true, 0, false));
        assert!(!is_clean_candidate(Some(1024), i64::MAX, false, 0, true));
        // 不入选：小仓库（再干净也不进清理候选）
        assert!(!is_clean_candidate(Some(1024), stale, false, 0, false));
        // 不入选：大而新
        assert!(!is_clean_candidate(big, fresh, false, 0, false));
        // 边界：恰好 200 MB 不算大（判据是严格大于）
        assert!(!is_clean_candidate(
            Some(200 * 1024 * 1024),
            stale,
            false,
            0,
            false
        ));
        // 边界：恰好 180 天不算久
        assert!(!is_clean_candidate(big, 180, false, 0, false));
    }

    /// 推送状态三态推断的纯函数样本。fail-closed 的核心在样本表里看得最
    /// 清楚：`@{u}` 解析失败不再是「按 ahead = 0 放行」，查不出与「没有」
    /// 从此是两回事。
    #[test]
    fn repos_push_state_fail_closed_samples() {
        use crate::scan::repos::{Push, push_state};

        // 正常路：两列，左落后右未推送（方向铁律，S2 钉过）
        assert_eq!(push_state(Some("0\t0"), None, true), Push::Clean);
        assert_eq!(
            push_state(Some("3\t2"), None, true),
            Push::Ahead {
                ahead: 2,
                behind: 3
            }
        );
        // 不是两列 / 数字解析不出 = 核实不了，不是「没有」
        assert_eq!(push_state(Some("garbage"), None, true), Push::Unknown);
        assert_eq!(push_state(Some("1"), None, true), Push::Unknown);

        // @{u} 失败退回「HEAD 不在任何远端上的提交数」
        assert_eq!(push_state(None, Some("0"), true), Push::Clean);
        assert_eq!(
            push_state(None, Some("4"), true),
            Push::Ahead {
                ahead: 4,
                behind: 0
            }
        );
        // 退路也解析不了 HEAD、git 本身好着 → 从未有过提交，没有可推的
        // （工作区内容与 stash 由另外两条否决拦）
        assert_eq!(push_state(None, None, true), Push::Clean);

        // git 整体不可信（stash list 都失败）→ 一律按未知
        assert_eq!(push_state(Some("0\t0"), None, false), Push::Unknown);
        assert_eq!(push_state(None, Some("0"), false), Push::Unknown);
    }

    /// `confirmed` 缺省时反序列化直接失败——第一道闸在进门处，不靠服务端想得起。
    #[test]
    fn repos_clean_plan_requires_confirmed_field() {
        let ok: Result<crate::scan::CleanPlan, _> =
            serde_json::from_str(r#"{"group_id":"repos","item_ids":["/tmp/x"],"confirmed":true}"#);
        assert!(ok.is_ok(), "三字段齐备应该能解析");

        let missing: Result<crate::scan::CleanPlan, _> =
            serde_json::from_str(r#"{"group_id":"repos","item_ids":["/tmp/x"]}"#);
        assert!(missing.is_err(), "缺 confirmed 必须拒——服务端不接受缺省");
    }

    /// 在 `dir` 里造一个临时 git 仓库（init + 身份 + 首个提交）。
    /// 身份用 `-c` 传入，不碰用户全局配置；git 缺失时直接失败——repos 扫描器
    /// 本来就靠 git 子进程，环境没有 git 是真问题，不该静默跳过。
    fn git_repo(tag: &str) -> PathBuf {
        let root = temp_dir(tag);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git_in(&repo, &["init", "-q"]);
        git_in(&repo, &["config", "user.email", "test@example.com"]);
        git_in(&repo, &["config", "user.name", "test"]);
        std::fs::write(repo.join("a.txt"), "hello").unwrap();
        git_in(&repo, &["add", "a.txt"]);
        git_in(&repo, &["commit", "-q", "-m", "first"]);
        repo
    }

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("环境里应能执行 git");
        assert!(
            out.status.success(),
            "git {args:?} 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// 把仓库装成「全部推过了」：挂一个不会被访问的 origin，远端跟踪引用
    /// 指向 HEAD。分支名取自 abbrev-ref，不写死 master。
    fn sync_with_phantom_origin(repo: &Path) {
        let branch = git_in(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let head = git_in(repo, &["rev-parse", "HEAD"]);
        git_in(
            repo,
            &["remote", "add", "origin", "https://example.com/no-op.git"],
        );
        git_in(
            repo,
            &["config", &format!("branch.{branch}.remote"), "origin"],
        );
        git_in(
            repo,
            &[
                "config",
                &format!("branch.{branch}.merge"),
                &format!("refs/heads/{branch}"),
            ],
        );
        git_in(
            repo,
            &[
                "update-ref",
                &format!("refs/remotes/origin/{branch}"),
                &head,
            ],
        );
    }

    /// `confirmed: false` 时整个计划拒绝，且**不碰磁盘**（仓库原地不动）。
    #[test]
    fn repos_clean_rejects_unconfirmed_plan_without_touching_disk() {
        let repo = git_repo("clean-unconfirmed");
        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: false,
            scope: crate::scan::CleanScope::Repo,
            artifact_targets: None,
        };

        let r = scanner.clean_repo(&plan);
        assert!(r.succeeded.is_empty(), "未确认不该成功任何一项");
        assert_eq!(r.failed.len(), 1);
        assert_eq!(
            text_key(&r.failed[0].reason),
            Some("data.cleanFail.unconfirmed")
        );
        assert_eq!(r.freed_bytes, 0);
        assert!(repo.exists(), "未确认时不该碰磁盘");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// 有未推送提交的仓库：`clean_repo` 真删前复核拒绝，且不进废纸篓。
    /// （这条同时是「显示判据收紧」的镜像——两侧判据必须一致。）
    #[test]
    fn repos_clean_rejects_unpushed_repo() {
        let repo = git_repo("clean-unpushed");
        // 第二个提交，并把「远端」停在第一个提交上 → HEAD ahead 1
        std::fs::write(repo.join("b.txt"), "second").unwrap();
        git_in(&repo, &["add", "b.txt"]);
        git_in(&repo, &["commit", "-q", "-m", "second"]);
        let first = git_in(&repo, &["rev-parse", "HEAD~1"]);
        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        // 必须真有一个 origin remote：只有 branch.*.remote 而没有 [remote "origin"]
        // 段时，@{u} 解析不出远程跟踪分支（fatal: not stored as a remote-tracking
        // branch），rev-list 失败 → ahead 恒 0 → 未推送检出不了。URL 不会被访问。
        git_in(
            &repo,
            &["remote", "add", "origin", "https://example.com/no-op.git"],
        );
        git_in(
            &repo,
            &["config", &format!("branch.{branch}.remote"), "origin"],
        );
        git_in(
            &repo,
            &[
                "config",
                &format!("branch.{branch}.merge"),
                "refs/heads/master",
            ],
        );
        git_in(&repo, &["update-ref", "refs/remotes/origin/master", &first]);

        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Repo,
            artifact_targets: None,
        };

        let r = scanner.clean_repo(&plan);
        assert!(r.succeeded.is_empty(), "有未推送提交不该被删");
        assert_eq!(r.failed.len(), 1);
        assert_eq!(
            text_key(&r.failed[0].reason),
            Some("data.cleanFail.unpushed")
        );
        assert_eq!(r.freed_bytes, 0);
        assert!(repo.exists(), "被拒的仓库必须原地留在磁盘上");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// ahead / behind 两列不能赋反（S2 抓到的真 bug）：`@{u}...HEAD` 的
    /// `--left-right` 输出是「落后数\t未推送数」，赋反了「真有未推送提交」的仓库
    /// 会绕过 `clean_repo` 的复核直接进废纸篓——方向反了比不判更危险。
    ///
    /// 用**扫描输出**断言方向（「未推送」标签与 dirty 指标），不碰 trash：
    /// 真删路径由 `repos_clean_rejects_unpushed_repo` 单独验。
    #[test]
    fn repos_scan_reports_unpushed_direction_correctly() {
        let root = temp_dir("ahead-behind");

        // 甲：本地领先远端一个提交（真·未推送）
        let unpushed = root.join("unpushed");
        std::fs::create_dir_all(&unpushed).unwrap();
        git_in(&unpushed, &["init", "-q"]);
        git_in(&unpushed, &["config", "user.email", "test@example.com"]);
        git_in(&unpushed, &["config", "user.name", "test"]);
        std::fs::write(unpushed.join("a.txt"), "1").unwrap();
        git_in(&unpushed, &["add", "a.txt"]);
        git_in(&unpushed, &["commit", "-q", "-m", "c1"]);
        std::fs::write(unpushed.join("b.txt"), "2").unwrap();
        git_in(&unpushed, &["add", "b.txt"]);
        git_in(&unpushed, &["commit", "-q", "-m", "c2"]);
        let c1 = git_in(&unpushed, &["rev-parse", "HEAD~1"]);
        git_in(
            &unpushed,
            &["remote", "add", "origin", "https://example.com/no-op.git"],
        );
        git_in(&unpushed, &["config", "branch.master.remote", "origin"]);
        git_in(
            &unpushed,
            &["config", "branch.master.merge", "refs/heads/master"],
        );
        git_in(
            &unpushed,
            &["update-ref", "refs/remotes/origin/master", &c1],
        );

        // 乙：本地落后远端一个提交（别人推的，本地没有——不该算「未推送」）
        let behind = root.join("behind");
        std::fs::create_dir_all(&behind).unwrap();
        git_in(&behind, &["init", "-q"]);
        git_in(&behind, &["config", "user.email", "test@example.com"]);
        git_in(&behind, &["config", "user.name", "test"]);
        std::fs::write(behind.join("a.txt"), "1").unwrap();
        git_in(&behind, &["add", "a.txt"]);
        git_in(&behind, &["commit", "-q", "-m", "c1"]);
        std::fs::write(behind.join("b.txt"), "2").unwrap();
        git_in(&behind, &["add", "b.txt"]);
        git_in(&behind, &["commit", "-q", "-m", "c2"]);
        let c2 = git_in(&behind, &["rev-parse", "HEAD"]);
        git_in(
            &behind,
            &["remote", "add", "origin", "https://example.com/no-op.git"],
        );
        git_in(&behind, &["config", "branch.master.remote", "origin"]);
        git_in(
            &behind,
            &["config", "branch.master.merge", "refs/heads/master"],
        );
        git_in(&behind, &["update-ref", "refs/remotes/origin/master", &c2]);
        git_in(&behind, &["reset", "--hard", "HEAD~1"]); // 本地退回 c1 → 落后 1

        let g = RepoScanner::new(vec![root.clone()]).scan();
        let find = |name: &str| {
            g.items
                .iter()
                .find(|i| i.name == name)
                .unwrap_or_else(|| panic!("{name} 应被扫到"))
                .clone()
        };

        let up = find("unpushed");
        assert!(
            up.tags.iter().any(|t| t.key == "unpushed"),
            "领先远端该标「未推送」：tags={:?}",
            up.tags
        );
        match up.metrics.get("dirty") {
            Some(MetricValue::Text(s)) => assert_eq!(s, "↑1 ↓0", "未推送 1、落后 0"),
            other => panic!("dirty 该是文本，实为 {other:?}"),
        }

        let be = find("behind");
        assert!(
            !be.tags.iter().any(|t| t.key == "unpushed"),
            "只是落后不该标「未推送」：tags={:?}",
            be.tags
        );
        match be.metrics.get("dirty") {
            Some(MetricValue::TextKey(k)) => {
                assert_eq!(k, "data.value.clean", "无未推送无改动 → 干净")
            }
            other => panic!("dirty 该是文案键，实为 {other:?}"),
        }

        std::fs::remove_dir_all(&root).ok();
    }

    /// 没配 upstream 的仓库曾经按 ahead = 0 放行——全部提交都没推送过的
    /// 恰恰是最该拦的。退路计数（HEAD 不在任何远端上）必须接住它：账面不标
    /// 可清理、点下去被拒，且给出确切的未推送数。
    #[test]
    fn repos_clean_refuses_repo_without_upstream() {
        let repo = git_repo("clean-no-upstream"); // 有提交、没远端、没 upstream
        let root = repo.parent().unwrap().to_path_buf();

        let g = RepoScanner::new(vec![root.clone()]).scan();
        let item = g.items.iter().find(|i| i.name == "repo").expect("应扫到");
        assert!(
            item.tags.iter().any(|t| t.key == "unpushed"),
            "退路计数应识别出未推送：tags={:?}",
            item.tags
        );
        assert!(!item.cleanable, "从未推送过的仓库不该标可清理");
        match item.metrics.get("dirty") {
            Some(MetricValue::Text(s)) => assert_eq!(s, "↑1 ↓0", "退路计数：未推送 1、无落后参照"),
            other => panic!("dirty 该是文本，实为 {other:?}"),
        }

        let scanner = RepoScanner::new(vec![root]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Repo,
            artifact_targets: None,
        };
        let r = scanner.clean_repo(&plan);
        assert!(r.succeeded.is_empty(), "从未推送过的仓库不该被删");
        assert_eq!(r.failed.len(), 1);
        assert_eq!(
            text_key(&r.failed[0].reason),
            Some("data.cleanFail.unpushed")
        );
        assert!(repo.exists(), "被拒的仓库必须原地留在磁盘上");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// 推过但 upstream 没配的仓库**不能被误伤**：`@{u}` 解析失败时退路计数
    /// 为 0，按「已核实无未推送」处理。这是 fail-closed 收紧的边界，不是
    /// 收紧对象——收紧过头会把能安全清理的仓库挡在外面。
    #[test]
    fn repos_pushed_without_upstream_stays_clean() {
        let repo = git_repo("pushed-no-upstream");
        sync_with_phantom_origin(&repo);
        // 撤掉 upstream 配置 → @{u} 解析失败，只剩远端跟踪引用
        git_in(&repo, &["config", "--unset", "branch.master.remote"]);
        git_in(&repo, &["config", "--unset", "branch.master.merge"]);

        let g = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]).scan();
        let item = g.items.iter().find(|i| i.name == "repo").expect("应扫到");
        assert!(
            !item.tags.iter().any(|t| t.key == "unpushed"),
            "推过（HEAD 在远端引用里）不该标未推送：tags={:?}",
            item.tags
        );
        match item.metrics.get("dirty") {
            Some(MetricValue::TextKey(k)) => assert_eq!(k, "data.value.clean"),
            other => panic!("dirty 该是文案键，实为 {other:?}"),
        }

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// stash 本体在 `.git` 里，整仓进废纸篓会连它一起没——`status --porcelain`
    /// 看不见 stash，stash 里有未提交工作的仓库曾经被当成「干净」放行。
    #[test]
    fn repos_clean_refuses_repo_with_stash() {
        let repo = git_repo("clean-stash");
        sync_with_phantom_origin(&repo); // 推送干净，把否决让给 stash 这条
        // 造一个**真 stash**：`git stash` 的高层命令跨版本有歧义（save 已废弃、
        // push 在部分版本报子命令不明确），create + store 是它俩的底层原语，
        // 写的正是真实 stash 的 reflog——`stash list` 能看到，工作区保持干净
        // （改动若留在 index / worktree 会先撞上「未提交改动」否决）。
        std::fs::write(repo.join("a.txt"), "changed").unwrap();
        let sha = git_in(&repo, &["stash", "create", "WIP on master"]);
        git_in(&repo, &["stash", "store", "-m", "WIP on master", &sha]);
        git_in(&repo, &["checkout", "-q", "--", "a.txt"]);

        let g = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]).scan();
        let item = g.items.iter().find(|i| i.name == "repo").expect("应扫到");
        assert!(
            item.tags.iter().any(|t| t.key == "stashed"),
            "有 stash 该打标签：tags={:?}",
            item.tags
        );
        assert!(!item.cleanable, "有 stash 不该标可清理");

        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Repo,
            artifact_targets: None,
        };
        let r = scanner.clean_repo(&plan);
        assert!(r.succeeded.is_empty(), "有 stash 不该被删");
        assert_eq!(r.failed.len(), 1);
        assert_eq!(text_key(&r.failed[0].reason), Some("data.cleanFail.stash"));
        assert!(repo.exists(), "被拒的仓库必须原地留在磁盘上");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// 从未有过提交的空 init 仓库是设计内候选（「从未提交」口径）——推送
    /// 核查与 stash 否决都不能误伤它：HEAD 解析不了且 git 正常 → 没有可推
    /// 的东西；stash list 正常返回空。
    #[test]
    fn repos_unborn_head_stays_candidate() {
        let root = temp_dir("unborn");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git_in(&repo, &["init", "-q"]); // 不提交

        let g = RepoScanner::new(vec![root.clone()]).scan();
        let item = g.items.iter().find(|i| i.name == "repo").expect("应扫到");
        assert!(
            !item.tags.iter().any(|t| t.key == "unpushed"),
            "没提交就没可推的：tags={:?}",
            item.tags
        );
        assert!(
            !item.tags.iter().any(|t| t.key == "stashed"),
            "空 init 没有 stash"
        );
        assert!(
            item.cleanable,
            "空 init 是设计内候选（从未提交口径）：tags={:?}",
            item.tags
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// detached HEAD 曾显示成字面 "HEAD"（abbrev-ref 对游离状态返回的就是它，
    /// 按空串判断永远接不到「游离」），且游离提交不在任何分支上、没推送过
    /// ——必须显示「游离」并拒删。
    #[test]
    fn repos_detached_head_shows_detached_and_refuses_clean() {
        let repo = git_repo("detached");
        git_in(&repo, &["checkout", "-q", "--detach", "HEAD"]); // 唯一提交不在任何远端

        let g = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]).scan();
        let item = g.items.iter().find(|i| i.name == "repo").expect("应扫到");
        match item.metrics.get("branch") {
            Some(MetricValue::TextKey(k)) => assert_eq!(
                k, "data.value.detachedBranch",
                "游离 HEAD 该下发文案键，不是某一种语言的成品",
            ),
            other => panic!("branch 该是文本，实为 {other:?}"),
        }
        assert!(
            item.tags.iter().any(|t| t.key == "unpushed"),
            "游离提交没推送过，退路计数应识别"
        );
        assert!(!item.cleanable);

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// git 坏掉的仓库（任何 git 调用都失败）：推送状态按未知，绝不标可清理。
    /// 这是 fail-closed 的兜底档。stash 没核实成——「有 stash」标签只挂核实过
    /// 的，不把保守假设当事实展示；否决由「推送未核实」承担，不靠 stash 假设。
    #[test]
    fn repos_broken_git_is_never_cleanable() {
        let root = temp_dir("broken-git");
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git").join("HEAD"), "not a real gitfile").unwrap();
        std::fs::write(repo.join("junk.txt"), "x").unwrap();

        let g = RepoScanner::new(vec![root.clone()]).scan();
        let item = g.items.iter().find(|i| i.name == "repo").expect("应扫到");
        assert!(
            !item.cleanable,
            "git 检查不了的仓库不该标可清理：tags={:?}",
            item.tags
        );
        assert!(
            item.tags.iter().any(|t| t.key == "unpushed"),
            "推送状态按未知处理：tags={:?}",
            item.tags
        );
        assert!(
            !item.tags.iter().any(|t| t.key == "stashed"),
            "stash 没核实成不许挂「有 stash」标签：tags={:?}",
            item.tags
        );
        match item.metrics.get("dirty") {
            Some(MetricValue::TextKey(k)) => assert_eq!(k, "data.value.pushUnknown"),
            other => panic!("dirty 该是文案键，实为 {other:?}"),
        }

        std::fs::remove_dir_all(&root).ok();
    }

    /// 「除清理入口外全程只读」的承诺对 git 子进程也成立：`git status` 会
    /// 机会性回写 `.git/index`（racy 清理：index 与文件同秒写入后，隔一秒
    /// 跑一次 status 就会回写、清掉 raciness），`GIT_OPTIONAL_LOCKS=0` 让
    /// 刷新只留在内存。对照组先证明「不保护真的会写」——git 哪天不再这么
    /// 做了，这条会先红提醒我们被测断言已失去前提。
    #[test]
    fn repos_git_calls_never_write_index() {
        let stamp = |repo: &Path| {
            let m = std::fs::metadata(repo.join(".git").join("index")).unwrap();
            let t = m
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            (t.as_nanos(), m.len())
        };
        let settle = || std::thread::sleep(std::time::Duration::from_millis(1100));

        // 强制 racy：把 .git/index 的 mtime 拨回与 a.txt 同刻。git 的 racy 判定是
        // 秒粒度（USE_NSEC 默认关），前提是「add 写 index 与写文件恰好同一墙钟秒」
        // ——那是 fixture 的时序运气，nextest 并行负载下实测撞过秒边界，对照组
        // 因此假红过。两边都钉死：对照组保证「status 真的有 racy 要清」，
        // 被测侧保证「不写 index」不是碰巧没事可做的空过。
        let force_racy = |repo: &Path| {
            let t = std::fs::metadata(repo.join("a.txt"))
                .unwrap()
                .modified()
                .unwrap();
            let index = repo.join(".git").join("index");
            let f = std::fs::File::options().write(true).open(&index).unwrap();
            f.set_modified(t).unwrap();
        };

        // 对照组：不带保护的 git status 会回写 index
        let control = git_repo("index-write-control");
        force_racy(&control);
        settle();
        let before = stamp(&control);
        git_in(&control, &["status", "--porcelain"]);
        assert_ne!(
            before,
            stamp(&control),
            "对照组失效：plain status 不再回写 index，被测断言的前提没了"
        );

        // 被测：走 git_output（GIT_OPTIONAL_LOCKS=0）不回写
        let repo = git_repo("index-write-protected");
        force_racy(&repo);
        settle();
        let before = stamp(&repo);
        crate::scan::repos::git_output(&repo, &["status", "--porcelain"]);
        assert_eq!(
            before,
            stamp(&repo),
            "git_output 不该写 .git/index（只读承诺）"
        );

        std::fs::remove_dir_all(control.parent().unwrap()).ok();
        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// 有未提交改动的仓库：同样拒绝。改动是**唯一副本**的代码，删了不可再生。
    #[test]
    fn repos_clean_rejects_dirty_repo() {
        let repo = git_repo("clean-dirty");
        // 先装成「推过了」：fail-closed 之后没推过的仓库会先撞上「未推送」
        // 否决，这条测试要隔离验证的只是「未提交改动」这一条
        sync_with_phantom_origin(&repo);
        std::fs::write(repo.join("a.txt"), "modified").unwrap(); // 已跟踪文件被改

        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Repo,
            artifact_targets: None,
        };

        let r = scanner.clean_repo(&plan);
        assert!(r.succeeded.is_empty(), "有未提交改动不该被删");
        assert_eq!(r.failed.len(), 1);
        assert!(
            text_key(&r.failed[0].reason) == Some("data.cleanFail.dirtyFiles"),
            "reason: {:?}",
            r.failed[0].reason
        );
        assert!(repo.exists(), "被拒的仓库必须原地留在磁盘上");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    // ---- 构建产物白名单与产物清理（P2）----

    /// 白名单整表钉死（哨兵）：条目、标记的写法与内容都在这里。加 / 删条目时
    /// 这条会红——想清楚再动表：名字不在表里的一律不是产物，每个条目必须有
    /// 标记文件门控（`src/bin` 教训），vendor 类「常被刻意提交」的不进表。
    #[test]
    fn artifact_whitelist_is_pinned() {
        let rules = artifact_rules_for_test();
        let names: Vec<_> = rules.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "target",
                "node_modules",
                "__pycache__",
                ".venv",
                "venv",
                ".tox",
                ".pytest_cache",
                "build",
                "dist",
                "bin",
                "obj",
                "zig-cache",
                "zig-out",
                ".stack-work",
                "dist-newstyle",
                "_build",
                ".build",
                ".dart_tool",
            ]
        );
        for (name, markers) in &rules {
            assert!(!markers.is_empty(), "{name} 没有任何标记文件");
        }
        let get = |n: &str| rules.iter().find(|(x, _)| *x == n).map(|(_, m)| m).unwrap();
        assert_eq!(
            get("bin"),
            &vec![
                ("suffix", ".csproj"),
                ("suffix", ".fsproj"),
                ("suffix", ".vbproj")
            ]
        );
        assert!(get("node_modules").contains(&("exact", "package.json")));
        assert!(get("target").contains(&("exact", "Cargo.toml")));
        assert!(get("target").contains(&("exact", "pom.xml")));
        assert!(get("build").contains(&("exact", "build.gradle")));
        assert!(get("build").contains(&("exact", "pubspec.yaml")));
        assert!(get(".dart_tool").contains(&("exact", "pubspec.yaml")));
        assert!(
            !names.contains(&"vendor"),
            "vendor 常被刻意提交，不进白名单"
        );
    }

    /// 发现集逐条钉死：命中（带标记的产物目录）、跳过（无标记同名目录、
    /// Rust 源码目录 src/bin、非白名单名字）、剪枝（产物内部不再深入、
    /// 嵌套仓库的产物归它自己那一行）。喂临时目录，不依赖本机。
    #[test]
    fn artifact_discovery_set_is_exact() {
        let root = temp_dir("artifact-discovery");
        std::fs::create_dir_all(root.join(".git")).unwrap();

        // 命中：根下 node_modules（标记 package.json 在兄弟位置——项目根）；
        // 内部的嵌套 node_modules 不单独立账（点中的子树不再深入）
        std::fs::create_dir_all(root.join("node_modules/.pnpm/x/node_modules")).unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();
        // 命中：Cargo target
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();
        // 跳过：src/bin 是 Cargo 的源码目录约定，没有 .csproj 标记不许命中
        std::fs::create_dir_all(root.join("src/bin")).unwrap();
        std::fs::write(root.join("src/bin/main.rs"), "fn main() {}").unwrap();
        // 跳过：vendor 不在白名单（有 go.mod 也救不了它）
        std::fs::create_dir_all(root.join("vendor")).unwrap();
        std::fs::write(root.join("go.mod"), "module x").unwrap();
        // 跳过：无标记的 build（手写脚本目录的同名陷阱）
        std::fs::create_dir_all(root.join("build")).unwrap();
        // 命中：任意深度的 node_modules（monorepo 子包，标记在兄弟位置）
        std::fs::create_dir_all(root.join("packages/app/node_modules")).unwrap();
        std::fs::write(root.join("packages/app/package.json"), "{}").unwrap();
        // 剪枝：嵌套仓库里的产物不归本行
        let nested = root.join("vendored-checkout");
        std::fs::create_dir_all(nested.join(".git")).unwrap();
        std::fs::create_dir_all(nested.join("node_modules")).unwrap();

        let found = find_artifact_dirs(&root);
        let rel: Vec<String> = found
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            rel,
            vec!["node_modules", "packages/app/node_modules", "target"]
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// 完整列表不能因展示上限丢目录；体积用独立遍历作基准，不解析格式化文本。
    #[test]
    fn repos_artifact_entries_are_complete_sorted_and_exact() {
        let root = temp_dir("repos-artifact-entries");
        let repo = root.join("repo-a");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join("package.json"), "{}").unwrap();
        std::fs::write(repo.join("Cargo.toml"), "[package]").unwrap();
        let mut paths = vec![
            (PathBuf::from("node_modules"), 4096),
            (PathBuf::from("target"), 16384),
        ];
        // 同体积且逆序创建，防止目录遍历顺序冒充稳定排序。
        for i in (0..13).rev() {
            let package = repo.join("packages").join(format!("包 {i:02} · app"));
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(package.join("package.json"), "{}").unwrap();
            paths.push((
                package.strip_prefix(&repo).unwrap().join("node_modules"),
                8192,
            ));
        }
        let mut expected = Vec::new();
        for (rel, bytes) in paths {
            let path = repo.join(&rel);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("out.bin"), vec![1u8; bytes]).unwrap();
            expected.push((rel.to_string_lossy().into_owned(), tree_bytes(&path)));
        }
        expected.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        assert!(expected.windows(2).any(|pair| pair[0].1 == pair[1].1));
        let expected_json = serde_json::Value::Array(
            expected
                .iter()
                .map(
                    |(path, bytes)| serde_json::json!({"relative_path": path, "size_bytes": bytes}),
                )
                .collect(),
        );

        let scanner = RepoScanner::new(vec![root.clone()]);
        for _ in 0..2 {
            let g = scanner.scan();
            assert_eq!(g.items.len(), 1);
            let item = &g.items[0];
            assert_eq!(item.path, repo.display().to_string());
            let json = serde_json::to_value(item).unwrap();
            assert_eq!(
                json["artifact_entries"], expected_json,
                "全部 15 个目录与原始字节都应保留"
            );
            let line = detail_value(item, "artifacts").expect("detail 该有构建产物行");
            assert!(
                matches!(text_arg(line, "dirs"), Some(TextArg::Num(15))),
                "该是 15 个目录：{line:?}"
            );
            let list = match text_arg(line, "list") {
                Some(TextArg::Raw(s)) => s.as_str(),
                other => panic!("目录清单该是裸字串，实为 {other:?}"),
            };
            assert!(list.contains("node_modules×14"), "list: {list}");
            assert!(list.contains("target"), "list: {list}");
            assert!(item.tags.iter().any(|tag| tag.key == "artifacts"));
            match item.metrics.get("artifacts") {
                Some(MetricValue::Bytes(b)) => {
                    assert_eq!(*b, expected.iter().map(|(_, n)| n).sum::<u64>())
                }
                other => panic!("artifacts 指标缺失或类型不对：{other:?}"),
            }
            assert_frontend_contract(std::slice::from_ref(&g));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 标记门控的代表性样本：同名目录，标记在 → 命中；标记不在 → 不命中。
    /// 泛化名字（bin / build / dist / target）是误删风险的主要来源，逐个钉住。
    #[test]
    fn artifact_markers_gate_generic_names() {
        let root = temp_dir("artifact-markers");
        let mk = |rel: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(&p).unwrap();
            p
        };
        let wf = |dir: &Path, name: &str| std::fs::write(dir.join(name), "x").unwrap();

        // Cargo 的 src/bin：永远不该命中（没有 .NET 项目标记）
        assert!(!is_artifact_dir_under(&root, &mk("src/bin")));
        // .NET：bin / obj 在任意深度，兄弟有 .csproj 才命中
        let proj = mk("src/App");
        wf(&proj, "App.csproj");
        assert!(is_artifact_dir_under(&root, &proj.join("bin")));
        assert!(is_artifact_dir_under(&root, &proj.join("obj")));
        assert!(!is_artifact_dir_under(&root, &mk("src/NoProj/bin")));
        // node_modules：package.json 在 → 命中，不在 → 不命中
        assert!(!is_artifact_dir_under(&root, &mk("a/node_modules")));
        let a = mk("a");
        wf(&a, "package.json");
        assert!(is_artifact_dir_under(&root, &a.join("node_modules")));
        // build：Gradle / Flutter / setuptools 标记放行，裸名字不放行
        assert!(!is_artifact_dir_under(&root, &mk("b/build")));
        let b = mk("b");
        wf(&b, "build.gradle.kts");
        assert!(is_artifact_dir_under(&root, &b.join("build")));
        let c = mk("c");
        wf(&c, "pubspec.yaml");
        assert!(is_artifact_dir_under(&root, &c.join("build")));
        let d = mk("d");
        wf(&d, "pyproject.toml");
        assert!(is_artifact_dir_under(&root, &d.join("build")));
        // dist：pyproject.toml / package.json 放行
        assert!(is_artifact_dir_under(&root, &d.join("dist")));
        let e = mk("e");
        wf(&e, "package.json");
        assert!(is_artifact_dir_under(&root, &e.join("dist")));
        // target：Maven 放行，裸名字不放行
        assert!(!is_artifact_dir_under(&root, &mk("f/target")));
        let f = mk("f");
        wf(&f, "pom.xml");
        assert!(is_artifact_dir_under(&root, &f.join("target")));

        std::fs::remove_dir_all(&root).ok();
    }

    /// 扫描把产物记进账面：`metrics.artifacts`（字节）、标签「有产物」、
    /// detail「构建产物」一行。体积与独立累加（[`tree_bytes`]）逐字节相等；
    /// 无标记的同名目录不许混进账。
    #[test]
    fn repos_scan_reports_artifact_accounting() {
        let repo = git_repo("artifact-scan");
        let nm = repo.join("node_modules");
        std::fs::create_dir_all(nm.join("pkg")).unwrap();
        std::fs::write(repo.join("package.json"), "{}").unwrap();
        std::fs::write(nm.join("pkg/index.js"), vec![0u8; 8192]).unwrap();
        // 无标记的 dist 不许混进账（sub/ 下没有任何 JS/Py 项目标记）
        std::fs::create_dir_all(repo.join("sub/dist")).unwrap();

        let g = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]).scan();
        let item = g
            .items
            .iter()
            .find(|i| i.id == repo.display().to_string())
            .expect("该扫到这个仓库");
        match item.metrics.get("artifacts") {
            Some(MetricValue::Bytes(b)) => assert_eq!(*b, tree_bytes(&nm), "产物体积 = 独立累加"),
            other => panic!("该有 artifacts 指标，实为 {other:?}"),
        }
        assert!(item.tags.iter().any(|t| t.key == "artifacts"));
        let line = detail_value(item, "artifacts").expect("detail 该有构建产物行");
        let list = match text_arg(line, "list") {
            Some(TextArg::Raw(s)) => s.as_str(),
            other => panic!("目录清单该是裸字串，实为 {other:?}"),
        };
        assert!(list.contains("node_modules"), "list: {list}");
        assert!(!list.contains("dist"), "无标记的 dist 不进账: {list}");
        assert_eq!(
            item.artifact_entries,
            vec![crate::scan::ArtifactEntry {
                relative_path: "node_modules".into(),
                size_bytes: tree_bytes(&nm),
            }]
        );
        assert_frontend_contract(std::slice::from_ref(&g));

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// `scope` 字段：缺省 = repo（兼容 P2 之前的计划）；"artifacts" 可解析；
    /// 未知取值直接拒——范围打错不许退化成整仓删除。
    #[test]
    fn repos_clean_plan_scope_deserialization() {
        let base = r#"{"group_id":"repos","item_ids":["/tmp/x"],"confirmed":true"#;
        let missing: crate::scan::CleanPlan = serde_json::from_str(&format!("{base}}}")).unwrap();
        assert_eq!(missing.scope, crate::scan::CleanScope::Repo);

        let artifacts: crate::scan::CleanPlan =
            serde_json::from_str(&format!("{base},\"scope\":\"artifacts\"}}")).unwrap();
        assert_eq!(artifacts.scope, crate::scan::CleanScope::Artifacts);

        let bogus: Result<crate::scan::CleanPlan, _> =
            serde_json::from_str(&format!("{base},\"scope\":\"everything\"}}"));
        assert!(bogus.is_err(), "未知 scope 必须拒");
    }

    /// 产物清理：未确认整个拒绝，不碰磁盘（与整仓删除同一道闸）。
    #[test]
    fn repos_clean_artifacts_rejects_unconfirmed() {
        let repo = git_repo("art-unconfirmed");
        let nm = repo.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("package.json"), "{}").unwrap();

        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: false,
            scope: crate::scan::CleanScope::Artifacts,
            artifact_targets: None,
        };
        let r = scanner.clean_artifacts(&plan);
        assert!(r.succeeded.is_empty());
        assert_eq!(r.failed.len(), 1);
        assert_eq!(
            text_key(&r.failed[0].reason),
            Some("data.cleanFail.unconfirmed")
        );
        assert!(nm.exists(), "未确认时不该碰磁盘");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// 不是 Git 仓库（`.git` 不在）的目录：拒——产物清理只对着仓库行发起。
    #[test]
    fn repos_clean_artifacts_rejects_non_repo() {
        let root = temp_dir("art-nonrepo");
        let dir = root.join("not-a-repo");
        let nm = dir.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("package.json"), "{}").unwrap();

        let scanner = RepoScanner::new(vec![root.clone()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![dir.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Artifacts,
            artifact_targets: None,
        };
        let r = scanner.clean_artifacts(&plan);
        assert!(r.succeeded.is_empty(), "不该成功任何一项");
        assert_eq!(r.failed.len(), 1);
        assert!(
            text_key(&r.failed[0].reason) == Some("data.cleanFail.notGitRepo"),
            "reason: {:?}",
            r.failed[0].reason
        );
        assert!(nm.exists(), "被拒的目录必须原地不动");

        std::fs::remove_dir_all(&root).ok();
    }

    /// 仓库里没有产物：明确说「未发现」，而不是成功腾出 0 字节。
    #[test]
    fn repos_clean_artifacts_rejects_repo_without_artifacts() {
        let repo = git_repo("art-none");
        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Artifacts,
            artifact_targets: None,
        };
        let r = scanner.clean_artifacts(&plan);
        assert!(r.succeeded.is_empty());
        assert_eq!(r.failed.len(), 1);
        assert!(
            text_key(&r.failed[0].reason) == Some("data.cleanFail.noArtifacts"),
            "reason: {:?}",
            r.failed[0].reason
        );
        assert!(repo.exists(), "被拒的仓库必须原地留在磁盘上");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// 产物目录里藏着 Git 仓库：整目录拒——把一个仓库当产物连带进废纸篓，
    /// 超出「清理产物」的承诺。这是复核的拒绝路径，真删留给真人。
    #[test]
    fn repos_clean_artifacts_refuses_artifact_hiding_a_repo() {
        let repo = git_repo("art-hiding");
        let nm = repo.join("node_modules");
        std::fs::create_dir_all(nm.join("embedded/.git")).unwrap();
        std::fs::write(repo.join("package.json"), "{}").unwrap();

        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Artifacts,
            artifact_targets: None,
        };
        let r = scanner.clean_artifacts(&plan);
        assert!(r.succeeded.is_empty(), "藏着仓库的产物不许删");
        assert_eq!(r.failed.len(), 1);
        assert!(
            text_key(&r.failed[0].reason) == Some("data.cleanFail.artifactRecheckFailed"),
            "reason: {:?}",
            r.failed[0].reason
        );
        assert!(nm.exists(), "被拒的产物目录必须原地不动");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// 收紧后的判据在**真实扫描**上生效，两个方向都验：干净的大陈旧仓库算可回收
    /// （且回收量 = 整个目录体积），弄脏之后立刻不算。喂临时目录，不依赖本机。
    ///
    /// 为什么造 210 MB：入选口径要求 >200 MB，造小的会是一个空转的测试
    /// （小仓库本来就不入选，没有这条判据它也过）。git 提交用环境变量把
    /// **提交者日期**一起推到 400 天前——判据读的是 `%ct`，只改 author date 不够。
    #[test]
    fn repos_scan_cleanable_follows_tightened_judgment() {
        let root = temp_dir("clean-scan");
        let repo = root.join("big-stale");
        std::fs::create_dir_all(&repo).unwrap();
        git_in(&repo, &["init", "-q"]);
        git_in(&repo, &["config", "user.email", "test@example.com"]);
        git_in(&repo, &["config", "user.name", "test"]);

        let commit_old = |dir: &Path| {
            // 400 天前的绝对 Unix 时间戳。判据读 `%ct`（提交者日期），且
            // GIT_*_DATE 环境变量不吃 "400 days ago" 这种约对日期（fatal: invalid
            // date format），必须给能解析的格式。
            let old = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - 400 * 86_400;
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["commit", "-q", "-m", "old"])
                .env("GIT_AUTHOR_DATE", old.to_string())
                .env("GIT_COMMITTER_DATE", old.to_string())
                .output()
                .expect("git 可执行");
            assert!(
                out.status.success(),
                "提交失败: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };

        std::fs::write(repo.join("a.txt"), "hello").unwrap();
        git_in(&repo, &["add", "a.txt"]);
        commit_old(&repo);

        // 210 MB 工作树文件（.git 里压缩后很小，但判据数的是工作树占用）
        let big = repo.join("big.bin");
        let chunk = vec![0u8; 1024 * 1024];
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&big).unwrap();
            for _ in 0..210 {
                f.write_all(&chunk).unwrap();
            }
        }
        git_in(&repo, &["add", "big.bin"]);
        commit_old(&repo);
        // fail-closed 之后「干净」必须是**可核实的**干净：装成推过了，否则
        // 退路计数会把两个都没推过的提交算成未推送，方向一的断言红
        sync_with_phantom_origin(&repo);

        let g = RepoScanner::new(vec![root.clone()]).scan();
        let it = || {
            g.items
                .iter()
                .find(|i| i.name == "big-stale")
                .expect("临时仓库应被扫到")
        };

        // 方向一：干净 → 算可回收，回收量 = 整个仓库目录（含 .git 与工作树）
        assert!(it().cleanable, "大且久未动且干净：该算可回收");
        let rec = it().reclaimable.expect("该给出回收量");
        assert!(rec > 200 * 1024 * 1024, "回收量该超过 200 MB，实为 {rec}");

        // 方向二：弄脏 → 一票否决，标签照旧显示（信息不丢，只是不算可回收）
        std::fs::write(repo.join("a.txt"), "dirty").unwrap();
        let g2 = RepoScanner::new(vec![root.clone()]).scan();
        let it2 = g2
            .items
            .iter()
            .find(|i| i.name == "big-stale")
            .expect("临时仓库应被扫到");
        assert!(
            !it2.cleanable && it2.reclaimable.is_none(),
            "有未提交改动：不该标可回收（S2 收紧的核心）"
        );
        assert!(
            it2.tags.iter().any(|t| t.key == "modified"),
            "改动标签照旧显示：tags={:?}",
            it2.tags
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// `artifact_targets` 反序列化：缺省 None（= 全选语义）、数组正常解析、
    /// 类型不对必须拒——范围字段打错不许悄悄退化成「清全部」。
    #[test]
    fn repos_clean_plan_artifact_targets_deserialization() {
        let base =
            r#"{"group_id":"repos","item_ids":["/tmp/x"],"confirmed":true,"scope":"artifacts""#;
        let missing: crate::scan::CleanPlan = serde_json::from_str(&format!("{base}}}")).unwrap();
        assert_eq!(missing.artifact_targets, None);

        let picked: crate::scan::CleanPlan = serde_json::from_str(&format!(
            r#"{base},"artifact_targets":["node_modules","target"]}}"#
        ))
        .unwrap();
        assert_eq!(
            picked.artifact_targets,
            Some(vec!["node_modules".to_string(), "target".to_string()])
        );

        let bogus: Result<crate::scan::CleanPlan, _> =
            serde_json::from_str(&format!(r#"{base},"artifact_targets":"node_modules"}}"#));
        assert!(bogus.is_err(), "字符串不是数组，必须拒");
    }

    /// 勾选子集的路径解析：仓库内的产物目录放行；`..`、绝对路径、空串、
    /// 不存在的目录一律 None。光有 `starts_with` 拦不住 `..`（`Path::join`
    /// 不规范化），所以分量级先挡、软链拒收、canonicalize 再兜一次。
    #[test]
    fn repos_resolve_artifact_target_rejects_escapes() {
        let root = temp_dir("art-resolve");
        let repo = root.join("repo");
        let nm = repo.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(repo.join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("outside/node_modules")).unwrap();

        let got = resolve_artifact_target(&repo, "node_modules").expect("仓库内的产物目录该放行");
        // 必须是原始拼接路径，不是 canonicalize 的结果——删除复核在仓库 id 的
        // 路径空间里做 starts_with，两边不同空间就永远对不上。
        assert_eq!(got, nm);

        for bad in [
            "",
            "   ",
            "../outside/node_modules",
            "node_modules/../../outside",
            "/etc",
            "node_modules/./x",
            "不存在的目录",
        ] {
            assert!(
                resolve_artifact_target(&repo, bad).is_none(),
                "该拒: {bad:?}"
            );
        }

        std::fs::remove_dir_all(&root).ok();
    }

    /// 软链不收：扫描侧（find_artifact_dirs）不跟随软链、账上没有它，
    /// 勾选侧口径对齐——否则用户会勾到一个「账上不存在」的目录。
    #[cfg(unix)]
    #[test]
    fn repos_resolve_artifact_target_rejects_symlinked_dir() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("art-resolve-symlink");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("package.json"), "{}").unwrap();
        let real = root.join("elsewhere/node_modules");
        std::fs::create_dir_all(&real).unwrap();
        symlink(&real, repo.join("node_modules")).unwrap();

        assert!(resolve_artifact_target(&repo, "node_modules").is_none());

        std::fs::remove_dir_all(&root).ok();
    }

    /// 路径空间一致性（trash::delete 成功路径不进测试，硬约束 4——这里钉住
    /// 删除复核的前两道判据）：resolve 的返回值必须能直接过
    /// `d.starts_with(repo)` 与 `is_artifact_dir_under(repo, d)`。此前返回
    /// canonicalize 结果，与缓存里的仓库根前缀失配——macOS temp 根是
    /// `/var/...`（/var → /private/var）、Windows canonicalize 带 `\\?\`，
    /// 复核拒掉一切勾选，整条子集清理链路失效。
    #[test]
    fn repos_resolved_artifact_target_passes_deletion_review_predicates() {
        let root = temp_dir("art-review-space");
        let repo = root.join("repo");
        let nm = repo.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(repo.join("package.json"), "{}").unwrap();

        let got = resolve_artifact_target(&repo, "node_modules").expect("仓库内的产物目录该放行");
        assert!(
            got.starts_with(&repo),
            "resolve 返回值必须与仓库根同一路径空间: {got:?} vs {repo:?}"
        );
        assert!(
            is_artifact_dir_under(&repo, &got),
            "resolve 返回值必须直接过产物复核: {got:?}"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// 勾了却解析不出路径（跑出仓库的）：整项报失败、磁盘不动，
    /// **不静默跳过**——账面与实际删掉的东西必须逐条对得上（S2）。
    #[test]
    fn repos_clean_artifacts_rejects_target_outside_repo() {
        let repo = git_repo("art-target-escape");
        let nm = repo.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("package.json"), "{}").unwrap();

        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Artifacts,
            artifact_targets: Some(vec!["../elsewhere/node_modules".into()]),
        };
        let r = scanner.clean_artifacts(&plan);
        assert!(r.succeeded.is_empty());
        assert!(
            r.failed
                .iter()
                .any(|f| text_key(&f.reason) == Some("data.cleanFail.artifactOutsideRepo")),
            "failed: {:?}",
            r.failed
        );
        assert!(nm.exists(), "被拒的目录必须原地不动");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// 勾选了仓库内但**不是产物**的目录（如 src）：照样被白名单复核拒，
    /// 且不牵连真产物目录。前端选过不算数——服务端按白名单重新判。
    #[test]
    fn repos_clean_artifacts_rejects_picked_non_artifact() {
        let repo = git_repo("art-target-nonart");
        std::fs::write(repo.join("package.json"), "{}").unwrap();
        let src = repo.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("index.ts"), "x").unwrap();
        // 仓库里真的有一个产物目录：证明「未发现」不是靠空仓库蒙过去的
        let nm = repo.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("package.json"), "{}").unwrap();

        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Artifacts,
            artifact_targets: Some(vec!["src".into()]),
        };
        let r = scanner.clean_artifacts(&plan);
        assert!(r.succeeded.is_empty());
        assert!(
            r.failed
                .iter()
                .any(|f| text_key(&f.reason) == Some("data.cleanFail.artifactRecheckFailed")),
            "failed: {:?}",
            r.failed
        );
        assert!(src.exists(), "被拒的源码目录必须原地不动");
        assert!(nm.exists(), "没勾的产物目录不该被牵连");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    /// 空勾选：当成「什么都没选中」拒，**不退化成清全部**。
    #[test]
    fn repos_clean_artifacts_empty_targets_rejected() {
        let repo = git_repo("art-target-empty");
        let nm = repo.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("package.json"), "{}").unwrap();

        let scanner = RepoScanner::new(vec![repo.parent().unwrap().to_path_buf()]);
        let plan = crate::scan::CleanPlan {
            group_id: "repos".into(),
            item_ids: vec![repo.display().to_string()],
            confirmed: true,
            scope: crate::scan::CleanScope::Artifacts,
            artifact_targets: Some(vec![]),
        };
        let r = scanner.clean_artifacts(&plan);
        assert!(r.succeeded.is_empty());
        assert!(
            text_key(&r.failed[0].reason) == Some("data.cleanFail.noArtifacts"),
            "failed: {:?}",
            r.failed
        );
        assert!(nm.exists(), "空勾选必须一个都不删");

        std::fs::remove_dir_all(repo.parent().unwrap()).ok();
    }

    // Windows 守护：`CREATE_NO_WINDOW` 挂上后命令必须照常能跑。标志位的 getter
    // 是 unstable 的、断不了值本身，退而求其次验「挂标志不破坏执行」——cfg 分支
    // 写错编译就红，标志用错这里红。非 Windows 下助手是恒等函数，没有可断言的
    // 行为，用例整体 cfg 掉（Windows CI 跑 cargo test，正好是唯一该跑的机器）。
    #[cfg(windows)]
    #[test]
    fn without_console_helper_spawns_commands_that_still_run() {
        let out = crate::scan::without_console(std::process::Command::new("cmd"))
            .args(["/C", "echo", "ok"])
            .stdin(std::process::Stdio::null())
            .output()
            .expect("cmd /C echo 应该能起进程");
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }
}

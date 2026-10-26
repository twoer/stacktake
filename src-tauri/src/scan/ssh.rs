//! SSH 密钥扫描。
//!
//! 安全约束（硬性）：
//! - **绝不把私钥内容读成数据**。但「不读内容」不等于「一个字节都不读」——
//!   本文件会读私钥的**文件头**两次：`looks_like_private_key` 读前 64 字节判断它
//!   是不是密钥；`KeyMeta.encrypted` 读前 512 字节找 `ENCRYPTED`，只用来显示
//!   「加密 / 无密码」。两次都只做子串判断，内容不落缓存、不传给前端、不出进程。
//!   （H2 勘误：这里原先写的是「私钥只用来取创建时间和权限字节」，与实现对不上。）
//! - 指纹、类型、备注全部来自 `.pub` 文件，或 `ssh-keygen -lf <私钥>`（它只读公钥部分，
//!   加了 passphrase 的 key 也能直接出指纹，不需要解密）。其余字段走 `metadata()`。
//! - 前端拿不到任何私钥字节，也拿不到私钥路径之外的东西。
//!
//! 顺带解决一个实际痛点：`~/.ssh` 里同一台机器常有两份 key（`ip_ed25519` 和 `ip_id_ed25519`），
//! 以及新建的 key 忘了加进 config。这些会被标成「未被引用」和「疑似重复」。

use crate::scan::{
    AssetGroup, DetailRow, Field, Item, MetricValue, Scanner, Tag, Text, TextArg, field_text,
    without_console,
};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

const SSH_KEYGEN_TIMEOUT: Duration = Duration::from_secs(3);

pub struct SshScanner {
    ssh_dir: PathBuf,
}

impl SshScanner {
    pub fn new(ssh_dir: PathBuf) -> Self {
        Self { ssh_dir }
    }
}

impl Scanner for SshScanner {
    fn id(&self) -> &'static str {
        "ssh"
    }

    fn scan(&self) -> AssetGroup {
        if !self.ssh_dir.exists() {
            return AssetGroup {
                id: self.id().into(),
                label: self.label(),
                summary: Text::key("data.summary.sshMissingDir"),
                fields: ssh_fields(),
                items: vec![],
                error: Some(Text::raw(self.ssh_dir.display().to_string())),
                supports_clean: false,
                notes: None,
            };
        }

        let config = parse_ssh_config(&self.ssh_dir.join("config"));
        let home = dirs::home_dir();

        let entries = match std::fs::read_dir(&self.ssh_dir) {
            Ok(e) => e,
            Err(e) => {
                return AssetGroup {
                    id: self.id().into(),
                    label: self.label(),
                    summary: Text::key("data.summary.sshReadFailed"),
                    fields: ssh_fields(),
                    items: vec![],
                    error: Some(Text::raw(e.to_string())),
                    supports_clean: false,
                    notes: None,
                };
            }
        };

        let mut items: Vec<Item> = Vec::new();
        let mut pub_names: Vec<String> = Vec::new();

        let mut files: Vec<(String, PathBuf)> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                (name, e.path())
            })
            .collect();
        files.sort();

        // 第一轮：收集 .pub，作为指纹来源
        for (name, _) in &files {
            if name.ends_with(".pub") {
                pub_names.push(name.trim_end_matches(".pub").to_string());
            }
        }

        // 第二轮：识别私钥
        for (name, path) in &files {
            if name.ends_with(".pub") {
                continue;
            }
            if is_key_ignored(name) {
                continue;
            }
            if !looks_like_private_key(path) {
                continue;
            }

            let has_pub = files.iter().any(|(n, _)| n == &format!("{}.pub", name));

            let meta = key_metadata(path, name, has_pub);
            let referenced_by = config
                .iter()
                .filter(|(_, id)| identity_matches_key(id, path, &self.ssh_dir, home.as_deref()))
                .map(|(host, _)| host.clone())
                .collect::<Vec<_>>();

            items.push(build_item(
                name.clone(),
                path.clone(),
                meta,
                referenced_by,
                has_pub,
            ));
        }

        // 重复检测：指纹相同即视为同一把 key 的副本
        let mut by_fp: HashMap<String, Vec<String>> = HashMap::new();
        for it in &items {
            if let Some(MetricValue::Text(fp)) = it.metrics.get("fingerprint") {
                by_fp.entry(fp.clone()).or_default().push(it.name.clone());
            }
        }
        let dupes: Vec<(String, Vec<String>)> =
            by_fp.into_iter().filter(|(_, v)| v.len() > 1).collect();

        // 重建 items 以带上「重复」标记
        let items: Vec<Item> = items
            .into_iter()
            .map(|mut it| {
                if let Some(MetricValue::Text(fp)) = it.metrics.get("fingerprint").cloned()
                    && let Some((_, group)) = dupes.iter().find(|(f, _)| *f == fp)
                    && group.len() > 1
                {
                    it.tags.push(Tag::raw(format!("重复×{}", group.len())));
                    it.detail
                        .push(DetailRow::new("duplicateOf", group.join(", ")));
                }
                it
            })
            .collect();

        let mut notes: Vec<Text> = Vec::new();
        for (fp, group) in &dupes {
            notes.push(
                Text::fmt("data.note.sshDuplicates")
                    .arg("fp", &fp[..fp.len().min(20)])
                    .arg("n", TextArg::num(group.len()))
                    .arg("names", group.join(", ")),
            );
        }
        // 标签键自 P2a 起是 ASCII（`Tag::new("unreferenced")`），比较也得跟着走
        let orphans = items
            .iter()
            .filter(|i| i.tags.iter().any(|t| t.key == "unreferenced"))
            .count();
        if orphans > 0 {
            notes.push(Text::fmt("data.note.sshOrphans").arg("n", TextArg::num(orphans)));
        }

        let summary = Text::fmt("data.summary.ssh")
            .arg("count", TextArg::num(items.len()))
            .arg("orphans", TextArg::num(orphans));

        AssetGroup {
            id: self.id().into(),
            label: self.label(),
            summary,
            fields: ssh_fields(),
            items,
            notes: if notes.is_empty() { None } else { Some(notes) },
            error: None,
            supports_clean: false,
        }
    }
}

fn ssh_fields() -> Vec<Field> {
    vec![
        Field {
            key: "type".into(),
            label: field_text("type"),
            numeric: None,
            temporal: None,
        },
        Field {
            key: "created".into(),
            label: field_text("created"),
            numeric: Some(true),
            temporal: Some(true),
        },
        Field {
            key: "usage".into(),
            label: field_text("usage"),
            numeric: None,
            temporal: None,
        },
    ]
}

struct KeyMeta {
    bits: Option<u32>,
    algo: String,
    fingerprint: Option<String>,
    comment: Option<String>,
    encrypted: bool,
    created: Option<SystemTime>,
    /// 权限位是 unix 语义，**字段本身随 unix 存在**。原先字段无条件声明、
    /// 读写双方各自 cfg——H8 第一轮修复（把 cfg 收到读取方外层）之后，
    /// Windows 上这个字段变成只写不读，CI 的 dead_code 门禁即红。整条链路
    /// （写入、读取、`permissions_perm`）本来就全在 unix 分支里。
    #[cfg(unix)]
    perms: Option<u32>,
}

fn key_metadata(path: &Path, _name: &str, has_pub: bool) -> KeyMeta {
    let md = std::fs::metadata(path).ok();
    let created = md.as_ref().and_then(|m| m.created().ok());
    // 权限位按平台分开写：非 unix 没有 mode 位概念，`and_then` 里混 cfg 分支
    // 会被 clippy 建议改成 `map`，但那会把「未知」变成 `Some(0)`（显示成 000）。
    #[cfg(unix)]
    let perms = md.as_ref().map(|m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o777
    });

    let mut meta = KeyMeta {
        bits: None,
        algo: "未知".into(),
        fingerprint: None,
        comment: None,
        encrypted: false,
        created,
        #[cfg(unix)]
        perms,
    };

    // 只读文件头判断是否加密——读的是前几行头部，不是密钥内容。
    // 注意：这仅用于显示「已加密 / 未加密」，不会把内容传出。
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut head = vec![0u8; 512];
        let n = f.read(&mut head).unwrap_or(0);
        let head = String::from_utf8_lossy(&head[..n]);
        meta.encrypted = head.contains("ENCRYPTED") || head.contains("Proc-Type: 4,ENCRYPTED");
    }

    // 指纹来源：优先 .pub，其次直接对私钥跑 ssh-keygen -lf（它只读公钥部分）
    let target = if has_pub {
        let mut p = path.as_os_str().to_os_string();
        p.push(".pub");
        PathBuf::from(p)
    } else {
        path.to_path_buf()
    };

    if let Some(line) = ssh_keygen_lf(&target) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            meta.bits = parts[0].parse().ok();
            meta.fingerprint = Some(parts[1].to_string());
        }
        if let Some(idx) = line.rfind('(')
            && let Some(algo) = line.get(idx + 1..).and_then(|s| s.strip_suffix(')'))
        {
            meta.algo = algo.to_string();
        }
        let comment_start = 3.min(line.split_whitespace().count());
        let tail: Vec<&str> = line.split_whitespace().skip(comment_start).collect();
        let tail_len = tail.len();
        let tail: Vec<&str> = if tail_len > 1 {
            tail[..tail_len - 1].to_vec()
        } else {
            vec![]
        };
        if !tail.is_empty() {
            meta.comment = Some(tail.join(" "));
        }
    }

    meta
}

fn ssh_keygen_lf(path: &Path) -> Option<String> {
    let start = SystemTime::now();
    let out = without_console(Command::new("ssh-keygen"))
        .arg("-lf")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if start.elapsed().ok()? > SSH_KEYGEN_TIMEOUT {
        return None;
    }
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn build_item(
    name: String,
    path: PathBuf,
    meta: KeyMeta,
    referenced_by: Vec<String>,
    has_pub: bool,
) -> Item {
    let mut tags: Vec<Tag> = Vec::new();
    if !referenced_by.is_empty() {
        tags.push(Tag::new("referenced"));
    } else {
        tags.push(Tag::new("unreferenced"));
    }
    if meta.encrypted {
        tags.push(Tag::new("encrypted"));
    } else {
        tags.push(Tag::new("noPassphrase"));
    }
    if !has_pub {
        tags.push(Tag::new("missingPub"));
    }
    // 权限过宽是安全隐患（权限位是 unix 语义，Windows 上没有）
    #[cfg(unix)]
    if let Some(p) = meta.perms
        && p & 0o077 != 0
    {
        tags.push(Tag::raw(format!("权限{:o}", p)));
    }

    let usage = if referenced_by.is_empty() {
        MetricValue::key("data.value.unreferenced")
    } else {
        MetricValue::text(referenced_by.join(", "))
    };

    let mut metrics: HashMap<String, MetricValue> = HashMap::new();
    // `KeyMeta.algo` 解析不出时留的哨兵值是内部约定，不进指标——未知走键
    let type_metric = match (meta.bits, meta.algo.as_str()) {
        (Some(b), a) if a != "未知" => MetricValue::text(format!("{} {}", a, b)),
        (_, a) if a != "未知" => MetricValue::text(a.to_string()),
        _ => MetricValue::key("data.value.unknown"),
    };
    metrics.insert("type".into(), type_metric);
    metrics.insert(
        "created".into(),
        match meta
            .created
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        {
            Some(d) => MetricValue::Int(d.as_secs() as i64),
            None => MetricValue::key("data.value.unknown"),
        },
    );
    metrics.insert("usage".into(), usage);

    let mut detail = vec![DetailRow::new("path", path.display().to_string())];
    if let Some(fp) = &meta.fingerprint {
        detail.push(DetailRow::new("fingerprint", fp.clone()));
    }
    if let Some(c) = &meta.comment {
        detail.push(DetailRow::new("comment", c.clone()));
    }
    #[cfg(unix)]
    if let Some(p) = meta.permissions_perm() {
        detail.push(DetailRow::new("permissions", format!("{:o}", p)));
    }
    detail.push(DetailRow::new(
        "pubkey",
        Text::key(if has_pub {
            "data.value.pubkeyPresent"
        } else {
            "data.value.pubkeyMissing"
        }),
    ));

    Item {
        id: path.display().to_string(),
        tags,
        name,
        label: None,
        short_label: None,
        group_label: None,
        kind_label: None,
        group_key: None,
        path: path.display().to_string(),
        metrics,
        detail,
        artifact_entries: vec![],
        cleanable: false,
        reclaimable: None,
    }
}

impl KeyMeta {
    #[cfg(unix)]
    fn permissions_perm(&self) -> Option<u32> {
        self.perms
    }
}

fn is_key_ignored(name: &str) -> bool {
    name == "config"
        || name.starts_with("known_hosts")
        || name.starts_with("authorized_keys")
        || name.starts_with("environment")
        || name.ends_with(".md")
}

fn looks_like_private_key(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut head = vec![0u8; 64];
    let n = f.read(&mut head).unwrap_or(0);
    let head = String::from_utf8_lossy(&head[..n]);
    head.starts_with("-----BEGIN") || head.starts_with("PuTTY-User-Key-File")
}

/// 解析 ~/.ssh/config，取出每个 Host 块用到的 IdentityFile。
fn parse_ssh_config(path: &Path) -> Vec<(String, String)> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return vec![];
    };
    parse_config_text(&content)
}

/// 只做够用的解析：按 Host 累积指令，遇到下一个 Host 或文件结束时结算。
fn parse_config_text(content: &str) -> Vec<(String, String)> {
    let mut result: Vec<(String, String)> = Vec::new();
    let mut hosts: Vec<String> = Vec::new();
    let mut identity: Option<String> = None;

    let flush = |hosts: &mut Vec<String>,
                 identity: &mut Option<String>,
                 result: &mut Vec<(String, String)>| {
        if let Some(id) = identity.take() {
            for h in hosts.iter() {
                result.push((h.clone(), id.clone()));
            }
        }
        hosts.clear();
    };

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let key = parts.next().unwrap_or("").to_ascii_lowercase();
        let val = parts.next().unwrap_or("").trim().to_string();

        match key.as_str() {
            "host" => {
                flush(&mut hosts, &mut identity, &mut result);
                hosts.extend(val.split_whitespace().map(|s| s.to_string()));
            }
            "identityfile" => {
                identity = Some(val);
            }
            _ => {}
        }
    }
    flush(&mut hosts, &mut identity, &mut result);
    result
}

/// config 里的 IdentityFile 是否指向 `key_path` 这把密钥——**路径级相等**才算引用。
///
/// 曾经用 `id.contains(name)` 按文件名子串判断：IdentityFile 写 `id_rsa_old`
/// 会让 `id_rsa` 误标「已引用」。密钥命名里 `_old` / `_bak` / `_2024` 是常见
/// 习惯，名字互为前后缀时子串判断必然误判——误标让用户以为 key 还在用而不敢
/// 清理，漏标让「未被引用」清单少一项，计数与清单两头失真。
fn identity_matches_key(
    identity: &str,
    key_path: &Path,
    ssh_dir: &Path,
    home: Option<&Path>,
) -> bool {
    resolve_identity_path(identity, ssh_dir, home)
        .is_some_and(|resolved| resolved == normalize_path(key_path))
}

/// 把 IdentityFile 原始值解析成规范化绝对路径。解析不了返回 `None`（不指向任何本机密钥）。
///
/// 形态按 man ssh_config 的 IdentityFile：`~` / `~/…`（当前用户 home）、
/// `~other/…`（他人 home，本机展开不了）、绝对路径原样、其余相对路径按 `.ssh`
/// 目录拼齐——与 ssh 客户端在默认配置位置下的解析一致。`%d`（home）与 `%u`
/// （本地用户名）令牌一并展开；`%h` / `%r` / `%l` 依赖连接上下文，本地展开不了，
/// 留着不匹配——这种写法罕见，且指向的本来就不是本机 `.ssh` 里的 key。
fn resolve_identity_path(raw: &str, ssh_dir: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let expanded = expand_identity(raw, home)?;
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        ssh_dir.join(expanded)
    };
    Some(normalize_path(&joined))
}

fn expand_identity(raw: &str, home: Option<&Path>) -> Option<PathBuf> {
    // 先展开令牌再展开 ~：`~/%u_key` 这类混写两种都要生效
    let s = if raw.contains('%') {
        expand_tokens(raw, home)?
    } else {
        raw.to_string()
    };
    if s == "~" {
        return home.map(Path::to_path_buf);
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return home.map(|h| h.join(rest));
    }
    if s.starts_with('~') {
        return None;
    }
    Some(PathBuf::from(s))
}

/// `%d` → home，`%u` → 本地用户名（取 home 末段，`/Users/alice` → `alice`）。
/// 其余令牌返回 `None`。没有令牌时原样返回。
fn expand_tokens(raw: &str, home: Option<&Path>) -> Option<String> {
    let user = home
        .and_then(|h| h.file_name())
        .map(|n| n.to_string_lossy().to_string());
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('d') => out.push_str(&home?.to_string_lossy()),
            Some('u') => out.push_str(user.as_deref()?),
            _ => return None,
        }
    }
    Some(out)
}

/// 词法规范化：消掉 `.` 与 `..` 段，不碰文件系统。
///
/// 不用 `canonicalize`：config 引用的路径可能根本不存在（引用了已删除的 key），
/// 且它在 macOS 上会把 `/var` 解析到 `/private/var`，与 `read_dir` 出来的路径
/// 不同源，反而制造不匹配。
fn normalize_path(p: &Path) -> PathBuf {
    let mut comps: Vec<Component> = Vec::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match comps.last() {
                // 最后一段是真实目录名才回退；根目录后的 `..` 按 POSIX 吞掉（/.. 就是 /），
                // 相对路径开头的 `..` 保留（`../../key` 不是 `/key`）
                Some(Component::Normal(_)) => {
                    comps.pop();
                }
                Some(Component::RootDir) => {}
                _ => comps.push(comp),
            },
            other => comps.push(other),
        }
    }
    let mut out = PathBuf::new();
    for c in comps {
        out.push(c.as_os_str());
    }
    out
}

// ---- 测试辅助：把内部纯函数暴露给测试模块，避免重复实现 ----

#[cfg(test)]
pub fn parse_ssh_config_for_test(content: &str) -> Vec<(String, String)> {
    parse_config_text(content)
}

#[cfg(test)]
pub fn looks_like_private_key_for_test(path: &Path) -> bool {
    looks_like_private_key(path)
}

#[cfg(test)]
pub fn identity_matches_key_for_test(
    identity: &str,
    key_path: &Path,
    ssh_dir: &Path,
    home: Option<&Path>,
) -> bool {
    identity_matches_key(identity, key_path, ssh_dir, home)
}

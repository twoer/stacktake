//! 端到端验证：扫一次写缓存，再读缓存，计时并核对数据一致。
//! 跑法：cargo run --example dump
//!
//! 这个例子同时充当人工核对数据用（输出就是前端拿到的 JSON 结构）。
//!
//! 输出的分组列表与 app 完全一致：扫描器分组 + 现算的「总览」（`with_overview`）。
//! 缓存里存的是扫描器原始快照（不含总览），与 app 同一约定。

use stacktake_lib::cache;
use stacktake_lib::{scanners, with_overview};
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let cache_dir = PathBuf::from("/tmp/stacktake-cache-probe");
    let _ = std::fs::remove_dir_all(&cache_dir);

    // 启动决策：没有缓存时才扫。这正是 app 的启动路径。
    eprintln!("启动决策       {:?}", cache::decide_boot(Some(&cache_dir)));

    // 第一次：真扫（慢）。走 `scanners()` 而不是另写一份列表，
    // 否则新增 Scanner 后这里扫出来的分组和 app 里的对不上。
    // 并行跑与 app 的 `run_full_scan` 保持一致，否则这里计时的
    // 「全量扫描耗时」和真实重垦体验是两回事。
    let t = Instant::now();
    let groups: Vec<_> = {
        use rayon::prelude::*;
        scanners().par_iter().map(|s| s.scan()).collect()
    };
    let scan_ms = t.elapsed().as_millis();

    for g in &groups {
        eprintln!("  · {:<10} {} 个条目", g.id, g.items.len());
    }

    // 缓存只存扫描器快照；总览是现算的（app 的 bootstrap / run_full_scan 同一约定）
    cache::save(&cache_dir, &groups).expect("写缓存失败");
    let cache_bytes = std::fs::metadata(cache::cache_path(&cache_dir))
        .map(|m| m.len())
        .unwrap_or(0);

    // 第二次：模拟第二次启动——这时必须有缓存，且绝不能再扫。
    let t = Instant::now();
    let boot2 = cache::decide_boot(Some(&cache_dir));
    let decide_ms = t.elapsed().as_millis();
    eprintln!("二次启动决策   {:?}  ({decide_ms} ms)", boot2);

    let t = Instant::now();
    let loaded = cache::load(&cache_dir).expect("读缓存失败");
    let load_ms = t.elapsed().as_millis();

    // 前端看到的完整列表：缓存分组 + 现算总览（行数 = 分组数）
    let shown = with_overview(loaded.groups.clone());
    eprintln!(
        "总览（现算）   {} 个条目（合成，不进缓存）",
        shown
            .iter()
            .find(|g| g.id == "overview")
            .map(|g| g.items.len())
            .unwrap_or(0)
    );

    eprintln!("首次全量扫描   {scan_ms} ms");
    eprintln!(
        "读缓存         {load_ms} ms  （约 {x} 倍快）",
        x = scan_ms / load_ms.max(1)
    );
    eprintln!("缓存文件       {:.1} KB", cache_bytes as f64 / 1024.0);
    eprintln!(
        "缓存内条目     {} 个",
        loaded.groups.iter().map(|g| g.items.len()).sum::<usize>()
    );
    // 按 id 核对，不按下标——分组顺序调整或新增分组都不该让这条对不上
    let items_of = |gs: &[stacktake_lib::scan::AssetGroup], id: &str| -> usize {
        gs.iter()
            .find(|g| g.id == id)
            .map(|g| g.items.len())
            .unwrap_or(0)
    };
    let consistent = loaded.groups.len() == groups.len()
        && groups
            .iter()
            .all(|g| items_of(&loaded.groups, &g.id) == g.items.len());
    eprintln!("数据一致       {}", if consistent { "是" } else { "否" });

    // 私钥泄漏自检：缓存内容里不应出现任何私钥头部
    let raw = std::fs::read_to_string(cache::cache_path(&cache_dir)).unwrap();
    let leaked = raw.contains("PRIVATE KEY-----");
    eprintln!(
        "私钥泄漏       {}",
        if leaked { "是（异常！）" } else { "否" }
    );

    println!("{}", serde_json::to_string_pretty(&shown).unwrap());
}

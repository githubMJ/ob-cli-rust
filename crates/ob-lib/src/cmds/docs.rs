use crate::common::*;
use crate::crud;
use crate::query;
use crate::vault;
use regex::Regex;
use std::process::exit;


pub fn run_list(ctx: &Ctx) {
    let vp = &ctx.vp;
    let flags = &ctx.flags;
    let kv = &ctx.kv;
    let num = |name: &str, default: usize| -> usize {
        flags
            .iter()
            .find_map(|f| f.strip_prefix(&format!("{name}=")).and_then(|v| v.parse().ok()))
            .unwrap_or(default)
    };
    let as_json = flags.iter().any(|x| x == "--json");
    let kw = kv.iter().find(|(k, _)| k == "keyword").map(|(_, v)| v.to_lowercase());
    let limit = num("--limit", usize::MAX);
    let page = num("--page", 0);

    let mut all: Vec<String> = Vec::new();
    for d in vault::load_docs(vp) {
        if let Some(k) = &kw {
            if !d.rel.to_lowercase().contains(k) {
                continue;
            }
        }
        all.push(d.rel);
    }
    all.sort();

    let total = all.len();
    let lim = if limit == usize::MAX { total } else { limit };
    let offset = page * lim;
    let page_items: Vec<&String> = all.iter().skip(offset).take(lim).collect();
    let has_more = offset + page_items.len() < total;

    if as_json {
        let items: Vec<String> = page_items
            .iter()
            .map(|p| format!("\"{}\"", p.replace('"', "'")))
            .collect();
        println!(
            "{{\"total\":{},\"page\":{},\"limit\":{},\"has_more\":{},\"count\":{},\"notes\":[{}]}}",
            total,
            page,
            lim,
            has_more,
            page_items.len(),
            items.join(",")
        );
    } else {
        for p in &page_items {
            println!("{p}");
        }
        if has_more {
            eprintln!(
                "[has_more] total={total}, shown={} (page {page}), next: --page={}",
                page_items.len(),
                page + 1
            );
        }
    }
}

pub fn run_print(ctx: &Ctx) {
    let vp = &ctx.vp;
    let flags = &ctx.flags;
    let pos_args = &ctx.pos_args;
    let has = |f: &str| flags.iter().any(|x| x == f);

    // 收集目标: 位置参数 (可多个) + --files 逗号/换行分隔
    let mut targets: Vec<String> = pos_args.iter().skip(1).cloned().collect();
    if let Some(list) = ctx.kv.iter().find(|(k, _)| k == "files").map(|(_, v)| v) {
        for t in list.split([',', '\n']).map(|s| s.trim()).filter(|s| !s.is_empty()) {
            targets.push(t.to_string());
        }
    }
    if targets.is_empty() {
        usage();
    }

    let db_path = ctx
        .kv
        .iter()
        .find(|(k, _)| k == "index-path")
        .map(|(_, v)| std::path::PathBuf::from(v))
        .map(|p| {
            if p.extension().map(|e| e == "db").unwrap_or(false) {
                p
            } else {
                p.join("index.db")
            }
        })
        .unwrap_or_else(|| vp.join("index.db"));
    let store = crate::store::Store::open(&db_path).ok();

    let as_json = has("--json");
    let multi = targets.len() > 1;

    // JSON 数组输出 (多篇)
    let mut json_items: Vec<String> = Vec::new();
    let mut not_found: Vec<String> = Vec::new();

    for title in &targets {
        let found = resolve_note(vp, store.as_ref(), title);
        match found {
            Some((rel, content)) => {
                if as_json {
                    json_items.push(format!(
                        "{{\"file\":\"{}\",\"content\":\"{}\"}}",
                        crate::json::json_escape(&rel),
                        crate::json::json_escape(&content)
                    ));
                } else if multi {
                    println!("════════ {rel} ════════");
                    println!("{content}");
                    println!();
                } else {
                    print!("{content}");
                }
            }
            None => {
                not_found.push(title.clone());
                if !as_json {
                    eprintln!("Note not found: {title}");
                }
            }
        }
    }

    if as_json {
        if multi {
            println!("{{\"count\":{},\"notes\":[{}]}}", json_items.len(), json_items.join(","));
        } else if let Some(item) = json_items.first() {
            println!("{item}");
        }
        for nf in &not_found {
            eprintln!("Note not found: {nf}");
        }
    }
    if !not_found.is_empty() && not_found.len() == targets.len() {
        exit(1);
    }
}

/// 批量读取笔记全文 (供 search --read 与 print 共用)
pub fn read_notes(
    store: &crate::store::Store,
    vp: &std::path::Path,
    targets: &[String],
    as_json: bool,
    max_chars: Option<usize>,
) -> Result<String, String> {
    let multi = targets.len() > 1;
    let mut out = String::new();
    let mut items: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for t in targets {
        match resolve_note(vp, Some(store), t) {
            Some((rel, mut content)) => {
                // 内容截断 (默认 8000 字符, 约 4K tokens, 防止撑爆上下文)
                let cap = max_chars.unwrap_or(8000);
                let total = content.chars().count();
                if total > cap {
                    content = content.chars().take(cap).collect::<String>();
                    content.push_str(&format!(
                        "\n\n...[已截断, 原文 {total} 字符, 仅显示前 {cap}]"
                    ));
                }
                if as_json {
                    items.push(format!(
                        "{{\"file\":\"{}\",\"content\":\"{}\",\"truncated\":{}}}",
                        crate::json::json_escape(&rel),
                        crate::json::json_escape(&content),
                        total > cap
                    ));
                } else if multi {
                    out.push_str(&format!("════════ {rel} ════════\n{content}\n\n"));
                } else {
                    out.push_str(&content);
                }
            }
            None => missing.push(t.clone()),
        }
    }

    if as_json {
        if multi {
            out.push_str(&format!(
                "{{\"count\":{},\"notes\":[{}]}}",
                items.len(),
                items.join(",")
            ));
        } else if let Some(i) = items.first() {
            out.push_str(i);
        }
    }
    if !missing.is_empty() {
        return Err(format!("not found: {}", missing.join(", ")));
    }
    Ok(out)
}

/// 解析笔记: 先按路径找文件, 失败则用索引兜底
fn resolve_note(vp: &std::path::Path, store: Option<&crate::store::Store>, title: &str) -> Option<(String, String)> {
    if let Some(p) = crate::common::find_note(&vp.to_path_buf(), title) {
        let rel = p
            .strip_prefix(vp)
            .map(|r| r.to_string_lossy().to_string())
            .unwrap_or_else(|_| title.to_string());
        if let Ok(c) = std::fs::read_to_string(&p) {
            return Some((rel, c));
        }
    }
    // 索引兜底
    let st = store?;
    let base = title.trim_end_matches(".md").rsplit('/').next().unwrap_or(title).to_lowercase();
    st.content_of(&base).map(|(path, _t, c)| (path, c))
}

pub fn run_create(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            // 内容来源优先级: --file 本地文件 > --content 内联文本
            let content = if let Some(f) = kv.iter().find(|(k, _)| k == "file").map(|(_, v)| v.clone()) {
                std::fs::read_to_string(&f).unwrap_or_else(|e| {
                    eprintln!("Error reading file {f}: {e}");
                    std::process::exit(1);
                })
            } else {
                kv.iter()
                    .find(|(k, _)| k == "content")
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
                    .replace("\\n", "\n")
            };
            let mut path = vp.join(title);
            path.set_extension("md");
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, content).unwrap();
            println!("Created: {}", path.display());
}

pub fn run_move(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
            let (from, to) = (pos_args.get(1).unwrap_or_else(|| usage()), pos_args.get(2).unwrap_or_else(|| usage()));
            crud::mv(&vp, from, to);
}

pub fn run_delete(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            crud::delete(&vp, title, has("--force"));
}

pub fn run_frontmatter(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            let action = pos_args.get(2).map(|s| s.as_str()).unwrap_or("get");
            let key = pos_args.get(3).map(|s| s.as_str()).unwrap_or("");
            let value = pos_args.get(4).cloned();
            crud::frontmatter(&vp, title, action, key, value);
}

pub fn run_hdaily(ctx: &Ctx) {
    let vp = &ctx.vp;
    let dir = ctx.pos_args.get(1).map(|s| s.as_str()).unwrap_or("05-Daily");
    crate::crud::hdaily(&vp, dir, None);
}

/// index: 构建/重建 SQLite 全文索引 (单文件 .db, 跨平台可拷贝)
/// 用法: ob-cli index --path <vault> [--index-path <dir>]
pub fn run_index(ctx: &Ctx) {
    // --incremental / --incr: 增量模式 (只更新变化的文件, ~100ms)
    // 默认: 全量重建 (~3.4s)
    let incremental = ctx.flags.iter().any(|f| f == "--incremental" || f == "--incr");
    if incremental {
        return run_sync_impl(ctx);
    }

    // 索引路径: 默认 <vault>/index.db, 可用 --index-path 指定目录或文件
    let idx_arg = ctx
        .kv
        .iter()
        .find(|(k, _)| k == "index-path")
        .map(|(_, v)| std::path::PathBuf::from(v));
    let db_path = match idx_arg {
        Some(p) => {
            // 目录 → 目录/index.db; 文件 → 直接用
            if p.extension().map(|e| e == "db").unwrap_or(false) {
                p
            } else {
                p.join("index.db")
            }
        }
        None => ctx.vp.join("index.db"),
    };

    let t0 = std::time::Instant::now();
    let docs = crate::vault::load_docs(&ctx.vp);
    let load_ms = t0.elapsed().as_millis();

    let t1 = std::time::Instant::now();
    let mut store = match crate::store::Store::open(&db_path) {
        Ok(s) => s,
        Err(e) => {
            println!("{{\"status\":\"error\",\"error\":\"{}\"}}", e);
            std::process::exit(1);
        }
    };
    if let Err(e) = store.rebuild(&docs) {
        println!("{{\"status\":\"error\",\"error\":\"{}\"}}", e);
        std::process::exit(1);
    }
    let build_ms = t1.elapsed().as_millis();

    let (notes, links, aliases) = store.stats().unwrap_or((0, 0, 0));
    let size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    println!(
        "{{\"status\":\"ok\",\"notes\":{},\"links\":{},\"aliases\":{},\"db\":\"{}\",\"size_mb\":{:.1},\"load_ms\":{},\"build_ms\":{}}}",
        notes,
        links,
        aliases,
        db_path.display(),
        size as f64 / 1048576.0,
        load_ms,
        build_ms
    );
}

/// sync: 增量同步 (MCP 下载新 zip 解压后调用)
/// 对比笔记目录与索引, 只更新变化的文件 (新增/修改/删除)
/// 用法: ob-cli sync --path <vault> --index-path <dir>
pub fn run_sync(ctx: &Ctx) {
    run_sync_impl(ctx)
}

fn run_sync_impl(ctx: &Ctx) {
    let idx_arg = ctx
        .kv
        .iter()
        .find(|(k, _)| k == "index-path")
        .map(|(_, v)| std::path::PathBuf::from(v));
    let db_path = match idx_arg {
        Some(p) => {
            if p.extension().map(|e| e == "db").unwrap_or(false) {
                p
            } else {
                p.join("index.db")
            }
        }
        None => ctx.vp.join("index.db"),
    };

    let t0 = std::time::Instant::now();
    let mut store = match crate::store::Store::open(&db_path) {
        Ok(s) => s,
        Err(e) => {
            println!("{{\"status\":\"error\",\"error\":\"{}\"}}", e);
            std::process::exit(1);
        }
    };

    // 1. 索引侧现状: path -> mtime
    let indexed = store.indexed_paths().unwrap_or_default();

    // 2. 磁盘侧现状: 遍历所有 .md, 读 mtime (不读内容, 很快)
    let files = crate::vault::collect_md_paths(&ctx.vp);
    let mut disk: std::collections::HashMap<String, (std::path::PathBuf, i64)> =
        std::collections::HashMap::new();
    for f in &files {
        if let Ok(rel) = f.strip_prefix(&ctx.vp) {
            let rel = rel.to_string_lossy().to_string();
            let mtime = std::fs::metadata(f)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            disk.insert(rel, (f.clone(), mtime));
        }
    }

    // 3. 差异计算
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut removed = 0usize;

    // 新增 + 修改
    for (rel, (path, mtime)) in &disk {
        let need_update = match indexed.get(rel) {
            Some(old_mtime) => *old_mtime != *mtime,   // mtime 变了 → 更新
            None => true,                              // 索引里没有 → 新增
        };
        if !need_update {
            continue;
        }
        // 只读变化的文件
        match crate::vault::load_one(&ctx.vp, path) {
            Some(doc) => {
                let is_new = !indexed.contains_key(rel);
                match store.upsert(&doc) {
                    Ok(_) => {
                        if is_new { added += 1; } else { updated += 1; }
                    }
                    Err(e) => eprintln!("[sync] upsert failed for {rel}: {e}"),
                }
            }
            None => eprintln!("[sync] load failed for {rel}"),
        }
    }

    // 删除: 索引里有但磁盘没了
    for rel in indexed.keys() {
        if !disk.contains_key(rel) {
            if store.delete(rel).is_ok() {
                removed += 1;
            }
        }
    }

    let (notes, links, aliases) = store.stats().unwrap_or((0, 0, 0));
    println!(
        "{{\"status\":\"ok\",\"added\":{},\"updated\":{},\"removed\":{},\"total_notes\":{},\"total_links\":{},\"total_aliases\":{},\"cost_ms\":{}}}",
        added,
        updated,
        removed,
        notes,
        links,
        aliases,
        t0.elapsed().as_millis()
    );
}

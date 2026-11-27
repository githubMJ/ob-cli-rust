// drill.rs — 多轮下钻导航
//
// AI 分析流程:
//   1. ob-cli search "问题" --discover        → 找入口笔记
//   2. ob-cli drill <note>                    → 看该笔记内容 + 可选下钻方向
//   3. 选择 can_drill 中的一项, 再次 drill     → 逐层深入
//   4. 需要回退时, 用 breadcrumb 中的路径重新 drill
//
// 输出设计:
//   - current: 当前节点 (标题/路径/内容摘要)
//   - breadcrumb: 从哪里来的 (AI 据此回退)
//   - can_drill.down: 本笔记引用了谁 (下钻看细节)
//   - can_drill.up:   谁引用了本笔记 (上溯看汇总)
//   每条候选带 why (引用上下文) + excerpt (内容摘要), AI 无需读全文即可判断
use crate::common::*;
use crate::json::json_escape;
use crate::store::{DrillItem, Store};
use std::path::PathBuf;
use std::process::exit;

pub fn run_drill(ctx: &Ctx) {
    let flags = &ctx.flags;
    let pos_args = &ctx.pos_args;
    let has = |f: &str| flags.iter().any(|x| x == f);
    let num = |name: &str, default: usize| -> usize {
        flags
            .iter()
            .find_map(|f| f.strip_prefix(&format!("{name}=")).and_then(|v| v.parse().ok()))
            .unwrap_or(default)
    };

    let target = pos_args.get(1).unwrap_or_else(|| {
        eprintln!("Usage: ob-cli drill <note-path-or-name> [--from <prev>] [--limit N]");
        exit(2)
    });

    let limit = num("--limit", 5);
    let from = flags.iter().find_map(|f| f.strip_prefix("--from="));

    let db_path = resolve_db(ctx);
    if !db_path.exists() {
        eprintln!("Error: index not found at {}", db_path.display());
        exit(3);
    }
    let store = match Store::open(&db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: cannot open index: {e}");
            exit(3);
        }
    };

    let depth = flags
        .iter()
        .find_map(|f| f.strip_prefix("--depth=").and_then(|v| v.parse().ok()))
        .unwrap_or(1);
    match drill_one(&store, target, from, limit, depth, has("--json")) {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("Error: {e}");
            exit(4);
        }
    }
}

/// 解析索引 db 路径
pub fn resolve_db(ctx: &Ctx) -> PathBuf {
    ctx.flags
        .iter()
        .find_map(|f| f.strip_prefix("--index-path=").map(PathBuf::from))
        .map(|p| {
            if p.extension().map(|e| e == "db").unwrap_or(false) {
                p
            } else {
                p.join("index.db")
            }
        })
        .unwrap_or_else(|| ctx.vp.join("index.db"))
}

/// 核心: 对单篇笔记做"下钻检索" (供 search --note 与 drill 共用)
pub fn drill_one(
    store: &Store,
    target: &str,
    from: Option<&str>,
    limit: usize,
    depth: usize,
    as_json: bool,
) -> Result<String, String> {
    // 解析 target: 支持完整 path / 部分 path / base 名
    let resolved = if target.ends_with(".md") {
        store
            .content_by_path(target)
            .or_else(|| store.find_by_path_suffix(target.trim_start_matches('/')))
            .or_else(|| {
                let base = target.trim_end_matches(".md").rsplit('/').next().unwrap_or("");
                store.content_of(&base.to_lowercase())
            })
    } else {
        let base = target
            .trim_end_matches(".md")
            .rsplit('/')
            .next()
            .unwrap_or(target)
            .to_lowercase();
        store.content_of(&base)
    };

    let (path, title, content) = resolved.ok_or_else(|| format!("note not found: {target}"))?;
    let base = path
        .trim_end_matches(".md")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();
    // 多级下钻 (depth >= 2): 输出树形路径
    if depth >= 2 {
        let tree = store
            .drill_tree(&base, depth, limit)
            .unwrap_or_default();
        let mut out = String::new();
        if as_json {
            out.push_str("{\n");
            out.push_str(&format!("  \"current\": {{\"title\": \"{}\", \"path\": \"{}\"}},\n",
                json_escape(&title), json_escape(&path)));
            out.push_str(&format!("  \"depth\": {},\n", depth));
            out.push_str("  \"tree\": [\n");
            let items: Vec<String> = tree.iter().map(|(d, p, it)| {
                format!(
                    "    {{\"depth\": {}, \"path\": \"{}\", \"title\": \"{}\", \"file\": \"{}\", \"why\": \"{}\"}}",
                    d,
                    json_escape(&p.join(" > ")),
                    json_escape(&it.title),
                    json_escape(&it.path),
                    json_escape(&it.why)
                )
            }).collect();
            out.push_str(&items.join(",\n"));
            out.push_str("\n  ]\n}");
        } else {
            out.push_str(&format!("📍 {} ({})\n", title, path));
            out.push_str(&format!("   多级下钻 depth={}, 共 {} 个节点:\n\n", depth, tree.len()));
            for (d, p, it) in tree.iter() {
                let indent = "  ".repeat(*d);
                out.push_str(&format!("{}{} {}\n", indent, if d == &1 { "▸" } else { "·" }, it.title));
                out.push_str(&format!("{}   路径: {}\n", indent, p.join(" > ")));
                out.push_str(&format!("{}   file: {}\n", indent, it.path));
                if !it.why.is_empty() {
                    let why = if it.why.chars().count() > 60 {
                        format!("{}...", it.why.chars().take(60).collect::<String>())
                    } else {
                        it.why.clone()
                    };
                    out.push_str(&format!("{}   原因: {}\n", indent, why));
                }
                out.push_str(&format!("{}   查看: ob-cli print \"{}\" --path <vault>\n", indent, it.path));
            }
        }
        return Ok(out);
    }

    let (down, up) = store
        .drill_options(&base, limit)
        .unwrap_or((Vec::new(), Vec::new()));

    let mut out = String::new();
    if as_json {
        out.push_str("{\n");
        out.push_str("  \"current\": {\n");
        out.push_str(&format!("    \"title\": \"{}\",\n", json_escape(&title)));
        out.push_str(&format!("    \"path\": \"{}\",\n", json_escape(&path)));
        out.push_str(&format!("    \"base\": \"{}\",\n", json_escape(&base)));
        out.push_str(&format!("    \"excerpt\": \"{}\"\n", json_escape(&excerpt_of(&content))));
        out.push_str("  },\n");
        if let Some(f) = from {
            out.push_str(&format!(
                "  \"breadcrumb\": [{{\"title\": \"{}\", \"type\": \"prev\"}}, {{\"title\": \"{}\", \"type\": \"current\"}}],\n",
                json_escape(f),
                json_escape(&title)
            ));
        } else {
            out.push_str(&format!(
                "  \"breadcrumb\": [{{\"title\": \"{}\", \"type\": \"current\"}}],\n",
                json_escape(&title)
            ));
        }
        out.push_str("  \"can_drill\": {\n    \"down\": [\n");
        out.push_str(&items_json(&down, 4));
        out.push_str("\n    ],\n    \"up\": [\n");
        out.push_str(&items_json(&up, 4));
        out.push_str("\n    ]\n  },\n");
        out.push_str(&format!(
            "  \"can_go_back\": {}\n}}",
            if from.is_some() { "true" } else { "false" }
        ));
    } else {
        out.push_str(&format!("📍 {}\n   path: {}\n\n   {}\n", title, path, excerpt_of(&content)));
        if let Some(f) = from {
            out.push_str(&format!("\n⬅  from: {f}\n"));
        }
        if !down.is_empty() {
            out.push_str("\n🔽 下钻 (本笔记引用了):\n");
            for (i, d) in down.iter().enumerate() {
                out.push_str(&format!(
                    "   [{}] {}\n       file: {}\n       原因: {}\n       摘要: {}\n       读取: ob-cli print \"{}\" --path <vault>\n",
                    i + 1,
                    d.title,
                    d.path,
                    d.why,
                    d.excerpt.chars().take(100).collect::<String>(),
                    d.path
                ));
            }
        }
        if !up.is_empty() {
            out.push_str("\n🔼 上溯 (引用了本笔记):\n");
            for (i, u) in up.iter().enumerate() {
                out.push_str(&format!(
                    "   [{}] {}\n       file: {}\n       原因: {}\n       读取: ob-cli print \"{}\" --path <vault>\n",
                    i + 1,
                    u.title,
                    u.path,
                    u.why,
                    u.path
                ));
            }
        }
        if down.is_empty() && up.is_empty() {
            out.push_str("\n(无关联笔记)\n");
        }
        out.push_str(&format!("\n提示: 下钻更深 → ob-cli search --note \"{}\" --depth 2\n", path));
        out.push_str(&format!("      读全文 → ob-cli print \"{}\" --path <vault>\n", path));
    }
    Ok(out)
}

fn items_json(items: &[DrillItem], indent: usize) -> String {
    let pad = " ".repeat(indent);
    items
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let comma = if i + 1 < items.len() { "," } else { "" };
            format!(
                "{}{{\"title\": \"{}\", \"path\": \"{}\", \"file\": \"{}\", \"why\": \"{}\", \"excerpt\": \"{}\"}}{}",
                pad,
                json_escape(&d.title),
                json_escape(&d.path),
                json_escape(&d.path),
                json_escape(&d.why),
                json_escape(&d.excerpt.chars().take(200).collect::<String>()),
                comma
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn excerpt_of(content: &str) -> String {
    let t = content.trim_start();
    let body = if t.starts_with("---") {
        match t[3..].find("\n---") {
            Some(e) => &t[3 + e + 4..],
            None => content,
        }
    } else {
        content
    };
    let text: String = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() > 300 {
        format!("{}...", text.chars().take(300).collect::<String>())
    } else {
        text
    }
}



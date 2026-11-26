// main.rs — ob-cli-rust: CLI + MCP server for Obsidian vaults
// 模块: common(共享) / vault(文档层) / query(检索语法) / crud(操作实现)
//       json(JSON) / mcp(MCP server) / commands/*(命令层)
mod commands;
mod common;
mod crud;
mod json;
mod mcp;
mod query;
mod vault;

use common::{usage, vault_path, Ctx};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut vault_flag = None;
    let mut pos_args = Vec::new();
    let mut flags: Vec<String> = Vec::new();
    let mut kv: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--path" {
            vault_flag = args.get(i + 1).cloned();
            i += 2;
            continue;
        } else if let Some(v) = a.strip_prefix("--content") {
            if let Some(val) = v.strip_prefix('=') {
                kv.push(("content".into(), val.to_string()));
            } else {
                kv.push(("content".into(), args.get(i + 1).cloned().unwrap_or_default()));
                i += 1;
            }
        } else if a == "--max" || a == "--context" || a == "--depth" {
            let val = args.get(i + 1).cloned().unwrap_or_default();
            flags.push(format!("{}={}", a, val));
            i += 1;
        } else if a.starts_with("--") {
            flags.push(a.to_string());
        } else {
            pos_args.push(a.clone());
        }
        i += 1;
    }
    let vp = vault_path(vault_flag);
    let cmd = pos_args.first().map(|s| s.as_str()).unwrap_or_else(|| usage()).to_string();
    let ctx = Ctx { vp, flags, pos_args, kv };

    match cmd.as_str() {
        "search" => commands::search::run_search(&ctx),
        "backlinks" => commands::links::run_backlinks(&ctx),
        "links" => commands::links::run_links(&ctx),
        "graph" => commands::graph::run_graph(&ctx),
        "hybrid" => commands::hybrid::run_hybrid(&ctx),
        "list" => commands::docs::run_list(&ctx),
        "print" => commands::docs::run_print(&ctx),
        "create" => commands::docs::run_create(&ctx),
        "move" => commands::docs::run_move(&ctx),
        "delete" => commands::docs::run_delete(&ctx),
        "frontmatter" => commands::docs::run_frontmatter(&ctx),
        "hdaily" => commands::docs::run_hdaily(&ctx),
        "tags" => commands::stats::run_tags(&ctx),
        "orphans" => commands::stats::run_orphans(&ctx),
        "unresolved" => commands::stats::run_unresolved(&ctx),
        "serve" => mcp::run(&ctx.vp),
        _ => usage(),
    }
}

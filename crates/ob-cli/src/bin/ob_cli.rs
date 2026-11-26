// ob-cli — 总入口: 全命令路由 + MCP server
#[path = "../mcp.rs"]
mod mcp;
use ob_lib::cmds;
use ob_lib::common;

fn usage() -> ! {
    eprintln!(
        "ob-cli — Obsidian vault 总入口 | Unified entry + MCP server

USAGE:
  ob-cli <command> ...          # 覆盖 ob-search / ob-core 全部命令, 自动路由
  ob-cli serve                  [--path <vault>]    # MCP stdio server

MODULES:
  ob-search   检索类 (search/backlinks/links/hybrid/graph)
  ob-ops      操作类 (create/print/list/move/delete/frontmatter/tags/...)"
    );
    std::process::exit(1)
}

fn main() {
    let (cmd, ctx) = ob_lib::parse_cli();
    match cmd.as_str() {
        "search" => cmds::search::run_search(&ctx),
        "backlinks" => cmds::links::run_backlinks(&ctx),
        "links" => cmds::links::run_links(&ctx),
        "hybrid" => cmds::hybrid::run_hybrid(&ctx),
        "graph" => cmds::graph::run_graph(&ctx),
        "create" => cmds::docs::run_create(&ctx),
        "print" => cmds::docs::run_print(&ctx),
        "list" => cmds::docs::run_list(&ctx),
        "move" => cmds::docs::run_move(&ctx),
        "delete" => cmds::docs::run_delete(&ctx),
        "frontmatter" => cmds::docs::run_frontmatter(&ctx),
        "tags" => cmds::stats::run_tags(&ctx),
        "orphans" => cmds::stats::run_orphans(&ctx),
        "unresolved" => cmds::stats::run_unresolved(&ctx),
        "hdaily" => cmds::docs::run_hdaily(&ctx),
        "serve" => mcp::run(&ctx.vp),
        _ => usage(),
    }
}

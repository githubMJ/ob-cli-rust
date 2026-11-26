// ob-cli — 总入口: 转发到 ob-search / ob-ops, 或直接运行 MCP server
use notesmd_rs::common;

fn usage() -> ! {
    eprintln!(
        "ob-cli — Obsidian vault 总入口

USAGE:
  ob-cli <command> ...          # 同 ob-search / ob-ops 的所有命令, 自动路由
  ob-cli serve                  [--path <vault>]    # MCP stdio server

MODULES:
  ob-search   检索类 (search/backlinks/links/hybrid/graph)
  ob-ops      操作类 (create/print/list/move/delete/frontmatter/tags/...)"
    );
    std::process::exit(1)
}

fn main() {
    let (cmd, ctx) = notesmd_rs::parse_cli();
    match cmd.as_str() {
        // 检索类 → 内置路由
        "search" => notesmd_rs::cmds::search::run_search(&ctx),
        "backlinks" => notesmd_rs::cmds::links::run_backlinks(&ctx),
        "links" => notesmd_rs::cmds::links::run_links(&ctx),
        "hybrid" => notesmd_rs::cmds::hybrid::run_hybrid(&ctx),
        "graph" => notesmd_rs::cmds::graph::run_graph(&ctx),
        // 操作类 → 内置路由
        "create" => notesmd_rs::cmds::docs::run_create(&ctx),
        "print" => notesmd_rs::cmds::docs::run_print(&ctx),
        "list" => notesmd_rs::cmds::docs::run_list(&ctx),
        "move" => notesmd_rs::cmds::docs::run_move(&ctx),
        "delete" => notesmd_rs::cmds::docs::run_delete(&ctx),
        "frontmatter" => notesmd_rs::cmds::docs::run_frontmatter(&ctx),
        "tags" => notesmd_rs::cmds::stats::run_tags(&ctx),
        "orphans" => notesmd_rs::cmds::stats::run_orphans(&ctx),
        "unresolved" => notesmd_rs::cmds::stats::run_unresolved(&ctx),
        "hdaily" => notesmd_rs::cmds::docs::run_hdaily(&ctx),
        // MCP
        "serve" => notesmd_rs::mcp::run(&ctx.vp),
        _ => usage(),
    }
}

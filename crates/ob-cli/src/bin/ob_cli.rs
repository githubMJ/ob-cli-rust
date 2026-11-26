// ob-cli — 总入口: 全命令路由
use ob_lib::cmds;
use ob_lib::common;

use ob_lib::common::usage;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        ob_lib::common::help();
    }
    if argv.iter().any(|a| a == "--version" || a == "-V") {
        println!("ob-cli {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
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
        _ => usage(),
    }
}

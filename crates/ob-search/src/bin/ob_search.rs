// ob-search — 检索模块 | Retrieval module
use ob_lib::cmds;
use ob_lib::common;

fn usage() -> ! {
    eprintln!(
        "ob-search — 检索模块 | Retrieval module

USAGE:
  ob-search search <query>   [--path <vault>] [--lines] [--paragraph] [--context N] [--fuzzy] [--mode exact|fuzzy|regex] [--json] [--max N]
  ob-search backlinks <note> [--path <vault>] [--json]
  ob-search links <note>     [--path <vault>]
  ob-search hybrid <query>   [--path <vault>] [--limit N]
  ob-search graph <note>     [--path <vault>] [--depth N]"
    );
    std::process::exit(1)
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        ob_lib::common::help();
    }
    if argv.iter().any(|a| a == "--version" || a == "-V") {
        println!("ob-search {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    let (cmd, ctx) = ob_lib::parse_cli();
    match cmd.as_str() {
        "search" => cmds::search::run_search(&ctx),
        "backlinks" => cmds::links::run_backlinks(&ctx),
        "links" => cmds::links::run_links(&ctx),
        "hybrid" => cmds::hybrid::run_hybrid(&ctx),
        "graph" => cmds::graph::run_graph(&ctx),
        _ => usage(),
    }
}

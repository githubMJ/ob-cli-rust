// ob-search — 检索模块: 语法检索/模糊/双链/图/混合
use notesmd_rs::cmds;
use notesmd_rs::common;

fn usage() -> ! {
    eprintln!(
        "ob-search — 检索模块

USAGE:
  ob-search search <query>   [--path <vault>] [--links] [--lines] [--paragraph] [--context N] [--fuzzy] [--json] [--max N]
  ob-search backlinks <note> [--path <vault>] [--json]
  ob-search links <note>     [--path <vault>]
  ob-search hybrid <query>   [--path <vault>] [--max N]
  ob-search graph <note>     [--path <vault>] [--depth N]"
    );
    std::process::exit(1)
}

fn main() {
    let (cmd, ctx) = notesmd_rs::parse_cli();
    match cmd.as_str() {
        "search" => cmds::search::run_search(&ctx),
        "backlinks" => cmds::links::run_backlinks(&ctx),
        "links" => cmds::links::run_links(&ctx),
        "hybrid" => cmds::hybrid::run_hybrid(&ctx),
        "graph" => cmds::graph::run_graph(&ctx),
        _ => usage(),
    }
}

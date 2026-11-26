// ob-core — 核心操作模块 | Core operations
use ob_lib::cmds;
use ob_lib::common;

fn usage() -> ! {
    eprintln!(
        "ob-core — 核心操作模块 | Core operations

USAGE:
  ob-core create <title> [--content <text>] [--file <file>] [--path <vault>]
  ob-core print <note>         [--path <vault>]
  ob-core list                 [--path <vault>]
  ob-core move <from> <to>     [--path <vault>]    # rename + update all links
  ob-core delete <note> [--force] [--path <vault>]
  ob-core frontmatter <note> get|[set key val]|[remove key] [--path <vault>]
  ob-core tags [tag]           [--path <vault>]
  ob-core orphans              [--path <vault>]
  ob-core unresolved           [--path <vault>]
  ob-core hdaily [dir]         [--path <vault>]"
    );
    std::process::exit(1)
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        ob_lib::common::help();
    }
    if argv.iter().any(|a| a == "--version" || a == "-V") {
        println!("ob-core {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    let (cmd, ctx) = ob_lib::parse_cli();
    match cmd.as_str() {
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

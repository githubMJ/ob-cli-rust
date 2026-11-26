// ob-core — 核心操作模块 | Core operations module | Operations module
use ob_lib::cmds;
use ob_lib::common;

fn usage() -> ! {
    eprintln!(
        "ob-core — 核心操作模块 | Core operations module | Operations module

USAGE:
  ob-ops create <title> [--content <text>] [--path <vault>]
  ob-ops print <note>         [--path <vault>]
  ob-ops list                 [--path <vault>]
  ob-ops move <from> <to>     [--path <vault>]    # rename + update all links
  ob-ops delete <note> [--force] [--path <vault>]
  ob-ops frontmatter <note> get|[set key val]|[remove key] [--path <vault>]
  ob-ops tags [tag]           [--path <vault>]
  ob-ops orphans              [--path <vault>]
  ob-ops unresolved           [--path <vault>]
  ob-ops hdaily [dir]         [--path <vault>]"
    );
    std::process::exit(1)
}

fn main() {
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

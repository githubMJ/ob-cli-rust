// ob-ops — 操作模块: 笔记/目录增删改 + 标签/健康统计
use notesmd_rs::cmds;
use notesmd_rs::common;

fn usage() -> ! {
    eprintln!(
        "ob-ops — 操作模块

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
    let (cmd, ctx) = notesmd_rs::parse_cli();
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

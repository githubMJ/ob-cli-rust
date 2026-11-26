// common.rs — 共享工具: 上下文/路径解析/笔记查找
use std::path::PathBuf;

pub fn help_text() -> String {
    format!(
        "ob-cli-rust — Obsidian vault CLI + MCP server (Rust)

BINARIES:
  ob-search   检索模块 | Retrieval: search/backlinks/links/hybrid/graph
  ob-core     核心操作模块 | Core operations: create/print/list/move/delete/frontmatter/tags/orphans/unresolved/hdaily
  ob-cli      总入口 + MCP server | Unified entry + MCP server (serve)

USAGE:
  ob-cli search <query>        [--path <vault>] [--links] [--lines] [--paragraph] [--context N] [--fuzzy] [--json] [--max N]
  ob-cli backlinks <note>      [--path <vault>] [--json]
  ob-cli links <note>          [--path <vault>]
  ob-cli hybrid <query>        [--path <vault>] [--max N]
  ob-cli graph <note>          [--path <vault>] [--depth N]
  ob-cli list                  [--path <vault>]
  ob-cli print <note>          [--path <vault>]
  ob-cli create <title> [--content <text>] [--path <vault>]
  ob-cli move <from> <to>      [--path <vault>]
  ob-cli delete <note> [--force] [--path <vault>]
  ob-cli frontmatter <note> get|[set key val]|[remove key] [--path <vault>]
  ob-cli tags [tag]            [--path <vault>]
  ob-cli orphans               [--path <vault>]
  ob-cli unresolved            [--path <vault>]
  ob-cli hdaily [dir]          [--path <vault>]
  ob-cli serve                 [--path <vault>]    # MCP stdio server

QUERY SYNTAX (Obsidian compatible):
  word1 word2 = AND | OR = or | -word = exclude | \"phrase\" = exact
  path:dir file:name tag:#tag line:(a b)"
    )
}

/// 打印帮助后退出 (0)
pub fn help() -> ! {
    println!("{}", help_text());
    std::process::exit(0)
}

/// 用法错误退出 (1)
pub fn usage() -> ! {
    eprintln!("{}", help_text());
    std::process::exit(1)
}

pub fn vault_path(flag: Option<String>) -> PathBuf {
    let p = flag.unwrap_or_else(|| "/data/llm_wiki".to_string());
    PathBuf::from(p)
}

pub fn find_note(vault: &PathBuf, title: &str) -> Option<PathBuf> {
    let t = title.trim_end_matches(".md");
    let direct = vault.join(format!("{t}.md"));
    if direct.exists() {
        return Some(direct);
    }
    let want = t.rsplit('/').next().unwrap_or(t);
    let mut found = None;
    fn walk(dir: &PathBuf, want: &str, found: &mut Option<PathBuf>) {
        if found.is_some() {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path();
                if p.is_dir() {
                    if !p.file_name().unwrap().to_string_lossy().starts_with('.') {
                        walk(&p, want, found);
                    }
                } else if p
                    .file_name()
                    .map(|n| n.to_string_lossy().trim_end_matches(".md") == want)
                    .unwrap_or(false)
                {
                    *found = Some(p);
                    return;
                }
            }
        }
    }
    walk(vault, want, &mut found);
    found
}

/// 命令执行上下文: 解析后的参数
pub struct Ctx {
    pub vp: PathBuf,
    pub flags: Vec<String>,
    pub pos_args: Vec<String>,
    pub kv: Vec<(String, String)>,
}

pub fn d_lines(d: &crate::vault::Doc) -> Vec<&str> {
    if d.lines.is_empty() {
        d.content.split('\n').collect()
    } else {
        d.lines.iter().map(|s| s.as_str()).collect()
    }
}

// common.rs — 共享工具: 上下文/路径解析/笔记查找
use std::path::PathBuf;

pub fn help_text() -> String {
    format!(
        "ob-cli-rust — Obsidian vault CLI (Rust) | 知识库检索与管理

WORKSPACE:
  所有命令必须通过 --path 指定 vault 目录 (或环境变量 OBSIDIAN_VAULT), 检索与操作严格限制在该目录内。
  All commands require --path <vault-dir> (or env OBSIDIAN_VAULT). Access is confined to that directory.

BINARIES:
  ob-search   检索模块 | Retrieval: search/backlinks/links/hybrid/graph
  ob-core     核心操作模块 | Core operations: create/print/list/move/delete/frontmatter/tags/...
  ob-cli      总入口 | Unified entry: all commands above + automatic routing

COMMANDS:
  search <query>        全文检索 (默认附带每条命中的双向链接邻居)
                        Full-text search; every hit includes its bidirectional link neighbors.
  backlinks <note>      反向链接 | Notes that reference the given note.
  links <note>          正向链接 | Wiki links inside the given note.
  hybrid <query>        混合检索 | Keyword hits + 1-hop link expansion.
  graph <note>          图检索 | n-hop bidirectional BFS from a note (--depth N).
  list                  列出全部笔记 | List all notes (relative paths).
  print <note>          打印全文 | Print full note content (byte-identical).
  create <title>        创建/覆盖笔记 | Create or overwrite; --content text; --file src to import a local file.
  append <note>         追加内容 | Append --content text to note end.
  move <from> <to>      移动/重命名 | Move or rename; updates all wiki links vault-wide.
  delete <note>         删除 | Delete (--force skips confirmation).
  frontmatter <note>    YAML 属性 | get / set key val / remove key.
  tags [tag]            标签 | Tag counts, or notes having a tag (frontmatter + inline #tag).
  dirs                  目录结构 | List directory tree.
  orphans               孤立笔记 | Notes with no inbound links.
  unresolved            断链 | Wiki links pointing to non-existent notes.
  hdaily [dir]          每日笔记 | Create/show today's daily note (default dir: 05-Daily).
  help                  完整手册 | Show this manual.

QUERY SYNTAX (search, 与 Obsidian 兼容):
  word1 word2           同文件 AND | both terms in same file
  word1 OR word2        或 | either term
  -word                 排除 | exclude files containing term
  \"exact phrase\"       短语 | exact phrase
  path:dir/             按路径过滤 | filter by path
  file:name             按文件名过滤 | filter by file name
  tag:#tag              按标签 (含嵌套 #a/b) | filter by tag
  line:(w1 w2)          同行同时出现 | both terms on the same line

SEARCH OPTIONS:
  --limit N             返回笔记条数上限 | max notes returned (default 20)
  --mode M              搜索模式 | exact(子串,默认) | fuzzy(模糊子序列) | regex(正则)
                                | graph(关键词种子+多跳图遍历) | link(命中笔记的双向邻居)
                                | backlinks(反向链接, query=笔记名) | links-of(正向链接, query=笔记名)
  --lines               显示命中行 | show matching lines
  --context N           命中行前后 N 行 | context lines around each match (requires --lines)
  --paragraph           显示命中段落 | show full paragraphs containing matches
  --max-lines N         每篇命中行数上限 | max matching lines per note (0 = unlimited)
  --max-chars N         内容块字符上限 | char cap per snippet/match/paragraph (appends ...[truncated])
  --json                JSON 输出 | structured output: {{query, count, results:[{{file,title,snippet,links_to,linked_by,matches,paragraphs}}]}}

GLOBAL OPTIONS:
  --path <dir>          workspace 目录 (必传) | vault directory (required)
  --help / -h           帮助 | show this help
  --version / -V        版本 | show version

EXAMPLES:
  ob-cli search \"归并排序\" --path /data/llm_wiki --paragraph
  ob-cli search \"RTA tag:#广告 -00-Inbox\" --path /data/llm_wiki --json --limit 5
  ob-cli search \"file:算法目录\" --path /data/llm_wiki --mode=link
  ob-cli search \"归并排序\" --path /data/llm_wiki --mode=graph --depth=2
  ob-cli print \"1.4 - 归并排序\" --path /data/llm_wiki
  ob-cli move \"旧标题\" \"新目录/新标题\" --path /data/llm_wiki"
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

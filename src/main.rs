// main.rs — notesmd-rs: Rust CLI + MCP server for Obsidian vaults
mod crud;
mod query;
mod vault;

use regex::Regex;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::exit;
use vault::{build_link_index, extract_links, Doc};

fn usage() -> ! {
    eprintln!(
        "notesmd-rs — Obsidian vault CLI + MCP server (Rust)

USAGE:
  notesmd-rs search <query>   [--path <vault>] [--links] [--lines] [--paragraph] [--context N] [--fuzzy] [--json] [--max N]
  notesmd-rs backlinks <note> [--path <vault>] [--json]
  notesmd-rs links <note>     [--path <vault>]
  notesmd-rs hybrid <query>   [--path <vault>] [--max N]
  notesmd-rs graph <note>     [--path <vault>] [--depth N]   # n-hop link graph BFS
  notesmd-rs list             [--path <vault>]
  notesmd-rs print <note>     [--path <vault>]
  notesmd-rs create <title> [--content <text>] [--path <vault>]
  notesmd-rs move <from> <to> [--path <vault>]    # rename + update all links
  notesmd-rs delete <note> [--force] [--path <vault>]
  notesmd-rs frontmatter <note> get|[set key val]|[remove key] [--path <vault>]
  notesmd-rs tags [tag]       [--path <vault>]    # tag list or notes-by-tag
  notesmd-rs orphans          [--path <vault>]
  notesmd-rs unresolved       [--path <vault>]
  notesmd-rs hdaily [dir]     [--path <vault>]    # headless daily note
  notesmd-rs serve            [--path <vault>]    # MCP stdio server

QUERY SYNTAX (Obsidian compatible):
  word1 word2 = AND | OR = or | -word = exclude | \"phrase\" = exact
  path:dir file:name tag:#tag line:(a b)"
    );
    exit(1)
}

fn vault_path(flag: Option<String>) -> PathBuf {
    let p = flag
        .or_else(|| std::env::var("OBSIDIAN_VAULT").ok())
        .unwrap_or_else(|| "/data/llm_wiki".to_string());
    PathBuf::from(p)
}

fn find_note(vault: &PathBuf, title: &str) -> Option<PathBuf> {
    let t = title.trim_end_matches(".md");
    let direct = vault.join(format!("{t}.md"));
    if direct.exists() {
        return Some(direct);
    }
    // 按文件名(不含路径)在整个 vault 中查找
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut vault_flag = None;
    let mut pos_args = Vec::new();
    let mut flags: Vec<String> = Vec::new();
    let mut kv: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--path" {
            vault_flag = args.get(i + 1).cloned();
            i += 2;
            continue;
        } else if let Some(v) = a.strip_prefix("--content") {
            if let Some(val) = v.strip_prefix('=') {
                kv.push(("content".into(), val.to_string()));
            } else {
                kv.push(("content".into(), args.get(i + 1).cloned().unwrap_or_default()));
                i += 1;
            }
        } else if a == "--max" || a == "--context" || a == "--depth" {
            let val = args.get(i + 1).cloned().unwrap_or_default();
            flags.push(format!("{}={}", a, val));
            i += 1;
        } else if a.starts_with("--") {
            flags.push(a.to_string());
        } else {
            pos_args.push(a.clone());
        }
        i += 1;
    }
    let has = |f: &str| flags.iter().any(|x| x == f);
    let _get_num = |f: &str, default: usize| -> usize {
        flags
            .iter()
            .find_map(|x| x.strip_prefix(f).and_then(|v| v.parse().ok()))
            .unwrap_or(default)
    };
    let vp = vault_path(vault_flag);
    let cmd = pos_args.first().map(|s| s.as_str()).unwrap_or_else(|| usage());

    match cmd {
        "search" => {
            let para = has("--paragraph");
            let ctx: usize = flags
                .iter()
                .find_map(|f| f.strip_prefix("--context=").and_then(|v| v.parse().ok()))
                .unwrap_or(0);
            let q = pos_args.get(1).unwrap_or_else(|| usage());
            let ast = query::parse(q).unwrap_or_else(|| usage());
            let docs = vault::load_docs(&vp);
            let max: usize = flags
                .iter()
                .find_map(|f| f.strip_prefix("--max=").and_then(|v| v.parse().ok()))
                .unwrap_or(20);
            let fuzzy = has("--fuzzy");
            let matcher = |d: &Doc| query::match_doc_mode(d, &ast, fuzzy);
            let expand = has("--links");
            let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();

            let mut matched: Vec<&Doc> = Vec::new();
            let mut by_base: std::collections::HashMap<String, &Doc> = std::collections::HashMap::new();
            for d in &docs {
                by_base.entry(d.base.to_lowercase()).or_insert(d);
                if matched.len() < max && matcher(d) {
                    matched.push(d);
                }
            }

            // 提取命中所在的段落 (以空行分块)
            fn find_paragraphs(d: &Doc, q_lower: &str) -> Vec<String> {
                let mut paras: Vec<Vec<&str>> = Vec::new();
                let mut cur: Vec<&str> = Vec::new();
                for line in d.content.split('\n') {
                    if line.trim().is_empty() {
                        if !cur.is_empty() { paras.push(std::mem::take(&mut cur)); }
                    } else { cur.push(line); }
                }
                if !cur.is_empty() { paras.push(cur); }
                let mut out = Vec::new();
                // 段落级 AND 语义: 所有词都出现在该段落中
                let words: Vec<&str> = q_lower.split_whitespace().collect();
                for p in paras {
                    let ptext = p.join("\n").to_lowercase();
                    if words.iter().all(|w| ptext.contains(w)) {
                        out.push(p.join("\n"));
                    }
                }
                out
            }
            if has("--json") {
                let mut items = Vec::new();
                for m in &matched {
                    let mut obj = format!("{{\"file\":\"{}\"", json_escape(&m.rel));
                    if expand {
                        let fwd: Vec<String> = extract_links(&m.content, &link_re)
                            .into_iter()
                            .filter_map(|l| by_base.get(&l).map(|d| d.rel.clone()))
                            .collect();
                        let back: Vec<String> = docs
                            .iter()
                            .filter(|d| d.rel != m.rel)
                            .filter(|d| {
                                extract_links(&d.content, &link_re)
                                    .iter()
                                    .any(|l| l == &m.base.to_lowercase())
                            })
                            .map(|d| d.rel.clone())
                            .collect();
                        obj += &format!(
                            ",\"links_to\":{},\"linked_by\":{}",
                            json_arr(&fwd),
                            json_arr(&back)
                        );
                    }
                    if para {
                        let paras = find_paragraphs(m, &q.to_lowercase());
                        obj += &format!(",\"paragraphs\":{}", json_arr(&paras));
                    }
                    obj += "}";
                    items.push(obj);
                }
                println!(
                    "{{\"query\":\"{}\",\"count\":{},\"results\":[{}]}}",
                    json_escape(q),
                    items.len(),
                    items.join(",")
                );
                return;
            }

            if matched.is_empty() {
                println!("No matches: {q}");
                return;
            }
            for m in &matched {
                println!("{}", m.rel);
                if para {
                    for p in find_paragraphs(m, &q.to_lowercase()) {
                        for l in p.split('\n') { println!("    │ {l}"); }
                        println!("    │");
                    }
                } else if ctx > 0 {
                    let dl: Vec<&str> = d_lines(m);
                    for (i, line) in dl.iter().enumerate() {
                        if line.to_lowercase().contains(&q.to_lowercase()) {
                            let s2 = i.saturating_sub(ctx);
                            let e2 = (i + ctx + 1).min(dl.len());
                            for (j, l) in dl[s2..e2].iter().enumerate() {
                                let mark = if s2 + j == i { ">" } else { " " };
                                println!("  {mark} {}: {}", s2 + j + 1, l.trim());
                            }
                            println!("  ---");
                        }
                    }
                }
                if expand {
                    for l in extract_links(&m.content, &link_re) {
                        if let Some(d) = by_base.get(&l) {
                            if d.rel != m.rel {
                                println!("  ► links to: {}", d.rel);
                            }
                        }
                    }
                    for d in &docs {
                        if d.rel == m.rel {
                            continue;
                        }
                        if extract_links(&d.content, &link_re)
                            .iter()
                            .any(|l| *l == m.base.to_lowercase())
                        {
                            println!("  ◄ linked by: {}", d.rel);
                        }
                    }
                }
            }
            eprintln!("\n{} notes matched", matched.len());
        }

        "backlinks" => {
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            let target = title
                .trim_end_matches(".md")
                .rsplit('/')
                .next()
                .unwrap()
                .to_lowercase();
            let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
            let docs = vault::load_docs(&vp);
            let mut out = Vec::new();
            for d in &docs {
                if d.base.to_lowercase() == target {
                    continue;
                }
                if extract_links(&d.content, &link_re).iter().any(|l| *l == target) {
                    out.push(d.rel.clone());
                }
            }
            out.sort();
            if has("--json") {
                println!("{}", json_arr(&out));
            } else if out.is_empty() {
                println!("No backlinks found for: {title}");
            } else {
                for r in out {
                    println!("{r}");
                }
            }
        }

        "links" => {
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            let path = find_note(&vp, title).unwrap_or_else(|| {
                eprintln!("Note not found: {title}");
                exit(1);
            });
            let content = std::fs::read_to_string(path).unwrap();
            let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
            let mut seen = std::collections::HashSet::new();
            for l in extract_links(&content, &link_re) {
                if seen.insert(l.clone()) {
                    println!("{l}");
                }
            }
        }

        "graph" => {
            // 图检索: 从指定笔记出发, 沿 wikilink 多跳遍历 (BFS)
            // 用法: graph <note> [--depth N] [--path <vault>]
            let start = pos_args.get(1).unwrap_or_else(|| usage());
            let depth: usize = flags
                .iter()
                .find_map(|f| f.strip_prefix("--depth=").and_then(|v| v.parse().ok()))
                .unwrap_or(2);
            let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
            let docs = vault::load_docs(&vp);
            let idx = vault::build_link_index(&docs);
            let start_base = start.trim_end_matches(".md").rsplit('/').next().unwrap().to_lowercase();
            let start_rel = idx.get(&start_base).cloned().unwrap_or_else(|| {
                eprintln!("Note not found: {start}");
                std::process::exit(1);
            });
            // 邻接表: rel -> (出链 rels, 入链 rels)
            let mut out_links: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
            let mut in_links: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
            for d in &docs {
                for l in extract_links(&d.content, &link_re) {
                    if let Some(target) = idx.get(&l) {
                        out_links.entry(d.rel.clone()).or_default().push(target.clone());
                        in_links.entry(target.clone()).or_default().push(d.rel.clone());
                    }
                }
            }
            // BFS 双向遍历
            use std::collections::VecDeque;
            let mut visited: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            let mut queue = VecDeque::new();
            visited.insert(start_rel.clone(), 0);
            queue.push_back((start_rel.clone(), 0));
            let mut edges: Vec<(usize, String, String, String)> = Vec::new();
            let mut seen_edges: std::collections::HashSet<String> = std::collections::HashSet::new();
            while let Some((cur, d)) = queue.pop_front() {
                if d >= depth { continue; }
                let neighbors: Vec<(String, String)> = out_links
                    .get(&cur)
                    .map(|v| v.iter().map(|t| (t.clone(), "→".to_string())).collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
                    .chain(in_links.get(&cur).map(|v| {
                        v.iter().map(|s| (s.clone(), "◄".to_string())).collect::<Vec<_>>()
                    }).unwrap_or_default())
                    .collect();
                for (next, arrow) in neighbors {
                    let nd = d + 1;
                    let is_new = !visited.contains_key(&next);
                    // 去重: 无向边只保留一个方向
                    let key = if cur < next {
                        format!("{cur}|{next}")
                    } else {
                        format!("{next}|{cur}")
                    };
                    if !seen_edges.contains(&key) {
                        seen_edges.insert(key);
                        edges.push((nd, cur.clone(), arrow, next.clone()));
                    }
                    if is_new {
                        visited.insert(next.clone(), nd);
                        queue.push_back((next, nd));
                    }
                }
            }
            println!("Graph from [{}] (depth {}): {} nodes", start_rel, depth, visited.len());
            edges.sort_by_key(|e| (e.0, e.1.clone()));
            for (d, from, arrow, to) in &edges {
                println!("  {}{} {} {}", "  ".repeat(*d), from, arrow, to);
            }
            eprintln!("\n{} nodes, {} edges", visited.len(), edges.len());
        }

        "hybrid" => {
            let q = pos_args.get(1).unwrap_or_else(|| usage());
            let ast = query::parse(q).unwrap_or_else(|| usage());
            let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
            let docs = vault::load_docs(&vp);
            let max: usize = flags
                .iter()
                .find_map(|f| f.strip_prefix("--max=").and_then(|v| v.parse().ok()))
                .unwrap_or(5);
            let mut hits: Vec<&Doc> = Vec::new();
            for d in &docs {
                if hits.len() >= max {
                    break;
                }
                if query::match_doc(d, &ast) {
                    hits.push(d);
                }
            }
            if hits.is_empty() {
                println!("No matches: {q}");
                return;
            }
            println!("═══ ① Keyword hits ═══");
            let mut in_graph: Vec<String> = Vec::new();
            for h in &hits {
                println!("  {}", h.rel);
                in_graph.push(h.rel.clone());
            }
            println!();
            println!("═══ ② Link expansion (1-hop) ═══");
            let idx = build_link_index(&docs);
            for h in &hits {
                for l in extract_links(&h.content, &link_re) {
                    if let Some(rel) = idx.get(&l) {
                        if !in_graph.contains(rel) && rel != &h.rel {
                            in_graph.push(rel.clone());
                            println!("  ► {} links to: {}", h.rel, rel);
                        }
                    }
                }
                for d in &docs {
                    if d.rel != h.rel && !in_graph.contains(&d.rel) {
                        if extract_links(&d.content, &link_re)
                            .iter()
                            .any(|l| *l == h.base.to_lowercase())
                        {
                            in_graph.push(d.rel.clone());
                            println!("  ◄ referenced by [{}]: {}", h.base, d.rel);
                        }
                    }
                }
            }
        }

        "list" => {
            for d in vault::load_docs(&vp) {
                println!("{}", d.rel);
            }
        }

        "print" => {
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            let path = find_note(&vp, title).unwrap_or_else(|| {
                eprintln!("Note not found: {title}");
                exit(1);
            });
            print!("{}", std::fs::read_to_string(path).unwrap());
        }

        "create" => {
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            let content = kv
                .iter()
                .find(|(k, _)| k == "content")
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
                .replace("\\n", "\n");
            let mut path = vp.join(title);
            path.set_extension("md");
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, content).unwrap();
            println!("Created: {}", path.display());
        }

        "move" => {
            let (from, to) = (pos_args.get(1).unwrap_or_else(|| usage()), pos_args.get(2).unwrap_or_else(|| usage()));
            crud::mv(&vp, from, to);
        }
        "delete" => {
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            crud::delete(&vp, title, has("--force"));
        }
        "frontmatter" => {
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            let action = pos_args.get(2).map(|s| s.as_str()).unwrap_or("get");
            let key = pos_args.get(3).map(|s| s.as_str()).unwrap_or("");
            let value = pos_args.get(4).cloned();
            crud::frontmatter(&vp, title, action, key, value);
        }
        "tags" => crud::tags(&vp, pos_args.get(1).map(|s| s.as_str())),
        "orphans" => crud::orphans(&vp),
        "unresolved" => crud::unresolved(&vp),
        "hdaily" => crud::hdaily(&vp, pos_args.get(1).map(|s| s.as_str()).unwrap_or("05-Daily"), None),
        "serve" => run_mcp(&vp),

        _ => usage(),
    }
}

// ---------- MCP stdio server ----------
fn run_mcp(vp: &PathBuf) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.unwrap_or_default();
        if line.trim().is_empty() {
            continue;
        }
        let v: Json = match parse_json(&line) {
            Some(v) => v,
            None => continue,
        };
        let id = v.get("id").cloned().unwrap_or(Json::Null);
        let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let result = match method {
            "initialize" => r#"{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"notesmd-rs","version":"0.1.0"}}"#.to_string(),
            "tools/list" => TOOLS_LIST.to_string(),
            "tools/call" => {
                let name = v.get("params").and_then(|p| p.get("name")).and_then(|x| x.as_str()).unwrap_or("");
                let args: Option<&Json> = v.get("params").and_then(|p| p.get("arguments"));
                let text = match name {
                    "search_notes" => {
                        let q = args.and_then(|a| a.get("query")).and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let maxn = args.and_then(|a| a.get("max")).and_then(|x| x.as_u64()).unwrap_or(20) as usize;
                        let ast = query::parse(&q);
                        let mut lines = Vec::new();
                        if let Some(ast) = ast {
                            for d in vault::load_docs(vp) {
                                if lines.len() >= maxn { break; }
                                if query::match_doc(&d, &ast) {
                                    let mut dl = d.lines.clone();
                                    if dl.is_empty() { dl = d.content.split('\n').map(|s| s.to_string()).collect(); }
                                    for (i, l) in dl.iter().enumerate() {
                                        if lines.len() >= maxn { break; }
                                        if l.to_lowercase().contains(&q.to_lowercase()) {
                                            lines.push(format!("{}:{}: {}", d.rel, i + 1, l.trim()));
                                        }
                                    }
                                }
                            }
                        }
                        if lines.is_empty() { format!("No matches: {q}") } else { lines.join("\n") }
                    }
                    "read_note" => {
                        let title = args.and_then(|a| a.get("title")).and_then(|x| x.as_str()).unwrap_or("").to_string();
                        match find_note(vp, &title).and_then(|p| std::fs::read_to_string(p).ok()) {
                            Some(c) => c,
                            None => format!("Note not found: {title}"),
                        }
                    }
                    "backlinks" => {
                        let title = args.and_then(|a| a.get("title")).and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let target = title.trim_end_matches(".md").rsplit('/').next().unwrap_or("").to_lowercase();
                        let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
                        let mut out: Vec<String> = Vec::new();
                        for d in vault::load_docs(vp) {
                            if d.base.to_lowercase() != target
                                && extract_links(&d.content, &link_re).iter().any(|l| *l == target)
                            {
                                out.push(d.rel);
                            }
                        }
                        if out.is_empty() { "No backlinks".into() } else { out.join("\n") }
                    }
                    "list_notes" => {
                        let docs = vault::load_docs(vp);
                        docs.iter().map(|d| d.rel.clone()).collect::<Vec<_>>().join("\n")
                    }
                    _ => format!("Unknown tool: {name}"),
                };
                format!(
                    "{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}]}}",
                    json_escape(&text)
                )
            }
            "ping" => "{}".to_string(),
            _ => continue,
        };
        writeln!(
            out,
            "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}",
            json_value_to_string(&id),
            result
        )
        .unwrap();
    }
}

const TOOLS_LIST: &str = r#"{"tools":[
{"name":"search_notes","description":"Full-text search (Chinese/regex)","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"max":{"type":"number"}},"required":["query"]}},
{"name":"read_note","description":"Read full note content","inputSchema":{"type":"object","properties":{"title":{"type":"string"}},"required":["title"]}},
{"name":"backlinks","description":"Notes linking to the given note","inputSchema":{"type":"object","properties":{"title":{"type":"string"}},"required":["title"]}},
{"name":"list_notes","description":"List all notes","inputSchema":{"type":"object"}}
]}"#;

// ---------- 极简 JSON 解析 (零依赖) ----------
#[derive(Debug, Clone)]
enum Json {
    Null,
    Str(String),
    Num(f64),
    Bool(bool),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}


impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(n) => Some(*n as u64),
            _ => None,
        }
    }
}

struct JParser<'a> {
    c: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> JParser<'a> {
    fn skip_ws(&mut self) {
        while matches!(self.c.peek(), Some(c) if c.is_whitespace()) {
            self.c.next();
        }
    }
    fn parse(&mut self) -> Option<Json> {
        self.skip_ws();
        match self.c.peek()? {
            '{' => {
                self.c.next();
                let mut items = Vec::new();
                self.skip_ws();
                if self.c.peek() == Some(&'}') {
                    self.c.next();
                    return Some(Json::Obj(items));
                }
                loop {
                    self.skip_ws();
                    let key = match self.parse() {
                        Some(Json::Str(s)) => s,
                        _ => return None,
                    };
                    self.skip_ws();
                    if self.c.next() != Some(':') {
                        return None;
                    }
                    let val = self.parse()?;
                    items.push((key, val));
                    self.skip_ws();
                    match self.c.next() {
                        Some(',') => continue,
                        Some('}') => break,
                        _ => return None,
                    }
                }
                Some(Json::Obj(items))
            }
            '[' => {
                self.c.next();
                let mut items = Vec::new();
                self.skip_ws();
                if self.c.peek() == Some(&']') {
                    self.c.next();
                    return Some(Json::Arr(items));
                }
                loop {
                    let val = self.parse()?;
                    items.push(val);
                    self.skip_ws();
                    match self.c.next() {
                        Some(',') => continue,
                        Some(']') => break,
                        _ => return None,
                    }
                }
                Some(Json::Arr(items))
            }
            '"' => {
                self.c.next();
                let mut s = String::new();
                loop {
                    match self.c.next()? {
                        '"' => break,
                        '\\' => match self.c.next()? {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            'u' => {
                                let hex: String = self.c.by_ref().take(4).collect();
                                if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                                    if let Some(ch) = char::from_u32(cp) {
                                        s.push(ch);
                                    }
                                }
                            }
                            other => s.push(other),
                        },
                        c => s.push(c),
                    }
                }
                Some(Json::Str(s))
            }
            't' => {
                for _ in 0..3 {
                    self.c.next();
                }
                Some(Json::Bool(true))
            }
            'f' => {
                for _ in 0..4 {
                    self.c.next();
                }
                Some(Json::Bool(false))
            }
            'n' => {
                for _ in 0..3 {
                    self.c.next();
                }
                Some(Json::Null)
            }
            _ => {
                let mut s = String::new();
                while let Some(c) = self.c.peek() {
                    if c.is_ascii_digit() || *c == '-' || *c == '.' || *c == 'e' || *c == '+' {
                        s.push(*c);
                        self.c.next();
                    } else {
                        break;
                    }
                }
                s.parse::<f64>().ok().map(Json::Num)
            }
        }
    }
}

fn parse_json(s: &str) -> Option<Json> {
    JParser { c: s.chars().peekable() }.parse()
}

fn json_value_to_string(v: &Json) -> String {
    match v {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Num(n) => {
            if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
        Json::Str(s) => format!("\"{}\"", json_escape(s)),
        Json::Arr(items) => format!(
            "[{}]",
            items.iter().map(json_value_to_string).collect::<Vec<_>>().join(",")
        ),
        Json::Obj(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", json_escape(k), json_value_to_string(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn d_lines(d: &Doc) -> Vec<&str> {
    d.content.split('\n').collect()
}

fn json_arr(items: &[String]) -> String {
    format!(
        "[{}]",
        items.iter().map(|s| format!("\"{}\"", json_escape(s))).collect::<Vec<_>>().join(",")
    )
}

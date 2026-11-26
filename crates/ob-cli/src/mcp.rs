use ob_core::common::find_note;
use ob_core::json::{json_arr, json_escape, json_value_to_string, parse_json, Json};
use ob_core::query;
use ob_core::vault::{self, extract_links};
use regex::Regex;
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub fn run(vp: &PathBuf) {
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
            "initialize" => r#"{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"ob-cli-rust","version":"0.1.0"}}"#.to_string(),
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


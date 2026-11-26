use crate::common::*;
use crate::query;
use crate::json::{json_arr, json_escape};
use crate::vault::{self, extract_links, Doc};
use crate::common::*;
use regex::Regex;
use std::process::exit;


pub fn run_search(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
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
            let mode = flags
                .iter()
                .find_map(|f| f.strip_prefix("--mode=").map(|v| v.to_string()))
                .unwrap_or_else(|| if has("--fuzzy") { "fuzzy".to_string() } else { "exact".to_string() });
            let depth: usize = flags
                .iter()
                .find_map(|f| f.strip_prefix("--depth=").and_then(|v| v.parse().ok()))
                .unwrap_or(1);
            if !matches!(mode.as_str(), "exact" | "fuzzy" | "regex" | "graph" | "link") {
                eprintln!("Error: unknown --mode '{}'. Use: exact | fuzzy | regex | graph | link", mode);
                std::process::exit(2);
            }
            let text_mode = if mode == "graph" || mode == "link" { "exact" } else { mode.as_str() };
            let matcher = |d: &Doc| query::match_doc_mode(d, &ast, text_mode);
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
            // 图模式: 关键词命中为种子, BFS 多跳扩展
            if mode == "graph" {
                let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
                let idx = vault::build_link_index(&docs);
                use std::collections::VecDeque;
                let mut visited: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                let mut queue = VecDeque::new();
                for h in &matched { visited.entry(h.rel.clone()).or_insert(0); queue.push_back((h.rel.clone(), 0usize)); }
                let mut edges: Vec<(usize, String, String, String)> = Vec::new();
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut out_links: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
                let mut in_links: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
                for d in &docs {
                    for l in extract_links(&d.content, &link_re) {
                        if let Some(t) = idx.get(&l) {
                            out_links.entry(d.rel.clone()).or_default().push(t.clone());
                            in_links.entry(t.clone()).or_default().push(d.rel.clone());
                        }
                    }
                }
                while let Some((cur, d)) = queue.pop_front() {
                    if d >= depth { continue; }
                    let mut neighbors: Vec<(String, &str)> = Vec::new();
                    if let Some(v) = out_links.get(&cur) { for t in v { neighbors.push((t.clone(), "→")); } }
                    if let Some(v) = in_links.get(&cur) { for s2 in v { neighbors.push((s2.clone(), "◄")); } }
                    for (next, arrow) in neighbors {
                        let nd = d + 1;
                        let key = if cur < next { format!("{cur}|{next}") } else { format!("{next}|{cur}") };
                        let is_new = !visited.contains_key(&next);
                        if !seen.contains(&key) {
                            seen.insert(key);
                            edges.push((nd, cur.clone(), arrow.to_string(), next.clone()));
                        }
                        if is_new { visited.insert(next.clone(), nd); queue.push_back((next, nd)); }
                    }
                }
                println!("Graph search [{}] depth {}: {} nodes, {} edges", q, depth, visited.len(), edges.len());
                edges.sort_by_key(|e| (e.0, e.1.clone()));
                for (d, from, arrow, to) in &edges {
                    println!("  {}{} {} {}", "  ".repeat(*d), from, arrow, to);
                }
                return;
            }
            // 双链模式: 命中笔记的正向+反向链接邻居
            if mode == "link" {
                let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
                let idx = vault::build_link_index(&docs);
                println!("Link search [{}]: {} seed notes", q, matched.len());
                for h in &matched {
                    println!("  ● {}", h.rel);
                    for l in extract_links(&h.content, &link_re) {
                        if let Some(rel) = idx.get(&l) {
                            if rel != &h.rel { println!("    ► links to: {}", rel); }
                        }
                    }
                    for d in &docs {
                        if d.rel != h.rel && extract_links(&d.content, &link_re).iter().any(|l| *l == h.base.to_lowercase()) {
                            println!("    ◄ linked by: {}", d.rel);
                        }
                    }
                }
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

use crate::common::*;
use crate::query;
use crate::vault::{self, extract_links};
use crate::common::*;
use regex::Regex;
use std::process::exit;


pub fn run_graph(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
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

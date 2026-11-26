use crate::common::*;
use crate::query;
use crate::vault::{self, build_link_index, extract_links, Doc};
use crate::common::*;
use regex::Regex;
use std::process::exit;


pub fn run_hybrid(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
            let q = pos_args.get(1).unwrap_or_else(|| usage());
            let ast = query::parse(q).unwrap_or_else(|| usage());
            let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
            let docs = vault::load_docs(&vp);
            let max: usize = flags
                .iter()
                .find_map(|f| f.strip_prefix("--limit=").and_then(|v| v.parse().ok()))
                .or_else(|| flags.iter().find_map(|f| f.strip_prefix("--max=").and_then(|v| v.parse().ok())))
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

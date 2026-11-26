use crate::common::*;
use crate::query;
use crate::json::json_arr;
use crate::vault::{self, extract_links};
use crate::common::*;
use regex::Regex;
use std::process::exit;


pub fn run_backlinks(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
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

pub fn run_links(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
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

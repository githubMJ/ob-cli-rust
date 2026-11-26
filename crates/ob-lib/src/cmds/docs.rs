use crate::common::*;
use crate::crud;
use crate::query;
use crate::vault;
use regex::Regex;
use std::process::exit;


pub fn run_list(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
            for d in vault::load_docs(&vp) {
                println!("{}", d.rel);
            }
}

pub fn run_print(ctx: &Ctx) {
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
            print!("{}", std::fs::read_to_string(path).unwrap());
}

pub fn run_create(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
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

pub fn run_move(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
            let (from, to) = (pos_args.get(1).unwrap_or_else(|| usage()), pos_args.get(2).unwrap_or_else(|| usage()));
            crud::mv(&vp, from, to);
}

pub fn run_delete(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            crud::delete(&vp, title, has("--force"));
}

pub fn run_frontmatter(ctx: &Ctx) {
        let vp = &ctx.vp;
        let flags = &ctx.flags;
        let pos_args = &ctx.pos_args;
        let kv = &ctx.kv;
        let has = |f: &str| flags.iter().any(|x| x == f);
            let title = pos_args.get(1).unwrap_or_else(|| usage());
            let action = pos_args.get(2).map(|s| s.as_str()).unwrap_or("get");
            let key = pos_args.get(3).map(|s| s.as_str()).unwrap_or("");
            let value = pos_args.get(4).cloned();
            crud::frontmatter(&vp, title, action, key, value);
}

pub fn run_hdaily(ctx: &Ctx) {
    let vp = &ctx.vp;
    let dir = ctx.pos_args.get(1).map(|s| s.as_str()).unwrap_or("05-Daily");
    crate::crud::hdaily(&vp, dir, None);
}

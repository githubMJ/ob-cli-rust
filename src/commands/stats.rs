use crate::common::*;
use crate::query;
use crate::vault;
use regex::Regex;
use std::process::exit;


pub fn run_tags(ctx: &Ctx) {
    let vp = &ctx.vp;
    let tag = ctx.pos_args.get(1).map(|s| s.as_str());
    crate::crud::tags(&vp, tag);
}

pub fn run_orphans(ctx: &Ctx) {
    let vp = &ctx.vp;
    crate::crud::orphans(&vp);
}

pub fn run_unresolved(ctx: &Ctx) {
    let vp = &ctx.vp;
    crate::crud::unresolved(&vp);
}

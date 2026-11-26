// lib.rs — 共享库: 三个二进制 (ob-cli/ob-search/ob-ops) 的公共实现
pub mod cmds;
pub mod mcp;
pub mod common;
pub mod crud;
pub mod json;
pub mod query;
pub mod vault;

use common::Ctx;

/// CLI 参数解析 (三个二进制共用)
pub fn parse_cli() -> (String, Ctx) {
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
    let cmd = pos_args.first().cloned().unwrap_or_default();
    let vp = common::vault_path(vault_flag);
    (cmd, Ctx { vp, flags, pos_args, kv })
}

// lib.rs — 共享核心: 文档层/检索层/操作层/JSON
pub mod cmds;
pub mod common;
pub mod crud;
pub mod json;
pub mod query;
pub mod vault;

use common::Ctx;
use std::path::PathBuf;

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
        } else if let Some(v) = a.strip_prefix("--file") {
            if let Some(val) = v.strip_prefix('=') {
                kv.push(("file".into(), val.to_string()));
            } else {
                kv.push(("file".into(), args.get(i + 1).cloned().unwrap_or_default()));
                i += 1;
            }
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
    // help 子命令不需要 vault
    if cmd == "help" {
        return (cmd, Ctx { vp: PathBuf::new(), flags, pos_args, kv });
    }
    // workspace 必传: 所有检索/操作都限制在指定目录内
    let vp = match vault_flag.or_else(|| std::env::var("OBSIDIAN_VAULT").ok()) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("Error: --path <vault> is required (workspace directory to operate on)");
            eprintln!("错误: 必须通过 --path 指定 vault 工作空间目录");
            std::process::exit(2);
        }
    };
    if !vp.is_dir() {
        eprintln!("Error: vault path does not exist or is not a directory: {}", vp.display());
        eprintln!("错误: vault 路径不存在或不是目录: {}", vp.display());
        std::process::exit(2);
    }
    (cmd, Ctx { vp, flags, pos_args, kv })
}

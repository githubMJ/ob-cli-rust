// crud.rs — move(重命名+更新全库链接) / delete / frontmatter / tags / orphans / unresolved / hdaily
use crate::vault::{load_docs, Doc};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn find_note(vault: &Path, title: &str) -> Option<PathBuf> {
    let t = title.trim_end_matches(".md");
    let direct = vault.join(format!("{t}.md"));
    if direct.exists() {
        return Some(direct);
    }
    let want = t.rsplit('/').next().unwrap_or(t).to_lowercase();
    fn walk(dir: &Path, want: &str, found: &mut Option<PathBuf>) {
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
                    .map(|n| n.to_string_lossy().trim_end_matches(".md").to_lowercase() == want)
                    .unwrap_or(false)
                {
                    *found = Some(p);
                    return;
                }
            }
        }
    }
    let mut found = None;
    walk(vault, &want, &mut found);
    found
}

/// move: 重命名/移动笔记并更新全库 [[链接]]
pub fn mv(vault: &Path, from: &str, to: &str) {
    let src = find_note(vault, from).unwrap_or_else(|| {
        eprintln!("Note not found: {from}");
        std::process::exit(1);
    });
    let mut dst = vault.join(to);
    dst.set_extension("md");
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let old_base = src
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_lowercase();
    let new_base = dst
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    std::fs::rename(&src, &dst).unwrap();
    println!("Moved:\n  from {}\n  to   {}", src.display(), dst.display());

    // 更新全库链接: [[旧名...]] → [[新名...]] (保留 |别名 和 #标题 部分)
    let link_re = Regex::new(r"\[\[([^\]\|]+)(\|[^\]]*)?\]\]").unwrap();
    let mut updated = 0;
    for entry in walkdir(vault) {
        let p = PathBuf::from(&entry);
        if p == dst {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&p) {
            let mut changed = false;
            let new_content = link_re
                .replace_all(&content, |caps: &regex::Captures| {
                    let target = &caps[1];
                    let name = target.split(['|', '#']).next().unwrap_or(target);
                    let name_clean = name.trim().trim_end_matches(".md");
                    if name_clean.to_lowercase() == old_base {
                        changed = true;
                        updated += 1;
                        // 保留路径部分: 若旧链接带路径则替换文件名
                        let new_ref = if target.contains('/') {
                            // 保留旧链接的目录部分, 只替换文件名
                            let dir_part = &target[..target.rfind('/').unwrap() + 1];
                            format!("{dir_part}{new_base}")
                        } else {
                            new_base.clone()
                        };
                        let suffix = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                        format!("[[{new_ref}{suffix}]]")
                    } else {
                        caps[0].to_string()
                    }
                })
                .to_string();
            if changed {
                std::fs::write(&p, new_content).unwrap();
            }
        }
    }
    eprintln!("  {updated} links updated");
}

fn walkdir(vault: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<String>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path();
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                if p.is_dir() {
                    if !name.starts_with('.') {
                        walk(&p, out);
                    }
                } else if name.to_lowercase().ends_with(".md") {
                    out.push(p.to_string_lossy().to_string());
                }
            }
        }
    }
    walk(vault, &mut out);
    out
}

/// delete: 删除笔记
pub fn delete(vault: &Path, title: &str, force: bool) {
    let path = find_note(vault, title).unwrap_or_else(|| {
        eprintln!("Note not found: {title}");
        std::process::exit(1);
    });
    if !force {
        eprint!("Delete {}? [y/N] ", path.display());
        std::io::Write::flush(&mut std::io::stderr()).unwrap();
        let mut ans = String::new();
        std::io::stdin().read_line(&mut ans).unwrap();
        if !ans.trim().eq_ignore_ascii_case("y") {
            println!("Aborted");
            return;
        }
    }
    std::fs::remove_file(&path).unwrap();
    println!("Deleted: {}", path.display());
}

/// frontmatter: 读/改/删 YAML frontmatter 键
pub fn frontmatter(vault: &Path, title: &str, action: &str, key: &str, value: Option<String>) {
    let path = find_note(vault, title).unwrap_or_else(|| {
        eprintln!("Note not found: {title}");
        std::process::exit(1);
    });
    let content = std::fs::read_to_string(&path).unwrap();
    let has_fm = content.starts_with("---");
    let (fm_body, body) = if has_fm {
        match content[3..].find("\n---") {
            Some(i) => (content[3..3 + i].to_string(), content[3 + i + 4..].to_string()),
            None => (content[3..].to_string(), String::new()),
        }
    } else {
        (String::new(), content.clone())
    };

    match action {
        "get" | "print" => {
            if key.is_empty() {
                print!("{fm_body}");
            } else {
                for line in fm_body.lines() {
                    if let Some(rest) = line.strip_prefix(&format!("{key}:")) {
                        println!("{}", rest.trim());
                        return;
                    }
                }
                println!("(key not found: {key})");
            }
        }
        "set" => {
            let val = value.unwrap_or_default();
            let newline = format!("{key}: {val}");
            let mut new_fm = String::new();
            let mut replaced = false;
            for line in fm_body.lines() {
                if line.to_lowercase().starts_with(&format!("{}:", key.to_lowercase())) {
                    new_fm.push_str(&newline);
                    replaced = true;
                } else {
                    new_fm.push_str(line);
                }
                new_fm.push('\n');
            }
            if !replaced {
                new_fm.push_str(&newline);
                new_fm.push('\n');
            }
            let out = if has_fm {
                format!("---\n{new_fm}---\n{body}")
            } else {
                format!("---\n{new_fm}---\n{content}")
            };
            std::fs::write(&path, out).unwrap();
            println!("Set {key} = {val}");
        }
        "remove" => {
            let mut new_fm = String::new();
            for line in fm_body.lines() {
                if !line.to_lowercase().starts_with(&format!("{}:", key.to_lowercase())) {
                    new_fm.push_str(line);
                    new_fm.push('\n');
                }
            }
            let out = format!("---\n{new_fm}---\n{body}");
            std::fs::write(&path, out).unwrap();
            println!("Removed {key}");
        }
        _ => eprintln!("Unknown action: {action} (use get/set/remove)"),
    }
}

/// tags: 列出所有标签计数, 或列出含某标签的笔记
pub fn tags(vault: &Path, tag: Option<&str>) {
    let docs = load_docs(vault);
    match tag {
        None => {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for d in &docs {
                for t in &d.tags {
                    *counts.entry(t.clone()).or_insert(0) += 1;
                }
            }
            let mut list: Vec<_> = counts.into_iter().collect();
            list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for (t, c) in list {
                println!("{c:5}  #{t}");
            }
        }
        Some(want) => {
            let want = want.trim_start_matches('#');
            let mut out: Vec<String> = docs
                .iter()
                .filter(|d: &&Doc| d.has_tag(want))
                .map(|d| d.rel.clone())
                .collect();
            out.sort();
            if out.is_empty() {
                println!("No notes with tag: {want}");
            }
            for r in out {
                println!("{r}");
            }
        }
    }
}

/// orphans: 无入链的孤立笔记
pub fn orphans(vault: &Path) {
    let docs = load_docs(vault);
    let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
    let mut linked: HashMap<String, bool> = HashMap::new();
    for d in &docs {
        for l in extract_links2(&d.content, &link_re) {
            linked.insert(l, true);
        }
    }
    let mut out: Vec<String> = docs
        .iter()
        .filter(|d| !linked.get(&d.base.to_lowercase()).copied().unwrap_or(false))
        .map(|d| d.rel.clone())
        .collect();
    out.sort();
    for r in &out {
        println!("{r}");
    }
    eprintln!("\n{} orphan notes", out.len());
}

/// unresolved: 指向不存在笔记的断链
pub fn unresolved(vault: &Path) {
    let docs = load_docs(vault);
    let link_re = Regex::new(r"\[\[([^\]|#]+)").unwrap();
    let exists: std::collections::HashSet<String> = docs
        .iter()
        .map(|d| d.base.to_lowercase())
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for d in &docs {
        for l in extract_links2(&d.content, &link_re) {
            if !exists.contains(&l) && seen.insert(l.clone()) {
                out.push(format!("{} → [[{}]]", d.rel, l));
            }
        }
    }
    out.sort();
    for r in &out {
        println!("{r}");
    }
    eprintln!("\n{} unresolved links", out.len());
}

/// hdaily: 无头每日笔记
pub fn hdaily(vault: &Path, dir: &str, template: Option<&str>) {
    let date = chrono_today();
    let ddir = vault.join(dir);
    std::fs::create_dir_all(&ddir).unwrap();
    let note = ddir.join(format!("{date}.md"));
    if !note.exists() {
        let content = template
            .map(|t| t.to_string())
            .unwrap_or_else(|| format!("# {date}\n\n"));
        std::fs::write(&note, content).unwrap();
        println!("Created: {}", note.display());
    } else {
        println!("Exists: {}", note.display());
    }
}

fn chrono_today() -> String {
    // 无 chrono 依赖: 从 SystemTime 计算日期 (UTC)
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let days = secs.div_euclid(86400);
    // civil_from_days (Howard Hinnant 算法)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn extract_links2(content: &str, re: &Regex) -> Vec<String> {
    let mut out = Vec::new();
    for cap in re.captures_iter(content) {
        let target = &cap[1];
        let name = target.split(['|', '#']).next().unwrap_or(target);
        let name = name.rsplit('/').next().unwrap_or(name).trim().to_lowercase();
        if !name.is_empty() {
            out.push(name);
        }
    }
    out
}

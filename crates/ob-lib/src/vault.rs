// vault.rs — vault 遍历、文档模型、标签解析 (frontmatter + 行内 #tag)
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Doc {
    pub rel: String,
    pub base: String,
    pub content: String,
    pub lower: String,
    pub lines: Vec<String>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub links_out: Vec<String>,
    pub mtime: i64,   // 文件修改时间 (秒), 用于增量同步
}

impl Doc {
    pub fn has_tag(&self, tag: &str) -> bool {
        let tag = tag.trim_start_matches('#');
        self.tags.iter().any(|t| t == tag || t.starts_with(&format!("{tag}/")))
    }
}

fn parse_frontmatter_tags(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !content.starts_with("---") {
        return out;
    }
    let mut in_tags = false;
    for line in content[3..].lines() {
        if line.starts_with("---") {
            break;
        }
        let lower = line.to_lowercase();
        if lower.starts_with("tags:") {
            in_tags = true;
            let v = line[5..].trim().trim_start_matches('[').trim_end_matches(']');
            if !v.is_empty() {
                for t in v.split(',') {
                    let t = t.trim().trim_matches(|c| c == '"' || c == '\'').trim_start_matches('#');
                    if !t.is_empty() {
                        out.push(t.to_string());
                    }
                }
            }
        } else if in_tags {
            let t = line.trim();
            if let Some(t) = t.strip_prefix("- ") {
                let t = t.trim().trim_start_matches('#');
                if !t.is_empty() {
                    out.push(t.to_string());
                }
            } else if !t.is_empty() {
                break;
            }
        }
    }
    out
}

fn parse_inline_tags(content: &str) -> Vec<String> {
    // 跳过 frontmatter 区域
    let body = if content.starts_with("---") {
        match content[3..].find("\n---") {
            Some(i) => &content[3 + i..],
            None => content,
        }
    } else {
        content
    };
    let re = Regex::new(r"(?:^|\s)#([\p{L}\p{N}/_\-]+)").unwrap();
    let mut out = Vec::new();
    // 行内标签只在非 frontmatter、非代码块行匹配
    let mut in_code = false;
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code || t.starts_with("http") {
            continue;
        }
        for cap in re.captures_iter(line) {
            out.push(cap[1].to_string());
        }
    }
    out
}

/// 遍历 vault, 返回所有 .md 文档 (跳过隐藏目录)
/// 优化: 按顶层子目录并行读取 (std scoped threads, 无额外依赖)
pub fn load_docs(vault: &Path) -> Vec<Doc> {
    let root = vault.to_path_buf();
    // 阶段 1: 收集全部 md 路径 (纯目录遍历, 很快)
    let mut files: Vec<PathBuf> = Vec::new();
    collect_md_paths_into(&root, &root, &mut files);

    // 阶段 2: 均分给线程并行读取 (每批 512 个文件)
    const BATCH: usize = 512;
    let batches: Vec<Vec<PathBuf>> = files.chunks(BATCH).map(|c| c.to_vec()).collect();
    let mut handles = Vec::new();
    for batch in batches {
        let r = root.clone();
        handles.push(std::thread::spawn(move || {
            let mut docs = Vec::with_capacity(batch.len());
            for path in batch {
                if let Some(d) = read_doc(&r, &path) {
                    docs.push(d);
                }
            }
            docs
        }));
    }
    let mut docs = Vec::with_capacity(files.len());
    for h in handles {
        if let Ok(mut part) = h.join() {
            docs.append(&mut part);
        }
    }
    docs.sort_by(|a, b| a.rel.cmp(&b.rel));
    docs
}

/// 收集全部 .md 路径 (跳过隐藏目录)
pub fn collect_md_paths_into(_root: &Path, dir: &Path, files: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
        Err(_) => return,
    };
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            collect_md_paths_into(dir, &path, files);
        } else if name.to_lowercase().ends_with(".md") {
            files.push(path);
        }
    }
}

/// 仅收集 .md 文件路径 (不读内容)
pub fn collect_md_paths(vault: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_md_paths_into(vault, vault, &mut files);
    files
}

/// 加载单个文件 (供增量同步使用)
pub fn load_one(vault: &Path, path: &Path) -> Option<Doc> {
    read_doc(vault, path)
}

fn read_doc(vault: &Path, path: &Path) -> Option<Doc> {
    let content = fs::read_to_string(path).ok()?;
    let rel = path.strip_prefix(vault).ok()?.to_string_lossy().to_string();
    let base = path.file_name()?.to_string_lossy().trim_end_matches(".md").to_string();
    // 统一走 parse 模块 (pulldown-cmark, 排除代码块, 支持别名/块引用/嵌入)
    let parsed = crate::parse::parse_note(&content);
    let links_out = crate::parse::extract_link_targets(&parsed);
    let mtime = fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Some(Doc {
        rel,
        base,
        lower: content.to_lowercase(),
        content,
        lines: Vec::new(),
        tags: parsed.tags,
        aliases: parsed.aliases,
        links_out,
        mtime,
    })
}

fn walk(root: &Path, dir: &Path, docs: &mut Vec<Doc>) {
    let mut entries: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
        Err(_) => return,
    };
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            walk(root, &path, docs);
        } else if name.to_lowercase().ends_with(".md") {
            if let Some(d) = read_doc(root, &path) {
                docs.push(d);
            }
        }
    }
}

impl Doc {
    pub fn get_lines(&mut self) {
        if self.lines.is_empty() {
            self.lines = self.content.split('\n').map(|s| s.to_string()).collect();
        }
    }
}

/// wiki 链接索引: 笔记名 -> 相对路径
pub fn build_link_index(docs: &[Doc]) -> HashMap<String, String> {
    let mut idx = HashMap::new();
    for d in docs {
        idx.entry(d.base.to_lowercase()).or_insert_with(|| d.rel.clone());
    }
    idx
}

pub fn extract_links(content: &str, re: &Regex) -> Vec<String> {
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

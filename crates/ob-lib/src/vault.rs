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
pub fn load_docs(vault: &Path) -> Vec<Doc> {
    let mut docs = Vec::new();
    walk(vault, vault, &mut docs);
    docs
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
            if let Ok(content) = fs::read_to_string(&path) {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let base = name.trim_end_matches(".md").to_string();
                let mut tags = parse_frontmatter_tags(&content);
                tags.extend(parse_inline_tags(&content));
                docs.push(Doc {
                    lower: content.to_lowercase(),
                    base,
                    rel,
                    content,
                    lines: Vec::new(),
                    tags,
                });
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

// parse.rs — Obsidian Flavored Markdown 解析层
// 参考 turbovault-parser 的分层设计:
//   frontmatter → wikilinks/embeds → tags → headings → tasks
// 用 pulldown-cmark 排除代码块内的假链接
use pulldown_cmark::{Event, Options, Parser as CmParser, Tag, TagEnd};
use regex::Regex;
use std::sync::OnceLock;

/// 链接类型 (对齐 Obsidian 语义)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Wiki,       // [[Note]]
    Embed,      // ![[Note]]
    Heading,    // [[Note#Heading]]
    BlockRef,   // [[Note#^block]]
    Markdown,   // [text](other-note.md)  — 库内 md 链接
    External,   // [text](https://...)    — 站外链接
}

/// 解析出的链接
#[derive(Debug, Clone)]
pub struct ParsedLink {
    pub target: String,      // 目标笔记名/路径 (不含 # 与别名)
    pub display: Option<String>, // [[Note|别名]]
    pub kind: LinkKind,
    pub anchor: Option<String>,  // #Heading 或 #^block
}

/// 解析结果
#[derive(Debug, Default, Clone)]
pub struct ParsedNote {
    pub frontmatter: Vec<(String, String)>,  // YAML 键值对
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub links: Vec<ParsedLink>,
    pub headings: Vec<(usize, String)>,      // (level, text)
    pub tasks: Vec<(bool, String)>,          // (done, text)
    pub content: String,                     // 去 frontmatter 的正文
}

// ---------- 正则 (懒初始化) ----------

fn re_wikilink() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(!?)\[\[([^\[\]]+?)\]\]").unwrap())
}

fn re_inline_tag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[\s(（])#([A-Za-z0-9_\-\u4e00-\u9fa5/]+)").unwrap())
}

// ---------- 主入口 ----------

/// 解析一篇 Obsidian 笔记
pub fn parse_note(raw: &str) -> ParsedNote {
    let mut note = ParsedNote::default();

    // 1. frontmatter
    let (fm_lines, body) = split_frontmatter(raw);
    for line in fm_lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_string();
            let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
            // aliases 特殊处理
            if key.eq_ignore_ascii_case("aliases") || key.eq_ignore_ascii_case("alias") {
                note.aliases = parse_yaml_list(&val);
            }
            // tags 特殊处理
            if key.eq_ignore_ascii_case("tags") {
                for t in parse_yaml_list(&val) {
                    if !note.tags.contains(&t) {
                        note.tags.push(t);
                    }
                }
            }
            note.frontmatter.push((key, val));
        }
    }
    note.content = body.to_string();

    // 2. 用 pulldown-cmark 遍历, 跳过代码块
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_STRIKETHROUGH);

    let mut in_code_block = false;
    let mut heading_level = 0usize;
    let mut heading_text = String::new();
    let mut in_heading = false;
    // 累积非代码文本 (pulldown-cmark 会把 [[A]] 拆成多个 Text 事件)
    let mut buf = String::new();

    for ev in CmParser::new_ext(body, opts) {
        match ev {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Code(_) => { /* 行内代码: 跳过 */ }
            Event::Start(Tag::Heading { level, .. }) => {
                heading_level = level as usize;
                heading_text.clear();
                in_heading = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                note.headings.push((heading_level, heading_text.trim().to_string()));
                in_heading = false;
            }
            Event::Text(t) => {
                if in_code_block {
                    continue;
                }
                if in_heading {
                    heading_text.push_str(&t);
                }
                // 累积到缓冲区, 段落结束后统一扫描
                buf.push_str(&t);
            }
            Event::End(TagEnd::Paragraph) | Event::SoftBreak | Event::HardBreak => {
                scan_links_and_tags(&buf, &mut note);
                buf.clear();
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                if !in_code_block {
                    let url = dest_url.to_string();
                    let kind = classify_link(&url);
                    if let Some(kind) = kind {
                        note.links.push(ParsedLink {
                            target: url.clone(),
                            display: None,
                            kind,
                            anchor: None,
                        });
                    }
                }
            }
            Event::TaskListMarker(done) => {
                note.tasks.push((done, String::new()));
            }
            _ => {}
        }
    }
    if !buf.is_empty() {
        scan_links_and_tags(&buf, &mut note);
    }

    note
}

/// 判断 Markdown 链接类型
/// - http(s):// → External
/// - *.md / 相对路径 → Markdown (库内)
/// - 其它 (mailto:, #anchor 等) → None (忽略)
fn classify_link(url: &str) -> Option<LinkKind> {
    let u = url.trim();
    if u.is_empty() {
        return None;
    }
    if u.starts_with("http://") || u.starts_with("https://") {
        return Some(LinkKind::External);
    }
    if u.starts_with("mailto:") || u.starts_with('#') || u.starts_with("tel:") {
        return None;
    }
    // 库内 md 链接: 相对路径或绝对路径
    if u.ends_with(".md") || u.contains(".md#") {
        return Some(LinkKind::Markdown);
    }
    // 其它相对路径 (可能是附件/图片) → 也记为 Markdown
    if !u.contains("://") {
        return Some(LinkKind::Markdown);
    }
    None
}

/// 扫描一段纯文本中的 wikilink 与行内标签 (已排除代码块)
fn scan_links_and_tags(text: &str, note: &mut ParsedNote) {
    for cap in re_wikilink().captures_iter(text) {
        let is_embed = &cap[1] == "!";
        let inner = &cap[2];
        let (link_part, display) = match inner.split_once('|') {
            Some((l, d)) => (l, Some(d.trim().to_string())),
            None => (inner, None),
        };
        let (target, anchor) = match link_part.split_once('#') {
            Some((t, a)) => (t.trim(), Some(a.trim().to_string())),
            None => (link_part.trim(), None),
        };
        if target.is_empty() && anchor.is_none() {
            continue;
        }
        let kind = if is_embed {
            LinkKind::Embed
        } else if anchor.as_deref().map(|a| a.starts_with('^')).unwrap_or(false) {
            LinkKind::BlockRef
        } else if anchor.is_some() {
            LinkKind::Heading
        } else {
            LinkKind::Wiki
        };
        note.links.push(ParsedLink {
            target: target.to_string(),
            display,
            kind,
            anchor,
        });
    }
    for cap in re_inline_tag().captures_iter(text) {
        let tag = cap[1].to_string();
        if !note.tags.contains(&tag) {
            note.tags.push(tag);
        }
    }
}

// ---------- frontmatter ----------

/// 拆分 frontmatter 与正文
fn split_frontmatter(raw: &str) -> (Vec<&str>, &str) {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return (Vec::new(), raw);
    }
    let after = &trimmed[3..];
    if let Some(end) = after.find("\n---") {
        let fm = &after[..end];
        let body = &after[end + 4..];
        return (fm.lines().collect(), body.trim_start_matches('\n'));
    }
    (Vec::new(), raw)
}

/// 解析 YAML 内联列表: [a, b] 或 a, b 或单值
fn parse_yaml_list(val: &str) -> Vec<String> {
    let v = val.trim();
    let v = v.strip_prefix('[').unwrap_or(v);
    let v = v.strip_suffix(']').unwrap_or(v);
    v.split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------- 便捷: 提取所有链接目标 (供建图/索引) ----------

/// 提取 links_out: 去重后的目标笔记名 (小写, 去 .md)
pub fn extract_link_targets(note: &ParsedNote) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in &note.links {
        if l.target.is_empty() {
            continue;
        }
        // 外链不进入图关系 (只记录, 不建边)
        if l.kind == LinkKind::External {
            continue;
        }
        let name = l.target.trim_end_matches(".md");
        // 去掉 URL 锚点
        let name = name.split('#').next().unwrap_or(name);
        let name = name.rsplit('/').next().unwrap_or(name).to_lowercase();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// 提取外链 (供展示/统计, 不建图关系)
pub fn extract_external_links(note: &ParsedNote) -> Vec<String> {
    note.links
        .iter()
        .filter(|l| l.kind == LinkKind::External)
        .map(|l| l.target.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wikilink() {
        let n = parse_note("[[A]] and [[B|别名]] and ![[C]] and [[D#标题]] and [[E#^blk]]");
        assert_eq!(n.links.len(), 5);
        assert_eq!(n.links[0].kind, LinkKind::Wiki);
        assert_eq!(n.links[1].display.as_deref(), Some("别名"));
        assert_eq!(n.links[2].kind, LinkKind::Embed);
        assert_eq!(n.links[3].kind, LinkKind::Heading);
        assert_eq!(n.links[4].kind, LinkKind::BlockRef);
    }

    #[test]
    fn test_code_block_excluded() {
        let n = parse_note("真链接 [[Real]]\n\n```\n假链接 [[Fake]]\n```\n\n行内 `[[AlsoFake]]`");
        assert_eq!(n.links.len(), 1);
        assert_eq!(n.links[0].target, "Real");
    }

    #[test]
    fn test_frontmatter() {
        let n = parse_note("---\ntags: [a, b]\naliases: [x, y]\n---\n\n正文 [[L]]");
        assert_eq!(n.tags, vec!["a", "b"]);
        assert_eq!(n.aliases, vec!["x", "y"]);
        assert_eq!(n.links.len(), 1);
    }
}

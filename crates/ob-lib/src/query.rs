use regex::Regex;

// query.rs — Obsidian 兼容搜索语法: AND(空格) OR -排除 "短语" path: file: tag: line:()
use crate::vault::Doc;

#[derive(Debug)]
pub enum Node {
    And(Vec<Node>),
    Or(Vec<Node>),
    Not(Box<Node>),
    Word(String),
    Phrase(String),
    Path(String),
    File(String),
    Tag(String),
    Line(Vec<String>),
}

pub fn tokenize(q: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let chars: Vec<char> = q.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            '"' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '"' {
                    j += 1;
                }
                toks.push(format!("\"{}\"", chars[i + 1..j].iter().collect::<String>()));
                i = j + 1;
            }
            '(' | ')' => {
                toks.push(c.to_string());
                i += 1;
            }
            _ => {
                let mut j = i;
                while j < chars.len()
                    && !matches!(chars[j], ' ' | '\t' | '(' | ')' | '"')
                {
                    j += 1;
                }
                toks.push(chars[i..j].iter().collect::<String>());
                i = j;
            }
        }
    }
    toks
}

struct Parser {
    toks: Vec<String>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&String> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<String> {
        let t = self.peek().cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_or(&mut self) -> Option<Node> {
        let mut left = self.parse_and()?;
        while let Some(t) = self.peek() {
            if t.eq_ignore_ascii_case("OR") {
                self.next();
                if let Some(right) = self.parse_and() {
                    left = match left {
                        Node::Or(mut v) => {
                            v.push(right);
                            Node::Or(v)
                        }
                        other => Node::Or(vec![other, right]),
                    };
                }
            } else {
                break;
            }
        }
        Some(left)
    }

    fn parse_and(&mut self) -> Option<Node> {
        let mut kids: Vec<Node> = Vec::new();
        loop {
            match self.peek() {
                None => break,
                Some(t) if t.eq_ignore_ascii_case("OR") || t == ")" => break,
                _ => {}
            }
            if let Some(n) = self.parse_term() {
                kids.push(n);
            }
        }
        match kids.len() {
            0 => None,
            1 => kids.pop(),
            _ => Some(Node::And(kids)),
        }
    }

    fn parse_term(&mut self) -> Option<Node> {
        let t = self.peek()?.clone();
        let (neg, tok) = if let Some(stripped) = t.strip_prefix('-') {
            if stripped.is_empty() {
                self.next();
                (true, self.next()?)
            } else {
                self.next();
                (true, stripped.to_string())
            }
        } else {
            self.next();
            (false, t)
        };
        let node = self.parse_primary(&tok)?;
        Some(if neg { Node::Not(Box::new(node)) } else { node })
    }

    fn parse_primary(&mut self, tok: &str) -> Option<Node> {
        if tok == ")" || tok == "(" {
            if tok == "(" {
                self.next();
                let n = self.parse_or();
                if self.peek().map(|t| t == ")").unwrap_or(false) {
                    self.next();
                }
                return n;
            }
            return None;
        }
        if let Some(idx) = tok.find(':') {
            if !tok.starts_with('"') && idx > 0 {
                let name = tok[..idx].to_lowercase();
                let val = &tok[idx + 1..];
                match name.as_str() {
                    "path" | "file" | "tag" => {
                        return Some(match name.as_str() {
                            "path" => Node::Path(val.trim_matches('"').to_string()),
                            "file" => Node::File(val.trim_matches('"').to_string()),
                            _ => Node::Tag(val.trim_matches('"').trim_start_matches('#').to_string()),
                        });
                    }
                    "line" | "task" | "section" => {
                        let mut words = Vec::new();
                        // tokenizer 会把 ( 和 ) 拆成独立 token: "line:" "(" "w1" "w2" ")"
                        if val == "(" || val.is_empty() {
                            let mut depth = 1usize;
                            loop {
                                match self.next() {
                                    None => break,
                                    Some(t) if t == "(" => depth += 1,
                                    Some(t) if t == ")" => {
                                        depth -= 1;
                                        if depth == 0 { break; }
                                    }
                                    Some(t) => words.push(t),
                                }
                            }
                        } else if val.starts_with('(') {
                            let inner = val.trim_start_matches('(');
                            if !inner.is_empty() { words.push(inner.to_string()); }
                            let mut depth = 1usize;
                            loop {
                                match self.next() {
                                    None => break,
                                    Some(t) if t == "(" => depth += 1,
                                    Some(t) if t == ")" => {
                                        depth -= 1;
                                        if depth == 0 { break; }
                                    }
                                    Some(t) => words.push(t),
                                }
                            }
                        } else {
                            words.push(val.to_string());
                        }
                        words.retain(|w| !w.is_empty());
                        return Some(Node::Line(words));
                    }
                    _ => {}
                }
            }
        }
        if let Some(v) = tok.strip_prefix('"') {
            return Some(Node::Phrase(v.trim_end_matches('"').to_lowercase()));
        }
        if tok.eq_ignore_ascii_case("AND") {
            return None;
        }
        Some(Node::Word(tok.to_lowercase()))
    }
}

/// 模糊子序列匹配: 按顺序包含所有字符 (允许间隔)
pub fn fuzzy_contains(haystack: &str, needle: &str) -> bool {
    let h: Vec<char> = haystack.to_lowercase().chars().collect();
    let n: Vec<char> = needle.to_lowercase().chars().collect();
    if n.is_empty() { return true; }
    let mut hi = 0;
    for nc in &n {
        while hi < h.len() && h[hi] != *nc { hi += 1; }
        if hi >= h.len() { return false; }
        hi += 1;
    }
    true
}

pub fn parse(q: &str) -> Option<Node> {
    let mut p = Parser { toks: tokenize(q), pos: 0 };
    p.parse_or()
}

// ---------- 匹配 ----------
pub fn match_doc(doc: &Doc, node: &Node) -> bool {
    match_doc_mode(doc, node, "exact")
}

/// mode: "exact" 子串 | "fuzzy" 子序列 | "regex" 正则
pub fn match_doc_mode(doc: &Doc, node: &Node, mode: &str) -> bool {
    let word_match = |w: &str| -> bool {
        match mode {
            "fuzzy" => fuzzy_contains(&doc.lower, w),
            "regex" => Regex::new(w).map(|re| re.is_match(&doc.content)).unwrap_or(false),
            _ => doc.lower.contains(&w.to_lowercase()),
        }
    };
    match node {
        Node::Word(w) | Node::Phrase(w) => word_match(w),
        Node::And(kids) => kids.iter().all(|k| match_doc(doc, k)),
        Node::Or(kids) => kids.iter().any(|k| match_doc(doc, k)),
        Node::Not(k) => !match_doc(doc, k),
        Node::Path(v) => doc.rel.to_lowercase().contains(&v.to_lowercase()),
        Node::File(v) => doc.base.to_lowercase().contains(&v.to_lowercase()),
        Node::Tag(t) => doc.has_tag(t),
        Node::Line(words) => {
            let mut lines = doc.lines.clone();
            if lines.is_empty() {
                lines = doc.content.split('\n').map(|s| s.to_string()).collect();
            }
            lines.iter().any(|line| {
                let l = line.to_lowercase();
                words.iter().all(|w| l.contains(&w.to_lowercase()))
            })
        }
    }
}

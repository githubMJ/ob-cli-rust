/// 极简 JSON 解析/序列化 (零依赖)
#[derive(Debug, Clone)]
pub enum Json {
    Null,
    Str(String),
    Num(f64),
    Bool(bool),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}


impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(n) => Some(*n as u64),
            _ => None,
        }
    }
}

pub struct JParser<'a> {
    c: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> JParser<'a> {
    fn skip_ws(&mut self) {
        while matches!(self.c.peek(), Some(c) if c.is_whitespace()) {
            self.c.next();
        }
    }
    fn parse(&mut self) -> Option<Json> {
        self.skip_ws();
        match self.c.peek()? {
            '{' => {
                self.c.next();
                let mut items = Vec::new();
                self.skip_ws();
                if self.c.peek() == Some(&'}') {
                    self.c.next();
                    return Some(Json::Obj(items));
                }
                loop {
                    self.skip_ws();
                    let key = match self.parse() {
                        Some(Json::Str(s)) => s,
                        _ => return None,
                    };
                    self.skip_ws();
                    if self.c.next() != Some(':') {
                        return None;
                    }
                    let val = self.parse()?;
                    items.push((key, val));
                    self.skip_ws();
                    match self.c.next() {
                        Some(',') => continue,
                        Some('}') => break,
                        _ => return None,
                    }
                }
                Some(Json::Obj(items))
            }
            '[' => {
                self.c.next();
                let mut items = Vec::new();
                self.skip_ws();
                if self.c.peek() == Some(&']') {
                    self.c.next();
                    return Some(Json::Arr(items));
                }
                loop {
                    let val = self.parse()?;
                    items.push(val);
                    self.skip_ws();
                    match self.c.next() {
                        Some(',') => continue,
                        Some(']') => break,
                        _ => return None,
                    }
                }
                Some(Json::Arr(items))
            }
            '"' => {
                self.c.next();
                let mut s = String::new();
                loop {
                    match self.c.next()? {
                        '"' => break,
                        '\\' => match self.c.next()? {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            'u' => {
                                let hex: String = self.c.by_ref().take(4).collect();
                                if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                                    if let Some(ch) = char::from_u32(cp) {
                                        s.push(ch);
                                    }
                                }
                            }
                            other => s.push(other),
                        },
                        c => s.push(c),
                    }
                }
                Some(Json::Str(s))
            }
            't' => {
                for _ in 0..3 {
                    self.c.next();
                }
                Some(Json::Bool(true))
            }
            'f' => {
                for _ in 0..4 {
                    self.c.next();
                }
                Some(Json::Bool(false))
            }
            'n' => {
                for _ in 0..3 {
                    self.c.next();
                }
                Some(Json::Null)
            }
            _ => {
                let mut s = String::new();
                while let Some(c) = self.c.peek() {
                    if c.is_ascii_digit() || *c == '-' || *c == '.' || *c == 'e' || *c == '+' {
                        s.push(*c);
                        self.c.next();
                    } else {
                        break;
                    }
                }
                s.parse::<f64>().ok().map(Json::Num)
            }
        }
    }
}

pub fn parse_json(s: &str) -> Option<Json> {
    JParser { c: s.chars().peekable() }.parse()
}

pub fn json_value_to_string(v: &Json) -> String {
    match v {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Num(n) => {
            if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
        Json::Str(s) => format!("\"{}\"", json_escape(s)),
        Json::Arr(items) => format!(
            "[{}]",
            items.iter().map(json_value_to_string).collect::<Vec<_>>().join(",")
        ),
        Json::Obj(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", json_escape(k), json_value_to_string(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}


pub fn json_arr(items: &[String]) -> String {
    format!(
        "[{}]",
        items.iter().map(|s| format!("\"{}\"", json_escape(s))).collect::<Vec<_>>().join(",")
    )
}

// search.rs — 统一检索：图 + 索引（唯一路径）
//
// 设计：一条管线，三个参数控制形态
//   ob-cli search "词"                    → 默认: BM25 + 一跳图传播
//   ob-cli search "词" --hops=0           → 纯 BM25
//   ob-cli search "词" --hops=2           → 两跳传播
//   ob-cli search "词" --discover         → 渐进式结构化输出 (含翻页)
//   ob-cli search "词" --json             → JSON 输出
//
// 已删除: scan 模式 / fuzzy / regex / graph / link / backlinks / links-of 独立模式
// 双链信息统一通过返回字段 links_to / linked_by 提供。
use crate::common::*;
use crate::store::{SearchOpts, Store};
use crate::json::{json_arr, json_escape};
use std::path::PathBuf;
use std::process::exit;

pub fn run_search(ctx: &Ctx) {
    let vp = &ctx.vp;
    let flags = &ctx.flags;
    let pos_args = &ctx.pos_args;
    let has = |f: &str| flags.iter().any(|x| x == f);
    let num = |name: &str, default: usize| -> usize {
        flags
            .iter()
            .find_map(|f| f.strip_prefix(&format!("{name}=")).and_then(|v| v.parse().ok()))
            .unwrap_or(default)
    };

    // ---------- 参数 ----------
    // --note / --read 模式不需要关键词
    let has_note = ctx.kv.iter().any(|(k, _)| k == "note" || k == "read")
        || flags.iter().any(|f| f == "--index-doc");
    let q = if has_note {
        pos_args.get(1).cloned().unwrap_or_default()
    } else {
        pos_args.get(1).unwrap_or_else(|| usage()).clone()
    };
    let hops = num("--hops", 1);
    let limit = num("--limit", 20);
    let page = num("--page", 0);
    let offset = page * limit;
    let as_json = has("--json");
    let discover = has("--discover");
    let with_backlinks = has("--backlinks") || discover;

    // 索引路径: --index-path 优先 (目录→/index.db), 否则 <vault>/index.db
    let db_path: PathBuf = flags
        .iter()
        .find_map(|f| f.strip_prefix("--index-path=").map(PathBuf::from))
        .map(|p| {
            if p.extension().map(|e| e == "db").unwrap_or(false) {
                p
            } else {
                p.join("index.db")
            }
        })
        .unwrap_or_else(|| vp.join("index.db"));

    if !db_path.exists() {
        eprintln!(
            "Error: index not found at {}\nRun: ob-cli index --path {} --index-path {}",
            db_path.display(),
            vp.display(),
            db_path.parent().unwrap_or(&vp).display()
        );
        exit(3);
    }

    let store = match Store::open(&db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: cannot open index at {}: {e}", db_path.display());
            exit(3);
        }
    };

    // ---------- 分支: 总入口索引 (--index-doc) ----------
    if flags.iter().any(|f| f == "--index-doc") {
        match store.master_index() {
            Some((path, content)) => {
                let cap = flags.iter()
                    .find_map(|f| f.strip_prefix("--max-chars=").and_then(|v| v.parse().ok()))
                    .unwrap_or(12000);
                let truncated = content.chars().count() > cap;
                let body: String = if truncated {
                    content.chars().take(cap).collect()
                } else {
                    content.clone()
                };
                if as_json {
                    println!(
                        "{{\"file\":\"{}\",\"truncated\":{},\"content\":\"{}\"}}",
                        json_escape(&path),
                        truncated,
                        json_escape(&body)
                    );
                } else {
                    println!("════════ {path} ════════");
                    print!("{body}");
                }
                return;
            }
            None => {
                eprintln!("Error: no master index document (need frontmatter priority >= 50)");
                exit(5);
            }
        }
    }

    // ---------- 分支: 读取全文 (--read <path> 可多次 / --read a,b,c) ----------
    let reads: Vec<String> = {
        let mut v = Vec::new();
        for (k, val) in ctx.kv.iter() {
            if k == "read" {
                for t in val.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    v.push(t.to_string());
                }
            }
        }
        v
    };
    if !reads.is_empty() {
        match {
            // --max-chars 控制单篇截断
            let mc = flags.iter()
                .find_map(|f| f.strip_prefix("--max-chars=").and_then(|v| v.parse().ok()));
            crate::cmds::docs::read_notes(&store, vp, &reads, as_json, mc)
        } {
            Ok(out) => {
                print!("{out}");
                return;
            }
            Err(e) => {
                eprintln!("Error: {e}");
                exit(4);
            }
        }
    }

    // ---------- 分支 0: 指定笔记 (等价 drill) ----------
    if let Some(note) = ctx.kv.iter().find(|(k, _)| k == "note").map(|(_, v)| v.as_str()) {
        let from = ctx.kv.iter().find(|(k, _)| k == "from").map(|(_, v)| v.as_str());
        let depth = num("--depth", 1);
        match crate::cmds::drill::drill_one(&store, note, from, limit, depth, as_json) {
            Ok(out) => {
                println!("{out}");
                return;
            }
            Err(e) => {
                eprintln!("Error: {e}");
                exit(4);
            }
        }
    }

    // ---------- 分支 1: 渐进式发现 ----------
    if discover {
        // discover 默认每页 3 条 (agent 逐页挑选)
        let dl = if flags.iter().any(|f| f == "--limit" || f.starts_with("--limit=")) {
            limit
        } else {
            3
        };
        match run_discover(&store, &q, hops, page * dl, dl, vp) {
            Ok(d) => {
                if as_json {
                    print_discovery(&q, page, &d);
                } else {
                    print_discovery_text(&q, page, &d);
                }
                return;
            }
            Err(e) => {
                eprintln!("Error: discover failed: {e}");
                exit(3);
            }
        }
    }

    // ---------- 分支 2: 图增强检索 (默认路径) ----------
    let opts = SearchOpts { hops, limit, alpha: 0.3 };
    let hits = match store.search(&q, &opts) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: search failed: {e}");
            exit(3);
        }
    };

    if as_json {
        print_json(&q, hops, &hits, with_backlinks);
    } else {
        print_text(&q, hops, &hits);
    }
}

// ---------- 输出: JSON ----------

fn print_json(q: &str, hops: usize, hits: &[crate::store::Hit], with_bl: bool) {
    let items: Vec<String> = hits
        .iter()
        .map(|h| {
            format!(
                "{{\"file\":\"{}\",\"base\":\"{}\",\"title\":\"{}\",\"snippet\":\"{}\",\"score\":{:.2},\"fts\":{:.2},\"graph\":{:.2},\"links_to\":{},\"links_to_total\":{},\"linked_by\":{},\"linked_by_total\":{}}}",
                json_escape(&h.path),
                json_escape(&h.base),
                json_escape(&h.title),
                json_escape(&h.snippet),
                h.final_score,
                h.fts_score,
                h.graph_score,
                json_arr(&h.links_to),
                h.links_to_total,
                if with_bl { json_arr(&h.linked_by) } else { "[]".to_string() },
                if with_bl { h.linked_by_total } else { 0 }
            )
        })
        .collect();
    println!(
        "{{\"query\":\"{}\",\"hops\":{},\"count\":{},\"results\":[{}]}}",
        json_escape(q),
        hops,
        hits.len(),
        items.join(",")
    );
}

// ---------- 输出: 文本 ----------

fn print_text(q: &str, hops: usize, hits: &[crate::store::Hit]) {
    if hits.is_empty() {
        println!("No matches: {q}");
        return;
    }
    println!("Search [{}] hops={}: {} hits\n", q, hops, hits.len());
    for h in hits {
        println!(
            "  {:.2}  (fts={:.2} graph=+{:.2})  {}",
            h.final_score, h.fts_score, h.graph_score, h.path
        );
        if !h.title.is_empty() {
            println!("         {}", h.title);
        }
    }
}

// ---------- 渐进式发现 ----------

#[derive(Default)]
struct DiscoveryOut {
    entry_notes: Vec<EntryOut>,
    related_notes: Vec<RelatedOut>,
    paths: Vec<String>,
    total_entries: usize,
    offset: usize,
    has_more: bool,
    // 需要向用户澄清时的建议
    clarification: Option<Clarification>,
    // 业务话术扩展记录
    expanded_from: Option<String>,
    expanded_to: Vec<String>,
}

/// 查询过于模糊时的澄清建议
struct Clarification {
    level: String,             // vague / ambiguous / none
    reason: String,
    suggestion: Vec<String>,   // 建议的反问话术
    alternatives: Vec<String>, // 建议的替代查询
}

struct EntryOut {
    title: String,
    path: String,
    file: String,           // 完整路径, 供 --note / --read 直接使用
    reason: String,         // 命中的字段说明
    score: f32,
    snippet: String,
    drill_down: usize,
    drill_up: usize,
    next_note: String,
    next_read: String,
    // ── 判据 (供 agent 决定是否深入) ──
    matched_terms: Vec<String>,   // 实际匹配到的查询词
    missing_terms: Vec<String>,   // 未匹配的查询词 (越大越不相关)
    match_field: String,          // title / content / tags / both
    relevance: String,            // exact / strong / partial / weak
    total_patterns: usize,        // 该笔记摘要长度 (越小越可能是短卡片)
}

struct RelatedOut {
    title: String,
    path: String,
    file: String,
    relation: String,
    source: String,
    hop: usize,
    snippet: String,
}

fn run_discover(
    store: &Store,
    q: &str,
    hops: usize,
    offset: usize,
    limit: usize,
    vp: &std::path::Path,
) -> Result<DiscoveryOut, Box<dyn std::error::Error>> {
    use std::collections::HashSet;

    // 1. 入口: FTS5 全部命中 (用于统计 + 翻页)
    let all = store.search(q, &SearchOpts { hops: 0, limit: 500, alpha: 0.0 })?;
    let total_entries = all.len();
    let page: Vec<_> = all.into_iter().skip(offset).take(limit).collect();

    let mut out = DiscoveryOut {
        total_entries,
        offset,
        has_more: offset + page.len() < total_entries,
        ..Default::default()
    };

    // 业务话术扩展: 仅当原查询无明确领域词时才用扩展
    let domain_marks = [
        "消耗", "成本", "素材", "指标", "库表", "报表", "开户", "转化",
        "归因", "回传", "预算", "出价", "监控", "盯盘", "口径", "分时",
        "小时", "日报", "ROI", "CPA", "CVR", "CTR", "RTA", "UV", "eCPM",
        "t_", "dal_", "ods_", "dim_",
    ];
    let has_domain = domain_marks.iter().any(|d| q.to_lowercase().contains(&d.to_lowercase()));
    let (biz_terms, biz_raw) = crate::store::expand_business_terms(q);
    let biz_hit = biz_raw && !biz_terms.is_empty() && !has_domain;
    // 判据词: 有业务扩展时用扩展词, 否则用原查询分词
    let words: Vec<String> = if biz_hit && !biz_terms.is_empty() {
        biz_terms.clone()
    } else {
        tokenize_query(q)
    };
    let mut seen: HashSet<String> = HashSet::new();

    for h in &page {
        // 命中原因
        let title_l = h.title.to_lowercase();
        let body_l = h.snippet.to_lowercase();
        let mut reasons = Vec::new();
        if words.iter().any(|w| title_l.contains(w)) {
            reasons.push("标题命中");
        }
        if words.iter().all(|w| body_l.contains(w)) {
            reasons.push("正文命中");
        } else if words.iter().any(|w| body_l.contains(w)) {
            reasons.push("正文部分命中");
        }
        if reasons.is_empty() {
            reasons.push("相关命中");
        }

        seen.insert(h.base.to_lowercase());
        // 预取该笔记的下钻能力 (供 agent 判断)
        let base_norm = h.path.trim_end_matches(".md").rsplit('/').next().unwrap_or("").to_lowercase();
        let (dc, uc) = store
            .drill_options(&base_norm, 1)
            .map(|(d, u)| (d.len(), u.len()))
            .unwrap_or((0, 0));

        // 判据: 哪些词匹配了 / 哪些没匹配
        let title_n = normalize_qa(&h.title);
        let snip_n = normalize_qa(&h.snippet);
        let mut matched = Vec::new();
        let mut missing = Vec::new();
        for w in &words {
            let wn = normalize_qa(w);
            if wn.is_empty() {
                continue;
            }
            if title_n.contains(&wn) || snip_n.contains(&wn) {
                matched.push(w.clone());
            } else {
                missing.push(w.clone());
            }
        }
        // 业务扩展场景: 命中任一扩展词即算相关 (不要求全命中)
        if biz_hit && !matched.is_empty() {
            missing.clear();
        }
        let in_title = matched.iter().any(|m| title_n.contains(&normalize_qa(m)));
        let match_field = if in_title && !matched.is_empty() {
            "title".to_string()
        } else if !matched.is_empty() {
            "content".to_string()
        } else {
            "none".to_string()
        };
        // 相关度档位
        let relevance = if biz_hit && in_title {
            "strong"          // 业务扩展 + 标题命中
        } else if biz_hit && !matched.is_empty() {
            "partial"
        } else if missing.is_empty() && in_title {
            "exact"           // 所有词都命中且含标题
        } else if missing.is_empty() {
            "strong"          // 所有词都命中(正文)
        } else if matched.len() * 2 >= words.len() && in_title {
            "partial"         // 一半以上命中且含标题
        } else if !matched.is_empty() {
            "weak"
        } else {
            "none"
        };

        // 入口文档标记
        let is_index_doc = h.path.contains("总入口") || h.base.contains("总入口")
            || h.title.contains("总入口索引");
        out.entry_notes.push(EntryOut {
            title: h.title.clone(),
            path: h.path.clone(),
            file: h.path.clone(),
            reason: reasons.join(" + "),
            score: h.fts_score,
            snippet: h.snippet.clone(),
            drill_down: dc,
            drill_up: uc,
            next_note: h.path.clone(),
            next_read: h.path.clone(),
            matched_terms: matched,
            missing_terms: missing,
            match_field,
            relevance: relevance.to_string(),
            total_patterns: h.snippet.chars().count(),
        });
        if let Some(last) = out.entry_notes.last_mut() {
            if is_index_doc {
                last.reason = "📖 总入口索引（全库导航）".to_string();
                last.relevance = "index".to_string();
            }
        }

        // 2. 关联: 出链 + 反链
        if hops > 0 {
            for dst in &h.links_to {
                let key = dst.to_lowercase();
                if !seen.insert(key.clone()) {
                    continue;
                }
                if let Some(path) = store.path_of(&key) {
                    let title = std::fs::read_to_string(vp.join(&path))
                        .ok()
                        .and_then(|c| {
                            c.lines()
                                .find(|l| l.starts_with("# "))
                                .map(|l| l.trim_start_matches("# ").to_string())
                        })
                        .unwrap_or_else(|| key.clone());
                    out.related_notes.push(RelatedOut {
                        title,
                        path: path.clone(),
                        file: path,
                        relation: "outgoing".into(),
                        source: h.base.clone(),
                        hop: 1,
                        snippet: String::new(),
                    });
                    out.paths.push(format!("{} -> {}", h.base, dst));
                }
            }
            for bl in &h.linked_by {
                let base = bl
                    .trim_end_matches(".md")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_lowercase();
                if !seen.insert(base.clone()) {
                    continue;
                }
                out.related_notes.push(RelatedOut {
                    title: base.clone(),
                    path: bl.clone(),
                    file: bl.clone(),
                    relation: "incoming".into(),
                    source: h.base.clone(),
                    hop: 1,
                    snippet: String::new(),
                });
                out.paths.push(format!("{} <- {}", h.base, base));
            }
        }
    }
    if biz_hit {
        out.expanded_from = Some(q.to_string());
        out.expanded_to = biz_terms;
    }
    // ===== 模糊度评估 =====
    out.clarification = assess_clarity(&out, q);
    Ok(out)
}

/// 评估查询模糊度, 决定是否需要反问用户
fn assess_clarity(d: &DiscoveryOut, query: &str) -> Option<Clarification> {
    let n = d.entry_notes.len();
    if n == 0 {
        return Some(Clarification {
            level: "none".into(),
            reason: format!("知识库中未找到与「{query}」相关的内容"),
            suggestion: vec!["请确认关键词是否正确，或换一种说法描述".into()],
            alternatives: vec![],
        });
    }

    let strong = d.entry_notes.iter().filter(|e| e.relevance == "exact" || e.relevance == "strong").count();
    let weak = d.entry_notes.iter().filter(|e| e.relevance == "weak" || e.relevance == "none").count();
    let total = d.total_entries;

    // 高度模糊: 无强相关 + 大量弱相关 + 候选爆炸
    if strong == 0 && total >= 200 {
        return Some(Clarification {
            level: "vague".into(),
            reason: format!(
                "「{query}」过于宽泛：{total} 条候选中无精确匹配，返回的都是泛相关结果"
            ),
            suggestion: vec![
                "请问你具体想了解哪个业务/指标/报表？".into(),
                "可以告诉我：① 业务域（证券/理财通）② 关注点（消耗/成本/素材/归因）".into(),
            ],
            alternatives: suggest_alternatives(query),
        });
    }

    // 部分模糊: 有候选但强相关很少
    if strong == 0 && weak >= n.saturating_sub(1) && total >= 50 {
        return Some(Clarification {
            level: "ambiguous".into(),
            reason: format!(
                "「{query}」含义不明确：{total} 条候选均为弱匹配（命中词不全）"
            ),
            suggestion: vec![
                "请补充更具体的关键词，例如指标名、表名或业务场景".into(),
            ],
            alternatives: suggest_alternatives(query),
        });
    }

    None
}

/// 基于查询词给出替代查询建议
fn suggest_alternatives(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let pool = [
        ("投放", "字节 消耗"),
        ("效果", "开户成本"),
        ("数据", "指标 消耗"),
        ("报表", "证券素材报表"),
        ("成本", "开户成本"),
        ("素材", "素材效果"),
        ("归因", "归因分析"),
        ("监控", "盯盘 告警"),
    ];
    for (k, alt) in pool.iter() {
        if query.contains(k) && !out.contains(&alt.to_string()) {
            out.push(alt.to_string());
        }
    }
    out.truncate(3);
    out
}

fn print_discovery(q: &str, page: usize, d: &DiscoveryOut) {
    println!("{{");
    println!("  \"query\": \"{}\",", json_escape(q));
    println!("  \"page\": {},", page);
    println!("  \"page_size\": {},", d.entry_notes.len());
    println!("  \"offset\": {},", d.offset);
    println!("  \"total_entries\": {},", d.total_entries);
    println!("  \"has_more\": {},", d.has_more);
    // 业务话术扩展说明
    if let Some(from) = &d.expanded_from {
        println!("  \"expanded_from\": \"{}\",", json_escape(from));
        println!("  \"expanded_to\": {},", json_arr(&d.expanded_to));
    }
    // 澄清信号
    if let Some(c) = &d.clarification {
        println!("  \"needs_clarification\": true,");
        println!("  \"clarification\": {{");
        println!("    \"level\": \"{}\",", c.level);
        println!("    \"reason\": \"{}\",", json_escape(&c.reason));
        println!("    \"ask_user\": [");
        for (i, s_) in c.suggestion.iter().enumerate() {
            let comma = if i + 1 < c.suggestion.len() { "," } else { "" };
            println!("      \"{}\"{}", json_escape(s_), comma);
        }
        println!("    ],");
        println!("    \"try_queries\": [");
        for (i, a) in c.alternatives.iter().enumerate() {
            let comma = if i + 1 < c.alternatives.len() { "," } else { "" };
            println!("      \"{}\"{}", json_escape(a), comma);
        }
        println!("    ]");
        println!("  }},");
    } else {
        println!("  \"needs_clarification\": false,");
    }
    println!("  \"entry_notes\": [");
    for (i, e) in d.entry_notes.iter().enumerate() {
        let comma = if i + 1 < d.entry_notes.len() { "," } else { "" };
        println!(
            "    {{\"title\": \"{}\", \"file\": \"{}\", \"reason\": \"{}\", \"score\": {:.2}, \"relevance\": \"{}\", \"match_field\": \"{}\", \"matched\": {},\"missing\": {}, \"drill_down\": {}, \"drill_up\": {}, \"snippet\": \"{}\"}}{}",
            json_escape(&e.title),
            json_escape(&e.file),
            json_escape(&e.reason),
            e.score,
            e.relevance,
            e.match_field,
            json_arr(&e.matched_terms),
            json_arr(&e.missing_terms),
            e.drill_down,
            e.drill_up,
            json_escape(&e.snippet),
            comma
        );
    }
    println!("  ],");
    println!("  \"related_notes\": [");
    for (i, r) in d.related_notes.iter().take(30).enumerate() {
        let comma = if i + 1 < d.related_notes.len().min(30) { "," } else { "" };
        println!(
            "    {{\"title\": \"{}\", \"path\": \"{}\", \"file\": \"{}\", \"relation\": \"{}\", \"source\": \"{}\", \"hop\": {}, \"snippet\": \"{}\"}}{}",
            json_escape(&r.title),
            json_escape(&r.path),
            json_escape(&r.path),
            r.relation,
            json_escape(&r.source),
            r.hop,
            json_escape(&r.snippet),
            comma
        );
    }
    println!("  ],");
    println!("  \"paths\": [");
    for (i, p) in d.paths.iter().take(20).enumerate() {
        let comma = if i + 1 < d.paths.len().min(20) { "," } else { "" };
        println!("    \"{}\"{}", json_escape(p), comma);
    }
    println!("  ]");
    println!("}}");
}

/// 查询分词: 优先抽取领域关键词, 再补 bigram
fn tokenize_query(q: &str) -> Vec<String> {
    // 领域词表: 命中则作为完整 token (避免跨词 bigram)
    let domain: &[&str] = &[
        "证券", "消耗", "投放", "素材", "开户", "成本", "消耗", "渠道", "效果",
        "归因", "回传", "预算", "出价", "监控", "盯盘", "报表", "指标", "口径",
        "库表", "数据", "时效", "实时", "离线", "账户", "计划", "创意", "文案",
        "素材", "CTR", "CVR", "CPA", "ROI", "RTA", "AMS", "UV", "eCPM",
    ];
    let mut out: Vec<String> = Vec::new();
    // 1. 先扫领域词 (长的优先)
    let mut sorted = domain.to_vec();
    sorted.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    let mut rest = q.to_string();
    for w in &sorted {
        if rest.contains(w) {
            out.push(w.to_string());
            rest = rest.replace(w, " ");
        }
    }
    // 2. 剩余部分按 bigram (仅中文连续段)
    for seg in rest.split_whitespace() {
        let chars: Vec<char> = seg.chars().filter(|c| c.is_alphanumeric()).collect();
        if chars.is_empty() {
            continue;
        }
        let has_cjk = chars.iter().any(|c| !c.is_ascii());
        if has_cjk && chars.len() > 1 {
            // 只在连续中文段内做 bigram
            let mut run: Vec<char> = Vec::new();
            let mut flush = |run: &mut Vec<char>, out: &mut Vec<String>| {
                if run.len() >= 2 {
                    for w in run.windows(2) {
                        out.push(w.iter().collect::<String>());
                    }
                } else if run.len() == 1 {
                    out.push(run[0].to_string());
                }
                run.clear();
            };
            for c in chars {
                if c.is_ascii() {
                    flush(&mut run, &mut out);
                    out.push(c.to_string());
                } else {
                    run.push(c);
                }
            }
            flush(&mut run, &mut out);
        } else if chars.len() == 1 {
            out.push(chars[0].to_string());
        } else {
            out.push(chars.iter().collect());
        }
    }
    out.dedup();
    out
}

fn tokenize_query_old(q: &str) -> Vec<String> {
    let stop: &[&str] = &[
        "的", "了", "是", "和", "与", "在", "有", "吗", "呢", "吧", "啊", "么",
        "怎", "样", "如", "何", "什", "什", "哪", "个", "些", "为", "什",
        "我", "你", "他", "她", "它", "们", "这", "那", "请问", "帮", "看",
    ];
    let mut out = Vec::new();
    for w in q.split_whitespace() {
        let has_cjk = w.chars().any(|c| c.is_alphabetic() && !c.is_ascii());
        if has_cjk {
            let chars: Vec<char> = w.chars().filter(|c| c.is_alphanumeric()).collect();
            if chars.len() == 1 {
                if !stop.contains(&&*chars[0].to_string()) {
                    out.push(chars[0].to_string());
                }
            } else {
                // 生成 bigram, 但过滤明显的跨词噪声 (含停用字在边界的)
                for win in chars.windows(2) {
                    let t: String = win.iter().collect();
                    if stop.contains(&&*t) {
                        continue;
                    }
                    // 过滤两端都是停用字/虚词的组合
                    let c0 = win[0].to_string();
                    let c1 = win[1].to_string();
                    if stop.contains(&&*c0) || stop.contains(&&*c1) {
                        continue;
                    }
                    out.push(t);
                }
            }
        } else if !w.is_empty() {
            out.push(w.to_lowercase());
        }
    }
    out
}

/// 文本归一化 (去装饰符, 小写)
fn normalize_qa(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '%')
        .collect::<String>()
        .to_lowercase()
}

/// discover 人类可读输出: 每页 3 条, 每条带下钻/读取命令
fn print_discovery_text(q: &str, page: usize, d: &DiscoveryOut) {
    println!("🔍 {}  (第 {} 页, 共 {} 条候选{})\n",
        q, page + 1, d.total_entries,
        if d.has_more { ", 有更多" } else { "" });
    // 业务扩展提示
    if let Some(from) = &d.expanded_from {
        println!("  💡 业务话术扩展: \"{}\" → {}", from, d.expanded_to.join(" / "));
        println!();
    }

    // 澄清提示 (最显眼位置)
    if let Some(c) = &d.clarification {
        let icon = if c.level == "none" { "❌" } else { "⚠️" };
        println!("  {} 需要澄清: {}", icon, c.reason);
        println!();
        println!("  建议反问用户:");
        for s_ in &c.suggestion {
            println!("    · {}", s_);
        }
        if !c.alternatives.is_empty() {
            println!("  可以试试这些查询:");
            for a in &c.alternatives {
                println!("    · ob-cli search \"{}\"", a);
            }
        }
        println!();
        println!("  ─── 以下为模糊匹配结果，仅供参考 ───");
        println!();
    }

    for (i, e) in d.entry_notes.iter().enumerate() {
        let rel_mark = match e.relevance.as_str() {
            "index" => "📖 总入口",
            "exact" => "🎯 精确",
            "strong" => "✅ 强相关",
            "partial" => "🟡 部分相关",
            "weak" => "⚠️ 弱相关",
            _ => "❌ 不相关",
        };
        println!("  [{}] {}   ({})", i + 1, e.title, rel_mark);
        println!("      file:   {}", e.file);
        println!("      匹配:   {} | 字段: {}", e.reason, e.match_field);
        println!("      命中词: {:?}", e.matched_terms);
        if e.relevance == "index" {
            println!("      用途:   全库导航索引，可 read 后按目录直达目标卡");
        } else if !e.missing_terms.is_empty() {
            println!("      缺词:   {:?}  ← 缺失越多越不相关", e.missing_terms);
        }
        println!("      关联:   下钻 {} / 上溯 {}", e.drill_down, e.drill_up);
        let sn: String = e.snippet.chars().take(70).collect();
        if !sn.is_empty() {
            println!("      摘要:   {}", sn);
        }
        println!("      ▸ 看关联: ob-cli search --note \"{}\" --index-path <dir>", e.next_note);
        println!("      ▸ 读全文: ob-cli search --read \"{}\" --index-path <dir>", e.next_read);
        println!();
    }
    if let Some((i, e)) = d.entry_notes.first().map(|e| (&1, e)) {
        let _ = i;
        let _ = e;
    }
    if !d.related_notes.is_empty() {
        println!("  ── 关联笔记 ({} 条) ──", d.related_notes.len());
        for r in d.related_notes.iter().take(5) {
            println!("      {} {}  ← {}", if r.relation == "incoming" { "◄" } else { "→" }, r.title, r.source);
            println!("         file: {}", r.file);
        }
    }
    if d.has_more {
        println!("\n  下一页: 加 --page={}", page + 1);
    }
}

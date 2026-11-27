// store.rs — SQLite FTS5 存储层 (替代 Tantivy)
//
// 为什么用 SQLite 而不是 Tantivy:
//   ✅ 单文件 .db, 跨平台通用 (可预构建、可跨目录迁移)
//   ✅ 增量更新 (UPDATE 单条, 无需全量重建)
//   ✅ 无平台相关的二进制格式问题
//
// schema:
//   notes     (id, path, base, title, content, mtime)
//   links     (src_id, dst_name, kind)          -- 出链, 用于图检索
//   aliases   (alias, note_id)                  -- Obsidian aliases
//   tags      (note_id, tag)
//   notes_fts (FTS5 虚表: title, content, tags)  -- 全文检索
use crate::vault::Doc;
use rusqlite::{params, Connection, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// 检索选项
#[derive(Debug, Clone)]
pub struct SearchOpts {
    pub hops: usize,      // 0=纯 FTS5, 1=一跳图传播
    pub limit: usize,
    pub alpha: f32,       // 图传播权重
}

impl Default for SearchOpts {
    fn default() -> Self {
        Self { hops: 1, limit: 20, alpha: 0.3 }
    }
}

/// 检索结果
#[derive(Debug, Clone)]
pub struct Hit {
    pub path: String,
    pub base: String,
    pub title: String,
    pub snippet: String,
    pub fts_score: f32,
    pub graph_score: f32,
    pub final_score: f32,
    pub links_to: Vec<String>,
    pub linked_by: Vec<String>,
    pub links_to_total: usize,   // 出链总数 (可能 > links_to.len())
    pub linked_by_total: usize,  // 反链总数
}

pub struct Store {
    conn: Connection,
}

/// 下钻候选项
#[derive(Debug, Clone)]
pub struct DrillItem {
    pub title: String,
    pub path: String,
    pub base: String,
    pub excerpt: String,  // 内容摘要 (300字)
    pub why: String,      // 为什么该走这条边 (引用上下文)
}

/// 内容摘要: 跳过 frontmatter 与标题, 取前 N 字
fn excerpt(content: &str) -> String {
    let body = strip_fm(content);
    let text: String = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() > 300 {
        format!("{}...", text.chars().take(300).collect::<String>())
    } else {
        text
    }
}

impl Store {
    /// 打开或创建索引 (单文件 .db)
    pub fn open(db_path: &Path) -> Result<Self> {
        // 只对目录做 create_dir_all (传文件路径时 parent 已存在, 无需创建)
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).ok();
            }
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS notes (
                id       INTEGER PRIMARY KEY,
                path     TEXT NOT NULL UNIQUE,
                base     TEXT NOT NULL,
                title    TEXT NOT NULL DEFAULT '',
                content  TEXT NOT NULL,
                mtime    INTEGER NOT NULL DEFAULT 0,
                priority INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_notes_base ON notes(base);
            CREATE INDEX IF NOT EXISTS idx_notes_path ON notes(path);

            CREATE TABLE IF NOT EXISTS links (
                src_id   INTEGER NOT NULL,
                dst_name TEXT NOT NULL,
                kind     TEXT NOT NULL DEFAULT 'wiki'
            );
            CREATE INDEX IF NOT EXISTS idx_links_src ON links(src_id);
            CREATE INDEX IF NOT EXISTS idx_links_dst ON links(dst_name);

            CREATE TABLE IF NOT EXISTS aliases (
                alias    TEXT NOT NULL,
                note_id  INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_aliases ON aliases(alias);

            CREATE TABLE IF NOT EXISTS tags (
                note_id  INTEGER NOT NULL,
                tag      TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_tags ON tags(tag);

            -- FTS5 全文索引 (内容与 notes 同步)
            CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
                title, content, tags,
                content='',                 -- 只存索引, 不存原文
                contentless_delete=1,       -- 支持按 rowid 删除 (SQLite 3.43+)
                tokenize='unicode61'
            );
            "#,
        )?;
        Ok(Self { conn })
    }

    /// 全量重建 (清空 + 重写)
    pub fn rebuild(&mut self, docs: &[Doc]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM notes", [])?;
        tx.execute("DELETE FROM links", [])?;
        tx.execute("DELETE FROM aliases", [])?;
        tx.execute("DELETE FROM tags", [])?;
        // contentless FTS5 不支持 DELETE FROM, 用 drop+recreate
        tx.execute("DROP TABLE IF EXISTS notes_fts", [])?;
        tx.execute(
            "CREATE VIRTUAL TABLE notes_fts USING fts5(title, content, tags, content='', contentless_delete=1, tokenize='unicode61')",
            [],
        )?;

        for d in docs {
            let prio = extract_priority(&d.content);
            tx.execute(
                "INSERT INTO notes (path, base, title, content, mtime, priority) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![d.rel, d.base, extract_title(&d.content, &d.base), d.content, d.mtime, prio],
            )?;
            let id = tx.last_insert_rowid();
            for link in &d.links_out {
                tx.execute(
                    "INSERT INTO links (src_id, dst_name) VALUES (?1, ?2)",
                    params![id, link],
                )?;
            }
            for a in &d.aliases {
                tx.execute(
                    "INSERT INTO aliases (alias, note_id) VALUES (?1, ?2)",
                    params![a.to_lowercase(), id],
                )?;
            }
            for t in &d.tags {
                tx.execute("INSERT INTO tags (note_id, tag) VALUES (?1, ?2)", params![id, t])?;
            }
            let tags_joined = d.tags.join(" ");
            tx.execute(
                "INSERT INTO notes_fts (rowid, title, content, tags) VALUES (?1, ?2, ?3, ?4)",
                params![
                    id,
                    segment_cjk(&extract_title(&d.content, &d.base)),
                    segment_cjk(&d.content),
                    segment_cjk(&tags_joined)
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 增量更新单个文件 (按 path 判断新增/更新/删除)
    pub fn upsert(&mut self, doc: &Doc) -> Result<()> {
        let tx = self.conn.transaction()?;
        // 删除旧记录
        if let Ok(old_id) = tx.query_row(
            "SELECT id FROM notes WHERE path = ?1",
            params![doc.rel],
            |r| r.get::<_, i64>(0),
        ) {
            tx.execute("DELETE FROM links WHERE src_id = ?1", params![old_id])?;
            tx.execute("DELETE FROM aliases WHERE note_id = ?1", params![old_id])?;
            tx.execute("DELETE FROM tags WHERE note_id = ?1", params![old_id])?;
            delete_fts_row(&tx, old_id)?;
            tx.execute("DELETE FROM notes WHERE id = ?1", params![old_id])?;
        }
        let title = extract_title(&doc.content, &doc.base);
        let prio = extract_priority(&doc.content);
        tx.execute(
            "INSERT INTO notes (path, base, title, content, mtime, priority) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![doc.rel, doc.base, title, doc.content, doc.mtime, prio],
        )?;
        let id = tx.last_insert_rowid();
        for link in &doc.links_out {
            tx.execute("INSERT INTO links (src_id, dst_name) VALUES (?1, ?2)", params![id, link])?;
        }
        for a in &doc.aliases {
            tx.execute(
                "INSERT INTO aliases (alias, note_id) VALUES (?1, ?2)",
                params![a.to_lowercase(), id],
            )?;
        }
        for t in &doc.tags {
            tx.execute("INSERT INTO tags (note_id, tag) VALUES (?1, ?2)", params![id, t])?;
        }
        tx.execute(
            "INSERT INTO notes_fts (rowid, title, content, tags) VALUES (?1, ?2, ?3, ?4)",
            params![id, segment_cjk(&title), segment_cjk(&doc.content), segment_cjk(&doc.tags.join(" "))],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 删除笔记
    pub fn delete(&mut self, path: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        if let Ok(id) = tx.query_row("SELECT id FROM notes WHERE path = ?1", params![path], |r| {
            r.get::<_, i64>(0)
        }) {
            tx.execute("DELETE FROM links WHERE src_id = ?1", params![id])?;
            tx.execute("DELETE FROM aliases WHERE note_id = ?1", params![id])?;
            tx.execute("DELETE FROM tags WHERE note_id = ?1", params![id])?;
            delete_fts_row(&tx, id)?;
            tx.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 检索: FTS5 (BM25) + 图传播
    pub fn search(&self, query: &str, opts: &SearchOpts) -> Result<Vec<Hit>> {
        // 1. FTS5 BM25 检索 (候选池)
        let pool = (opts.limit * 5).max(100);
        let mut stmt = self.conn.prepare(
            r#"
            SELECT n.id, n.path, n.base, n.title, n.content,
                   bm25(notes_fts) AS score
            FROM notes_fts
            JOIN notes n ON n.id = notes_fts.rowid
            WHERE notes_fts MATCH ?1 AND n.priority = 0
            ORDER BY score
            LIMIT ?2
            "#,
        )?;
        // 业务话术扩展: 仅当原查询缺少明确领域词时才扩展
        // 规则: 原查询已含领域词(消耗/成本/素材/指标/表名等) → 不扩展, 保留原意
        let domain_marks = [
            "消耗", "成本", "素材", "指标", "库表", "报表", "开户", "转化",
            "归因", "回传", "预算", "出价", "监控", "盯盘", "口径", "分时",
            "小时", "日报", "ROI", "CPA", "CVR", "CTR", "RTA", "UV", "eCPM",
            "t_", "dal_", "ods_", "dim_",
        ];
        let has_domain = domain_marks.iter().any(|d| query.to_lowercase().contains(&d.to_lowercase()));
        let (biz_terms, biz_hit) = expand_business_terms(query);
        let use_expand = biz_hit && !biz_terms.is_empty() && !has_domain;
        let effective_query: String = if use_expand {
            biz_terms.join(" ")
        } else {
            query.to_string()
        };

        // 三级降级: AND 全词 → 去停用词 → 区分度最高的 OR
        let mut rows: Vec<(i64, String, String, String, String, f64)> = Vec::new();
        for attempt in build_fts_query_ladder(&effective_query) {
            // __SELECTIVE_AND__ / __SELECTIVE_OR__: 按 df 选最有区分度的词
            let is_and_marker = attempt.starts_with("__SELECTIVE_AND__");
            let sel_rest = attempt
                .strip_prefix("__SELECTIVE_AND__")
                .or_else(|| attempt.strip_prefix("__SELECTIVE_OR__"));
            let attempt = if let Some(rest) = sel_rest {
                let is_and = is_and_marker;
                let words: Vec<&str> = rest.split(',').filter(|s| !s.is_empty()).collect();
                if words.is_empty() {
                    continue;
                }
                let mut with_df: Vec<(&str, i64)> = Vec::new();
                for w in &words {
                    let df: i64 = self.conn
                        .query_row("SELECT COUNT(*) FROM notes_fts WHERE notes_fts MATCH ?1",
                                   params![w], |r| r.get(0))
                        .unwrap_or(0);
                    with_df.push((w, df));
                }
                // 噪声词 + 单字 + 虚词组合 不参与 OR
                let noise: &[&str] = &[
                    "昨天", "今天", "明天", "前天", "上周", "本周", "上月", "本月",
                    "最近", "现在", "当前", "目前", "如何", "怎么", "什么", "哪些",
                    "么样", "怎样", "哪些", "哪个", "这个", "那个", "我们", "你们",
                ];
                let mut cands: Vec<(&str, i64)> = with_df
                    .into_iter()
                    .filter(|(w, _)| {
                        // 过滤: 噪声词 / 单字 / 纯虚词组合
                        if noise.contains(w) || w.chars().count() < 2 {
                            return false;
                        }
                        // 至少含一个有意义的实字
                        !w.chars().all(|c| "的了是和与在有吗呢吧啊么怎样如何什么哪些哪个".contains(c))
                    })
                    .collect();
                if cands.is_empty() {
                    continue;
                }
                // 排序: 罕见优先 (df 小), df 相同则长词优先
                cands.sort_by(|a, b| a.1.cmp(&b.1).then(b.0.len().cmp(&a.0.len())));
                let picked: Vec<&str> = cands.iter().take(3).map(|(w, _)| *w).collect();
                picked.join(if is_and { " " } else { " OR " })
            } else {
                attempt
            };
            rows = stmt
                .query_map(params![attempt, pool as i64], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
                })?
                .filter_map(|r| r.ok())
                .collect();
            if !rows.is_empty() {
                break;
            }
        }

        if rows.is_empty() {
            return Ok(Vec::new());
        }

        // 标题加权: 标题精确/前缀命中的文档额外加分 (解决短卡片被长文档碾压)
        let q_norm = normalize_text(query);
        let mut title_boost: HashMap<i64, f32> = HashMap::new();
        {
            let mut tstmt = self.conn.prepare("SELECT id, title, base FROM notes")?;
            let all: Vec<(i64, String, String)> = tstmt
                .query_map([], |r| {
                    Ok::<(i64, String, String), rusqlite::Error>((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();
            for (id, title, base) in all {
                let tn = normalize_text(&title);
                let bn = normalize_text(&base);
                // -- 入口索引文档特殊处理 --
                // 完全相等 → 高分; 前缀/包含 → 递减
                // 标题权重需与长文档的 BM25 分抗衡 (长文档常有 10-30 分)
                // 规则: 完全相等 > 以查询开头 > 包含
                let score = if tn == q_norm || bn == q_norm {
                    40.0
                } else if tn.starts_with(&q_norm) || bn.starts_with(&q_norm) {
                    24.0
                } else if tn.contains(&q_norm) || bn.contains(&q_norm) {
                    12.0
                } else {
                    0.0
                };
                if score > 0.0 {
                    title_boost.insert(id, score);
                }
            }
        }

        // 2. 收集 id -> (index, fts_score)。bM25 越小越相关，转为越大越好
        let mut scores: HashMap<i64, (usize, f32)> = HashMap::new();
        for (i, (id, _, _, _, _, s)) in rows.iter().enumerate() {
            // bm25 返回负数 (越负越相关), 取反归一
            scores.insert(*id, (i, (-*s as f32).max(0.01)));
        }
        // 业务话术扩展时, 给「指标 ·」类权威卡片额外加成
        if use_expand {
            for (id, boost) in title_boost.iter_mut() {
                if let Ok(title) = self.conn.query_row(
                    "SELECT title FROM notes WHERE id = ?1",
                    params![*id],
                    |r| r.get::<_, String>(0),
                ) {
                    if title.starts_with("指标 ·") || title.starts_with("指标·") {
                        *boost += 8.0;   // 指标卡优先
                    }
                }
            }
        }

        // 标题加权并入
        for (id, boost) in &title_boost {
            scores.entry(*id).and_modify(|(_, s)| *s += boost).or_insert((usize::MAX, *boost));
        }

        // 3. 图传播
        let mut graph_boost: HashMap<i64, f32> = HashMap::new();
        if opts.hops > 0 {
            let mut frontier: Vec<(i64, f32, usize)> = rows
                .iter()
                .take(opts.limit)
                .map(|(id, _, _, _, _, s)| (*id, (-*s as f32).max(0.01), 0))
                .collect();
            let mut visited: HashSet<(i64, usize)> = HashSet::new();

            while !frontier.is_empty() {
                let mut next = Vec::new();
                for (id, base_score, hop) in frontier {
                    if hop >= opts.hops || !visited.insert((id, hop)) {
                        continue;
                    }
                    let nh = hop + 1;
                    let decay = (nh as f32).powf(1.5);
                    // 出链 + 入链
                    let neighbors = self.neighbors(id)?;
                    let degree = neighbors.len().max(1) as f32;
                    let contrib = opts.alpha * base_score / (decay * degree.sqrt());
                    for nb in neighbors {
                        *graph_boost.entry(nb).or_insert(0.0) += contrib;
                        next.push((nb, base_score, nh));
                    }
                }
                frontier = next;
            }
        }

        // 4. 组装结果
        let mut hits: Vec<Hit> = Vec::new();
        let mut all_ids: Vec<i64> = scores.keys().copied().collect();
        let seen: HashSet<i64> = all_ids.iter().copied().collect();
        // 图传播引入的节点
        for id in graph_boost.keys() {
            if !seen.contains(id) {
                all_ids.push(*id);
            }
        }
        // 标题精确匹配的节点 (即使未被 FTS 召回)
        for (id, boost) in &title_boost {
            if *boost >= 12.0 && !all_ids.contains(id) {
                all_ids.push(*id);
            }
        }

        for id in all_ids {
            let (_, path, base, title, content, _) = self
                .conn
                .query_row(
                    "SELECT id, path, base, title, content, priority FROM notes WHERE id = ?1",
                    params![id],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, f64>(5)?,
                        ))
                    },
                )
                .unwrap_or((0, String::new(), String::new(), String::new(), String::new(), 0.0));

            let fts = scores.get(&id).map(|(_, s)| *s).unwrap_or(0.0);
            // 标题命中但不在 FTS 结果里 → 用 title_boost 作为基分
            let fts = if fts < 0.01 {
                *title_boost.get(&id).unwrap_or(&0.0) * 0.8
            } else {
                fts
            };
            let gb = *graph_boost.get(&id).unwrap_or(&0.0);
            // 枢纽惩罚: graph 不超过 3x 词法分
            let cap = 3.0 * fts.max(8.0);
            let gb = gb.min(cap);
            let final_score = fts + gb;

            hits.push(Hit {
                path,
                base,
                title,
                snippet: make_snippet(&content, query),
                fts_score: fts,
                graph_score: gb,
                final_score,
                links_to: Vec::new(),
                linked_by: Vec::new(),
                links_to_total: 0,
                linked_by_total: 0,
            });
        }

        hits.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(opts.limit);

        // 5. 填充双链信息 (每项最多 20 条, 附总数)
        for h in hits.iter_mut() {
            h.links_to = self.links_of(&h.base)?;
            h.linked_by = self.backlinks(&h.base)?;
            h.links_to_total = self.links_of_count(&h.base).unwrap_or(h.links_to.len());
            h.linked_by_total = self.backlink_count(&h.base).unwrap_or(h.linked_by.len());
        }
        Ok(hits)
    }

    /// 出链: 该笔记引用了谁 (返回 base 名, 按目标被引次数排序, 最多 limit 条)
    pub fn links_of(&self, base: &str) -> Result<Vec<String>> {
        self.links_of_limit(base, 20)
    }

    /// 出链 (带条数限制): 目标按全局被引次数降序 (重要节点优先)
    pub fn links_of_limit(&self, base: &str, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT l.dst_name,
                   (SELECT COUNT(*) FROM links l2 WHERE l2.dst_name = l.dst_name) AS pop
            FROM links l JOIN notes n ON n.id = l.src_id
            WHERE n.base = ?1
            ORDER BY pop DESC
            LIMIT ?2
            "#,
        )?;
        let v = stmt
            .query_map(params![base.to_lowercase(), limit as i64], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    }

    /// 反链: 谁引用了该笔记 (返回 path, 按来源重要性排序, 最多 limit 条)
    pub fn backlinks(&self, base: &str) -> Result<Vec<String>> {
        self.backlinks_limit(base, 20)
    }

    /// 反链 (带条数限制): 来源按自身出链数降序 (枢纽页优先)
    pub fn backlinks_limit(&self, base: &str, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT n.path,
                   (SELECT COUNT(*) FROM links l2 WHERE l2.src_id = n.id) AS degree
            FROM links l JOIN notes n ON n.id = l.src_id
            WHERE l.dst_name = ?1
            ORDER BY degree DESC
            LIMIT ?2
            "#,
        )?;
        let v = stmt
            .query_map(params![base.to_lowercase(), limit as i64], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    }

    /// 反链总数 (用于统计, 提示 LLM 还有多少未展示)
    pub fn backlink_count(&self, base: &str) -> Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM links WHERE dst_name = ?1",
                params![base.to_lowercase()],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as usize)
    }

    /// 出链总数
    pub fn links_of_count(&self, base: &str) -> Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM links l JOIN notes n ON n.id = l.src_id WHERE n.base = ?1",
                params![base.to_lowercase()],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as usize)
    }

    /// 邻居 (出链 + 入链的 note id)
    fn neighbors(&self, id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT n2.id FROM links l
            JOIN notes n2 ON n2.base = l.dst_name
            WHERE l.src_id = ?1
            UNION
            SELECT l2.src_id FROM links l2
            JOIN notes n1 ON n1.id = l2.src_id
            WHERE l2.dst_name = (SELECT base FROM notes WHERE id = ?1)
            "#,
        )?;
        let v = stmt
            .query_map(params![id], |r| r.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    }

    // ---------- 多轮下钻 ----------

    /// 读取笔记内容 (用于 drill 返回摘要)
    pub fn content_of(&self, base: &str) -> Option<(String, String, String)> {
        self.conn
            .query_row(
                "SELECT path, title, content FROM notes WHERE base = ?1 LIMIT 1",
                params![base.to_lowercase()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()
    }

    /// 按 path 读取笔记
    pub fn content_by_path(&self, path: &str) -> Option<(String, String, String)> {
        self.conn
            .query_row(
                "SELECT path, title, content FROM notes WHERE path = ?1 LIMIT 1",
                params![path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()
    }

    /// 按路径后缀查找 (容错: 忽略 knowledge/ 等前缀差异)
    pub fn find_by_path_suffix(&self, suffix: &str) -> Option<(String, String, String)> {
        self.conn
            .query_row(
                "SELECT path, title, content FROM notes WHERE path LIKE ?1 LIMIT 1",
                params![format!("%{suffix}")],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()
    }

    /// 提取引用上下文: 在 src 的内容中找到提到 target 的那一行
    pub fn citation_context(&self, src_base: &str, target_name: &str) -> Option<String> {
        let (_, _, content) = self.content_of(src_base)?;
        let target = target_name.to_lowercase();
        for line in content.lines() {
            let ll = line.to_lowercase();
            // 匹配: [[target]] / [[target|alias]] / [[path/target]] / [text](target.md)
            let hit = ll.contains(&format!("[[{}]]", target))
                || ll.contains(&format!("[[{}|", target))
                || ll.contains(&format!("/{}]]", target))
                || ll.contains(&format!("/{}|", target))
                || (ll.contains(&format!("({}.md", target)) || ll.contains(&format!("/{}.md", target)));
            if hit {
                let t = line.trim();
                // 去掉行首的列表符号
                let t = t.trim_start_matches('-').trim();
                return Some(if t.chars().count() > 200 {
                    format!("{}...", t.chars().take(200).collect::<String>())
                } else {
                    t.to_string()
                });
            }
        }
        None
    }

    /// 候选下钻方向: 出链 (down) + 反链 (up)
    pub fn drill_options(&self, base: &str, limit: usize) -> Result<(Vec<DrillItem>, Vec<DrillItem>)> {
        self.drill_options_ex(base, limit, None)
    }

    /// exclude_path: 排除自身文件路径（同名不同路径的笔记不互相污染）
    pub fn drill_options_ex(&self, base: &str, limit: usize, exclude_path: Option<&str>) -> Result<(Vec<DrillItem>, Vec<DrillItem>)> {
        let b = base.to_lowercase();
        // 出链: 我引用了谁 (排除自身)
        let mut down = Vec::new();
        for dst in self.links_of(&b)?.into_iter().take(limit) {
            if dst.to_lowercase() == b { continue; }
            if let Some((path, title, content)) = self.content_of(&dst) {
                let why = self
                    .citation_context(&b, &dst)
                    .unwrap_or_else(|| format!("本笔记引用了「{}」", dst));
                down.push(DrillItem {
                    title,
                    path,
                    base: dst,
                    excerpt: excerpt(&content),
                    why,
                });
            }
        }
        // 反链: 谁引用了我
        let mut up = Vec::new();
        for src_path in self.backlinks(&b)?.into_iter().take(limit) {
            let src_base = src_path
                .trim_end_matches(".md")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_lowercase();
            if src_base == b { continue; }
            if let Some(ex) = exclude_path {
                if src_path.to_lowercase() == ex.to_lowercase() { continue; }
            }
            if let Some((path, title, content)) = self.content_by_path(&src_path) {
                let why = self
                    .citation_context(&src_base, &b)
                    .unwrap_or_else(|| format!("「{}」引用了本笔记", src_base));
                up.push(DrillItem {
                    title,
                    path,
                    base: src_base,
                    excerpt: excerpt(&content),
                    why,
                });
            }
        }
        Ok((down, up))
    }

    /// 获取所有已索引笔记的 (path -> 有记录) 用于差异比对
    pub fn indexed_paths(&self) -> Result<HashMap<String, i64>> {
        let mut stmt = self.conn.prepare("SELECT path, mtime FROM notes")?;
        let mut map = HashMap::new();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for r in rows.flatten() {
            map.insert(r.0, r.1);
        }
        Ok(map)
    }

    /// 多级下钻: 从 base 出发 BFS 展开, 返回 (深度, 路径, DrillItem)
    /// 用于一次性展开 N 层, 输出可读的树形路径
    pub fn drill_tree(&self, base: &str, max_depth: usize, per_node: usize) -> Result<Vec<(usize, Vec<String>, DrillItem)>> {
        use std::collections::HashSet;
        let mut out: Vec<(usize, Vec<String>, DrillItem)> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        // 标记起点文件路径, 防止 A→B→A 循环
        if let Some((start_path, _, _)) = self.content_of(base) {
            visited.insert(start_path.to_lowercase());
        }
        visited.insert(base.to_lowercase());

        // 起点显示
        let start_title = self.content_of(base).map(|(_, t, _)| t).unwrap_or_else(|| base.to_string());
        // 收集起点的文件路径, 用于路径级防环
        let start_files: Vec<String> = {
            let mut v = vec![base.to_lowercase()];
            if let Some((sp, _, _)) = self.content_of(base) {
                v.push(sp.to_lowercase());
            }
            v
        };
        let mut frontier: Vec<(String, Vec<String>, usize)> = vec![(base.to_lowercase(), vec![start_title], 0)];

        while let Some((cur, path, depth)) = frontier.pop() {
            if depth >= max_depth {
                continue;
            }
            // cur 可能是 path (含 /) 或 base; 统一转成 base 查询
            let cur_base = cur
                .trim_end_matches(".md")
                .rsplit('/')
                .next()
                .unwrap_or(&cur)
                .to_lowercase();
            let (all_down, all_up) = self.drill_options_ex(&cur_base, 50, Some(&cur))?;
            // 枢纽过滤: 邻居数 > HUB_THRESHOLD 的节点不再展开 (避免爆炸)
            let cur_degree = all_down.len() + all_up.len();
            let expand = cur_degree <= HUB_THRESHOLD || depth == 0;
            let down: Vec<_> = if expand { all_down.into_iter().take(per_node).collect() } else { vec![] };
            let up: Vec<_> = if expand { all_up.into_iter().take(per_node).collect() } else { vec![] };
            for item in down {
                // 去重键 = 完整文件路径 (同名不同路径是不同文档)
                let item_file = item.path.to_lowercase();
                if !visited.insert(item_file.clone()) {
                    continue;
                }
                let mut p = path.clone();
                p.push(item.title.clone());
                out.push((depth + 1, p.clone(), item.clone()));
                frontier.push((item.path.to_lowercase(), p, depth + 1));
            }
            for item in up {
                let item_file = item.path.to_lowercase();
                if !visited.insert(item_file) {
                    continue;
                }
                let mut p = path.clone();
                p.push(format!("(←) {}", item.title));
                out.push((depth + 1, p.clone(), item.clone()));
                frontier.push((item.path.to_lowercase(), p, depth + 1));
            }
        }
        // 按深度和路径排序
        out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.join("/").cmp(&b.1.join("/"))));
        Ok(out)
    }

    /// 获取总入口索引文档 (priority 最高的一篇)
    pub fn master_index(&self) -> Option<(String, String)> {
        self.conn
            .query_row(
                "SELECT path, content FROM notes WHERE priority >= 50 ORDER BY priority DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    /// 统计
    pub fn stats(&self) -> Result<(i64, i64, i64)> {
        let notes: i64 = self.conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?;
        let links: i64 = self.conn.query_row("SELECT COUNT(*) FROM links", [], |r| r.get(0))?;
        let aliases: i64 = self.conn.query_row("SELECT COUNT(*) FROM aliases", [], |r| r.get(0))?;
        Ok((notes, links, aliases))
    }

    /// 通过 base 查 path
    pub fn path_of(&self, base: &str) -> Option<String> {
        self.conn
            .query_row("SELECT path FROM notes WHERE base = ?1 LIMIT 1", params![base], |r| {
                r.get::<_, String>(0)
            })
            .ok()
    }
}

// ---------- 辅助 ----------

/// 删除 contentless FTS5 表中的一行
/// contentless 表不支持 DELETE, 需用 'delete' 命令并回传原内容
/// 但 content='' 的表无法回传内容 → 改用 'delete-all' 或直接建表时用 contentless_delete
fn delete_fts_row(tx: &rusqlite::Transaction, rowid: i64) -> Result<()> {
    // 尝试 contentless_delete=1 语法 (SQLite 3.43+)
    // 若表创建时未启用, 退化为忽略 (rebuild 时会重建)
    let _ = tx.execute("DELETE FROM notes_fts WHERE rowid = ?1", params![rowid]);
    Ok(())
}

/// 从内容提取标题: 首个 # 标题, 或 frontmatter title, 或文件名
fn extract_priority(content: &str) -> i64 {
    let t = content.trim_start();
    if !t.starts_with("---") {
        return 0;
    }
    if let Some(end) = t[3..].find("\n---") {
        for line in t[3..3 + end].lines() {
            if let Some(v) = line.strip_prefix("priority:") {
                return v.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

fn extract_title(content: &str, base: &str) -> String {
    // frontmatter title
    let trimmed = content.trim_start();
    if trimmed.starts_with("---") {
        if let Some(end) = trimmed[3..].find("\n---") {
            let fm = &trimmed[3..3 + end];
            for line in fm.lines() {
                if let Some(v) = line.strip_prefix("title:") {
                    let t = v.trim().trim_matches('"').trim_matches('\'');
                    if !t.is_empty() {
                        return t.to_string();
                    }
                }
            }
        }
    }
    // 首个 # 标题
    for line in content.lines() {
        if let Some(t) = line.strip_prefix("# ") {
            return t.trim().to_string();
        }
    }
    base.to_string()
}

/// CJK 分词: 中文二元组 + 单字 (bigram + unigram), 英文按词保留
/// 二元组显著提升短语精度 (避免"开户成本"被单字稀释)
fn segment_cjk(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphanumeric() {
            // 英文连续词
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.extend(&chars[i..j]);
            out.push(' ');
            i = j;
        } else if c.is_alphanumeric() {
            // CJK: 单字 + 二元组都写入
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push(c);
            out.push(' ');
            // 二元组 (当前字 + 下一个 CJK 字)
            if i + 1 < chars.len() && chars[i + 1].is_alphanumeric() && !chars[i + 1].is_ascii() {
                out.push(c);
                out.push(chars[i + 1]);
                out.push(' ');
            }
            i += 1;
        } else {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            i += 1;
        }
    }
    out
}

/// 枢纽节点阈值: 邻居总数超过此值不再展开下一层
const HUB_THRESHOLD: usize = 15;

/// 文本归一化 (去装饰符 + 小写), 用于标题匹配
fn normalize_text(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '%')
        .collect::<String>()
        .to_lowercase()
}

/// 业务同义词/话术映射: 把业务口语转成检索意图
/// 返回 (扩展后的查询词列表, 是否命中映射)
pub fn expand_business_terms(q: &str) -> (Vec<String>, bool) {
    // (触发词, 扩展词列表)
    let rules: &[(&[&str], &[&str])] = &[
        // 投放整体表现 → 消耗/转化/成本
        (&["投放怎么样", "投放如何", "投放效果", "投放表现", "投放情况", "投放好不好"],
         &["消耗", "转化", "开户成本", "转化成本"]),
        // 效果 → 转化率/成本
        (&["效果怎么样", "效果如何", "效果好", "效果差", "效果不好"],
         &["转化率", "成本", "开户成本", "转化成本"]),
        // 花钱 → 消耗
        (&["花钱", "花费", "支出", "跑了多少", "花了多少"],
         &["消耗", "消耗金额"]),
        // 线索/开户 → 开户数/成本
        (&["开户怎么样", "开户情况", "开户效果", "拿量", "量怎么样"],
         &["开户数", "开户成本", "提交开户", "成功开户"]),
        // 贵/便宜 → 成本
        (&["太贵", "贵了", "便宜", "成本高", "成本低", "降价"],
         &["成本", "开户成本", "转化成本", "CPA"]),
        // 素材表现
        (&["素材怎么样", "素材效果", "素材行不行", "图好不好"],
         &["素材", "点击率", "CTR", "转化率"]),
        // 归因/对不上
        (&["对不上", "不一致", "差异大", "数据不准"],
         &["对账", "口径", "归因"]),
        // 监控/异常
        (&["异常", "出问题", "不对了", "掉量", "掉了一半"],
         &["告警", "监控", "排查", "异动"]),
        // 时段
        (&["昨天", "今天", "昨日", "今日"],
         &["分时", "日报", "小时"]),
    ];

    let ql = q.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut hit = false;
    for (triggers, expands) in rules {
        if triggers.iter().any(|t| ql.contains(t)) {
            hit = true;
            for e in expands.iter() {
                if !out.contains(&e.to_string()) {
                    out.push(e.to_string());
                }
            }
        }
    }
    (out, hit)
}

/// 三级降级查询序列: [AND 全词, 去停用词 AND, OR 任一词]
pub fn build_fts_query_ladder(q: &str) -> Vec<String> {
    let base = build_fts_query(q);
    if base.is_empty() {
        return vec![];
    }
    // 第 0 级: 原查询按空格原样 (保留专业术语完整性, 如 t_bytedance_hourly_unit_report)
    let raw = q.split_whitespace()
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !raw.is_empty() && raw != base {
        let mut v = vec![raw];
        v.extend(build_ladder_rest(q, &base));
        return v;
    }
    build_ladder_rest(q, &base)
}

fn build_ladder_rest(q: &str, base: &str) -> Vec<String> {
    let base = base.to_string();
    // 显式操作符不降级
    if q.contains(" OR ") || q.contains(" AND ") || q.starts_with('"') {
        return vec![base];
    }

    let mut ladder = vec![base.clone()];

    // 第2级: 去掉停用词 (怎么/如何/的/是/什么/有哪些...)
    let stop: &[&str] = &[
        "怎么", "如何", "什么", "哪些", "哪个", "为什么", "多少", "是否",
        "可以", "需要", "应该", "有没有", "以及", "还有", "一个", "这个", "那个",
        "的", "了", "是", "和", "与", "或", "在", "有", "要", "能", "会", "把", "被",
    ];
    let tokens: Vec<&str> = q.split_whitespace().collect();
    let filtered: Vec<&str> = tokens.iter().copied().filter(|t| !stop.contains(t)).collect();
    if filtered.len() < tokens.len() && !filtered.is_empty() {
        let lvl2 = build_fts_query(&filtered.join(" "));
        if !lvl2.is_empty() && lvl2 != base {
            ladder.push(lvl2);
        }
    }

    // 第3级: 去虚词后取最有区分度的 2-3 个词做 AND (保持精确性)
    let parts: Vec<String> = base.split_whitespace().map(|s| s.to_string()).collect();
    if parts.len() > 3 {
        ladder.push(format!("__SELECTIVE_AND__{}", parts.join(",")));
    }
    // 第4级: 才用 OR 兜底 (最后手段)
    if parts.len() > 1 {
        ladder.push(format!("__SELECTIVE_OR__{}", parts.join(",")));
    }
    ladder
}

/// 构造 FTS5 查询: 多词默认 AND, 支持 OR / 短语
pub fn build_fts_query(q: &str) -> String {
    let q = q.trim();
    if q.is_empty() {
        return String::new();
    }
    // 已有操作符时原样透传
    if q.contains(" OR ") || q.contains(" AND ") || q.starts_with('"') {
        return q.to_string();
    }
    // 分词策略:
    //   1. 先抽取领域词 (完整词, 不拆)
    //   2. 剩余文本按停用符切段, 段内 bigram (不跨段)
    let domain: &[&str] = &[
        "消耗", "转化", "开户", "成本", "素材", "指标", "库表", "报表", "归因",
        "回传", "预算", "出价", "监控", "盯盘", "口径", "分时", "小时", "日报",
        "账户", "计划", "创意", "点击", "曝光", "漏斗", "阈值", "异常", "排查",
    ];
    let mut parts: Vec<String> = Vec::new();
    let mut masked = q.to_string();
    // 长词优先
    let mut ds = domain.to_vec();
    ds.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    for d in &ds {
        if masked.contains(d) {
            if !parts.contains(&d.to_string()) {
                parts.push(d.to_string());
            }
            masked = masked.replace(d, " ");
        }
    }
    // 剩余: 按空格/标点切段, 段内 bigram
    let noise_chars = "的了是和与在有吗呢吧啊么怎样如何什么哪些哪个这个那个我们你们我想看";
    for seg in masked.split(|c: char| c.is_whitespace() || "，。、！？,.;:!?()（）[]【】".contains(c)) {
        let chars: Vec<char> = seg.chars().filter(|c| c.is_alphanumeric()).collect();
        if chars.is_empty() {
            continue;
        }
        let has_cjk = chars.iter().any(|c| !c.is_ascii());
        if !has_cjk {
            let w: String = chars.iter().collect();
            if !parts.contains(&w) {
                parts.push(w);
            }
            continue;
        }
        // 过滤纯虚词段
        let meaningful: Vec<char> = chars
            .iter()
            .copied()
            .filter(|c| !noise_chars.contains(*c))
            .collect();
        if meaningful.is_empty() {
            continue;
        }
        // 段长 <= 3: 整段作为一个词 (避免切碎)
        if meaningful.len() <= 3 {
            let t: String = meaningful.iter().collect();
            if !parts.contains(&t) {
                parts.push(t);
            }
            continue;
        }
        // 长段: bigram, 但要求两端都不是高频虚字
        let high_freq = "到各分怎配渠看想据";
        for win in meaningful.windows(2) {
            let t: String = win.iter().collect();
            // 跳过含高频虚字的组合
            if win.iter().any(|c| high_freq.contains(*c)) {
                continue;
            }
            if !parts.contains(&t) {
                parts.push(t);
            }
        }
        // 兜底: 若全被过滤, 保留首两个字符
        if !parts.iter().any(|p_| p_.chars().all(|c| meaningful.contains(&c))) {
            let t: String = meaningful.iter().take(2).collect();
            if !parts.contains(&t) {
                parts.push(t);
            }
        }
    }
    parts.join(" ")
}

/// 生成命中片段
/// 生成检索片段: 中文按 bigram+单字匹配, 取命中行 + 后续 1-2 行作上下文
fn make_snippet(content: &str, query: &str) -> String {
    // 查询词: 中文 bigram + 单字, 英文整词 (与索引一致)
    let mut words: Vec<String> = Vec::new();
    for w in query.split_whitespace() {
        let has_cjk = w.chars().any(|c| c.is_alphabetic() && !c.is_ascii());
        if has_cjk {
            let chars: Vec<char> = w.chars().filter(|c| c.is_alphanumeric()).collect();
            if chars.len() == 1 {
                words.push(chars[0].to_string());
            } else {
                for win in chars.windows(2) {
                    words.push(win.iter().collect());
                }
            }
        } else if !w.is_empty() {
            words.push(w.to_lowercase());
        }
    }
    if words.is_empty() {
        return first_para(content);
    }

    let body = strip_fm(content);
    let lines: Vec<&str> = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| {
            if l.is_empty() || l.starts_with('#') || l.starts_with("---") {
                return false;
            }
            // 过滤元数据行: 形如 xxx:: [[...]] 或 xxx:: value 的关系声明
            // 判定: 前 40 字符内出现 "::" 且冒号前是纯 ASCII 标识符
            if let Some(pos) = l.find("::") {
                if pos < 40 {
                    let key = &l[..pos];
                    if !key.is_empty()
                        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        return false;
                    }
                }
            }
            // 过滤 yaml 风格 key: value (label: / type: 等)
            if let Some(pos) = l.find(':') {
                if pos < 20 {
                    let key = &l[..pos];
                    if !key.is_empty()
                        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !key.contains(' ')
                    {
                        return false;
                    }
                }
            }
            // 纯 wikilink 行也跳过
            if l.starts_with("[[") && l.ends_with("]]") {
                return false;
            }
            true
        })
        .collect();
    if lines.is_empty() {
        return first_para(content);
    }

    // 打分: 命中词数越多越优先; 标题式短行(无标点结尾)适当降权
    let mut best: Option<(usize, usize)> = None; // (命中数, 行号)
    for (i, l) in lines.iter().enumerate() {
        let ll = l.to_lowercase();
        let hits = words.iter().filter(|w| ll.contains(w.as_str())).count();
        if hits == 0 {
            continue;
        }
        match best {
            Some((bh, _)) if hits <= bh => {}
            _ => best = Some((hits, i)),
        }
    }

    let start = best.map(|(_, i)| i).unwrap_or(0);
    // 取命中行 + 后续最多 2 行 (合并为一段)
    let mut parts: Vec<String> = Vec::new();
    let mut budget = 400usize; // 字符预算
    for l in lines.iter().skip(start) {
        if budget == 0 {
            break;
        }
        let take = l.chars().count().min(budget);
        parts.push(l.chars().take(take).collect::<String>());
        budget = budget.saturating_sub(take);
        if parts.len() >= 3 {
            break;
        }
    }
    let joined = parts.join(" ");
    if joined.chars().count() > 400 {
        format!("{}…", joined.chars().take(400).collect::<String>())
    } else if joined.is_empty() {
        first_para(content)
    } else {
        joined
    }
}

/// 首段兜底 (跳过 frontmatter 与标题)
fn first_para(content: &str) -> String {
    let body = strip_fm(content);
    let text: String = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"))
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    text.chars().take(300).collect()
}

fn strip_fm(content: &str) -> &str {
    let t = content.trim_start();
    if t.starts_with("---") {
        if let Some(e) = t[3..].find("\n---") {
            return &t[3 + e + 4..];
        }
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_doc(rel: &str, content: &str, links: Vec<&str>) -> Doc {
        let base = rel.trim_end_matches(".md").rsplit('/').next().unwrap().to_string();
        Doc {
            rel: rel.to_string(),
            base,
            content: content.to_string(),
            lower: content.to_lowercase(),
            lines: Vec::new(),
            tags: Vec::new(),
            aliases: Vec::new(),
            links_out: links.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn test_rebuild_and_search() {
        let mut store = Store::open(Path::new(":memory:")).unwrap();
        let docs = vec![
            mk_doc("a.md", "# 笔记A\n\n这是关于字节消耗的内容", vec!["笔记B"]),
            mk_doc("b.md", "# 笔记B\n\n这是关于腾讯消耗的内容", vec!["笔记A"]),
            mk_doc("c.md", "# 笔记C\n\n无关内容", vec![]),
        ];
        store.rebuild(&docs).unwrap();

        let hits = store.search("字节", &SearchOpts { hops: 0, ..Default::default() }).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].base, "a");
    }

    #[test]
    fn test_backlinks() {
        let mut store = Store::open(Path::new(":memory:")).unwrap();
        let docs = vec![
            mk_doc("a.md", "# A\n内容", vec!["b"]),
            mk_doc("b.md", "# B\n内容", vec![]),
        ];
        store.rebuild(&docs).unwrap();
        let bl = store.backlinks("b").unwrap();
        assert_eq!(bl.len(), 1);
        assert!(bl[0].contains("a.md"));
    }

    #[test]
    fn test_graph_boost() {
        let mut store = Store::open(Path::new(":memory:")).unwrap();
        let docs = vec![
            mk_doc("hit.md", "# 命中\n字节消耗内容", vec!["neighbor"]),
            mk_doc("neighbor.md", "# 邻居\n无关内容", vec![]),
        ];
        store.rebuild(&docs).unwrap();
        let hits = store.search("字节消耗", &SearchOpts { hops: 1, limit: 10, alpha: 0.3 }).unwrap();
        // 邻居应该通过图传播上榜
        assert!(hits.iter().any(|h| h.base == "neighbor"));
    }

    #[test]
    fn test_fts_query_builder() {
        // 中文逐字拆分 (与 segment_cjk 对称)
        assert_eq!(build_fts_query("字节 消耗"), "字节 消耗");
        // 英文保持
        assert_eq!(build_fts_query("rta"), "rta");
        // 显式操作符透传
        assert_eq!(build_fts_query("A OR B"), "A OR B");
    }
}

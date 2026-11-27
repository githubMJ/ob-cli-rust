# ob-cli

Rust 实现的 Obsidian 知识库 CLI。单二进制（~5MB），内嵌 SQLite FTS5 全文索引 + 双向链接图检索，专为 AI Agent 多轮检索设计。

## 特性

- **全文检索**：SQLite FTS5 + 中文 bigram 分词 + BM25 排序，三级降级（AND → 去停用词 → OR），自然语言长句可用
- **图检索**：双向链接（正链/反链）索引化，一跳图传播重排序，多跳 BFS 下钻
- **AI Agent 友好**：渐进发现（discover）+ 下钻（note + breadcrumb）+ 读全文（read），每步输出 `file` 字段可直接串链
- **业务话术映射**："投放怎么样" 自动扩展为 消耗/转化/开户成本
- **模糊度检测**：泛化查询返回 `needs_clarification` + 反问话术
- **单二进制**：内嵌 SQLite，仅 3 个依赖（pulldown-cmark / regex / rusqlite）
- **索引跨平台**：SQLite 格式通用，macOS 建库 Linux 直接用

## 快速开始

```bash
# 建索引（13.5k 篇约 4s）
ob-cli index --path /path/to/vault --index-path /path/to/vault/.index

# 检索
ob-cli search "开户成本" --path /path/to/vault --index-path /path/to/vault/.index --discover

# 下钻
ob-cli search --note "knowledge/01-metrics/开户成本.md" --path /path/to/vault --index-path /path/to/vault/.index

# 读全文
ob-cli search --read "knowledge/01-metrics/开户成本.md" --path /path/to/vault --index-path /path/to/vault/.index

# 增量同步（笔记增删改后）
ob-cli index --path /path/to/vault --index-path /path/to/vault/.index --incremental
```

## 检索模式

| 模式 | 参数 | 说明 |
|---|---|---|
| A 关键词检索 | `query` | BM25 + 一跳图传播，返回 score/fts/graph 分解 |
| B 渐进发现 | `--discover` | 每页 3 条候选，带 relevance 判据 / matched/missing 词 / drill 数 |
| C 下钻 | `--note <file>` | current + can_drill.down/up（带 why 引用上下文）+ breadcrumb |
| C+ 多级树 | `--note <file> --depth 2` | 展开多跳树，防循环、枢纽过滤 |
| D 读全文 | `--read <file>[,file2]` | Markdown 原文，自动截断（--max-chars）|
| 导航 | `--index-doc` | 总入口索引文档 |

通用参数：`--json` / `--limit N` / `--page N` / `--hops 0|1|2` / `--backlinks`

## 索引管理

```bash
index                  # 全量建库（13.5k 篇 ~4s）
index --incremental    # 增量同步（mtime 比对，增删改自动处理，~20ms）
```

索引为单文件 `index.db`（SQLite），可跨平台拷贝。

## 笔记操作

```bash
list [--keyword X] [--page N] [--limit N] [--json]   # 列出笔记（过滤+分页）
print <note>                                          # 打印全文
frontmatter <note> get|set|remove                     # YAML 属性
tags / dirs / orphans / unresolved                    # 标签/目录/孤立/断链
create / append / move / delete / hdaily              # 笔记管理
```

## 架构

```
crates/
├── ob-lib/           核心库
│   ├── parse.rs      OFM 解析（pulldown-cmark，代码块排除，别名/块引用/嵌入）
│   ├── vault.rs      文件遍历 + Doc 模型
│   ├── store.rs      SQLite FTS5 存储层 + 图传播 + discover/drill
│   └── cmds/         命令实现（search/docs/drill...）
└── ob-cli/           二进制入口
```

依赖：`pulldown-cmark` + `regex` + `rusqlite`（bundled SQLite）

## 性能（13.5k 篇 / 57MB）

| 操作 | 耗时 |
|---|---|
| 全量建索引 | 3.4s |
| 增量同步 | 20ms |
| 检索 | 8-29ms |
| 下钻（单级）| 5-8ms |
| 读全文 | 3ms |

准确率（181 题 QA）：Top1 91.7% / Top3 96.1%

## 编译

```bash
cargo build --release
# Linux 静态
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

要求 Rust 1.75+。

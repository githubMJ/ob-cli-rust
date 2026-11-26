# ob-cli-rust

Rust 实现的 Obsidian vault CLI + MCP server。单静态二进制（~1.6MB），零运行时依赖，支持本地知识库的全栈检索、双向链接分析、图谱遍历与笔记管理。

## 构建

```bash
# macOS / Linux
cargo build --release

# Linux x86_64 交叉编译 (macOS 上, 静态二进制)
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
# 产物: target/x86_64-unknown-linux-musl/release/notesmd-rs

# 产物位置
target/release/notesmd-rs
```

要求 Rust 1.75+。依赖仅 `regex` 一个 crate。

## 模块结构

```
src/
├── main.rs     # 入口 + 命令分发 + MCP server (serve) + 参数解析
├── vault.rs    # 【文档层】vault 遍历、文档模型、frontmatter/行内标签解析、链接索引
├── query.rs    # 【检索层】Obsidian 兼容查询语法解析器 + 匹配引擎 + 模糊匹配
└── crud.rs     # 【操作层】笔记增删改、重命名更新链接、frontmatter 读写、标签/健康分析
```

### 按功能分类

| 分类 | 命令 |
|---|---|
| **检索类** | `search`（Obsidian 语法：AND/OR/`-排除`/`"短语"`/`path:`/`file:`/`tag:`/`line:`，支持 `--fuzzy` 模糊 / `--lines` 行级 / `--context=N` 上下文 / `--paragraph` 段落级 / `--links` 双链扩展 / `--json`）、`hybrid`（关键词+双链一跳）、`graph`（多跳图遍历 `--depth=N`）、`backlinks`（反向链接）、`links`（正向链接） |
| **文档/目录操作类** | `create`（新建，自动建目录）、`move`（重命名/移动并自动更新全库链接）、`delete`（删除）、`print`（读全文）、`frontmatter`（YAML 属性 get/set/remove）、`hdaily`（无头每日笔记） |
| **列表/统计展示类** | `list`（列笔记）、`tags`（标签计数 / 按标签列笔记）、`orphans`（孤立笔记）、`unresolved`（断链） |
| **Agent 接入类** | `serve`（MCP stdio server：search_notes / read_note / backlinks / list_notes） |

## 查询语法（与 Obsidian 一致）

```
word1 word2          同文件 AND
word1 OR word2       或
-word                排除
"exact phrase"       短语
path:"04-Wiki/"      按路径过滤
file:"名字"          按文件名
tag:#标签            按标签 (含嵌套 tag:#a/b)
line:(word1 word2)   同一行同时出现
--fuzzy              模糊子序列匹配 (容忍拼写缺漏)
```

## 粒度控制

```bash
notesmd-rs search "归并排序" --path <vault>   # 文件级
  --lines                # + 命中行+行号
  --context=2 --lines    # + 前后 2 行上下文
  --paragraph            # + 命中所在完整段落
  --links                # + 双向链接扩展
  --json                 # 结构化输出 (agent/脚本)
```

## 示例

```bash
# 复杂检索: Wiki 目录下带广告标签、排除 Inbox 的笔记, 返回段落
notesmd-rs search "RTA tag:#广告 -00-Inbox" --path ~/vault --paragraph

# 图检索: 从一篇笔记出发 3 跳双向遍历
notesmd-rs graph "1.4 - 归并排序" --path ~/vault --depth=3

# 重命名笔记, 全库链接自动更新
notesmd-rs move "旧标题" "新目录/新标题" --path ~/vault

# MCP: agent 接入
notesmd-rs serve --path ~/vault
```

## 性能

516 篇 / 158MB 真实库：全量检索 ~150ms（CPU 时间），单二进制 1.6MB。瓶颈在磁盘 IO；每次调用全量扫描，无索引文件，笔记改动即时生效。

# ob-cli-rust

Rust 实现的 Obsidian vault CLI。单静态二进制（~1.6MB），零运行时依赖，支持本地知识库的全栈检索、双向链接分析、图谱遍历与笔记管理。

## 构建

```bash
# macOS / Linux
cargo build --release

# Linux x86_64 交叉编译 (macOS 上, 静态二进制)
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
# 产物: target/x86_64-unknown-linux-musl/release/ob-cli-rust

# 产物位置
target/release/ob-cli-rust
```

要求 Rust 1.75+。依赖仅 `regex` 一个 crate。

## 模块结构

三个二进制 + 一个共享库:

| 二进制 | 职责 | 命令 |
|---|---|---|
| **ob-search** | 检索模块 | Retrieval: search(语法/模糊/段落/上下文/JSON)、backlinks、links、hybrid、graph |
| **ob-core** | 核心操作模块 | Core operations: create、print、list、move、delete、frontmatter、tags、orphans、unresolved、hdaily |
| **ob-cli** | 总入口 | Unified entry: 以上全部命令自动路由 |

Cargo workspace，四个 crate：

```
crates/
├── ob-lib/             # 共享库 crate
│   ├── src/lib.rs      # + CLI 参数解析 (三二进制共用)
│   ├── src/common.rs   # 【共享】Ctx 上下文、usage、笔记查找、行工具
│   ├── src/vault.rs    # 【文档层】vault 遍历、文档模型、frontmatter/行内标签解析、链接索引
│   ├── src/query.rs    # 【检索层】Obsidian 兼容查询语法解析器 + 匹配引擎 + 模糊匹配
│   ├── src/crud.rs     # 【操作层实现】move/delete/frontmatter/tags/orphans/unresolved/hdaily
│   ├── src/json.rs     # 【JSON】零依赖 JSON 解析/序列化
│   └── src/cmds/       # 【命令实现】被三个二进制共享
│       ├── search.rs   #   search (语法/模糊/段落/上下文/JSON)
│       ├── graph.rs    #   graph 多跳图遍历
│       ├── hybrid.rs   #   hybrid 混合检索
│       ├── links.rs    #   backlinks / links 双向链接
│       ├── docs.rs     #   create/print/list/move/delete/frontmatter/hdaily
│       └── stats.rs    #   tags/orphans/unresolved
├── ob-search/          # 检索二进制
├── ob-core/            # 核心操作二进制
└── ob-cli/             # 总入口
```

构建产物：`target/release/{ob-search, ob-ops, ob-cli}`

### 按功能分类

| 分类 | 命令 |
|---|---|
| **检索类** → `ob-search` | `search`（Obsidian 语法：AND/OR/`-排除`/`"短语"`/`path:`/`file:`/`tag:`/`line:`，支持 `--fuzzy` 模糊 / `--lines` 行级 / `--context=N` 上下文 / `--paragraph` 段落级 / `--json`；**默认自动附带每条命中的双向链接邻居**）、`hybrid`（关键词+双链一跳）、`graph`（多跳图遍历 `--depth=N`）、`backlinks`（反向链接）、`links`（正向链接） |
| **文档/目录操作类** → `ob-core` | `create`（新建，自动建目录）、`move`（重命名/移动并自动更新全库链接）、`delete`（删除）、`print`（读全文）、`frontmatter`（YAML 属性 get/set/remove）、`hdaily`（无头每日笔记） |
| **列表/统计展示类** → `ob-core` | `list`（列笔记）、`tags`（标签计数 / 按标签列笔记）、`orphans`（孤立笔记）、`unresolved`（断链） |


## Workspace 参数（必传）

所有命令必须通过 `--path <目录>` 指定 vault 工作空间，检索/操作严格限制在该目录内：

```bash
ob-cli search "关键词" --path /data/llm_wiki     # ✅
ob-cli search "关键词"                            # ❌ exit 2: 缺少 --path
ob-cli search "关键词" --path /不存在            # ❌ exit 2: 目录不存在
```

> 也支持环境变量 `OBSIDIAN_VAULT=/data/llm_wiki`（--path 优先级更高）。
> 适合作为外部封装（如 MCP 工具）的固定参数：绑定 workspace，所有调用透传该路径。

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
--fuzzy              等价于 --mode=fuzzy
--mode=exact|fuzzy|regex|graph|link  指定搜索模式
```

## 检索模式

| --mode | 说明 | 配合参数 |
|---|---|---|
| `exact` | 子串精确匹配（默认） | |
| `fuzzy` | 模糊子序列匹配 | |
| `regex` | 正则表达式 | |
| `graph` | 关键词命中为种子的多跳图遍历（返回节点+边） | `--depth=N` |
| `link` | 命中笔记的正向+反向链接邻居（双向链检索） | |

## 粒度控制

```bash
ob-cli-rust search "归并排序" --path <vault>   # 文件级
  --lines                # + 命中行+行号
  --context=2 --lines    # + 前后 2 行上下文
  --paragraph            # + 命中所在完整段落
  --links                # + 双向链接扩展
  --json                 # 结构化输出 (agent/脚本)
```

## 示例

```bash
# 复杂检索: Wiki 目录下带广告标签、排除 Inbox 的笔记, 返回段落
ob-cli-rust search "RTA tag:#广告 -00-Inbox" --path ~/vault --paragraph

# 图检索: 从一篇笔记出发 3 跳双向遍历
ob-cli-rust graph "1.4 - 归并排序" --path ~/vault --depth=3

# 重命名笔记, 全库链接自动更新
ob-cli-rust move "旧标题" "新目录/新标题" --path ~/vault

# MCP: agent 接入
ob-cli-rust serve --path ~/vault
```

## 性能

516 篇 / 158MB 真实库：全量检索 ~150ms（CPU 时间）。三二进制各 1.5-1.7MB。瓶颈在磁盘 IO；每次调用全量扫描，无索引文件，笔记改动即时生效。

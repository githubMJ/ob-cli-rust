# notesmd-rs

Rust 实现的 Obsidian vault CLI + MCP server（参考 notesmd-cli Go 增强版重写）。

## 功能
- `search` — Obsidian 兼容搜索语法 (AND/OR/-排除/"短语"/path:/file:/tag:/line:)
- `backlinks` / `links` — 双向链接
- `hybrid` — 关键词检索 + 双链一跳扩展
- `list` / `print` / `create` — 笔记基础操作
- `serve` — MCP stdio server (search_notes/read_note/backlinks/list_notes)

## 构建
cargo build --release        # macOS/Linux
CGO 不需要, 纯 Rust, 二进制 ~1.6MB

## 基准 (516篇/158MB 真实库, M系列 Mac)
性能与 Go 版持平 (瓶颈在磁盘 IO), 二进制体积 1.6M vs 7.6M

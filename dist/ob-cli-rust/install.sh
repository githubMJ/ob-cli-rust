#!/bin/bash
set -e
DIR=$(cd $(dirname $0) && pwd)
TARGET=${1:-/usr/local/app/bin}
mkdir -p $TARGET && cp $DIR/ob-cli $TARGET/ob-cli && chmod +x $TARGET/ob-cli
ln -sf $TARGET/ob-cli /usr/local/bin/ob-cli
echo "✅ $(/usr/local/bin/ob-cli --version)"
cat <<'USAGE'

统一入口: ob-cli search (参数切换模式)
  建库     index --path <vault> --index-path <dir>
  增量     index --path <vault> --index-path <dir> --incremental
  检索     search "词" --path <vault> --index-path <dir>
  发现     search "词" --discover --path <vault> --index-path <dir>
  下钻     search --note "<file>" [--depth 2] --path <vault> --index-path <dir>
  读全文   search --read "<file>"[,file2] --path <vault> --index-path <dir>
  总入口   search --index-doc --path <vault> --index-path <dir>
USAGE

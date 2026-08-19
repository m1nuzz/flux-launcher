#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
source "$HOME/.cargo/env"

cargo build -p flux-launcher -p flux-plugin-example
stage_dir="$repo_root/target/native-plugin-host-smoke/plugins/example"
rm -rf "$repo_root/target/native-plugin-host-smoke"
mkdir -p "$stage_dir"
cp crates/flux-plugin-example/plugin.toml "$stage_dir/plugin.toml"
cp target/debug/libflux_plugin_example.so "$stage_dir/flux_plugin_example.dll"

printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"query","params":{"query":"ex hello","action_keyword":"ex","locale":"en-US"}}' \
  '{"jsonrpc":"2.0","id":2,"method":"execute","plugin":"Example Native","params":{"action":{"type":"copy_text","text":"hello"}}}' \
  | target/debug/flux-launcher --plugin-host "$repo_root/target/native-plugin-host-smoke/plugins" \
  | tee "$repo_root/target/native-plugin-host-smoke/responses.jsonl"

grep -q 'Example: hello' "$repo_root/target/native-plugin-host-smoke/responses.jsonl"
grep -q '"success":true' "$repo_root/target/native-plugin-host-smoke/responses.jsonl"
echo 'native plugin host smoke passed'

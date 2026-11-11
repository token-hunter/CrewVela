#!/bin/bash
set -euo pipefail

# Invoke the binary directly: `open -a` can route to an existing normal instance.
# No user shell initialization, account discovery, migration or profile copying.
script_path="${BASH_SOURCE[0]}"
case "$script_path" in */*) script_dir="${script_path%/*}" ;; *) script_dir=. ;; esac
project_root="$(cd -- "$script_dir" && pwd -P)"
data_dir="$project_root/.test-data"
app_path=""

usage() {
  printf '%s\n' '用法：start-first-run-test.command [--app <CrewVela.app>] [--data-dir <独立测试目录>]'
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --app|--data-dir)
      if [ "$#" -lt 2 ] || [ -z "$2" ]; then
        usage >&2
        exit 2
      fi
      if [ "$1" = --app ]; then app_path="$2"; else data_dir="$2"; fi
      shift 2
      ;;
    --help|-h) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

# Relative overrides are relative to the invoking terminal, not the .app bundle.
case "$data_dir" in /*) ;; *) data_dir="$PWD/$data_dir" ;; esac
if [ -z "$app_path" ]; then
  for candidate in \
    "$project_root/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/CrewVela.app" \
    "$project_root/src-tauri/target/release/bundle/macos/CrewVela.app" \
    "$project_root/CrewVela.app"; do
    if [ -d "$candidate" ]; then app_path="$candidate"; break; fi
  done
fi
case "$app_path" in ""|/*) ;; *) app_path="$PWD/$app_path" ;; esac
binary="$app_path/Contents/MacOS/CrewVela"
if [ -z "$app_path" ] || [ ! -d "$app_path" ] || [ ! -x "$binary" ]; then
  printf '%s\n' '未找到可执行的 CrewVela.app。请在 Mac 执行 npm run build:macos，或用 --app 指定 .app 路径。' >&2
  exit 2
fi

printf '启动 CrewVela 新用户隔离测试：%s\n' "$data_dir"
exec "$binary" --test-data-dir "$data_dir"

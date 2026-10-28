#!/bin/bash
set -euo pipefail
repo_root=$(cd "$(dirname "$0")/.." && pwd)
exec bash "$repo_root/apps/usque_gui/macos/NetworkExtensions/build.sh" "$@"

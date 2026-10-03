#!/usr/bin/env bash
# Linux development entry point. Local SDK paths remain outside Git.
set -euo pipefail
usque_dev_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$(uname -s)" != Linux ]]; then
    printf '%s\n' 'This entry point requires Linux or WSL.' >&2
    exit 1
fi
if [[ -f "$usque_dev_root/.toolchains/env.sh" ]]; then
    # shellcheck source=/dev/null
    source "$usque_dev_root/.toolchains/env.sh"
fi
# Keep Windows PATH imports out of this process without changing WSL settings.
IFS=: read -r -a usque_dev_paths <<< "$PATH"
usque_dev_linux_paths=()
for usque_dev_path in "${usque_dev_paths[@]}"; do
    case "$usque_dev_path" in
        /mnt/[a-z]/*|'') ;;
        *) usque_dev_linux_paths+=("$usque_dev_path") ;;
    esac
done
PATH="$(IFS=:; printf '%s' "${usque_dev_linux_paths[*]}")"
export PATH
usque_dev_python="$(command -v python3 || command -v python)"
case "$usque_dev_python" in
    /mnt/[a-z]/*|*.exe|*.bat|*.cmd)
        printf '%s\n' 'Use a Linux Python executable.' >&2
        exit 1
        ;;
esac
exec "$usque_dev_python" "$usque_dev_root/tool/linux_dev.py" "$@"

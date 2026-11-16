#!/usr/bin/env bash
# Only for fresh GitHub-hosted Macs. Build as the ordinary user before calling this script.
set -euo pipefail
[[ ${GITHUB_ACTIONS:-} == true && ${RUNNER_ENVIRONMENT:-} == github-hosted &&
   ${USQUE_DISPOSABLE_MAC:-} == 1 && $(uname -s) == Darwin && $(id -u) != 0 ]]
[[ $# == 3 ]]
umask 077
engine=$1
agent=$2
test_binary=$3
installed='/Library/PrivilegedHelperTools/com.usque.agent'
journal='/Library/Application Support/Usque/Agent'
test ! -e "$installed"
test ! -e "$journal"
sudo -n true
arch=$(uname -m)
bash tool/build_macos_agent_maintenance.sh "$arch"
fixture=$(mktemp -d /private/tmp/usque-engine-e2e.XXXXXX)
contents="$fixture/Usque.app/Contents"
mkdir -p "$contents/MacOS" "$contents/Helpers"
cp "$engine" "$contents/MacOS/usque-engine"
cp "$agent" "$contents/Helpers/usque-agent"
maintenance="$contents/Helpers/usque-agent-maintenance"
cp "build/macos-agent/$arch/usque-agent-maintenance" "$maintenance"
for file in "$contents/MacOS/usque-engine" "$contents/Helpers/usque-agent" "$maintenance"; do
  codesign --force --options runtime --sign - "$file"
done

keychain="$fixture/fixture.keychain-db"
security list-keychains -d user > "$fixture/original-search-list"
original_default=$(security default-keychain -d user | sed -E 's/^ *"//; s/"$//')
daemon_started=false
cleanup() {
  local result=$?
  trap - EXIT
  set +e
  if [[ $daemon_started == true ]]; then
    sudo -n launchctl bootout system/com.usque.agent || result=1
    # The deadline supervisor handles recovery after the daemon releases its lock.
  fi
  if (( result != 0 )) && [[ ${USQUE_TEST_LIVE_WARP:-} != 1 ]]; then
    python3 tool/macos_tun_engine_failure.py "$fixture" "$installed/usque-engine"
  fi
  if [[ -f "$keychain" ]]; then
    security default-keychain -d user -s "$original_default" || result=1
    python3 - "$fixture/original-search-list" <<'PY' || result=1
import pathlib
import shlex
import subprocess
import sys

paths = shlex.split(pathlib.Path(sys.argv[1]).read_text())
subprocess.run(["security", "list-keychains", "-d", "user", "-s", *paths], check=True)
PY
    security delete-keychain "$keychain" || result=1
  fi
  exit "$result"
}
trap cleanup EXIT
sudo -n "$maintenance" install
daemon_started=true
# Keep all test device credentials in a fresh, unlocked runner keychain.
password=$(uuidgen)
security create-keychain -p "$password" "$keychain"
security unlock-keychain -p "$password" "$keychain"
security list-keychains -d user -s "$keychain"
security default-keychain -d user -s "$keychain"
unset password
export USQUE_TEST_KEYCHAIN="$keychain"
export USQUE_TEST_DIRECTORY="$fixture"
iface=$(route -n get -inet default | awk '$1 == "interface:" { print $2 }')
USQUE_TEST_PHYSICAL_V4=$(ipconfig getifaddr "$iface")
export USQUE_TEST_PHYSICAL_V4
if [[ ${USQUE_TEST_LIVE_WARP:-} == 1 ]]; then
  scenarios=(live_warp_connect_disconnect_and_exit)
else
  test -x "$USQUE_TEST_PEER"
  scenarios=(failed_endpoint_disconnect_and_exit connected_peer_loss_disconnect_and_exit)
fi
for scenario in "${scenarios[@]}"; do
  "$test_binary" --ignored --exact "$scenario" --test-threads=1 --nocapture \
    --format=pretty --color=never | tee "$fixture/$scenario.txt"
  grep -Eq '^test result: ok\. 1 passed; 0 failed; 0 ignored;' "$fixture/$scenario.txt"
done

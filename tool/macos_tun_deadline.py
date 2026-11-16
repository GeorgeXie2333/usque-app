"""Keep a local recovery path if a remote TUN test cuts off the Actions control channel."""

import os
import signal
import subprocess
import sys
import time
from pathlib import Path


def main() -> int:
    if (
        sys.platform != "darwin"
        or os.geteuid() == 0
        or os.environ.get("USQUE_DISPOSABLE_MAC") != "1"
        or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"
        or os.environ.get("GITHUB_ACTIONS") != "true"
    ):
        raise RuntimeError("requires an ordinary user on a disposable GitHub-hosted Mac")
    # The supervisor stays outside the test's process group and needs no network.
    with subprocess.Popen(sys.argv[1:], start_new_session=True) as child:  # noqa: S603
        try:
            status = child.wait(timeout=360 if os.environ.get("USQUE_TEST_LIVE_WARP") == "1" else 180)
        except subprocess.TimeoutExpired:
            print("TUN test exceeded its local deadline; terminating the test group", flush=True)
            for sig in (signal.SIGTERM, signal.SIGKILL):
                subprocess.run(  # noqa: S603
                    ["/usr/bin/sudo", "-n", "/bin/kill", f"-{sig.name[3:]}", "--", f"-{child.pid}"],
                    check=False,
                )
                try:
                    child.wait(timeout=5)
                    break
                except subprocess.TimeoutExpired:
                    continue
            status = 124
    if status != 0:
        agent = Path("/Library/PrivilegedHelperTools/com.usque.agent/usque-agent")
        if agent.is_file():
            print("Test FAILED; attempting administrator recovery for the CI runner", flush=True)
            subprocess.run(  # noqa: S603
                ["/usr/bin/sudo", "-n", "/bin/launchctl", "bootout", "system/com.usque.agent"],
                check=False,
                timeout=45,
            )
            # launchctl may return while the terminating daemon still owns
            # service.lock. Use the maintenance tool's bounded recovery window.
            recovery_deadline = time.monotonic() + 15
            while True:
                result = subprocess.run(  # noqa: S603
                    ["/usr/bin/sudo", "-n", str(agent), "--recover-state"],
                    check=False,
                    timeout=45,
                    capture_output=True,
                    text=True,
                )
                if result.returncode == 0:
                    break
                if time.monotonic() >= recovery_deadline:
                    print(f"Runner recovery incomplete: {result.stderr.strip()}", file=sys.stderr)
                    break
                time.sleep(0.2)
    return status if status >= 0 else 1


if __name__ == "__main__":
    sys.exit(main())

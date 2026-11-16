"""Keep a local recovery path if a remote TUN test cuts off the Actions control channel."""

import os
import signal
import subprocess
import sys
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
            status = child.wait(timeout=180)
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
            for command in (
                ["/bin/launchctl", "bootout", "system/com.usque.agent"],
                [str(agent), "--recover-state"],
            ):
                subprocess.run(["/usr/bin/sudo", "-n", *command], check=False, timeout=45)  # noqa: S603
    return status if status >= 0 else 1


if __name__ == "__main__":
    sys.exit(main())

"""Report stages and symbolic crash frames from the synthetic hosted Engine only."""

import json
import os
import subprocess
import sys
import time
from pathlib import Path


def report(fixture, engine):
    log = fixture / "engine-state/logs/engine.jsonl"
    if log.is_file():
        for line in log.read_text().splitlines()[-30:]:
            try:
                fields = json.loads(line).get("fields", {})
            except json.JSONDecodeError:
                continue
            safe = {
                k: fields[k]
                for k in (
                    "message",
                    "stage",
                    "kind",
                    "agent_failure",
                    "transport_failure",
                    "error_code",
                )
                if k in fields
            }
            print(safe, flush=True)

    directory = Path.home() / "Library/Logs/DiagnosticReports"
    for _ in range(12):
        candidates = sorted(directory.glob("usque-engine*.ips"), key=os.path.getmtime)
        if candidates:
            break
        time.sleep(1)
    if not candidates:
        print("macOS did not publish an Engine crash report")
        return
    text = candidates[-1].read_text()
    start = json.JSONDecoder().raw_decode(text)[1]
    crash = json.loads(text[start:])
    thread = next((t for t in crash["threads"] if t.get("triggered")), None)
    if thread is None:
        return
    images = crash["usedImages"]
    for frame in thread["frames"][:60]:
        img = images[frame["imageIndex"]]
        symbol = frame.get("symbol")
        if not symbol and img.get("name") == engine.name:
            addr = hex(img["base"] + frame["imageOffset"])
            output = subprocess.run(  # noqa: S603
                ["/usr/bin/atos", "-o", str(engine), "-l", hex(img["base"]), addr],
                capture_output=True,
                text=True,
            )
            symbol = output.stdout.strip()
        print(img.get("name"), symbol or hex(frame.get("imageOffset", 0)))


if __name__ == "__main__":
    report(Path(sys.argv[1]), Path(sys.argv[2]))

"""Run with python -m tool.proton_poc; secrets are accepted only on a local TTY."""

import argparse
import hashlib
import json
import sys
from dataclasses import asdict
from pathlib import Path


def implementation_digest() -> str:
    root = Path(__file__).parent
    digest = hashlib.sha256()
    for path in sorted(root.iterdir()):
        if path.suffix == ".py" or path.name == "requirements.lock":
            digest.update(path.name.encode())
            digest.update(path.read_bytes())
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description="Memory-only Proton control-plane PoC; no tunnel")
    parser.add_argument("identity", choices=["guest", "login"])
    network = parser.add_mutually_exclusive_group(required=True)
    network.add_argument(
        "--direct", action="store_true", help="Explicit HTTPS through host network"
    )
    network.add_argument("--proxy", help="HTTP CONNECT or socks5h URL, without credentials")
    parser.add_argument("--interactive", action="store_true", help="Allow local TTY verification")
    parser.add_argument(
        "--verification-handoff",
        choices=["browser", "paste"],
        default="browser",
        help="Local manual-puzzle callback (default) or hidden success-message paste",
    )
    parser.add_argument(
        "--export-config", type=Path, help="New .conf in an existing private directory"
    )
    args = parser.parse_args()
    try:
        from .flow import LocalInteraction, run
        from .transport import PocError, Transport
    except ImportError:
        print('{"error":"install_locked_poc_dependencies"}')
        return 2
    if args.identity == "login" and not args.interactive:
        print('{"error":"login_requires_local_interactive_terminal"}')
        return 2
    if args.interactive and (not sys.stdin.isatty() or not sys.stderr.isatty()):
        print('{"error":"local_tty_required"}')
        return 2
    try:
        transport = Transport(
            app_version="external-vpn@0.1.0", direct=args.direct, proxy=args.proxy
        )
    except PocError as error:
        print(json.dumps({"error": error.category}))
        return 2
    result = run(
        transport,
        identity=args.identity,
        interaction=(
            LocalInteraction(verification_mode=args.verification_handoff)
            if args.interactive
            else None
        ),
        export_path=args.export_config,
    )
    report = asdict(result)
    report["implementation_sha256"] = implementation_digest()
    print(json.dumps(report, indent=2))
    return (
        0
        if (
            result.error == "none"
            and result.phase == "complete"
            and result.config_ready == "passed"
            and result.cleanup == "revoked"
        )
        else 1
    )


if __name__ == "__main__":
    sys.exit(main())

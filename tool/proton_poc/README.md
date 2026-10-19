# Proton control-plane PoC / Proton 控制面实验

This standalone CLI explores guest and SRP password authentication, rotating
sessions and session-bound VPN certificates. It does not open a tunnel. See the
[implementation plan](../../docs/PROTON_VPN_POC_PLAN.md), [pinned protocol
contract](../../docs/PROTON_VPN_PROTOCOL.md), and [validation
record](../../docs/PROTON_VPN_POC_VALIDATION.md).

## Prepare and run

Use Linux Python 3.10+ from the repository root. Only published wheels are
permitted; a platform without a compatible wheel must stop. This environment
is independent of Cargo, Flutter and Android SDKs:

```shell
python3 -m venv /tmp/usque-proton-poc-venv
/tmp/usque-proton-poc-venv/bin/python -m pip install --require-hashes --only-binary=:all: -r tool/proton_poc/requirements.lock
/tmp/usque-proton-poc-venv/bin/python -m tool.proton_poc guest --direct
```

Each invocation attempts one identity. There is no automatic loop creating
identities or retrying rejected authentication. `--direct` explicitly uses the
host's ordinary HTTPS network. Alternatively, choose an existing proxy:

```shell
/tmp/usque-proton-poc-venv/bin/python -m tool.proton_poc guest --proxy socks5h://127.0.0.1:1080
```

HTTP CONNECT (`http://127.0.0.1:8080`) and HTTPS CONNECT proxies are also supported.
Proxy credentials in command arguments are rejected. SOCKS requires `socks5h`
so the proxy resolves the destination. Environment proxy settings are ignored;
proxy failure ends the experiment. This tool does not start a proxy or VPN.

## Guest human verification / 游客人机验证

Guest mode can require a human challenge without an account password. Run this
in your own local terminal to permit the interactive handoff:

```shell
/tmp/usque-proton-poc-venv/bin/python -m tool.proton_poc guest --direct --interactive
```

On API `9001`, the CLI keeps the pending request and current session in memory,
shows the official challenge URL and a callback listener only on
`/dev/tty`, and waits for you. Open the URL as a normal browser tab, run the
provided listener once in that tab's developer console, then **drag the puzzle
yourself**. The listener handles only the official success message; it performs
no puzzle actions. A temporary `127.0.0.1` endpoint receives the result in a POST
body, validates its origin, one-use state and message, then lets the original
request continue. The listener contains a challenge hash and temporary state,
not authentication or verification tokens. It refuses another challenge page,
origin or window source. The endpoint closes on success, expiry or cancel.

This browser handoff has not passed a real challenge or a real-browser test.
Browser mixed-form/local-network controls and Windows-to-WSL loopback reachability
may prevent the POST; they are not silently relaxed. Ctrl-C cancels and cleans
up. Browser networking is separate from the CLI's selected network path. The
five-minute challenge deadline includes time spent opening the browser.
After cancelling, also close the verification tab: the CLI can close its own
endpoint, while the page listener otherwise remains until its deadline.

For a later explicitly started experiment, a fallback mode accepts a legitimate
`HUMAN_VERIFICATION_SUCCESS` message copied from local developer tools at a hidden
terminal prompt. It does not open a loopback listener:

```shell
/tmp/usque-proton-poc-venv/bin/python -m tool.proton_poc guest --direct --interactive --verification-handoff paste
```

The ordinary browser page has no general copy-token button. Only use this mode
if you can obtain the message locally. The CLI validates the advertised method
and retries that exact challenge-bound request once.

Without `--interactive`, `9001` ends with `required_not_completed` and attempts
applicable session cleanup. Solving a challenge in a separate official-app
session does not complete the PoC's pending challenge. API `8004` is an unknown
rejection, not a challenge response: the observed `8004` supplied no verified
human-verification handoff, so the CLI cannot turn it into a captcha session.

## Account login / 账号登录

For a user-provided account, run this yourself in a local interactive terminal:

```shell
/tmp/usque-proton-poc-venv/bin/python -m tool.proton_poc login --direct --interactive
```

Enter the username, password and TOTP only at the hidden local prompts. Never
send them through chat, arguments, environment variables or files. Without a
TTY, interactive prompts fail. This prototype supports SRP v4 and passwords
of 1–72 UTF-8 bytes; other SRP/account/second-factor modes end with an explicit
unsupported result. Python integer exponentiation is unsuitable as a production
constant-time SRP implementation.

The independent client identifies itself as `external-vpn@0.1.0` with the
`Usque-Proton-PoC` user agent. Its device frame describes Linux and uses the
independent `usque-linux-v4-challenge-0` namespace. Acceptance of this third-party
identity is unverified. Rejection is evidence of the precise failed stage;
the CLI never switches to an invented official Android identity.

## Results and cleanup

Stdout contains only an allowlisted JSON report with phase, fixed error
categories, numeric API/HTTP codes, timestamp, implementation digest and cleanup
status. Neither server error text nor raw responses are printed. A successful
`auth_ok` requires a protected user API response; `wg_authorized` requires a
TLS-delivered certificate matching the generated key and time bounds. Its issuer
chain is not independently verified. `config_ready` means the standard
configuration passed local structural validation. `tunnel_connected` stays
`not_run`. Certificate renewal/expiry is tested using mocks, not real elapsed
certificate lifetimes.

The official session-certificate flow additionally uses an in-tunnel local-agent
exchange. This CLI does not implement it. A directory candidate and locally
validated configuration establish neither server availability nor a working
WireGuard data path.

All session tokens and private keys stay in process memory by default. On
success, errors and Ctrl-C, cleanup attempts to revoke every session created by
this run, including superseded guest sessions if their UID changes. Account
login starts directly with SRP info and does not require the guest bootstrap.
`revocation_failed` is a distinct unsuccessful cleanup result; an ambiguous
session-creation or rotation response without a handle is
`unknown_without_session_handle`. When known handles are revoked but another
remote session may remain, cleanup is `remote_session_state_unknown`; a failed
revoke in that situation is `revocation_failed_remote_state_unknown`. Malformed
success responses cannot become cleanup passes.
This tool does not enumerate or revoke the user's other sessions. Python object
cleanup is not a guarantee of secret zeroization in RAM.

To explicitly export a configuration for structural inspection, first create
an existing private directory outside the checkout:

```shell
mkdir -m 700 /tmp/usque-proton-private
/tmp/usque-proton-poc-venv/bin/python -m tool.proton_poc guest --direct --export-config /tmp/usque-proton-private/guest.conf
```

The exporter rejects existing files, symlinks, repository destinations and
non-private parent directories; the new file has mode `0600`. It never prints
the private key or configuration. Session revocation can invalidate the exported
configuration. There is no retain-session mode or plaintext session persistence.
Do not use `wg-quick` or alter routes, DNS, proxies, firewall or TUN devices to
exercise this experiment.

## Offline validation

Activate the dedicated environment so the exact repository checks resolve its
Python, pip and Ruff. These checks do not contact Proton or use a real account:

```shell
. /tmp/usque-proton-poc-venv/bin/activate
pip install ruff==0.16.0
ruff check tool
ruff format --check tool
python -m unittest discover -s tool -p "test_*.py" -v
python tool/check_repository_policy.py
git diff --check
```

Run the full suite in the locked PoC environment: optional crypto/HTTP tests
explicitly skip when these dependencies are absent from an ordinary tooling
environment. A skip is not a PoC pass. Tests use synthetic sessions/certificates
and licensed MIT SRP vectors. Dependency versions, licenses and measured local
storage are recorded in [DEPENDENCIES.md](DEPENDENCIES.md).

The callback tests open only temporary IPv4 loopback sockets; they require local
bind permission. The script tests execute the actual generated listener in an
offline DOM fixture using an existing `/usr/bin/node` and skip explicitly if it
is unavailable. Node is a test helper, not a CLI dependency; no npm packages,
browser or automation framework are installed. Neither test proves a real
browser can navigate from the official challenge page to the callback.

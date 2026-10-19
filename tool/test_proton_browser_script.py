"""Execute the actual browser handoff snippet in Node's offline DOM fixture.

No browser, socket, fetch, or npm dependency is used. This exercises script
logic; HTTPS-page CSP, mixed-content navigation and browser UI remain untested.
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import subprocess
import unittest
from urllib.parse import parse_qs, urlencode, urlsplit

NODE = "/usr/bin/node" if os.access("/usr/bin/node", os.X_OK) else None
HAS_HTTPX = importlib.util.find_spec("httpx") is not None
START_MS = 2_000_000_000_000
EXPIRES_MS = START_MS + 120000
FIXTURE = {
    "challenge": "synthetic-private-browser-challenge",
    "state": "synthetic-private-browser-state",
    "result": "synthetic-private-browser-result",
    "callback": "http://127.0.0.1:43129/human-verification",
}

# The program is fixed test code. The generated snippet and event fixtures are
# passed through stdin JSON; they never become command-line or shell arguments.
NODE_FIXTURE = r"""
const fs = require('node:fs');
const vm = require('node:vm');
const webcrypto = require('node:crypto').webcrypto;
const input = JSON.parse(fs.readFileSync(0, 'utf8'));

async function run() {
    const listeners = new Map();
    const timers = new Map();
    const forms = [];
    const metas = [];
    const submissions = [];
    const logs = [];
    let nextTimer = 1;
    let installed = 0;
    let clockMs = input.scenario.clock_ms;
    class FixtureDate extends Date {
        static now() { return clockMs; }
    }

    class Element {
        constructor(tag) {
            this.tagName = tag.toUpperCase();
            this.children = [];
            this.style = {};
        }
        setAttribute(name, value) { this[name] = String(value); }
        appendChild(child) { this.children.push(child); return child; }
        append(...children) { this.children.push(...children); }
        remove() {}
    }
    class Form extends Element {
        constructor() { super('form'); forms.push(this); }
        submit() {
            const fields = this.children.map(item => [item.name, item.value]);
            submissions.push({
                action: this.action,
                method: this.method,
                target: this.target,
                fields,
                body: new URLSearchParams(fields).toString(),
            });
        }
    }
    const head = new Element('head');
    const body = new Element('body');
    const document = {
        head,
        body,
        createElement(tag) {
            if (tag === 'form') return new Form();
            const element = new Element(tag);
            if (tag === 'meta') metas.push(element);
            return element;
        },
    };
    const location = new URL(input.scenario.url);
    const window = {
        location,
        document,
        addEventListener(name, handler) {
            if (!listeners.has(name)) listeners.set(name, new Set());
            listeners.get(name).add(handler);
            if (name === 'message') installed++;
        },
        removeEventListener(name, handler) { listeners.get(name)?.delete(handler); },
    };
    window.top = input.scenario.framed ? {} : window;
    const context = vm.createContext({
        window,
        document,
        location,
        URL,
        URLSearchParams,
        Date: FixtureDate,
        crypto: {
            subtle: {
                async digest(...args) {
                    const bytes = await webcrypto.subtle.digest(...args);
                    if (typeof input.scenario.digest_clock_ms === 'number') {
                        clockMs = input.scenario.digest_clock_ms;
                    }
                    if (typeof input.scenario.digest_url === 'string') {
                        window.location.href = input.scenario.digest_url;
                    }
                    if (input.scenario.digest_framed) window.top = {};
                    return bytes;
                },
            },
        },
        TextEncoder,
        HTMLFormElement: Form,
        setTimeout(handler, delay) {
            const id = nextTimer++;
            timers.set(id, {handler, delay});
            return id;
        },
        clearTimeout(id) { timers.delete(id); },
        console: Object.fromEntries(
            ['log', 'info', 'warn', 'error', 'debug'].map(name => [name, (...args) => {
                logs.push(args.map(String).join(' '));
            }])
        ),
    });
    let error = null;
    try {
        await vm.runInContext(input.snippet, context, {timeout: 1000});
    } catch (failure) {
        error = String(failure.message);
    }
    for (const event of input.scenario.events || []) {
        if (event.clock_ms !== undefined) clockMs = event.clock_ms;
        if (typeof event.url === 'string') window.location.href = event.url;
        if (event.framed !== undefined) window.top = event.framed ? {} : window;
        const message = {
            origin: event.origin || 'https://verify.proton.me',
            source: event.source === 'other' ? {} : window,
            data: event.data,
        };
        for (const handler of [...(listeners.get('message') || [])]) {
            await handler(message);
        }
    }
    return {
        error,
        installed,
        active: listeners.get('message')?.size || 0,
        timers: timers.size,
        submissions,
        forms: forms.map(form => ({action: form.action, method: form.method, target: form.target})),
        metas: metas.map(meta => ({name: meta.name, content: meta.content})),
        logs,
    };
}
run().then(result => process.stdout.write(JSON.stringify(result))).catch(() => {
    process.stderr.write('offline_vm_fixture_failed');
    process.exitCode = 1;
});
"""


@unittest.skipUnless(NODE and HAS_HTTPX, "Node and pinned PoC dependencies are required")
class ProtonBrowserScriptTests(unittest.TestCase):
    @staticmethod
    def success_message(*, method="captcha", result=None, event_type="HUMAN_VERIFICATION_SUCCESS"):
        return json.dumps(
            {
                "type": event_type,
                "payload": {
                    "type": method,
                    "token": FIXTURE["result"] if result is None else result,
                },
            },
            separators=(",", ":"),
        )

    def execute(
        self,
        *,
        url=None,
        events=(),
        framed=False,
        clock_ms=START_MS,
        digest_clock_ms=None,
        digest_url=None,
        digest_framed=False,
    ):
        from proton_poc.browser_handoff import _make_snippet

        snippet = _make_snippet(
            FIXTURE["callback"],
            FIXTURE["state"],
            hashlib.sha256(FIXTURE["challenge"].encode("ascii")).hexdigest(),
            ("captcha",),
            120000,
            EXPIRES_MS,
        )
        scenario = {
            "url": url or "https://verify.proton.me/?" + urlencode({"token": FIXTURE["challenge"]}),
            "events": list(events),
            "framed": framed,
            "clock_ms": clock_ms,
            "digest_clock_ms": digest_clock_ms,
            "digest_url": digest_url,
            "digest_framed": digest_framed,
        }
        completed = subprocess.run(  # noqa: S603 - fixed Node harness; script and fixtures use stdin.
            [NODE, "-e", NODE_FIXTURE],
            input=json.dumps({"snippet": snippet, "scenario": scenario}),
            text=True,
            capture_output=True,
            check=False,
            timeout=10,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(completed.stderr, "")
        result = json.loads(completed.stdout)
        self.assertEqual(result["logs"], [])
        return result

    def test_wrong_page_origin_token_or_frame_never_installs_a_submitter(self):
        cases = [
            {"url": "https://attacker.invalid/?" + urlencode({"token": FIXTURE["challenge"]})},
            {"url": "https://verify.proton.me/?token=synthetic-wrong-challenge"},
            {"url": "https://verify.proton.me/"},
            {
                "url": "https://verify.proton.me/?"
                + urlencode([("token", FIXTURE["challenge"]), ("token", FIXTURE["challenge"])])
            },
            {"framed": True},
        ]
        for case in cases:
            with self.subTest(case=case):
                result = self.execute(events=[{"data": self.success_message()}], **case)
                self.assertEqual(result["installed"], 0)
                self.assertEqual(result["active"], 0)
                self.assertEqual(result["submissions"], [])
                self.assertEqual(result["forms"], [])

    def test_untrusted_or_malformed_events_never_submit(self):
        success = self.success_message()
        events = [
            {"origin": "https://attacker.invalid", "data": success},
            {"origin": "https://verify.proton.me.attacker.invalid", "data": success},
            {"source": "other", "data": success},
            {"data": json.loads(success)},
            {"data": "not-json"},
            {"data": self.success_message(event_type="HUMAN_VERIFICATION_ERROR")},
            {"data": self.success_message(method="sms")},
            {"data": self.success_message(result="")},
            {"data": self.success_message(result="synthetic\ninvalid")},
            {"data": self.success_message(result="synthetic-\N{SNOWMAN}")},
            {"data": success + " " * 32769},
        ]
        for index, event in enumerate(events):
            with self.subTest(index=index):
                result = self.execute(events=[event])
                self.assertIsNone(result["error"])
                self.assertEqual(result["installed"], 1)
                self.assertEqual(result["active"], 1)
                self.assertEqual(result["submissions"], [])
                self.assertEqual(result["forms"], [])

    def test_current_trusted_success_submits_once_with_private_values_only_in_body(self):
        message = self.success_message()
        result = self.execute(
            events=[
                {"source": "other", "data": message},
                {"data": message},
                {"data": message},
            ]
        )
        self.assertIsNone(result["error"])
        self.assertEqual(result["installed"], 1)
        self.assertEqual(result["active"], 0)
        self.assertEqual(result["timers"], 0)
        self.assertEqual(len(result["submissions"]), 1)
        submission = result["submissions"][0]
        self.assertEqual(submission["action"], FIXTURE["callback"])
        self.assertEqual(submission["method"].upper(), "POST")
        self.assertEqual(submission["target"], "_self")
        self.assertEqual(submission["fields"], [["state", FIXTURE["state"]], ["message", message]])
        self.assertEqual(
            parse_qs(submission["body"]), {"state": [FIXTURE["state"]], "message": [message]}
        )
        self.assertEqual(result["metas"], [{"name": "referrer", "content": "origin"}])
        self.assertEqual(len(result["forms"]), 1)
        for form in result["forms"]:
            self.assertEqual(urlsplit(form["action"]).query, "")
            self.assertEqual(urlsplit(form["action"]).fragment, "")
            for private in (FIXTURE["challenge"], FIXTURE["state"], FIXTURE["result"], message):
                self.assertNotIn(private, form["action"])

    def test_pasted_after_absolute_expiry_never_installs_or_submits(self):
        cases = (
            {"clock_ms": EXPIRES_MS},
            {"clock_ms": EXPIRES_MS + 1000},
            # The real SHA256 is asynchronous; expiry while awaiting it must
            # also prevent installation of a callback to a recycled local port.
            {"clock_ms": EXPIRES_MS - 1, "digest_clock_ms": EXPIRES_MS},
        )
        for scenario in cases:
            with self.subTest(scenario=scenario):
                result = self.execute(events=[{"data": self.success_message()}], **scenario)
                self.assertEqual(result["installed"], 0)
                self.assertEqual(result["active"], 0)
                self.assertEqual(result["timers"], 0)
                self.assertEqual(result["submissions"], [])
                self.assertEqual(result["forms"], [])
                self.assertEqual(result["metas"], [])

    def test_success_at_or_after_absolute_expiry_cleans_up_without_submitting(self):
        for instant in (EXPIRES_MS, EXPIRES_MS + 1000):
            with self.subTest(instant=instant):
                result = self.execute(
                    events=[
                        {"clock_ms": instant, "data": self.success_message()},
                        {"clock_ms": instant + 1, "data": self.success_message()},
                    ]
                )
                self.assertIsNone(result["error"])
                self.assertEqual(result["installed"], 1)
                self.assertEqual(result["active"], 0)
                self.assertEqual(result["timers"], 0)
                self.assertEqual(result["submissions"], [])
                self.assertEqual(result["forms"], [])
                self.assertEqual(result["metas"], [])

    @staticmethod
    def changed_pages():
        return (
            "https://verify.proton.me/?token=synthetic-other-challenge",
            "https://verify.proton.me/?"
            + urlencode([("token", FIXTURE["challenge"]), ("token", FIXTURE["challenge"])]),
            "https://attacker.invalid/?" + urlencode({"token": FIXTURE["challenge"]}),
            "https://verify.proton.me/",
        )

    def test_page_changed_during_async_digest_never_installs_a_submitter(self):
        cases = [{"digest_url": page} for page in self.changed_pages()]
        cases.append({"digest_framed": True})
        for scenario in cases:
            with self.subTest(scenario=scenario):
                result = self.execute(events=[{"data": self.success_message()}], **scenario)
                self.assertEqual(result["installed"], 0)
                self.assertEqual(result["active"], 0)
                self.assertEqual(result["timers"], 0)
                self.assertEqual(result["submissions"], [])
                self.assertEqual(result["forms"], [])
                self.assertEqual(result["metas"], [])

    def test_page_changed_after_install_discards_success_and_cleans_up(self):
        cases = [{"url": page} for page in self.changed_pages()]
        cases.append({"framed": True})
        original_url = "https://verify.proton.me/?" + urlencode({"token": FIXTURE["challenge"]})
        for navigation in cases:
            with self.subTest(navigation=navigation):
                result = self.execute(
                    events=[
                        {**navigation, "data": self.success_message()},
                        # Returning to the original URL cannot reuse a listener
                        # retired because its challenge context changed.
                        {"url": original_url, "framed": False, "data": self.success_message()},
                    ]
                )
                self.assertIsNone(result["error"])
                self.assertEqual(result["installed"], 1)
                self.assertEqual(result["active"], 0)
                self.assertEqual(result["timers"], 0)
                self.assertEqual(result["submissions"], [])
                self.assertEqual(result["forms"], [])
                self.assertEqual(result["metas"], [])


if __name__ == "__main__":
    unittest.main()

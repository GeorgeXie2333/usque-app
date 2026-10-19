"""Offline native bridge regression using generated C++/Kotlin scripts.

The independent DOM fixture models the observed official interfaces from public
verify bundle 5.0.11.0 (SHA256 6984f7c6b16509a36440fe70433783cc637fcdd1a6a25fe1f9c5a9e9eb53c67e).
It includes platform selection and the actual iframe pm_captcha -> submit(token,
'captcha') -> embedded success routing. It does not vendor Proton JavaScript,
solve a puzzle, open a browser, authenticate, or prove real runtime behavior.
"""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CXX = shutil.which("c++")
NODE = shutil.which("node")
START_MS = 2_000_000_000_000
EXPIRES_MS = START_MS + 120000
FIXTURE = {
    "challenge": "synthetic-native-challenge",
    "result": "synthetic-native-captcha-result",
    "nonce": "a" * 32,
}
URL = f"https://verify.proton.me/?token={FIXTURE['challenge']}&embed=true&methods=captcha"

NODE_FIXTURE = r"""
const fs = require('node:fs');
const vm = require('node:vm');
const realCrypto = require('node:crypto').webcrypto;
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
async function run() {
    const listeners = new Map(), timers = new Set(), reports = [], privateMessages = [], logs = [];
    const pending = [];
    let clock = input.start, digestPromise;
    class Clock extends Date { static now() { return clock; } }
    const frameWindow = {}, unrelatedWindow = {};
    const frame = {src:input.frame_url, contentWindow:frameWindow};
    let frameVisible = true;
    const emit = event => {
        const handlers = [...listeners].sort((a,b)=>Number(b[1])-Number(a[1]));
        for(const [handler,capture] of handlers) {
            // A submit handler can remove the puzzle before bubble listeners.
            if(!capture && input.frame_removed_on_submit) frameVisible = false;
            handler(event);
        }
    };
    const window = {
        location:new URL(input.url), navigator:{userAgent:'fixture-browser'},
        addEventListener(type, handler, capture=false) { if(type==='message') listeners.set(handler,capture); },
        removeEventListener(type, handler) { if(type==='message') listeners.delete(handler); },
        alert(value) { if(!input.suppress_alert) reports.push(JSON.parse(value)); },
    };
    window.top = input.framed ? {} : window;
    window.parent = window;
    // The frame result and the subsequent top-level success are separate
    // messages. Exercise a missing second delivery without changing identity.
    window.postMessage = value => {
        if(!input.suppress_top_level_dispatch) pending.push({origin:'https://verify.proton.me', source:window, data:value});
    };
    // Match the exposed platform capabilities without changing the page's
    // selector. The dedicated native message channel must not change the
    // official page's choice of the empty webview branch.
    const originalChrome = {webview:{postMessage(value){reports.push(JSON.parse(value));}}};
    if(input.platform==='windows') window.chrome = originalChrome;
    if(input.platform==='linux') window.webkit = {messageHandlers:{proton:{postMessage(value){reports.push(JSON.parse(value));}}}};
    const port = {postMessage(value){privateMessages.push(value);},close(){}};
    const document = {querySelectorAll(name){return name==='iframe' && frameVisible ? (input.duplicate_frame ? [frame, frame] : [frame]) : [];}};
    const context = vm.createContext({window, document, URL, Date:Clock, TextEncoder,
        crypto:{subtle:{digest(...args){
            digestPromise=realCrypto.subtle.digest(...args).then(value=>{
                if(input.digest_url) window.location=new URL(input.digest_url);
                if(input.digest_expired) clock=input.expires;
                return value;
            }); return digestPromise;
        }}},
        setTimeout(handler){timers.add(handler);return handler;}, clearTimeout(handler){timers.delete(handler);},
        console:Object.fromEntries(['log','error','warn','debug'].map(name=>[name,(...args)=>logs.push(args)])),
    });
    vm.runInContext(input.script,context,{timeout:1000});
    if(input.platform==='android') {
        emit({data:input.nonce,ports:[port],origin:'',source:null});
    }
    if(digestPromise) await digestPromise;
    await new Promise(resolve=>setImmediate(resolve));
    if(input.event_url) window.location=new URL(input.event_url);
    if(input.event_expired) clock=input.expires;
    let selected = 'web';
    if(window.AndroidInterface !== undefined) selected='android';
    else if(window.chrome && window.chrome.webview) selected='webview';
    // Independent interface fixture: the official embedded captcha callback
    // uses the raw frame token unchanged. No token concatenation or bind API is
    // used with embed=true. The webview branch intentionally emits nothing.
    const officialDispatch = value => {
        if(selected==='android') window.AndroidInterface.dispatch(JSON.stringify(value));
        else if(selected==='web') window.parent.postMessage(JSON.stringify(value),'*');
    };
    const event = {origin:input.origin, source:input.other_source ? unrelatedWindow : frameWindow,
        data:{type:'pm_captcha',token:input.result}};
    for(let repeat=0;repeat<2;repeat++) {
        if(!input.top_level_only) emit(event);
        if(event.origin==='https://verify.proton.me' && event.source===frameWindow) {
            officialDispatch({type:'HUMAN_VERIFICATION_SUCCESS',payload:{token:event.data.token,type:input.method||'captcha'}});
        }
        while(pending.length) {
            const message = pending.shift();
            emit(message);
        }
    }
    return {selected,reports,privateMessages,logs,active:listeners.size,
        chrome_preserved:input.platform!=='windows'||window.chrome===originalChrome,
        no_identity_shim:window.AndroidInterface===undefined && !window.webkit?.messageHandlers?.iOS};
}
run().then(value=>process.stdout.write(JSON.stringify(value))).catch(()=>{
    process.stderr.write('native_bridge_fixture_failed');process.exitCode=1;
});
"""


@unittest.skipUnless(CXX and NODE, "native bridge VM requires existing C++ and Node tools")
class NativeBridgeTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.temporary = tempfile.TemporaryDirectory(prefix="usque-native-bridge-")
        cls.addClassCleanup(cls.temporary.cleanup)
        directory = Path(cls.temporary.name)
        helper = directory / "script.cc"
        executable = directory / "script"
        digest = hashlib.sha256(FIXTURE["challenge"].encode()).hexdigest()
        header = ROOT / "apps/usque_gui/native/proton_verification.h"
        helper.write_text(
            f"#include {json.dumps(str(header))}\n"
            "#include <iostream>\n"
            "int main(int argc,char**){usque::proton::Handoff h;"
            f"h.expires={EXPIRES_MS};h.digest={json.dumps(digest)};h.url={json.dumps(URL)};"
            'h.methods={"captcha","email"};if(argc>1){'
            "if(!usque::proton::CaptchaOnly(h))return 2;"
            f"std::cout<<usque::proton::CaptchaScript(h,{json.dumps(FIXTURE['nonce'])});"
            "}else{std::cout<<usque::proton::Script(h,"
            + json.dumps(
                "window.webkit.messageHandlers.proton.postMessage("
                f"JSON.stringify({{nonce:'{FIXTURE['nonce']}',message}}));"
            )
            + ");}}\n",
            encoding="utf-8",
        )
        subprocess.run(  # noqa: S603 - compiler and arguments are local controlled fixture paths.
            [CXX, "-std=c++17", "-Wall", "-Wextra", "-Werror", str(helper), "-o", str(executable)],
            check=True,
            capture_output=True,
            timeout=30,
        )
        cls.scripts = {}
        for platform, arguments in (("windows", ["captcha"]), ("linux", [])):
            cls.scripts[platform] = subprocess.run(  # noqa: S603 - freshly compiled local script producer only.
                [str(executable), *arguments],
                check=True,
                capture_output=True,
                text=True,
                timeout=5,
            ).stdout
        source = (
            ROOT
            / "apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/ProtonVerificationSession.kt"
        ).read_text(encoding="utf-8")
        template = source.split('return """', 1)[1].split('""".trimIndent()', 1)[0]
        cls.scripts["android"] = textwrap.dedent(template).replace(
            "$settings",
            json.dumps(
                {
                    "nonce": FIXTURE["nonce"],
                    "digest": digest,
                    "expires": EXPIRES_MS,
                    "methods": ["captcha"],
                }
            ),
        )

    def run_script(self, platform: str, **changes: object) -> dict[str, object]:
        scenario = {
            "script": self.scripts[platform],
            "platform": platform,
            "start": START_MS,
            "expires": EXPIRES_MS,
            "url": URL,
            "frame_url": f"https://verify.proton.me/core/v4/captcha?Token={FIXTURE['challenge']}&ForceWebMessaging=1",
            "nonce": FIXTURE["nonce"],
            "origin": "https://verify.proton.me",
            "result": FIXTURE["result"],
            **changes,
        }
        completed = subprocess.run(  # noqa: S603 - existing Node runs only this fixed offline VM fixture.
            [NODE, "-e", NODE_FIXTURE],
            input=json.dumps(scenario),
            check=True,
            text=True,
            capture_output=True,
            timeout=5,
        )
        self.assertEqual(completed.stderr, "")
        result = json.loads(completed.stdout)
        self.assertEqual(result["logs"], [])
        self.assertTrue(result["no_identity_shim"])
        return result

    def test_windows_binds_real_captcha_interface_despite_empty_official_webview_dispatch(
        self,
    ) -> None:
        result = self.run_script("windows")
        self.assertEqual(result["selected"], "webview")
        self.assertTrue(result["chrome_preserved"])
        reports = result["reports"]
        self.assertEqual(len(reports), 1)
        self.assertEqual(reports[0]["nonce"], FIXTURE["nonce"])
        value = json.loads(reports[0]["message"])
        self.assertEqual(
            value,
            {
                "type": "HUMAN_VERIFICATION_SUCCESS",
                "payload": {"token": FIXTURE["result"], "type": "captcha"},
            },
        )
        self.assertEqual(result["active"], 0)

    def test_windows_completion_does_not_depend_on_script_dialog_delivery(self) -> None:
        result = self.run_script("windows", suppress_alert=True)
        self.assertEqual(len(result["reports"]), 1)
        self.assertEqual(
            json.loads(result["reports"][0]["message"])["payload"]["token"], FIXTURE["result"]
        )

    def test_desktop_captcha_survives_missing_top_dispatch_and_removed_puzzle(self) -> None:
        for platform in ("windows", "linux"):
            with self.subTest(platform=platform):
                result = self.run_script(
                    platform,
                    suppress_top_level_dispatch=True,
                    frame_removed_on_submit=True,
                )
                self.assertEqual(len(result["reports"]), 1)
                self.assertEqual(result["active"], 0)

    def test_linux_and_android_receive_official_web_branch_via_their_distinct_native_bridges(
        self,
    ) -> None:
        for platform in ("linux", "android"):
            with self.subTest(platform=platform):
                result = self.run_script(platform, top_level_only=True)
                self.assertEqual(result["selected"], "web")
                messages = (
                    result["privateMessages"]
                    if platform == "android"
                    else [v["message"] for v in result["reports"]]
                )
                self.assertEqual(len(messages), 1)
                self.assertEqual(
                    json.loads(messages[0])["payload"],
                    {"token": FIXTURE["result"], "type": "captcha"},
                )
                self.assertEqual(result["active"], 0)

    def test_linux_retains_non_captcha_top_level_completion(self) -> None:
        result = self.run_script("linux", top_level_only=True, method="email")
        self.assertEqual(len(result["reports"]), 1)
        self.assertEqual(json.loads(result["reports"][0]["message"])["payload"]["type"], "email")

    def test_desktop_direct_captcha_requires_embedded_challenge(self) -> None:
        for platform in ("windows", "linux"):
            for url in (
                URL.replace("embed=true", "embed=false"),
                URL.replace("embed=true&", ""),
                URL + "&embed=true",
            ):
                with self.subTest(platform=platform, url=url):
                    self.assertEqual(
                        self.run_script(platform, url=url, suppress_top_level_dispatch=True)[
                            "reports"
                        ],
                        [],
                    )

    def test_android_captcha_result_survives_an_unavailable_top_level_dispatch(self) -> None:
        result = self.run_script("android", suppress_top_level_dispatch=True)
        self.assertEqual(result["selected"], "web")
        self.assertEqual(len(result["privateMessages"]), 1)
        self.assertEqual(
            json.loads(result["privateMessages"][0]),
            {
                "type": "HUMAN_VERIFICATION_SUCCESS",
                "payload": {"token": FIXTURE["result"], "type": "captcha"},
            },
        )
        self.assertEqual(result["active"], 0)

    def test_android_direct_captcha_result_requires_the_bound_frame_and_live_challenge(
        self,
    ) -> None:
        for change in (
            {"origin": "https://evil.example"},
            {"other_source": True},
            {
                "frame_url": "https://verify.proton.me/other?Token=synthetic-native-challenge&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://evil.example/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=wrong&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&Token=wrong&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=0"
            },
            {
                "frame_url": "https://user@verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=1&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=1#fragment"
            },
            {"duplicate_frame": True},
            {"result": "invalid\r\nheader"},
            {"framed": True},
            {"digest_url": URL.replace(FIXTURE["challenge"], "another")},
            {"digest_expired": True},
            {"event_url": URL.replace(FIXTURE["challenge"], "another")},
            {"event_expired": True},
        ):
            with self.subTest(change=change):
                result = self.run_script("android", suppress_top_level_dispatch=True, **change)
                self.assertEqual(result["privateMessages"], [])

    def test_desktop_rejects_unbound_frame_origin_source_path_and_query(self) -> None:
        changes = [
            {"origin": "https://evil.example"},
            {"other_source": True},
            {
                "frame_url": "https://verify.proton.me/other?Token=synthetic-native-challenge&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://evil.example/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=wrong&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&Token=wrong&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=0"
            },
            {
                "frame_url": "https://user@verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=1&ForceWebMessaging=1"
            },
            {
                "frame_url": "https://verify.proton.me/core/v4/captcha?Token=synthetic-native-challenge&ForceWebMessaging=1#fragment"
            },
            {"duplicate_frame": True},
            {"result": "invalid\r\nheader"},
        ]
        for platform in ("windows", "linux"):
            for change in changes:
                with self.subTest(platform=platform, change=change):
                    self.assertEqual(
                        self.run_script(platform, suppress_top_level_dispatch=True, **change)[
                            "reports"
                        ],
                        [],
                    )

    def test_desktop_rechecks_top_challenge_and_deadline_during_hash_and_event(self) -> None:
        changes = (
            {"framed": True},
            {"url": URL.replace("token=", "wrong=")},
            {"url": URL + "&token=other"},
            {"digest_url": URL.replace(FIXTURE["challenge"], "another")},
            {"digest_expired": True},
            {"event_url": URL.replace(FIXTURE["challenge"], "another")},
            {"event_expired": True},
            {"start": EXPIRES_MS},
        )
        for platform in ("windows", "linux"):
            for change in changes:
                with self.subTest(platform=platform, change=change):
                    self.assertEqual(
                        self.run_script(platform, suppress_top_level_dispatch=True, **change)[
                            "reports"
                        ],
                        [],
                    )


if __name__ == "__main__":
    unittest.main()

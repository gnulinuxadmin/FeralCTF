#!/usr/bin/env python3
"""FeralCTF browser UI tests (Selenium + Chrome).

    python3 tests/ui/run_ui_tests.py              # build, run all scenarios
    python3 tests/ui/run_ui_tests.py -k branding  # only scenarios matching "branding"
    python3 tests/ui/run_ui_tests.py --headed     # watch the browser

Needs Python 3.9+ and Cargo. The first run creates tests/ui/.venv and installs
the pinned packages from requirements.txt. Selenium Manager then uses an
installed Chrome, or downloads Chrome for Testing and chromedriver into
~/.cache/selenium if there is none (network needed once). Each run builds the debug
binary, starts it on a free port with a fresh database in a temporary
directory, seeds data through the API, drives the UI, and cleans up. Failures
save a screenshot and keep the temporary directory for inspection.
"""

import argparse
import functools
import http.server
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import traceback
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
VENV = HERE / ".venv"


def ensure_environment():
    """Re-run inside tests/ui/.venv, creating it and installing tooling if needed."""
    venv_python = VENV / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    if Path(sys.prefix).resolve() == VENV.resolve():
        return
    if not venv_python.exists():
        print("setting up tests/ui/.venv (first run)...", flush=True)
        subprocess.run([sys.executable, "-m", "venv", str(VENV)], check=True)
        subprocess.run([str(venv_python), "-m", "pip", "install", "--quiet", "--upgrade", "pip"], check=True)
    subprocess.run(
        [str(venv_python), "-m", "pip", "install", "--quiet", "-r", str(HERE / "requirements.txt")],
        check=True,
    )
    os.execv(str(venv_python), [str(venv_python), str(Path(__file__).resolve()), *sys.argv[1:]])


ensure_environment()

import requests  # noqa: E402
from selenium import webdriver  # noqa: E402

ADMIN = ("admin", "adminpass1")
PLAYER = ("alice", "password123")


# ---- server and API helpers ----------------------------------------------


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Server:
    def __init__(self, binary, workdir):
        self.workdir = workdir
        self.port = free_port()
        self.url = f"http://127.0.0.1:{self.port}"
        subprocess.run(
            [binary, "--config", "config.toml", "init"],
            cwd=workdir, check=True, stdout=subprocess.DEVNULL,
        )
        self.log = open(workdir / "server.log", "w")
        self.process = subprocess.Popen(
            [binary, "--config", "config.toml", "--port", str(self.port)],
            cwd=workdir, stdout=self.log, stderr=subprocess.STDOUT,
        )
        for _ in range(100):
            try:
                requests.get(self.url + "/api/competition", timeout=1)
                return
            except requests.ConnectionError:
                if self.process.poll() is not None:
                    raise RuntimeError(f"server exited; see {workdir / 'server.log'}")
                time.sleep(0.1)
        raise RuntimeError("server did not start")

    def api(self, method, path, body=None, token=None):
        headers = {"Authorization": f"Bearer {token}"} if token else {}
        response = requests.request(method, self.url + path, json=body, headers=headers, timeout=10)
        if not response.ok:
            raise RuntimeError(f"{method} {path} -> {response.status_code}: {response.text}")
        return response.json() if response.content else None

    def login(self, user):
        return self.api("POST", "/api/auth/login", {"username": user[0], "password": user[1]})["token"]

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
        self.log.close()


class LogoHost:
    """Serves a logo from a second origin to exercise the branding CSP."""

    def __init__(self, workdir):
        directory = workdir / "logo"
        directory.mkdir()
        shutil.copy(REPO / "frontend" / "favicon.png", directory / "logo.png")
        handler = functools.partial(QuietHandler, directory=str(directory))
        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", free_port()), handler)
        self.url = f"http://127.0.0.1:{self.httpd.server_port}/logo.png"
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def stop(self):
        self.httpd.shutdown()


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def challenge_body(title, flag, points, **extra):
    body = {
        "title": title, "category": "web", "description": "", "flag": flag,
        "flag_type": "static", "flag_case_sensitive": False, "points": points,
        "max_points": points, "min_points": max(1, points // 5), "decay_rate": 10,
        "author": None, "tags": [], "unlock_requires": None, "is_hidden": False,
    }
    body.update(extra)
    return body


def seed(server):
    """Admin, a player on team Red with 250 points, and challenges with hints/attachments."""
    server.api("POST", "/api/auth/register", {"username": ADMIN[0], "password": ADMIN[1]})
    admin = server.login(ADMIN)
    warmup = server.api("POST", "/api/admin/challenges", challenge_body("Warmup", "flag{warm}", 250), admin)
    packet = server.api("POST", "/api/admin/challenges", challenge_body(
        "Packet Party", "flag{pcap}", 150, category="forensics",
        description="Look at the capture.\nDocs: https://example.com/pcap",
    ), admin)
    server.api("POST", "/api/admin/challenges", challenge_body(
        "Locked Door", "flag{door}", 100, category="misc", unlock_requires=packet["id"],
    ), admin)
    server.api("POST", f"/api/admin/challenges/{packet['id']}/hints",
               {"content": "Filter on HTTP.\nThen follow the stream.", "cost_points": 0}, admin)
    server.api("POST", f"/api/admin/challenges/{packet['id']}/hints",
               {"content": "The flag is in a cookie", "cost_points": 50}, admin)
    server.api("POST", f"/api/admin/challenges/{packet['id']}/files",
               {"label": "capture.pcap", "url": "https://example.com/capture.pcap"}, admin)
    server.api("POST", "/api/admin/users", {
        "username": PLAYER[0], "password": PLAYER[1], "password_confirm": PLAYER[1],
        "team": {"new_name": "Red"},
    }, admin)
    server.api("POST", f"/api/challenges/{warmup['id']}/submit", {"flag": "flag{warm}"}, server.login(PLAYER))


# ---- browser wrapper --------------------------------------------------------


class Browser:
    """One fresh Chrome per scenario.

    JavaScript runs through the DevTools protocol (`Runtime.evaluate`) so test
    code is not blocked by the page's CSP, while the page itself still is.
    confirm()/alert() are answered "OK" and their messages recorded, and
    browser-log errors (exceptions, CSP violations, failed requests) collected.
    """

    def __init__(self, headed=False):
        options = webdriver.ChromeOptions()
        if not headed:
            options.add_argument("--headless=new")
        options.add_argument("--window-size=1280,1000")
        options.set_capability("goog:loggingPrefs", {"browser": "ALL"})
        self.driver = webdriver.Chrome(options=options)
        self.console_errors = []
        # Installed before every page load, so it survives navigation and reloads.
        self.driver.execute_cdp_cmd("Page.addScriptToEvaluateOnNewDocument", {"source": """
            window.__dialogs = JSON.parse(sessionStorage.getItem('__dialogs') || '[]');
            const record = (message) => {
              window.__dialogs.push(String(message));
              sessionStorage.setItem('__dialogs', JSON.stringify(window.__dialogs));
            };
            window.confirm = (message) => { record(message); return true; };
            window.alert = (message) => { record(message); };
        """})

    @property
    def dialogs(self):
        return self.evaluate("window.__dialogs || []")

    def _collect_logs(self):
        for entry in self.driver.get_log("browser"):
            if entry["level"] == "SEVERE":
                self.console_errors.append(entry["message"])

    def evaluate(self, expression):
        """Run a JavaScript expression in the page (promises awaited)."""
        result = self.driver.execute_cdp_cmd("Runtime.evaluate", {
            "expression": expression,
            "awaitPromise": True,
            "returnByValue": True,
        })
        if "exceptionDetails" in result:
            details = result["exceptionDetails"]
            text = details.get("exception", {}).get("description") or details.get("text")
            raise AssertionError(f"JavaScript error: {text}\n  in: {expression[:200]}")
        return result["result"].get("value")

    def wait_for(self, expression, timeout=10, message=None):
        """Poll a JavaScript expression until it is truthy (errors count as not yet)."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                if self.evaluate(expression):
                    return
            except Exception:  # noqa: BLE001 - element missing or page reloading
                pass
            time.sleep(0.15)
        raise AssertionError(message or f"timed out waiting for: {expression}")

    def goto(self, url):
        self.driver.get(url)

    def screenshot(self, path):
        self.driver.save_screenshot(str(path))

    def finish(self):
        """Gather remaining browser-log errors before the checks run."""
        self._collect_logs()

    def close(self):
        self.driver.quit()


# ---- UI helpers -------------------------------------------------------------


def js(value):
    return json.dumps(value)


def check(condition, message):
    if not condition:
        raise AssertionError(message)


def login(b, server, user):
    b.goto(server.url + "/")
    b.wait_for("!!document.getElementById('auth-form')")
    b.evaluate(f"""(() => {{
        document.getElementById('auth-username').value = {js(user[0])};
        document.getElementById('auth-password').value = {js(user[1])};
        document.getElementById('auth-form').requestSubmit();
    }})()""")
    b.wait_for("!!document.getElementById('logout-button')", message=f"login as {user[0]} failed")


def click(b, selector):
    b.wait_for(f"!!document.querySelector({js(selector)})", message=f"missing {selector}")
    b.evaluate(f"document.querySelector({js(selector)}).click()")


def click_row_button(b, selector, row_text):
    b.wait_for(f"""[...document.querySelectorAll({js(selector)})]
        .some(el => el.closest('tr').innerText.includes({js(row_text)}))""",
               message=f"no {selector} in row {row_text}")
    b.evaluate(f"""[...document.querySelectorAll({js(selector)})]
        .find(el => el.closest('tr').innerText.includes({js(row_text)})).click()""")


def open_admin(b, section):
    click(b, '[data-view="admin"]')
    click(b, f'[data-admin="{section}"]')


def text(b, selector):
    return b.evaluate(f"document.querySelector({js(selector)})?.innerText || ''")


def toast(b):
    return text(b, "#toast")


def wait_toast(b, needle):
    b.wait_for(f"document.getElementById('toast').innerText.includes({js(needle)})",
               message=f"expected toast containing {needle!r}, got {toast(b)!r}")


# ---- scenarios --------------------------------------------------------------


def test_player_challenges_and_hints(b, server, ctx):
    login(b, server, PLAYER)
    b.wait_for("document.querySelectorAll('.challenge-card').length === 3")
    cards = b.evaluate("[...document.querySelectorAll('.challenge-card')].map(c => c.innerText.replace(/\\s+/g, ' '))")
    packet = next(c for c in cards if "Packet Party" in c)
    check("💡2" in packet and "📎1" in packet, f"hint/attachment counts on card: {packet}")
    check(any("Locked Door" in c and "locked" in c for c in cards), "prerequisite card is locked")
    check(any("Warmup" in c and "solved" in c for c in cards), "solved card still listed with badge")
    colour = b.evaluate("getComputedStyle(document.querySelector('.challenge-card .category')).borderLeftColor")
    check(colour not in ("", "rgba(0, 0, 0, 0)"), f"category colour applied despite CSP: {colour}")

    b.evaluate("[...document.querySelectorAll('.challenge-card')].find(c => c.innerText.includes('Locked Door')).click()")
    wait_toast(b, "Packet Party")
    check(not b.evaluate("document.getElementById('modal').classList.contains('open')"), "locked card opens no modal")

    b.evaluate("[...document.querySelectorAll('.challenge-card')].find(c => c.innerText.includes('Packet Party')).click()")
    b.wait_for("!!document.getElementById('flag-input')")
    modal = text(b, ".modal-panel")
    check("Hint 1 (free)" in modal and "Hint 2 (50 pts)" in modal, "hints labelled by position and cost")
    link = b.evaluate("(() => { const a = document.querySelector('.file-link'); return [a.tagName, a.getAttribute('href'), a.target]; })()")
    check(link == ["A", "https://example.com/capture.pcap", "_blank"], f"attachment is an absolute link: {link}")

    b.evaluate("document.getElementById('flag-input').value = 'flag{typed}'")
    click(b, 'button[data-hint-index="0"]')
    b.wait_for("!!document.querySelector('details.hint')")
    check(b.evaluate("document.getElementById('flag-input').value") == "flag{typed}", "typed flag kept after unlock")
    check("Filter on HTTP.\nThen follow the stream." in text(b, "details.hint"), "hint shown with line breaks")

    click(b, 'button[data-hint-index="1"]')
    wait_toast(b, "-50 pts")
    check(any("Unlock Hint 2 for 50 pts? Team score: 250" in d for d in b.dialogs), f"paid hint confirm: {b.dialogs}")
    b.wait_for("document.querySelector('.user-badge.pts').innerText === '200 pts'", message="header score -> 200")

    b.evaluate("document.getElementById('flag-input').value = 'flag{pcap}'; document.getElementById('flag-form').requestSubmit()")
    wait_toast(b, "correct")

    click(b, '[data-view="profile"]')
    b.wait_for("document.getElementById('view').innerText.includes('Solve History')")
    profile = text(b, "#view")
    check("2\nhints used" in profile and "2\nfirst bloods" in profile, "profile stats")
    check("−50 pts" in profile and "free" in profile, "hint unlocks in history")
    check(b.evaluate("/[0-9a-f-]{36}/.test(document.querySelector('.invite-code')?.innerText || '')"), "own invite code shown")

    click(b, '[data-view="scoreboard"]')
    b.wait_for("!!document.querySelector('#score-graph svg path')", message="score graph rendered")


def test_admin_challenge_editors(b, server, ctx):
    login(b, server, ADMIN)
    open_admin(b, "challenges")
    b.wait_for("document.querySelector('#admin-content table')?.innerText.includes('Packet Party')")
    row = b.evaluate("[...document.querySelectorAll('#admin-content tr')].find(r => r.innerText.includes('Packet Party')).innerText")
    check("\t2\t1\t" in row, f"hint and attachment counts in admin table: {row!r}")

    click_row_button(b, "[data-edit-challenge]", "Packet Party")
    b.wait_for("document.getElementById('hint-editor').innerText.includes('unlocked by')")
    check(b.evaluate("document.querySelector('#file-editor [name=\"url\"]').value") == "https://example.com/capture.pcap",
          "attachment editor shows the URL")

    click(b, "#reveal-flag-btn")
    b.wait_for("!document.getElementById('revealed-flag').hidden")
    check(text(b, "#revealed-flag") == "flag{pcap}", f"revealed flag: {text(b, '#revealed-flag')!r}")

    b.evaluate("""(() => { const f = document.getElementById('add-hint-form');
        f.content.value = 'Third hint via UI'; f.cost_points.value = '5'; f.requestSubmit(); })()""")
    b.wait_for("document.querySelectorAll('#hint-editor [data-hint-row]').length === 3")
    b.evaluate("document.querySelectorAll('#hint-editor [data-hint-move=\"-1\"]')[2].click()")
    b.wait_for("document.querySelectorAll('#hint-editor [data-hint-row] textarea')[1]?.value === 'Third hint via UI'",
               message="moving hint 3 up puts it second")

    b.evaluate("""(() => { const f = document.getElementById('add-file-form');
        f.label.value = 'bad'; f.url.value = 'ftp://example.com/x'; f.requestSubmit(); })()""")
    wait_toast(b, "absolute")
    ctx["expected_errors"].append("status of 400")


def test_admin_users_and_submissions(b, server, ctx):
    login(b, server, ADMIN)
    open_admin(b, "users")
    click(b, "#new-user-btn")
    b.wait_for("!!document.getElementById('new-user-form')")
    b.evaluate("""(() => { const f = document.getElementById('new-user-form');
        f.username.value = 'carol'; f.password.value = 'password123'; f.password_confirm.value = 'password123';
        const radio = f.querySelector('[name="team_mode"][value="new"]');
        radio.click(); radio.dispatchEvent(new Event('change'));
        f.new_name.value = 'Blue'; f.requestSubmit(); })()""")
    b.wait_for("/carol\\s+player\\s+Blue/.test(document.querySelector('#admin-content table')?.innerText || '')",
               message="carol created on new team Blue")
    check("Red" in text(b, "#admin-content table"), "team names shown in users table")

    click_row_button(b, "[data-user-team]", "carol")
    b.wait_for("!!document.getElementById('user-team-form')")
    b.evaluate("""(() => { const f = document.getElementById('user-team-form');
        const radio = f.querySelector('[name="team_mode"][value="existing"]');
        radio.click(); radio.dispatchEvent(new Event('change'));
        f.existing_id.value = [...f.existing_id.options].find(o => o.text === 'Red').value;
        f.requestSubmit(); })()""")
    b.wait_for("/carol\\s+player\\s+Red/.test(document.querySelector('#admin-content table')?.innerText || '')",
               message="carol reassigned to Red")

    click(b, '[data-admin="submissions"]')
    b.wait_for("document.getElementById('admin-content').innerText.includes('submissions ·')")
    subs = text(b, "#admin-content")
    check("Red" in subs and "alice" in subs and "Packet Party" in subs, "submissions show names, not ids")


def test_settings_announcement_and_branding(b, server, ctx):
    logo = ctx["logo"].url
    login(b, server, ADMIN)
    check(b.evaluate("document.querySelector('.brand-icon').title").startswith("Version "), "version tooltip")
    open_admin(b, "settings")
    b.wait_for("!!document.getElementById('branding-form')")
    check("status: running" in text(b, "#admin-content").lower(), "competition status shown")

    b.evaluate("""(() => { const f = document.getElementById('announce-form');
        f.title.value = 'Welcome'; f.body.value = 'Good luck!'; f.requestSubmit(); })()""")
    b.wait_for("state.announcements[0]?.title === 'Welcome'", message="announcement arrives over WebSocket")

    b.evaluate(f"""(() => {{ const f = document.getElementById('branding-form');
        f.name.value = 'Squirrel Games'; f.logo_url.value = {js(logo)}; f.requestSubmit(); }})()""")
    # Saving a new logo origin reloads the page and returns to Settings.
    b.wait_for("document.getElementById('brand-name')?.textContent === 'Squirrel Games' && !!document.getElementById('branding-form')",
               timeout=15, message="branding saved and settings reopened")
    b.wait_for(f"document.getElementById('brand-logo').src === {js(logo)} && document.getElementById('brand-logo').naturalWidth > 0",
               message="external logo loads under the CSP")
    check(b.evaluate("document.title") == "Squirrel Games", "document title")
    ctx["expected_errors"].append(logo)  # the open page briefly tries the new origin before reloading

    click(b, "#logout-button")
    b.goto(server.url + "/")
    b.wait_for(f"document.getElementById('brand-logo').src === {js(logo)} && document.getElementById('brand-logo').naturalWidth > 0",
               message="players see the logo")
    # textContent, not innerText: CSS renders the header name uppercase.
    check(b.evaluate("document.getElementById('brand-name').textContent") == "Squirrel Games",
          "players see the competition name")
    served_title = b.evaluate("fetch('/').then(r => r.text()).then(t => /<title>([^<]*)</.exec(t)[1])")
    check(served_title == "Squirrel Games", f"server-rendered title: {served_title!r}")

    admin = server.login(ADMIN)
    server.api("PUT", "/api/admin/branding", {"name": None, "logo_url": None}, admin)
    b.goto(server.url + "/")
    b.wait_for("document.getElementById('brand-name').textContent === 'FeralCTF'", message="name reverts to config")
    check(b.evaluate("document.getElementById('brand-logo').src.endsWith('/feral10.jpg')"), "logo reverts to built-in")


SCENARIOS = [
    test_player_challenges_and_hints,
    test_admin_challenge_editors,
    test_admin_users_and_submissions,
    test_settings_announcement_and_branding,
]


# ---- runner -------------------------------------------------------------------


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("-k", "--filter", default="", help="run scenarios whose name contains this text")
    parser.add_argument("--binary", help="use this feralctf binary instead of building one")
    parser.add_argument("--headed", action="store_true", help="show the browser window")
    parser.add_argument("--keep", action="store_true", help="keep the temporary directory")
    args = parser.parse_args()

    binary = args.binary
    if not binary:
        print("building feralctf (cargo build)...", flush=True)
        subprocess.run(["cargo", "build", "--quiet"], cwd=REPO, check=True)
        binary = str(REPO / "target" / "debug" / "feralctf")

    workdir = Path(tempfile.mkdtemp(prefix="feralctf-ui-"))
    server = Server(binary, workdir)
    logo = LogoHost(workdir)
    failures = 0
    try:
        seed(server)
        for scenario in [s for s in SCENARIOS if args.filter in s.__name__]:
            ctx = {"logo": logo, "expected_errors": []}
            browser = Browser(headed=args.headed)
            try:
                scenario(browser, server, ctx)
                browser.finish()
                unexpected = [e for e in browser.console_errors
                              if not any(allowed in e for allowed in ctx["expected_errors"])]
                check(not unexpected, "browser console errors:\n    " + "\n    ".join(unexpected))
                print(f"PASS  {scenario.__name__}")
            except Exception as error:  # noqa: BLE001 - report every failure kind
                failures += 1
                shot = workdir / f"{scenario.__name__}.png"
                try:
                    browser.screenshot(shot)
                except Exception:  # noqa: BLE001
                    shot = None
                print(f"FAIL  {scenario.__name__}: {error}")
                if not isinstance(error, AssertionError):
                    traceback.print_exc()
                if shot:
                    print(f"      screenshot: {shot}")
            finally:
                browser.close()
    finally:
        logo.stop()
        server.stop()
        if failures or args.keep:
            print(f"kept {workdir} (server.log, screenshots)")
        else:
            shutil.rmtree(workdir, ignore_errors=True)

    print("all UI tests passed" if not failures else f"{failures} scenario(s) failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

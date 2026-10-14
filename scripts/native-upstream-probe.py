"""Run upstream chat handoffs in an isolated real Tauri/WebKit/X11 process.

Requires the upstream-window-fixture server, Vite, Xvfb, KWin, and Python Xlib.
The fixture backend supplies frames; all renderer/routing/window work runs in
the actual executable. No existing desktop process or user data is controlled.
"""

import argparse
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import tempfile
import time
import urllib.request
import uuid


BASE = "http://127.0.0.1:48911"
IMAGE = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg=="


def get_report():
    with urllib.request.urlopen(BASE + "/__fixture/report", timeout=5) as response:
        return json.load(response)


def post(path, data):
    request = urllib.request.Request(BASE + path, data=json.dumps(data).encode(),
                                    headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=5) as response:
        return json.load(response)


def wait_for(check, description, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(.1)
    raise AssertionError("Timed out: " + description)


def command(role, **fields):
    identifier = str(time.time_ns())
    post("/__fixture/commands?role=" + role, {"id": identifier, **fields})
    wait_for(lambda: identifier in get_report()["results"], str(fields))
    return get_report()["results"][identifier]


def find_window(root, title):
    try:
        if root.get_wm_name() == title:
            return root
        for child in root.query_tree().children:
            found = find_window(child, title)
            if found:
                return found
    except Exception:
        pass
    return None


def native_display(process, isolated_config):
    from Xlib import display
    # Resolve only descendants of this runner, then verify its own XDG fence.
    def descendants(pid):
        children = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()
        for child in children:
            yield int(child)
            yield from descendants(int(child))
    for pid in descendants(process.pid):
        directory = Path("/proc") / str(pid)
        if (directory / "comm").read_text().strip() != "neko-tauri":
            continue
        env = dict(entry.split("=", 1) for entry in (directory / "environ").read_bytes().decode().split("\0") if "=" in entry)
        assert env.get("XDG_CONFIG_HOME") == str(isolated_config)
        for key in ["DISPLAY", "XAUTHORITY"]:
            if key in env:
                os.environ[key] = env[key]
        return display.Display()
    raise AssertionError("Owned native test process was not found")


def run(binary, output):
    from Xlib import X
    from Xlib.ext import xtest
    initial = get_report()
    assert initial.get("runId") and initial.get("activeConnections") == 0, "An idle upstream fixture is required"
    output.mkdir(parents=True, exist_ok=True)
    evidence = {"fixtureRun": initial["runId"], "platform": "Linux X11 + Xvfb/KWin", "passed":False,"checks": []}
    started = time.time() * 1000
    with tempfile.TemporaryDirectory(prefix="neko-native-transfer-", ignore_cleanup_errors=True) as directory:
        isolated = Path(directory)
        env = os.environ.copy()
        env.update({"XDG_CONFIG_HOME": str(isolated / "config"), "XDG_DATA_HOME": str(isolated / "data"),
                    "XDG_CACHE_HOME": str(isolated / "cache"), "GDK_BACKEND": "x11",
                    "XDG_SESSION_TYPE": "x11", "XDG_CURRENT_DESKTOP": "KDE",
                    "NEKO_DISABLE_NIRI_CONFIG_PATCH": "1", "WEBKIT_DISABLE_DMABUF_RENDERER": "1",
                    "NEKO_TAURI_BRIDGE_DIAGNOSTIC": "1"})
        script = "kwin_x11 >" + shlex.quote(str(output.resolve() / "kwin.log")) + " 2>&1 & wm=$!; trap 'kill -TERM $wm 2>/dev/null' EXIT; " + shlex.quote(str(binary.resolve()))
        with (output / "native.log").open("w") as log:
            process = subprocess.Popen(["xvfb-run", "-a", "-s", "-screen 0 1280x1024x24", "dbus-run-session", "--", "bash", "-c", script],
                                       env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            connection = None
            try:
                def role_state(role):
                    data = get_report()["report"].get(role, {})
                    return data if data.get("updatedAt", 0) >= started else {}
                def response_state(role, text):
                    data = role_state(role)
                    for message in data.get("messages", []):
                        if message.get("role") == "assistant" and any(block.get("text") == text for block in message.get("blocks", [])):
                            return data
                    return None
                wait_for(lambda: role_state("compact-chat").get("mounted"), "native compact React mount", 40)
                command("compact-chat", kind="input")
                command("compact-chat", kind="merge-setting", enabled=True)
                attachments = command("compact-chat", kind="attachment", url=IMAGE,
                                      source="user-image", alt="Native image", position={"modelPosition": {"x": 10, "y": 20}})
                command("compact-chat", kind="draft", text="Native transfer draft")
                post("/__fixture/frame", {"type": "gemini_response", "isNewMessage": True, "text": "First half", "turn_id": "native-transfer"})
                source = wait_for(lambda: response_state("compact-chat", "First half"), "source response")
                original = next(message for message in source["messages"] if message.get("role") == "assistant")
                command("compact-chat", kind="mode", mode="full")
                full = wait_for(lambda: role_state("full-chat") if role_state("full-chat").get("draft") == "Native transfer draft" and role_state("full-chat").get("pendingAttachments") else None, "native draft and attachment handoff")
                assert full["pendingAttachments"][0]["id"] == attachments[0]["id"]
                command("full-chat", kind="merge-setting", enabled=True)
                post("/__fixture/frame", {"type": "gemini_response", "text": " + second half", "turn_id": "native-transfer"})
                full = wait_for(lambda: response_state("full-chat", "First half + second half"), "native stream continuation")
                response = [message for message in full["messages"] if message.get("role") == "assistant"]
                assert len(response) == 1 and response[0]["id"] == original["id"]
                assert response[0]["blocks"][0]["text"] == "First half + second half"
                evidence["checks"].append("draft/attachment/history/live merge stream handoff")
                assert get_report()["activeConnections"] == 1
                evidence["checks"].append("one real character WebSocket across both native chat windows")
                request_id = "native-" + uuid.uuid4().hex
                assert command("full-chat", kind="send", text="Native image submission", requestId=request_id) is True
                image_frame = wait_for(lambda: next((frame for frame in get_report()["received"] if frame.get("input_type") == "user_image" and frame.get("request_id") == request_id), None), "restored attachment send")
                assert image_frame["data"] == attachments[0]["url"]
                assert image_frame.get("avatar_position") == {"modelPosition": {"x": 10, "y": 20}}
                evidence["checks"].append("restored attachment sent with source and avatar metadata")
                command("pet", kind="native", action="collapse", options={"bounds": {"x": 100, "y": 150, "width": 88, "height": 88}})
                connection = native_display(process, isolated / "config")
                root = connection.screen().root
                ball = wait_for(lambda: find_window(root, "N.E.K.O [chat-ball]"), "native ball creation")
                wait_for(lambda: ball.get_attributes().map_state == X.IsViewable, "ball reveal")
                assert (ball.get_geometry().width, ball.get_geometry().height) == (88, 88)
                ball.configure(x=250, y=800)
                connection.sync()
                wait_for(lambda: root.translate_coords(ball, 0, 0).x == 250, "ball move")
                point = root.translate_coords(ball, 0, 0)
                xtest.fake_input(connection, X.MotionNotify, x=point.x + 40, y=point.y + 40)
                xtest.fake_input(connection, X.ButtonPress, 1)
                connection.sync()
                time.sleep(.05)
                xtest.fake_input(connection, X.ButtonRelease, 1)
                connection.sync()
                wait_for(lambda: ball.get_attributes().map_state != X.IsViewable, "ball dismissed after full chat restore")
                target = find_window(root, "N.E.K.O [chat-full]")
                assert target and target.get_attributes().map_state == X.IsViewable
                restored = wait_for(lambda: role_state("full-chat") if role_state("full-chat").get("bounds", {}).get("x") == 250 else None, "moved restore anchor")
                assert restored["mode"] == "full"
                evidence["restoredBounds"] = restored["bounds"]
                evidence["checks"].append("88px native ball restores the selected full surface from its moved anchor")
                connection.close()
                connection = None
                post("/__fixture/commands?role=pet", {"id": str(time.time_ns()), "kind": "native", "action": "close"})
                assert process.wait(timeout=15) == 0
                wait_for(lambda: get_report()["activeConnections"] == 0, "socket teardown")
                evidence["checks"].append("native exit and socket teardown")
                evidence["passed"] = True
            except Exception as error:
                evidence["failure"] = str(error)
                raise
            finally:
                if connection:
                    connection.close()
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=10)
                (output / "report.json").write_text(json.dumps(evidence, ensure_ascii=False, indent=2))
    print(json.dumps(evidence, ensure_ascii=False))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path(".artifacts/native-transfer-probe"))
    args = parser.parse_args()
    run(args.binary, args.output)

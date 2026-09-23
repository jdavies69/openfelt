#!/usr/bin/env python3
"""Real Unix PTY smoke tests. No third-party Python packages or API keys required."""
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

BINARY = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/openfelt").resolve()
ANSI = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]")


class Game:
    def __init__(self, root, *args, test_env=None):
        self.root = Path(root)
        self.master, slave = pty.openpty()
        self.resize(80, 30)
        env = dict(os.environ)
        env.pop("OPENAI_API_KEY", None)
        env.pop("ANTHROPIC_API_KEY", None)
        env.update(test_env or {})
        env["TERM"] = "xterm-256color"

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [str(BINARY), "--data-dir", str(root), "--seats", "2", *args],
            stdin=slave, stdout=slave, stderr=slave, env=env,
            preexec_fn=terminal_session,
        )
        os.close(slave)
        self.output = b""
        self.read(0.25)

    def resize(self, cols, rows):
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        if hasattr(self, "process"):
            os.kill(self.process.pid, signal.SIGWINCH)

    def read(self, seconds=0.15):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([self.master], [], [], max(0, deadline - time.monotonic()))[0]:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    break
                if not data:
                    break
                self.output += data

    def send(self, keys):
        os.write(self.master, keys.encode())
        self.read()

    def decisions(self):
        path = self.root / "decisions.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def quit(self):
        self.send("q")
        self.process.wait(timeout=3)
        self.read(0.05)
        assert self.process.returncode == 0, ANSI.sub(b"", self.output).decode(errors="replace")
        assert b"\x1b[?1049l" in self.output, "terminal was not restored"
        os.close(self.master)


with tempfile.TemporaryDirectory(prefix="openfelt-pty-") as root:
    for index, keys in enumerate(["f", "F", "c", "C", "r4\r", "R4\r", "a\r", "A\r"]):
        g = Game(Path(root) / str(index))
        g.send(keys)
        assert len(g.decisions()) == 1, (keys, g.decisions())
        g.send("C")
        assert len(g.decisions()) == 1, "call key must not continue or decide during coaching"
        g.read(0.5)
        assert len(g.decisions()) == 1
        g.send("?")
        g.quit()
    print("PASS: upper/lower F C R A, one accepted decision, coaching pause, details, quit")

    g = Game(Path(root) / "invalid")
    g.send("r")
    assert not g.decisions()
    g.send("\x1b")
    g.send("a")
    assert not g.decisions()
    g.send("\x1b")
    g.resize(60, 20)
    g.read()
    g.send("C")
    assert not g.decisions(), "small terminal must not accept actions"
    assert b"needs 80" in ANSI.sub(b"", g.output)
    g.resize(100, 36)
    g.read()
    g.send("F")
    assert len(g.decisions()) == 1
    g.send("\r")
    g.send("B")
    g.send("w5\r")
    g.send("v")
    assert b"COMPLETED HANDS" in ANSI.sub(b"", g.output), "V must open replay between hands"
    g.send("\r")
    assert b"DECISION REPLAY" in ANSI.sub(b"", g.output)
    g.send("b")
    # Bookmark persistence/redraw can exceed one read interval on busy CI runners.
    deadline = time.monotonic() + 3
    while b"BOOKMARKED" not in ANSI.sub(b"", g.output) and time.monotonic() < deadline:
        g.read(0.1)
    assert b"BOOKMARKED" in ANSI.sub(b"", g.output)
    g.send("\x1b")
    g.send("\x1b")
    assert g.process.poll() is None, "replay must return to the same live session"
    g.quit()
    cash = [json.loads(line) for line in (g.root / "cash-events.jsonl").read_text().splitlines()]
    assert cash[0]["added"] == 1 and cash[1]["withdrawn"] == 5, cash
    progress = json.loads((g.root / "progress.json").read_text())
    assert progress["hands"] == 1 and progress["profit_chips"] == -1, progress
    output = subprocess.check_output([str(BINARY), "--data-dir", str(g.root), "--stats"], text=True)
    assert "1 hands" in output and "1 decisions" in output
    bookmarks = json.loads((g.root / "bookmarks.json").read_text())
    assert len(bookmarks) == 1 and bookmarks[0]["decision"] == 0, bookmarks
    print("PASS: invalid actions, resize, cash ledger, in-app replay/bookmark, restart stats")

    g = Game(Path(root) / "settings-menu")
    g.send("s")
    assert b"SETTINGS" in ANSI.sub(b"", g.output), "S must open settings"
    g.send("\x1b")
    g.send("f")
    assert len(g.decisions()) == 1, "closing settings must return to the same playable session"
    g.quit()
    print("PASS: settings opens and safely returns to the live table")

    g = Game(Path(root) / "solver-setting")
    g.send("s")
    g.send("\x1b[B" * 6)
    g.send("?")
    help_text = re.sub(rb"\s+", b"", ANSI.sub(b"", g.output))
    assert b"SETTINGSHELP" in help_text
    assert b"Solverfeedback" in help_text
    g.send("\x1b")
    g.send("\x1b[C")
    g.send("?")
    g.send("?")
    g.send("\x1b[B" * 3)
    g.send("\r")
    saved = json.loads((g.root / "settings.json").read_text())
    assert saved["solver_feedback"] is True, "help must preserve the selected setting and draft"
    g.send("C")
    assert len(g.decisions()) == 1
    g.output = b""
    g.send("s")
    assert b"SETTINGS" in ANSI.sub(b"", g.output), "settings must open during paused feedback"
    g.send("\x1b")
    g.quit()
    g = Game(Path(root) / "solver-setting")
    g.send("s")
    g.send("\x1b[B" * 6)
    g.send("\x1b[D")
    g.send("\x1b[B" * 3)
    g.send("\r")
    saved = json.loads((g.root / "settings.json").read_text())
    assert saved["solver_feedback"] is False, "solver preference must persist across restarts"
    g.quit()
    print("PASS: selected-setting help preserves edits; solver On/Off persists across restarts")

    play_root = Path(root) / "play-pace"
    g = Game(play_root)
    g.send("s")
    g.send("\x1b[B" * 7)
    g.send("?")
    assert b"Practicepace" in re.sub(rb"\s+", b"", ANSI.sub(b"", g.output))
    g.send("\x1b")
    g.send("\x1b[C")
    g.send("\x1b[B" * 2)
    g.send("\r")
    assert json.loads((play_root / "settings.json").read_text())["practice_pace"] == "play"
    g.send("f")
    deadline = time.monotonic() + 3
    while not (play_root / "completed-hands.jsonl").exists() and time.monotonic() < deadline:
        g.read(0.1)
    assert (play_root / "completed-hands.jsonl").exists(), "Play must finish without coaching Enter"
    assert len(g.decisions()) == 1
    g.send("r")
    assert b"COMPLETED HANDS" in ANSI.sub(b"", g.output)
    g.send("\r")
    g.send("p")
    assert b"PRACTICE" in ANSI.sub(b"", g.output)
    for _ in range(3):
        g.send("1")
        g.send("\r")
    drill_stats = json.loads((play_root / "progress.json").read_text())["drills"]
    assert sum(item["attempts"] for item in drill_stats.values()) == 3
    g.send("\x1b")
    g.send("b")
    assert len(json.loads((play_root / "bookmarks.json").read_text())) == 1
    g.send("\x1b")
    g.send("\x1b")
    g.send("\r")
    for _ in range(10):
        g.read(0.5)
        g.send("c")
        if len(g.decisions()) >= 2:
            break
        # A bot can fold before hero acts; advance that completed hand.
        g.send("\r")
    assert len(g.decisions()) >= 2
    assert json.loads((play_root / "progress.json").read_text())["drills"] == drill_stats, "playing must preserve completed replay drills"
    g.quit()
    g = Game(play_root)
    g.send("f")
    g.read(0.3)
    hands = (play_root / "completed-hands.jsonl").read_text().splitlines()
    assert len(hands) >= 2, "Play preference must survive restart"
    g.quit()
    print("PASS: Play pace persists; hand ends without a pause; replay practice returns to bookmark")

    # A temporary data directory does not isolate the OS credential store.
    # An empty environment credential fails validation before any Keychain or
    # HTTP access. It lets us test activation/persistence without a real key.
    cloud_root = Path(root) / "cloud-persistent"
    g = Game(cloud_root, "--coaching", "openai", "--model", "fixture-model",
             "--save-settings", test_env={"OPENAI_API_KEY": ""})
    assert b"ENABLE OPTIONAL CLOUD COACHING" not in ANSI.sub(b"", g.output)
    g.send("C")
    assert len(g.decisions()) == 1
    g.quit()
    assert json.loads((cloud_root / "settings.json").read_text())["coaching"] == "openai"
    g = Game(cloud_root, test_env={"OPENAI_API_KEY": ""})
    assert b"ENABLE OPTIONAL CLOUD COACHING" not in ANSI.sub(b"", g.output)
    assert b"OpenAI coaching" in ANSI.sub(b"", g.output)
    g.send("s")
    g.send("\x1b[C" * 2)  # OpenAI -> Anthropic -> Local
    g.send("\x1b[B" * 9)
    g.send("\r")
    assert json.loads((cloud_root / "settings.json").read_text())["coaching"] == "local"
    g.send("C")
    assert len(g.decisions()) == 2
    g.quit()
    print("PASS: cloud provider persists/activates on restart; selecting Local disables it")

print("All OpenFelt PTY smoke tests passed.")

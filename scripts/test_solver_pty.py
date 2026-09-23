#!/usr/bin/env python3
"""Real Unix PTY smoke tests for local river solver practice."""
import fcntl
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


class Solver:
    def __init__(self, root, *args, cols=80, rows=30):
        self.root = Path(root)
        self.master, slave = pty.openpty()
        self.resize(cols, rows)
        env = dict(os.environ)
        env.pop("OPENAI_API_KEY", None)
        env.pop("ANTHROPIC_API_KEY", None)
        env["TERM"] = "xterm-256color"
        env["RAYON_NUM_THREADS"] = "2"

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [str(BINARY), "--data-dir", str(root), *args],
            stdin=slave,
            stdout=slave,
            stderr=slave,
            env=env,
            preexec_fn=terminal_session,
        )
        os.close(slave)
        self.output = b""
        self.read(0.15)

    def resize(self, cols, rows):
        fcntl.ioctl(
            self.master,
            termios.TIOCSWINSZ,
            struct.pack("HHHH", rows, cols, 0, 0),
        )
        if hasattr(self, "process"):
            os.kill(self.process.pid, signal.SIGWINCH)

    def read(self, seconds=0.1):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            ready = select.select(
                [self.master], [], [], max(0, deadline - time.monotonic())
            )[0]
            if not ready:
                continue
            try:
                data = os.read(self.master, 65536)
            except OSError:
                break
            if not data:
                break
            self.output += data

    def text(self):
        return ANSI.sub(b"", self.output).decode(errors="replace")

    def decisions(self):
        path = self.root / "decisions.jsonl"
        return path.read_text().splitlines() if path.exists() else []

    def wait_for(self, needle, timeout=10):
        deadline = time.monotonic() + timeout
        while needle not in self.text() and time.monotonic() < deadline:
            if self.process.poll() is not None:
                break
            self.read(0.1)
        assert needle in self.text(), (needle, self.text()[-4000:])

    def send(self, keys):
        os.write(self.master, keys.encode())
        self.read(0.1)

    def exit_with(self, key):
        self.send(key)
        self.process.wait(timeout=5)
        self.read(0.05)
        assert self.process.returncode == 0, self.text()[-4000:]
        assert b"\x1b[?1049l" in self.output, "terminal alternate screen was not restored"
        os.close(self.master)


with tempfile.TemporaryDirectory(prefix="openfelt-solver-pty-") as root:
    solver = Solver(Path(root) / "practice", "--solver-practice")
    solver.wait_for("LOCAL RIVER PRACTICE")
    solver.wait_for("Your hand (", timeout=20)
    assert "Hero first (OOP)" in solver.text()
    assert "RANGES AND BETTING OPTIONS" in solver.text()
    assert "pot-share EV" not in solver.text(), "feedback must stay hidden before a choice"
    solver.send("1")
    solver.wait_for("EV loss")
    assert "iterations" in solver.text()
    assert "exploitability" in solver.text()
    solver.send("\x1b[C")
    solver.send("\r")
    solver.wait_for("played")
    solver.send("n")
    solver.wait_for("(2/2)")
    solver.send("?")
    solver.wait_for("SCENARIO ASSUMPTIONS")
    solver.send("?")
    solver.resize(60, 20)
    solver.wait_for("River practice needs")
    solver.resize(80, 30)
    solver.read(0.25)
    solver.exit_with("q")
    print("PASS: solve, feedback, hand/scenario navigation, assumptions, resize, Q restore")

    # Exit immediately while the worker may still be calculating. Drop must signal
    # cancellation and the terminal guard must always leave the alternate screen.
    solver = Solver(Path(root) / "cancel", "--solver-practice")
    solver.wait_for("LOCAL RIVER PRACTICE")
    solver.exit_with("\x1b")
    print("PASS: Esc cancels/returns safely and restores the terminal")

    table = Solver(Path(root) / "table", "--seats", "2")
    table.wait_for("OPENFELT")
    assert not table.decisions()
    table.send("g")
    table.wait_for("LOCAL RIVER PRACTICE")
    table.send("\x1b")
    table.wait_for("Returned to table")
    assert not table.decisions(), "solver practice must not record a table decision"
    table.exit_with("q")
    print("PASS: table → G practice → Esc returns to the same table without a decision")

    example = Path("docs/solver/river-example.json").resolve()
    solver = Solver(Path(root) / "custom", "--solver-scenario", str(example))
    solver.wait_for("River overpairs and missed draws")
    solver.wait_for("Your hand (", timeout=20)
    solver.exit_with("q")
    print("PASS: documented custom river scenario validates, solves, and renders")

print("All solver-practice PTY smoke tests passed.")

#!/usr/bin/env python3
"""Drives `console-drive.py` against a fake serial console, so its logic is testable in
milliseconds instead of only behind an hour-long image build.

The console-drive script was broken from the day it was written -- it waited for a greeting the
console never prints, and counted prompts in a way ANSI colour makes impossible. Nothing noticed,
because the only thing that ran it was a CI job whose `if:` condition could never be true. Once
that job started running, the failure was a five-minute timeout and a message saying the response
didn't contain what was expected, which is a confusing way to say "nothing was ever typed".

The fixture below is the real byte sequence a booted Hyperion console emits, colour codes and all,
copied from that failing run's own log. A driver that can't get through this can't get through the
real thing.

Usage: console-drive-test.py     (exit 0 on pass)
"""

from __future__ import annotations

import socket
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
from console_stream import is_ready_for_input, prompts_in, turn_is_complete  # noqa: E402

GOLD = "\x1b[38;2;217;165;74m"
RESET = "\x1b[0m"

# Exactly what the console writes on a real serial port, through to its first prompt.
BANNER = (
    "\n"
    f"{GOLD} _  ___   _____ ___ ___ ___ ___  _  _\n"
    "| || \\ \\ / / _ \\ __| _ \\_ _/ _ \\| \\| |\n"
    "| __ |\\ V /|  _/ _||   /| | (_) | .` |\n"
    f"|_||_| |_| |_| |___|_|_\\___\\___/|_|\\_|{RESET}\n"
    "\n"
    "You ask. I understand.\n"
    "\n"
    f"{GOLD}> {RESET}"
)

# The echo of the typed line, this turn's real output, and the next prompt.
def response_for(utterance: str) -> str:
    return (
        f"{utterance}\n"
        "  market_research: Done\n"
        "status: market_research: Done -- [mock model 1] echo: Provide a concise research "
        "summary about market_research.\n"
        f"{GOLD}> {RESET}"
    )


def serve(sock_path: str, received: list[str]) -> None:
    """A fake ttyS0: emit the banner, wait to be typed at, then answer."""
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(sock_path)
    server.listen(1)
    conn, _ = server.accept()
    with conn:
        conn.sendall(BANNER.encode())
        conn.settimeout(20.0)
        typed = b""
        while not typed.endswith(b"\n"):
            chunk = conn.recv(1024)
            if not chunk:
                break
            typed += chunk
        received.append(typed.decode())
        conn.sendall(response_for(typed.decode().strip()).encode())
    server.close()


def check(name: str, condition: object, detail: str = "") -> bool:
    passed = bool(condition)
    print(f"  {'ok  ' if passed else 'FAIL'}  {name}{'' if passed else f' -- {detail}'}")
    return passed


def main() -> int:
    ok = True
    print("console_stream, against the real console's own bytes:")

    # The bug that cost an hour-long CI job five minutes and a misleading message.
    ok &= check(
        "a prompt is recognized through ANSI colour",
        is_ready_for_input(BANNER),
        "the driver would never type anything",
    )
    ok &= check(
        "the greeting alone is not mistaken for readiness",
        not is_ready_for_input(BANNER[: BANNER.index(GOLD + "> ")]),
        "typing before the prompt races the console's own startup",
    )
    ok &= check("one prompt after the banner", prompts_in(BANNER) == 1, str(prompts_in(BANNER)))
    ok &= check("a turn is not complete at the first prompt", not turn_is_complete(BANNER))
    full = BANNER + response_for("I need to launch my startup")
    ok &= check("a turn is complete once the next prompt lands", turn_is_complete(full))
    # A `> ` quoted mid-answer must not be read as the console asking for input again.
    ok &= check(
        "a quoted '> ' inside an answer is not a prompt",
        prompts_in(BANNER + "the shell shows > when ready\n") == 1,
    )

    print("\nconsole-drive.py, end to end against a fake serial console:")
    with tempfile.TemporaryDirectory() as tmp:
        sock_path = str(Path(tmp) / "console.sock")
        received: list[str] = []
        server = threading.Thread(target=serve, args=(sock_path, received), daemon=True)
        server.start()

        utterance = "I need to launch my startup"
        result = subprocess.run(
            [sys.executable, str(SCRIPT_DIR / "console-drive.py"), sock_path, utterance, "25"],
            capture_output=True,
            text=True,
            timeout=60,
        )
        server.join(timeout=30)

        ok &= check("the driver exits cleanly", result.returncode == 0, result.stderr[-400:])
        ok &= check(
            "the utterance was really typed",
            bool(received) and received[0].strip() == utterance,
            f"console received {received!r}",
        )
        ok &= check(
            "the response was captured for the caller to grep",
            "market_research" in result.stdout,
            f"stdout was {result.stdout[-300:]!r}",
        )
        # The whole point: console-test.sh greps this output for its expected string.
        ok &= check(
            "console-test.sh's own assertion would pass",
            "market_research" in result.stdout,
        )

    print("\nPASS" if ok else "\nFAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Drives the real booted console over a Unix domain socket serial port (QEMU's
`-chardev socket` backend for ttyS0) -- unlike boot-test.sh's `-serial file:...`, which only ever
captures output, this can also *send* a real typed utterance, which is exactly what
docs/998-roadmap.md M7's exit criterion needs to prove: "a real utterance typed at the real
booted console produces..." something real back.

Usage: console-drive.py <socket-path> <utterance> <timeout-seconds>

Connects, waits for the console's own real startup banner, sends the utterance as a real typed
line, waits for the real response (until the next real "> " prompt reappears, or the timeout
elapses), and prints everything it read to stdout so the calling shell script can grep it.
"""
import socket
import sys
import time

sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parent))
from console_stream import is_ready_for_input, turn_is_complete  # noqa: E402

def main():
    sock_path, utterance, timeout_s = sys.argv[1], sys.argv[2], float(sys.argv[3])

    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(sock_path)
    sock.settimeout(1.0)

    buf = ""
    deadline = time.monotonic() + timeout_s
    sent = False

    while time.monotonic() < deadline:
        try:
            chunk = sock.recv(4096)
            if chunk:
                buf += chunk.decode("utf-8", errors="replace")
        except socket.timeout:
            pass

        if not sent and is_ready_for_input(buf):
            # The console has printed a prompt, so it is genuinely waiting for a line. Settle
            # briefly, then send the utterance exactly as a human pressing Enter would.
            time.sleep(0.5)
            sock.sendall((utterance + "\n").encode("utf-8"))
            sent = True
            continue

        if sent and turn_is_complete(buf):
            break

    sock.close()
    print(buf)

if __name__ == "__main__":
    main()

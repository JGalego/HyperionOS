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

def main():
    sock_path, utterance, timeout_s = sys.argv[1], sys.argv[2], float(sys.argv[3])

    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(sock_path)
    sock.settimeout(1.0)

    buf = ""
    deadline = time.monotonic() + timeout_s
    sent = False
    prompts_seen = 0

    while time.monotonic() < deadline:
        try:
            chunk = sock.recv(4096)
            if chunk:
                buf += chunk.decode("utf-8", errors="replace")
        except socket.timeout:
            pass

        if not sent and "Hyperion -- tell me what you'd like to do." in buf:
            # The real console really started; give its first real "> " prompt a moment to
            # actually be written before sending, then send the real utterance as a real typed
            # line (with its own newline, exactly like a human pressing Enter).
            time.sleep(0.5)
            sock.sendall((utterance + "\n").encode("utf-8"))
            sent = True
            continue

        if sent:
            prompts_seen = buf.count("\n> ")
            # One prompt for the initial banner, a second once this turn's real output has been
            # printed and the loop is back waiting for the next line.
            if prompts_seen >= 2:
                break

    sock.close()
    print(buf)

if __name__ == "__main__":
    main()

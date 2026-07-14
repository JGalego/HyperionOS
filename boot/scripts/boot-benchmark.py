#!/usr/bin/env python3
"""docs/998-roadmap.md M12: measures real, end-to-end cold-boot time against docs/36's
budget -- "firmware -> login/shell -> first real Intent handled," not `hyperion_sim::boot`'s old
in-process 250ms slice, which only ever measured one sub-phase of a boot that didn't yet exist.

Reuses console-drive.py's exact connect/wait-for-banner/send-utterance/wait-for-response protocol
(same socket, same prompt-counting logic) -- this script's only real addition is timestamping each
milestone against `t0`, a wall-clock epoch time the calling shell script recorded *before* it even
launched qemu, so the reported elapsed times cover real qemu startup + real kernel boot + real
console readiness + a real Intent round-trip, not just this script's own connect-to-response
window.

Usage: boot-benchmark.py <socket-path> <t0-epoch-seconds> <utterance> <timeout-seconds>

Prints two greppable, shell-parseable lines:
  CONSOLE_READY_ELAPSED=<seconds>
  FIRST_INTENT_ELAPSED=<seconds>
"""
import socket
import sys
import time


def main():
    sock_path, t0, utterance, timeout_s = (
        sys.argv[1],
        float(sys.argv[2]),
        sys.argv[3],
        float(sys.argv[4]),
    )

    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(sock_path)
    sock.settimeout(1.0)

    buf = ""
    deadline = time.monotonic() + timeout_s
    sent = False
    prompts_seen = 0
    console_ready_elapsed = None
    first_intent_elapsed = None

    while time.monotonic() < deadline:
        try:
            chunk = sock.recv(4096)
            if chunk:
                buf += chunk.decode("utf-8", errors="replace")
        except socket.timeout:
            pass

        if not sent and "Hyperion -- tell me what you'd like to do." in buf:
            console_ready_elapsed = time.time() - t0
            # Same real settle delay console-drive.py itself uses before typing.
            time.sleep(0.5)
            sock.sendall((utterance + "\n").encode("utf-8"))
            sent = True
            continue

        if sent:
            prompts_seen = buf.count("\n> ")
            if prompts_seen >= 2:
                first_intent_elapsed = time.time() - t0
                break

    sock.close()
    print(buf)
    print(f"CONSOLE_READY_ELAPSED={console_ready_elapsed if console_ready_elapsed is not None else 'never'}")
    print(f"FIRST_INTENT_ELAPSED={first_intent_elapsed if first_intent_elapsed is not None else 'never'}")


if __name__ == "__main__":
    main()

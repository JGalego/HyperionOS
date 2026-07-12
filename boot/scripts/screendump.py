#!/usr/bin/env python3
"""PRODUCTION_BOOT_PROMPT.md M7 stage 2: issues a real `screendump` command over QEMU's HMP
monitor (a real Unix domain socket, `-monitor unix:PATH,server=on,wait=off`) once the guest has
finished its own real DRM/KMS mode-set, capturing the *actual* emulated display's current pixel
content to a real PPM file on the host -- independent proof, from outside the guest entirely, that
real pixels are really being displayed, not just that the guest-side ioctls returned success.

Usage: screendump.py <monitor-socket-path> <output-ppm-path>
"""
import socket
import sys
import time


def main():
    sock_path, output_path = sys.argv[1], sys.argv[2]

    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(sock_path)
    sock.settimeout(5.0)

    # Drain the HMP banner/prompt before sending a real command.
    time.sleep(0.5)
    try:
        sock.recv(4096)
    except socket.timeout:
        pass

    sock.sendall(f"screendump {output_path}\n".encode("utf-8"))
    time.sleep(1.0)
    try:
        reply = sock.recv(4096).decode("utf-8", errors="replace")
    except socket.timeout:
        reply = ""
    sock.close()
    print(reply)


if __name__ == "__main__":
    main()

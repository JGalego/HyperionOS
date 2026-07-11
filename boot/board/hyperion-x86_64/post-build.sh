#!/bin/sh
# Adds a getty on ttyS0 (serial -- the automated QEMU boot test's oracle) in
# addition to whatever BR2_TARGET_GENERIC_GETTY_PORT already put on tty1 (the
# console a person at a real screen, or QEMU's graphical window, sees).
set -u
set -e

BOARD_DIR="$(dirname "$0")"

if [ -e "${TARGET_DIR}/etc/inittab" ]; then
    grep -qE '^ttyS0::' "${TARGET_DIR}/etc/inittab" || \
        sed -i '/GENERIC_SERIAL/a\
ttyS0::respawn:/sbin/getty -L ttyS0 115200 vt100 # Hyperion boot-test console' \
        "${TARGET_DIR}/etc/inittab"
fi

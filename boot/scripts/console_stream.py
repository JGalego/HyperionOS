"""Reading a real booted Hyperion console over a serial port, correctly.

Shared by `console-drive.py` (M7's exit criterion) and `boot-benchmark.py` (M12's timing), which
each had their own copy of this logic and the same two bugs in it. Both bugs were invisible until
the boot-image CI job started running for real:

1. **They waited for a greeting the console does not print.** The check was for
   `"Hyperion -- tell me what you'd like to do."`; the console actually says `"You ask. I
   understand."`. The utterance was therefore never typed at all, and the test sat until its
   timeout and reported the response didn't contain what was expected -- which was true, because
   there was no response.

2. **They matched plain text against a coloured stream.** A real serial console is a TTY, so
   `hyperion-console` emits its prompt as `\\x1b[38;2;217;165;74m> \\x1b[0m`. Counting `"\\n> "`
   never matches that, so even a driver that did type would never notice the reply had finished.

Both are fixed here by matching on what the console *does*, not on what it says: strip the escape
sequences, then look for the prompt. A prompt is the console stating it is ready for input, which
is the actual thing a driver is waiting for, and it survives any future rewording of the greeting.
"""

from __future__ import annotations

import re

# CSI sequences (colour, cursor movement) -- everything hyperion-console's own `color` module
# emits, and enough of the rest to keep a stray sequence from splitting a prompt in two.
_ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")

# The console writes `print!("> ")` after the previous line ended, so a prompt is a `> ` opening
# a line. Anchoring to the line start keeps a `> ` inside a model's own answer from counting.
_PROMPT = re.compile(r"(?m)^> ")


def visible(stream: str) -> str:
    """`stream` with ANSI escape sequences removed -- what a person would actually see."""
    return _ANSI.sub("", stream)


def prompts_in(stream: str) -> int:
    """How many times the console has said it is ready for input."""
    return len(_PROMPT.findall(visible(stream)))


def is_ready_for_input(stream: str) -> bool:
    """True once the console has printed a prompt and is waiting for a line."""
    return prompts_in(stream) >= 1


def turn_is_complete(stream: str) -> bool:
    """True once a typed line's response has been printed and the next prompt has appeared.

    Two prompts: the one that invited the utterance, and the one that follows its output.
    """
    return prompts_in(stream) >= 2

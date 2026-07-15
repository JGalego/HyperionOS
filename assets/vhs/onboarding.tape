# Regenerates assets/demo-onboarding.gif -- a real, live recording (not staged output) of
# hyperion-console typed interactively, one utterance at a time, in the exact same order as
# scenarios/onboarding-new-user-launches-startup.txt. Typed interactively rather than fed as a
# file argument for two reasons: real interactive mode shows the real banner and the real spinner
# during "launch my startup" (run_scenario_file's own non-interactive path skips both), and typing
# each line separately with a Sleep after it gives a human viewer time to actually read one
# response before the next command appears, instead of a whole scenario file's output landing
# all at once (MockBackend answers instantly -- there is no real "thinking time" to wait out).
#
# The shell invocation and HYPERION_CONSOLE_DATA_DIR are Hidden -- the recording never shows that
# plumbing, only the real console's own startup banner (kept deliberately visible: it's the
# program's real output, not shell noise, and it's a real ttyd/xterm scrollback buffer that
# nothing ever clears once printed -- hiding it would need a real `clear` sent to the terminal
# *after* the banner, which isn't possible once hyperion-console itself owns stdin) followed
# immediately by the real "> help" prompt. `Set Framerate 25` (below the VHS default of 50) trades
# frame smoothness for fewer real screenshot calls per second -- verified live in this sandbox: at
# the default 50fps, background CPU contention from other concurrent processes made VHS silently
# drop a variable, inconsistent fraction of frames (~30-45% of the expected duration, different
# every run of the identical script); at 25fps the real recorded duration matched the scripted
# Sleep durations far more consistently across repeated runs.
#
# See assets/generate-scenario-gifs.sh for how this is actually invoked (must run from the repo
# root). Requires https://github.com/charmbracelet/vhs (`go install
# github.com/charmbracelet/vhs@latest`), which itself shells out to ttyd
# (https://github.com/tsl0922/ttyd), a headless Chromium, and ffmpeg to capture and encode.
#
# The theme below is website/app/globals.css's own real `.dark` palette (BG/FG/ACCENT/
# ACCENT_STRONG), the same one assets/generate-banner.py already uses for the README's animated
# banner -- not an invented color scheme. hyperion-console itself never emits ANSI color codes
# (CLAUDE.md's accessibility stance: nothing here depends on color to be legible), so only
# background/foreground/cursor actually appear; the rest of the theme is filled in with muted
# tints of the same palette for completeness, since VHS's Theme schema requires all 16 ANSI slots.

Output assets/demo-onboarding.gif

Set Shell "bash"
Set FontFamily "DejaVu Sans Mono"
Set FontSize 15
Set Width 1100
Set Height 680
Set Padding 20
Set Margin 20
Set MarginFill "#0b0a08"
Set BorderRadius 10
Set WindowBar Colorful
Set Framerate 25
Set TypingSpeed 45ms
Set Theme {"name":"Hyperion","background":"#0b0a08","foreground":"#f5f3ee","selection":"#d9a54a40","cursor":"#d9a54a","cursorAccent":"#0b0a08","black":"#0b0a08","brightBlack":"#4a4438","red":"#b0705f","brightRed":"#c98b7c","green":"#8fae6a","brightGreen":"#a9c98a","yellow":"#d9a54a","brightYellow":"#e6bb6e","blue":"#6a8caf","brightBlue":"#8fadc9","magenta":"#a679b0","brightMagenta":"#c199c9","cyan":"#6ab0ad","brightCyan":"#8fc9c6","white":"#f5f3ee","brightWhite":"#ffffff"}

Hide
Type "clear && rm -rf /tmp/hyperion-scratch && export HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch && ./target/debug/hyperion-console"
Enter
Sleep 1000ms
Show

Type "help"
Enter
Sleep 4000ms

Type "my name is Alex"
Enter
Sleep 2000ms

Type "what is my name"
Enter
Sleep 2500ms

Type "launch my startup"
Enter
Sleep 3500ms

Type "/result market_research"
Enter
Sleep 3000ms

Type "/recall"
Enter
Sleep 3500ms

Type "/why 1"
Enter
Sleep 2500ms

Type "/related 1"
Enter
Sleep 2500ms

Sleep 4s

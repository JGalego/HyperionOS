# Regenerates assets/demo-onboarding.gif -- a real, live recording (not staged output) of
# scenarios/onboarding-new-user-launches-startup.txt run against the real, compiled
# hyperion-console binary. See assets/generate-scenario-gifs.sh for how this is actually invoked
# (it needs to run from the repo root so the scenario file's own relative path resolves).
#
# Requires https://github.com/charmbracelet/vhs (`go install github.com/charmbracelet/vhs@latest`),
# which itself shells out to ttyd (https://github.com/tsl0922/ttyd), a headless Chromium, and
# ffmpeg to actually capture and encode the recording.
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
Set TypingSpeed 35ms
Set Theme {"name":"Hyperion","background":"#0b0a08","foreground":"#f5f3ee","selection":"#d9a54a40","cursor":"#d9a54a","cursorAccent":"#0b0a08","black":"#0b0a08","brightBlack":"#4a4438","red":"#b0705f","brightRed":"#c98b7c","green":"#8fae6a","brightGreen":"#a9c98a","yellow":"#d9a54a","brightYellow":"#e6bb6e","blue":"#6a8caf","brightBlue":"#8fadc9","magenta":"#a679b0","brightMagenta":"#c199c9","cyan":"#6ab0ad","brightCyan":"#8fc9c6","white":"#f5f3ee","brightWhite":"#ffffff"}

Hide
Type "clear && rm -rf /tmp/hyperion-scratch"
Enter
Sleep 300ms
Show

Type "HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch ./target/debug/hyperion-console scenarios/onboarding-new-user-launches-startup.txt"
Enter
Sleep 6s

# Regenerates assets/demo-knowledge-graph.gif -- a real, live recording of hyperion-console typed
# interactively, one utterance at a time, in the exact same order as
# scenarios/knowledge-graph-growth-audit.txt: an empty `/graph`, a real action that grows it, then
# both real dump formats. See assets/vhs/onboarding.tape's own header for why this is typed
# interactively rather than fed as a file (readable pacing, the real banner/spinner), why
# `Set Framerate 25` is used instead of VHS's default 50, and for the shared theme/tooling notes
# (vhs/ttyd/Chromium/ffmpeg). The shell invocation and HYPERION_CONSOLE_DATA_DIR are Hidden -- the
# recording never shows that plumbing, only the real console's own startup banner (kept
# deliberately visible -- see assets/vhs/onboarding.tape's own header for why) followed
# immediately by the real "> /graph" prompt.

Output assets/demo-knowledge-graph.gif

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

Type "/graph"
Enter
Sleep 2200ms

Type "launch my startup"
Enter
Sleep 3500ms

Type "/graph"
Enter
Sleep 4000ms

Type "/graph dot"
Enter
Sleep 4000ms

Sleep 4s

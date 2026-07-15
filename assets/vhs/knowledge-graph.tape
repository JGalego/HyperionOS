# Regenerates assets/demo-knowledge-graph.gif -- a real, live recording of
# scenarios/knowledge-graph-growth-audit.txt: an empty `/graph`, a real action that grows it, then
# both real dump formats. See assets/vhs/onboarding.tape's own header for the shared theme/tooling
# notes (vhs/ttyd/Chromium/ffmpeg), and assets/generate-scenario-gifs.sh for how this is invoked.

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
Set TypingSpeed 35ms
Set Theme {"name":"Hyperion","background":"#0b0a08","foreground":"#f5f3ee","selection":"#d9a54a40","cursor":"#d9a54a","cursorAccent":"#0b0a08","black":"#0b0a08","brightBlack":"#4a4438","red":"#b0705f","brightRed":"#c98b7c","green":"#8fae6a","brightGreen":"#a9c98a","yellow":"#d9a54a","brightYellow":"#e6bb6e","blue":"#6a8caf","brightBlue":"#8fadc9","magenta":"#a679b0","brightMagenta":"#c199c9","cyan":"#6ab0ad","brightCyan":"#8fc9c6","white":"#f5f3ee","brightWhite":"#ffffff"}

Hide
Type "clear && rm -rf /tmp/hyperion-scratch"
Enter
Sleep 300ms
Show

Type "HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch ./target/debug/hyperion-console scenarios/knowledge-graph-growth-audit.txt"
Enter
Sleep 6s

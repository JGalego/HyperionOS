# Regenerates assets/demo-cloud-providers.gif -- a real, live recording of
# scenarios/cloud-provider-comparison.txt: the same question asked of three real, paid cloud APIs
# (OpenAI, Anthropic, Groq) in one session. See assets/vhs/onboarding.tape's own header for the
# shared theme/tooling notes (vhs/ttyd/Chromium/ffmpeg).
#
# Needs a real, valid OPENAI_API_KEY/ANTHROPIC_API_KEY/GROQ_API_KEY in .env and this binary built
# with `--features openai-compat,anthropic` -- regenerating this one costs three real paid API
# calls each time. Never leaks a real key on screen: `hyperion-console` itself prints
# "> [key redacted]" in place of a real pasted key line (crates/hyperion-console/src/main.rs,
# `awaiting_secret_input`), verified live -- not something this recording relies on hiding after
# the fact. The `source .env` step is Hidden below only for a cleaner demo, not for secrecy.

Output assets/demo-cloud-providers.gif

Set Shell "bash"
Set FontFamily "DejaVu Sans Mono"
Set FontSize 15
Set Width 1100
Set Height 780
Set Padding 20
Set Margin 20
Set MarginFill "#0b0a08"
Set BorderRadius 10
Set WindowBar Colorful
Set TypingSpeed 35ms
Set Theme {"name":"Hyperion","background":"#0b0a08","foreground":"#f5f3ee","selection":"#d9a54a40","cursor":"#d9a54a","cursorAccent":"#0b0a08","black":"#0b0a08","brightBlack":"#4a4438","red":"#b0705f","brightRed":"#c98b7c","green":"#8fae6a","brightGreen":"#a9c98a","yellow":"#d9a54a","brightYellow":"#e6bb6e","blue":"#6a8caf","brightBlue":"#8fadc9","magenta":"#a679b0","brightMagenta":"#c199c9","cyan":"#6ab0ad","brightCyan":"#8fc9c6","white":"#f5f3ee","brightWhite":"#ffffff"}

Hide
Type "clear && rm -rf /tmp/hyperion-scratch && set -a && source .env && set +a"
Enter
Sleep 300ms
Show

Type "HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch ./target/debug/hyperion-console scenarios/cloud-provider-comparison.txt"
Enter
Sleep 25s

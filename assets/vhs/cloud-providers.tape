# Regenerates assets/demo-cloud-providers.gif -- a real, live recording of
# scenarios/cloud-provider-comparison.txt: the same question asked of three real, paid cloud APIs
# (OpenAI, Anthropic, Groq) in one session. Still fed as a file (not typed interactively like
# assets/vhs/onboarding.tape) because a real API key can only safely reach hyperion-console via
# that scenario file's own `$OPENAI_API_KEY`-style environment expansion
# (docs/999-usage-scenarios.md's "Running a scenario from a file") -- typing a real secret
# character-by-character into this tape file would mean committing it in plaintext. Real network
# latency across three real API calls already paces this recording -- unlike the two
# MockBackend demos, there's no instant-answer problem to work around here.
#
# The entire launch line -- the shell invocation, HYPERION_CONSOLE_DATA_DIR, sourcing .env, and
# the scenario file path -- is Hidden, same as the other two demos: the recording starts right at
# the console's own first real "> connect my openai account" line, not on any of that plumbing.
# See assets/vhs/onboarding.tape's own header for the shared theme/tooling notes and the
# `Set Framerate 25` reasoning (this sandbox silently dropped frames at the VHS default of 50fps
# under real background load).
#
# Needs a real, valid OPENAI_API_KEY/ANTHROPIC_API_KEY/GROQ_API_KEY in .env and this binary built
# with `--features openai-compat,anthropic` -- regenerating this one costs three real paid API
# calls each time. Never leaks a real key on screen: `hyperion-console` itself prints
# "> [key redacted]" in place of a real pasted key line (crates/hyperion-console/src/main.rs,
# `awaiting_secret_input`), verified live -- not something this recording relies on hiding after
# the fact.

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
Set Framerate 25
Set TypingSpeed 45ms
Set Theme {"name":"Hyperion","background":"#0b0a08","foreground":"#f5f3ee","selection":"#d9a54a40","cursor":"#d9a54a","cursorAccent":"#0b0a08","black":"#0b0a08","brightBlack":"#4a4438","red":"#b0705f","brightRed":"#c98b7c","green":"#8fae6a","brightGreen":"#a9c98a","yellow":"#d9a54a","brightYellow":"#e6bb6e","blue":"#6a8caf","brightBlue":"#8fadc9","magenta":"#a679b0","brightMagenta":"#c199c9","cyan":"#6ab0ad","brightCyan":"#8fc9c6","white":"#f5f3ee","brightWhite":"#ffffff"}

Hide
Type "clear && rm -rf /tmp/hyperion-scratch && set -a && source .env && set +a && export HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch && ./target/debug/hyperion-console scenarios/cloud-provider-comparison.txt"
Enter
Sleep 400ms
Show

Sleep 28s

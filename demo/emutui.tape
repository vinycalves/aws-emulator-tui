# emutui demo — rendered with VHS.
#
#   ./scripts/demo-setup.sh
#   vhs demo/emutui.tape
#
# Requires vhs, ttyd, and ffmpeg on PATH. The setup script writes demo/env.sh
# with an isolated HOME and a running, seeded emulator on port 9393.

Output "docs/demo-raw.mp4"

Set Shell bash
Set FontFamily "DejaVu Sans Mono"
Set FontSize 18
Set Columns 150
Set Rows 42
Set Padding 16
Set Margin 24
Set MarginFill "#0b0d12"
Set WindowBar Colorful
Set BorderRadius 10
Set TypingSpeed 12ms
Set CursorBlink false

# Setup is not part of the video: source the demo environment and launch the TUI.
Hide
Type "source demo/env.sh"
Enter
Type "./target/release/emutui --endpoint $EMULATOR_ENDPOINT"
Enter
Sleep 6s
Show
Sleep 1s

# Explorer: S3 bucket -> objects -> text preview.
Enter
Sleep 1.2s
Enter
Sleep 2.5s
Escape
Sleep 0.4s
Escape
Sleep 0.8s

# Drill into a DynamoDB table, move the selection, inspect one item.
Type "h"
Down
Sleep 1.2s
Type "l"
Type "o"
Sleep 2.8s
Down
Sleep 0.5s
Enter
Sleep 2s
Escape
Sleep 0.4s

# Browse any service: pick SQS, run ListQueues, inspect a row.
Type "2"
Sleep 0.8s
Type "s"
Type "sqs"
Sleep 0.4s
Enter
Sleep 0.8s
Type "ListQueues"
Sleep 0.4s
Enter
Sleep 0.6s
Ctrl+S
Sleep 2.8s
Down
Sleep 0.4s
Enter
Sleep 2s
Escape
Sleep 0.4s

# Command palette and diagnostics.
Ctrl+P
Sleep 1.8s
Escape
Sleep 0.3s
Type "D"
Sleep 3s
Down
Down
Sleep 0.5s
Escape
Sleep 0.5s

# Back to the explorer for the closing frame.
Type "1"
Sleep 2s
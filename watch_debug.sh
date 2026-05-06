#!/bin/bash
echo "Starting debug log watch..."
echo "Run the TUI in another terminal, then press arrow keys in /models or /agents dialogs"
echo ""
tail -f opencode_debug.log

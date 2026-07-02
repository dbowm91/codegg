---
description: Show provider quota as a dialog
runtime: process
command: python3
args: ["scripts/quota_json.py"]
stdin: json
stdout: json
timeout_ms: 5000
output: ["chat", "dialog"]
---

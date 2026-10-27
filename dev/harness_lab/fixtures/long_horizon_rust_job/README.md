# Multi-stage parser repair

Repair this crate through its public behavior contract. Start by reading the
library entry point and all named modules. The initial crate intentionally has
one compiler error. After that is fixed, its tests expose independent parsing
and normalization defects. Preserve the public tests and do not add a network
dependency. Run the native `test` tool in background mode, inspect this file
while it runs, and wait for the same CodeGG-owned job before deciding what to
edit next.

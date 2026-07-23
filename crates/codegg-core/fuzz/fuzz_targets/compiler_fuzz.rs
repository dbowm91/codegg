//! Fuzz target: full pipeline must never panic on arbitrary valid-UTF8 input.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = codegg_core::tool_program::compile_program(s);
    }
});

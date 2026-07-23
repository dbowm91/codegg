//! Fuzz target: IR serialization round-trip must not panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(result) = codegg_core::tool_program::compile_program(s) {
            if let Ok(bytes) = codegg_core::tool_program::serialize_ir(&result.ir) {
                if let Ok(restored) = codegg_core::tool_program::deserialize_ir(&bytes) {
                    let _ = codegg_core::tool_program::verify_ir_integrity(&restored);
                }
            }
        }
    }
});

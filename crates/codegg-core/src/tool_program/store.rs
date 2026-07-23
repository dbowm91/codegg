//! Program store integration for Tool Programs.
//!
//! Provides content-addressed IR storage, source verification,
//! compile diagnostics persistence, and IR reuse logic.
//!
//! # Invariants
//!
//! - Source and IR are immutable and content-addressed (SHA-256).
//! - IR is stored only after successful validation and verification.
//! - Compile diagnostics and terminal blocked/failed state are persisted
//!   without creating a runtime attempt.
//! - Existing IR is reused only when source, manifest, limits,
//!   language/compiler version, and parser version identity match.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::tool_program::diagnostics::Diagnostic;
use crate::tool_program::ir::{IrProgram, COMPILER_VERSION, LANGUAGE_VERSION, PARSER_VERSION};
use crate::tool_program::ToolProgramError;

/// Content-addressed store for compiled IR.
///
/// Stores IR by SHA-256 digest. Deduplicates identical compilations.
/// Thread-safe via interior mutability.
#[derive(Debug, Clone)]
pub struct ProgramStore {
    inner: Arc<Mutex<ProgramStoreInner>>,
}

#[derive(Debug, Default)]
struct ProgramStoreInner {
    /// IR stored by digest.
    ir_store: HashMap<String, IrProgram>,
    /// Source stored by digest.
    source_store: HashMap<String, String>,
    /// Compile diagnostics stored by source digest.
    diagnostics: HashMap<String, Vec<Diagnostic>>,
    /// Source digest → compilation metadata for cache key matching.
    compilation_keys: HashMap<String, CompilationKey>,
}

/// Key fields that must match for IR reuse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompilationKey {
    pub source_hash: String,
    pub manifest_hash: String,
    pub limits_hash: String,
    pub language_version: u32,
    pub compiler_version: u32,
    pub parser_version: u32,
}

/// Result of loading or compiling a program.
#[derive(Debug, Clone)]
pub enum ProgramLoadResult {
    /// IR was loaded from cache (exact key match).
    Cached(IrProgram),
    /// IR was freshly compiled.
    Compiled(IrProgram),
    /// Compilation failed; diagnostics are stored.
    Failed { diagnostics: Vec<Diagnostic> },
}

impl ProgramStore {
    /// Create a new empty program store.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ProgramStoreInner::default())),
        }
    }

    /// Compute SHA-256 digest of source bytes.
    pub fn digest_source(source: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Compute SHA-256 digest of arbitrary bytes.
    pub fn digest_bytes(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    /// Load source by immutable reference and verify digest.
    ///
    /// Returns `Ok(source)` if the digest matches, or `Err` if the
    /// source is not found or the digest doesn't match.
    pub fn load_source(&self, source_hash: &str, source: &str) -> Result<String, ToolProgramError> {
        let actual_hash = Self::digest_source(source);
        if actual_hash != source_hash {
            return Err(ToolProgramError::Verify(
                crate::tool_program::diagnostics::Diagnostic::new(
                    crate::tool_program::diagnostics::DiagnosticCode::VerificationFailed,
                    format!(
                        "source digest mismatch: expected {}, got {}",
                        source_hash, actual_hash
                    ),
                    crate::tool_program::diagnostics::SourceSpan::new(0, 0),
                ),
            ));
        }
        Ok(source.to_string())
    }

    /// Check if cached IR exists with a matching compilation key.
    ///
    /// Returns the cached IR if source, manifest, limits, language/compiler
    /// version, and parser version all match.
    pub fn check_cache(
        &self,
        source: &str,
        manifest_hash: &str,
        limits_hash: &str,
    ) -> Option<IrProgram> {
        let inner = self.inner.lock().unwrap();
        let source_hash = Self::digest_source(source);

        if let Some(key) = inner.compilation_keys.get(&source_hash) {
            if key.manifest_hash == manifest_hash
                && key.limits_hash == limits_hash
                && key.language_version == LANGUAGE_VERSION
                && key.compiler_version == COMPILER_VERSION
                && key.parser_version == PARSER_VERSION
            {
                if let Some(ir) = inner.ir_store.get(&source_hash) {
                    return Some(ir.clone());
                }
            }
        }
        None
    }

    /// Store compiled IR after successful validation and hash verification.
    ///
    /// The IR's source_hash must match the provided source digest.
    /// Returns `Ok(ir)` on success.
    pub fn store_ir(&self, source: &str, ir: IrProgram) -> Result<IrProgram, ToolProgramError> {
        let source_hash = Self::digest_source(source);

        // Verify the IR's source hash matches
        if ir.source_hash != source_hash {
            return Err(ToolProgramError::Verify(
                crate::tool_program::diagnostics::Diagnostic::new(
                    crate::tool_program::diagnostics::DiagnosticCode::VerificationFailed,
                    format!(
                        "IR source_hash mismatch: expected {}, got {}",
                        source_hash, ir.source_hash
                    ),
                    crate::tool_program::diagnostics::SourceSpan::new(0, 0),
                ),
            ));
        }

        let mut inner = self.inner.lock().unwrap();

        // Store the source
        inner
            .source_store
            .insert(source_hash.clone(), source.to_string());

        // Store the compilation key
        inner.compilation_keys.insert(
            source_hash.clone(),
            CompilationKey {
                source_hash: source_hash.clone(),
                manifest_hash: ir.manifest_hash.clone(),
                limits_hash: ir.limits_hash.clone(),
                language_version: ir.language_version,
                compiler_version: ir.compiler_version,
                parser_version: ir.parser_version,
            },
        );

        // Store the IR
        inner.ir_store.insert(source_hash, ir.clone());

        Ok(ir)
    }

    /// Persist compile diagnostics for a source.
    pub fn store_diagnostics(&self, source: &str, diagnostics: Vec<Diagnostic>) {
        let source_hash = Self::digest_source(source);
        let mut inner = self.inner.lock().unwrap();
        inner.diagnostics.insert(source_hash, diagnostics);
    }

    /// Retrieve stored diagnostics for a source.
    pub fn get_diagnostics(&self, source: &str) -> Vec<Diagnostic> {
        let source_hash = Self::digest_source(source);
        let inner = self.inner.lock().unwrap();
        inner
            .diagnostics
            .get(&source_hash)
            .cloned()
            .unwrap_or_default()
    }

    /// Retrieve stored IR by source digest.
    pub fn get_ir(&self, source: &str) -> Option<IrProgram> {
        let source_hash = Self::digest_source(source);
        let inner = self.inner.lock().unwrap();
        inner.ir_store.get(&source_hash).cloned()
    }

    /// Retrieve stored source by digest.
    pub fn get_source(&self, source_hash: &str) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        inner.source_store.get(source_hash).cloned()
    }

    /// Check if IR exists for a given source.
    pub fn contains_ir(&self, source: &str) -> bool {
        let source_hash = Self::digest_source(source);
        let inner = self.inner.lock().unwrap();
        inner.ir_store.contains_key(&source_hash)
    }

    /// Remove IR and associated data for a source.
    pub fn remove(&self, source: &str) -> bool {
        let source_hash = Self::digest_source(source);
        let mut inner = self.inner.lock().unwrap();
        let removed = inner.ir_store.remove(&source_hash).is_some();
        inner.source_store.remove(&source_hash);
        inner.compilation_keys.remove(&source_hash);
        inner.diagnostics.remove(&source_hash);
        removed
    }

    /// Number of stored IR entries.
    pub fn len(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.ir_store.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ProgramStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Serialize an IR program to JSON bytes.
pub fn serialize_ir(ir: &IrProgram) -> Result<Vec<u8>, ToolProgramError> {
    serde_json::to_vec(ir).map_err(|e| {
        ToolProgramError::Compile(crate::tool_program::diagnostics::Diagnostic::new(
            crate::tool_program::diagnostics::DiagnosticCode::InternalError,
            format!("IR serialization failed: {}", e),
            crate::tool_program::diagnostics::SourceSpan::new(0, 0),
        ))
    })
}

/// Deserialize an IR program from JSON bytes.
pub fn deserialize_ir(data: &[u8]) -> Result<IrProgram, ToolProgramError> {
    serde_json::from_slice(data).map_err(|e| {
        ToolProgramError::Verify(crate::tool_program::diagnostics::Diagnostic::new(
            crate::tool_program::diagnostics::DiagnosticCode::VerificationFailed,
            format!("IR deserialization failed: {}", e),
            crate::tool_program::diagnostics::SourceSpan::new(0, 0),
        ))
    })
}

/// Verify that deserialized IR matches the original's digest.
pub fn verify_ir_integrity(ir: &IrProgram) -> Result<(), ToolProgramError> {
    use crate::tool_program::compiler::compute_digest_public;
    let expected = compute_digest_public(ir);
    if ir.digest != expected {
        return Err(ToolProgramError::Verify(
            crate::tool_program::diagnostics::Diagnostic::new(
                crate::tool_program::diagnostics::DiagnosticCode::VerificationFailed,
                format!(
                    "IR digest mismatch after deserialization: expected {}, got {}",
                    expected, ir.digest
                ),
                crate::tool_program::diagnostics::SourceSpan::new(0, 0),
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_program::{compile_program, parse_source, static_bounds, validate};

    #[test]
    fn store_and_retrieve_ir() {
        let src = "emit({\"ok\": true})\n";
        let result = compile_program(src).unwrap();
        let store = ProgramStore::new();

        let stored = store.store_ir(src, result.ir.clone()).unwrap();
        assert_eq!(stored.digest, result.ir.digest);

        let retrieved = store.get_ir(src).unwrap();
        assert_eq!(retrieved.digest, result.ir.digest);
    }

    #[test]
    fn cache_hit_on_matching_key() {
        let src = "emit({\"ok\": true})\n";
        let result = compile_program(src).unwrap();
        let store = ProgramStore::new();
        store.store_ir(src, result.ir).unwrap();

        let cached = store.check_cache(src, "", "");
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().digest, store.get_ir(src).unwrap().digest);
    }

    #[test]
    fn cache_miss_on_different_manifest() {
        let src = "emit({\"ok\": true})\n";
        let result = compile_program(src).unwrap();
        let store = ProgramStore::new();
        store.store_ir(src, result.ir).unwrap();

        let cached = store.check_cache(src, "different_manifest", "");
        assert!(cached.is_none());
    }

    #[test]
    fn cache_miss_on_different_limits() {
        let src = "emit({\"ok\": true})\n";
        let result = compile_program(src).unwrap();
        let store = ProgramStore::new();
        store.store_ir(src, result.ir).unwrap();

        let cached = store.check_cache(src, "", "different_limits");
        assert!(cached.is_none());
    }

    #[test]
    fn load_source_verifies_digest() {
        let store = ProgramStore::new();
        let src = "emit({\"ok\": true})\n";
        let hash = ProgramStore::digest_source(src);

        // Correct digest
        assert!(store.load_source(&hash, src).is_ok());

        // Wrong digest
        assert!(store.load_source("wrong_hash", src).is_err());
    }

    #[test]
    fn store_ir_rejects_mismatched_source_hash() {
        let src = "emit({\"ok\": true})\n";
        let result = compile_program(src).unwrap();
        let store = ProgramStore::new();

        // Create IR with wrong source hash
        let mut ir = result.ir;
        ir.source_hash = "wrong_hash".to_string();

        assert!(store.store_ir(src, ir).is_err());
    }

    #[test]
    fn diagnostics_round_trip() {
        let store = ProgramStore::new();
        let src = "bad code";
        let diags = vec![crate::tool_program::diagnostics::Diagnostic::new(
            crate::tool_program::diagnostics::DiagnosticCode::UnsupportedSyntax,
            "test error".to_string(),
            crate::tool_program::diagnostics::SourceSpan::new(0, 5),
        )];

        store.store_diagnostics(src, diags.clone());
        let retrieved = store.get_diagnostics(src);
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].message, "test error");
    }

    #[test]
    fn remove_entry() {
        let src = "emit(1)\n";
        let result = compile_program(src).unwrap();
        let store = ProgramStore::new();
        store.store_ir(src, result.ir).unwrap();
        assert!(store.contains_ir(src));

        assert!(store.remove(src));
        assert!(!store.contains_ir(src));
        assert!(store.get_ir(src).is_none());
    }

    #[test]
    fn store_deduplicates_identical_compilations() {
        let src = "emit({\"ok\": true})\n";
        let result1 = compile_program(src).unwrap();
        let result2 = compile_program(src).unwrap();
        let store = ProgramStore::new();

        store.store_ir(src, result1.ir.clone()).unwrap();
        store.store_ir(src, result2.ir.clone()).unwrap();

        // Should still be 1 entry (deduplication by source hash)
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn serialize_round_trip() {
        let src = "x = 1\nemit(x)\n";
        let result = compile_program(src).unwrap();

        let bytes = serialize_ir(&result.ir).unwrap();
        let restored = deserialize_ir(&bytes).unwrap();

        assert_eq!(restored.digest, result.ir.digest);
        assert_eq!(restored.instructions, result.ir.instructions);
        assert_eq!(restored.local_count, result.ir.local_count);
        verify_ir_integrity(&restored).unwrap();
    }

    #[test]
    fn serialize_round_trip_complex() {
        let src = r#"
results = []
for file in ["a.py", "b.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results), "files": results})
"#;
        let result = compile_program(src).unwrap();
        let bytes = serialize_ir(&result.ir).unwrap();
        let restored = deserialize_ir(&bytes).unwrap();

        assert_eq!(restored.digest, result.ir.digest);
        assert_eq!(restored.instructions.len(), result.ir.instructions.len());
        assert_eq!(restored.strings, result.ir.strings);
        assert_eq!(restored.integers, result.ir.integers);
        verify_ir_integrity(&restored).unwrap();
    }

    #[test]
    fn serialize_corrupted_data_fails() {
        let bad_data = b"{\"not_valid_json";
        assert!(deserialize_ir(bad_data).is_err());
    }

    #[test]
    fn store_thread_safety() {
        use std::thread;

        let store = ProgramStore::new();
        let src = "emit(1)\n";
        let result = compile_program(src).unwrap();
        let ir = result.ir;

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let store = store.clone();
                let ir = ir.clone();
                let src = src.to_string();
                thread::spawn(move || {
                    for _ in 0..100 {
                        let _ = store.store_ir(&src, ir.clone());
                        let _ = store.get_ir(&src);
                        let _ = store.check_cache(&src, "", "");
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(store.len(), 1);
    }
}

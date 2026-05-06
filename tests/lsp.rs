use codegg::lsp::language::{detect_language, language_id_to_server_id};

#[test]
fn test_detect_language_rust() {
    let lang = detect_language("test.rs");
    assert!(lang.is_some());
    assert_eq!(lang.unwrap(), "rust");
}

#[test]
fn test_detect_language_python() {
    let lang = detect_language("test.py");
    assert!(lang.is_some());
    assert_eq!(lang.unwrap(), "python");
}

#[test]
fn test_detect_language_typescript() {
    let lang = detect_language("test.ts");
    assert!(lang.is_some());
    assert_eq!(lang.unwrap(), "typescript");
}

#[test]
fn test_detect_language_unknown() {
    let lang = detect_language("test.unknown");
    assert!(lang.is_none());
}

#[test]
fn test_language_id_to_server_id_rust() {
    let server_id = language_id_to_server_id("rust");
    assert!(server_id.is_some());
}

#[test]
fn test_language_id_to_server_id_unknown() {
    let server_id = language_id_to_server_id("nonexistent");
    assert!(server_id.is_none());
}

use codegg::memory::{Memory, MemoryStore};

fn create_memory_store() -> MemoryStore {
    MemoryStore::new().expect("failed to create memory store")
}

#[test]
fn test_memory_new() {
    let memory = Memory::new("test-namespace", "Test content");

    assert_eq!(memory.namespace, "test-namespace");
    assert_eq!(memory.content, "Test content");
    assert!(!memory.id.is_empty());
    assert!(memory.title.is_none());
    assert_eq!(memory.access_count, 0);
    assert_eq!(memory.importance, 0.5);
}

#[test]
fn test_memory_with_title() {
    let mut memory = Memory::new("test-namespace", "Test content");
    memory.title = Some("Test Title".to_string());

    assert_eq!(memory.title, Some("Test Title".to_string()));
}

#[test]
fn test_memory_store_add() {
    let store = create_memory_store();
    let memory = Memory::new("test-ns", "content");
    let id = memory.id.clone();

    let old = store.add(memory);
    assert!(old.is_none());

    let retrieved = store.get(&id);
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().content, "content");
}

#[test]
fn test_memory_store_list_empty() {
    let store = create_memory_store();
    let list = store.list("nonexistent");
    assert!(list.is_empty());
}

#[test]
fn test_memory_store_search_empty() {
    let store = create_memory_store();
    let results = store.search("nonexistent");
    assert!(results.is_empty());
}

#[test]
fn test_memory_store_delete_nonexistent() {
    let store = create_memory_store();
    let result = store.delete("nonexistent-id");
    assert!(result.is_none());
}

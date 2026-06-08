use dashmap::DashMap;
use std::sync::Arc;
use std::sync::LazyLock;

#[derive(Default)]
pub struct StringInterner {
    map: DashMap<Arc<str>, Arc<str>>,
}

impl StringInterner {
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    pub fn intern(&self, s: &str) -> Arc<str> {
        if let Some(existing) = self.map.get(s) {
            return existing.clone();
        }
        let interned: Arc<str> = Arc::from(s);
        self.map.insert(interned.clone(), interned.clone());
        interned
    }

    pub fn intern_string(&self, s: String) -> Arc<str> {
        self.intern(&s)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

static TOOL_STRING_INTERNER: LazyLock<StringInterner> = LazyLock::new(|| StringInterner::new());

pub fn tool_interner() -> &'static StringInterner {
    &TOOL_STRING_INTERNER
}

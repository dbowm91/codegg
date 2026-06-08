use std::path::PathBuf;

pub struct TestTempDir {
    temp_dir: tempfile::TempDir,
}

impl TestTempDir {
    pub fn new() -> Self {
        Self {
            temp_dir: tempfile::tempdir().expect("failed to create temp dir"),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.temp_dir.path().to_path_buf()
    }

    pub fn create_file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.temp_dir.path().join(name);
        std::fs::write(&path, content).expect("failed to write temp file");
        path
    }

    pub fn create_dir(&self, name: &str) -> PathBuf {
        let path = self.temp_dir.path().join(name);
        std::fs::create_dir_all(&path).expect("failed to create temp dir");
        path
    }
}

impl Default for TestTempDir {
    fn default() -> Self {
        Self::new()
    }
}

pub fn create_test_project() -> TestTempDir {
    TestTempDir::new()
}

pub fn make_skill_content(name: &str, description: &str, body: &str) -> String {
    format!(
        "---\nname: {}\ndescription: {}\n---\n{}\n",
        name, description, body
    )
}

pub async fn wait_for_async<T>(mut check: impl FnMut() -> Option<T>, timeout_secs: u64) -> T {
    use std::future::Future;
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    loop {
        if let Some(value) = check() {
            return value;
        }

        if start.elapsed() > timeout {
            panic!("timeout waiting for condition");
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub title: String,
    pub sections: Vec<String>,
}

pub fn parse(input: &str) -> anyhow::Result<Document> {
    let mut lines = input.lines();
    let title = lines.next().unwrap_or_default().trim().to_owned();
    anyhow::ensure!(!title.is_empty(), "missing title");
    let sections = lines.map(str::trim).filter(|line| !line.is_empty())
        .map(ToOwned::to_owned).collect();
    Ok(Document { title, sections })
}

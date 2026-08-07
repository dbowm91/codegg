//! Format-neutral structured-document parsing boundaries.
//!
//! YAML is currently a compatibility input format for markdown frontmatter.
//! Keep the parser dependency and its error model in this module so callers
//! cannot accidentally grow new direct parser-crate call sites.

use serde::de::DeserializeOwned;
use std::fmt;

/// Structured formats understood by the internal document boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat {
    /// TOML configuration or asset input.
    Toml,
    /// JSON5 configuration input.
    Json5,
    /// YAML frontmatter retained for compatibility with existing assets.
    YamlCompatibility,
}

impl fmt::Display for DocumentFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml => f.write_str("TOML"),
            Self::Json5 => f.write_str("JSON5"),
            Self::YamlCompatibility => f.write_str("YAML compatibility"),
        }
    }
}

/// Broad, stable error classes exposed to document consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentErrorClass {
    /// The byte input was not valid UTF-8.
    Encoding,
    /// The document syntax could not be parsed.
    Syntax,
    /// The document parsed but did not match the requested Serde type.
    Type,
    /// More than one YAML document was supplied to a single-document loader.
    MultiDocument,
    /// The bounded compatibility loader rejected the input before parsing.
    ResourceLimit,
}

impl fmt::Display for DocumentErrorClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Encoding => "encoding",
            Self::Syntax => "syntax",
            Self::Type => "type",
            Self::MultiDocument => "multiple-documents",
            Self::ResourceLimit => "resource-limit",
        })
    }
}

/// Typed parser failure shared by structured-document consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentParseError {
    pub format: DocumentFormat,
    pub source_name: String,
    pub class: DocumentErrorClass,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl fmt::Display for DocumentParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.format, self.source_name, self.message)?;
        if let Some(line) = self.line {
            write!(f, " (line {line}")?;
            if let Some(column) = self.column {
                write!(f, ", column {column}")?;
            }
            f.write_str(")")?;
        }
        Ok(())
    }
}

impl std::error::Error for DocumentParseError {}

/// Format-neutral dynamic YAML value used for metadata that must be retained.
pub type StructuredValue = serde_json::Value;

/// Maximum bytes accepted by one YAML frontmatter/document parse.
pub const MAX_YAML_DOCUMENT_BYTES: usize = 1 << 20;

/// Parse one YAML document through the compatibility boundary.
pub fn parse_yaml<T: DeserializeOwned>(
    source_name: impl Into<String>,
    bytes: &[u8],
) -> Result<T, DocumentParseError> {
    let source_name = source_name.into();
    if bytes.len() > MAX_YAML_DOCUMENT_BYTES {
        return Err(DocumentParseError {
            format: DocumentFormat::YamlCompatibility,
            source_name,
            class: DocumentErrorClass::ResourceLimit,
            line: None,
            column: None,
            message: format!(
                "document exceeds the {} byte compatibility limit",
                MAX_YAML_DOCUMENT_BYTES
            ),
        });
    }

    let source = std::str::from_utf8(bytes).map_err(|error| DocumentParseError {
        format: DocumentFormat::YamlCompatibility,
        source_name: source_name.clone(),
        class: DocumentErrorClass::Encoding,
        line: None,
        column: None,
        message: error.to_string(),
    })?;

    serde_norway::from_str(source).map_err(|error| {
        let message = error.to_string();
        let class = classify_error(&message);
        let location = error.location();
        DocumentParseError {
            format: DocumentFormat::YamlCompatibility,
            source_name,
            class,
            line: location.as_ref().map(serde_norway::Location::line),
            column: location.as_ref().map(serde_norway::Location::column),
            message,
        }
    })
}

fn classify_error(message: &str) -> DocumentErrorClass {
    let lower = message.to_ascii_lowercase();
    if lower.contains("more than one document") {
        DocumentErrorClass::MultiDocument
    } else if lower.contains("unknown field")
        || lower.contains("missing field")
        || lower.contains("invalid type")
        || lower.contains("expected ")
    {
        DocumentErrorClass::Type
    } else {
        DocumentErrorClass::Syntax
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Fixture {
        name: String,
        enabled: bool,
        values: Vec<String>,
    }

    #[test]
    fn parses_representative_yaml_without_exposing_parser_type() {
        let fixture: Fixture = parse_yaml(
            "fixtures/representative.md",
            br#"name: codegg
enabled: true
values: [one, "two words"]
"#,
        )
        .unwrap();
        assert_eq!(
            fixture,
            Fixture {
                name: "codegg".into(),
                enabled: true,
                values: vec!["one".into(), "two words".into()]
            }
        );
    }

    #[test]
    fn preserves_dynamic_metadata_as_json_value() {
        let value: StructuredValue =
            parse_yaml("fixtures/metadata.md", b"allowed-tools: [read, bash]\n").unwrap();
        assert_eq!(value["allowed-tools"][0], "read");
        assert_eq!(value["allowed-tools"][1], "bash");
    }

    #[test]
    fn duplicate_keys_have_explicit_last_value_compatibility() {
        let value: StructuredValue =
            parse_yaml("fixtures/duplicate.md", b"name: first\nname: second\n").unwrap();
        assert_eq!(value["name"], "second");
    }

    #[test]
    fn rejects_multi_document_streams() {
        let error =
            parse_yaml::<StructuredValue>("fixtures/multi.md", b"name: one\n---\nname: two\n")
                .unwrap_err();
        assert_eq!(error.class, DocumentErrorClass::MultiDocument);
    }

    #[test]
    fn rejects_non_utf8_and_oversized_input_before_parsing() {
        let error = parse_yaml::<StructuredValue>("fixtures/bytes.md", &[0xff]).unwrap_err();
        assert_eq!(error.class, DocumentErrorClass::Encoding);

        let oversized = vec![b'x'; MAX_YAML_DOCUMENT_BYTES + 1];
        let error = parse_yaml::<StructuredValue>("fixtures/large.md", &oversized).unwrap_err();
        assert_eq!(error.class, DocumentErrorClass::ResourceLimit);
    }
}

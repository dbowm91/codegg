use crate::error::AppError;

#[cfg(feature = "arboard")]
pub fn copy_to_clipboard(text: &str) -> Result<(), AppError> {
    arboard::Clipboard::new()
        .map_err(|e| AppError::Clipboard(e.to_string()))?
        .set_text(text)
        .map_err(|e| AppError::Clipboard(e.to_string()))
}

#[cfg(not(feature = "arboard"))]
pub fn copy_to_clipboard(_text: &str) -> Result<(), AppError> {
    Err(AppError::Clipboard(
        "clipboard support not enabled".to_string(),
    ))
}

#[cfg(feature = "arboard")]
pub fn read_from_clipboard() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok())
}

#[cfg(not(feature = "arboard"))]
pub fn read_from_clipboard() -> Option<String> {
    None
}

//! Errors shown in the UI. They serialize as a plain string, which is the shape
//! the frontend already displays.

use std::fmt;
use std::sync::PoisonError;

#[derive(Debug, Clone)]
pub struct AppError(String);

impl AppError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.0.contains(needle)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AppError {}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl From<String> for AppError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for AppError {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(value: tauri::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<notify::Error> for AppError {
    fn from(value: notify::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<renpy_core::rpa::RpaError> for AppError {
    fn from(value: renpy_core::rpa::RpaError) -> Self {
        Self(value.to_string())
    }
}

impl From<zip::result::ZipError> for AppError {
    fn from(value: zip::result::ZipError) -> Self {
        Self(value.to_string())
    }
}

impl From<std::string::FromUtf8Error> for AppError {
    fn from(value: std::string::FromUtf8Error) -> Self {
        Self(value.to_string())
    }
}

impl<T> From<PoisonError<T>> for AppError {
    fn from(value: PoisonError<T>) -> Self {
        Self(value.to_string())
    }
}

impl From<AppError> for String {
    fn from(value: AppError) -> Self {
        value.0
    }
}

use std::fmt;

/// Every user-visible failure. `Message` carries fixed wording: those strings
/// are already reviewed, reach the user and are asserted by tests, so they
/// must not drift.
#[derive(Debug)]
pub enum ConvoyError {
    Message(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for ConvoyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvoyError::Message(text) => f.write_str(text),
            ConvoyError::Io(error) => write!(f, "{error}"),
            ConvoyError::Json(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ConvoyError {}

impl From<std::io::Error> for ConvoyError {
    fn from(error: std::io::Error) -> Self {
        ConvoyError::Io(error)
    }
}

impl From<serde_json::Error> for ConvoyError {
    fn from(error: serde_json::Error) -> Self {
        ConvoyError::Json(error)
    }
}

impl ConvoyError {
    pub fn message(text: impl Into<String>) -> Self {
        ConvoyError::Message(text.into())
    }
}

pub type Result<T> = std::result::Result<T, ConvoyError>;

/// `Err(ConvoyError::Message(...))`, shaped like `throw new Error(...)`.
#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => {
        return Err($crate::ConvoyError::Message(format!($($arg)*)))
    };
}

/// `if condition { throw new Error(...) }`.
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $($arg:tt)*) => {
        if !($cond) {
            return Err($crate::ConvoyError::Message(format!($($arg)*)));
        }
    };
}

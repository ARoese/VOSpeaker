use crate::UIError;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// simultaneously makes a UIError struct and prints the error to the console
pub fn raise(reason: &str) -> UIError {
    eprintln!("ERROR: {}", reason);
    UIError {
        message: reason.into(),
    }
}

/// makes a UIError struct
pub fn make_error(message: &str) -> UIError {
    UIError {
        message: message.into()
    }
}

impl Display for UIError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

impl Error for UIError {}
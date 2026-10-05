//! Pickle reader for Ren'Py archives, compiled scripts and saves.
//!
//! Classes are never imported or called. A global becomes an inert object, or
//! is rejected, depending on the [`Policy`]. The archive writer lives in
//! `rpa::pickle` and only emits protocol 2.

pub(crate) mod opcode;
mod policy;
mod read;
mod text;

pub use policy::Policy;
pub use text::latin1_bytes;

/// Read one pickle. `policy` selects which globals are accepted and the size caps.
pub fn load(data: &[u8], policy: &Policy) -> Result<Value, PickleError> {
    if let Some(max) = policy.limits.max_input {
        if data.len() > max {
            return Err(PickleError::new("pickle is too large"));
        }
    }
    read::parse(data, policy)
}

#[derive(Debug)]
pub struct PickleError(pub String);

impl std::fmt::Display for PickleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PickleError {}

impl PickleError {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        PickleError(msg.into())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    None,
    Bool(bool),
    Int(i128),
    Float(f64),
    Bytes(Vec<u8>),
    Str(String),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    Set(Vec<Value>),
    Object {
        class: String,
        args: Vec<Value>,
        state: Option<Box<Value>>,
    },
}

#[cfg(test)]
mod tests;

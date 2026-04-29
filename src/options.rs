//! User-defined CLI options: the kind/value types, the `FromOptionValue`
//! trait, and the internal `CmdOption` registry entry used by the Installer.

/// Declared shape of a user-defined command-line option. Register via
/// [`crate::Installer::option`]; read parsed results via
/// [`crate::Installer::get_option`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKind {
    /// Presence-only switch. `--name` → `true`, absent → `false`.
    Flag,
    /// String value. `--name value` or `--name=value`.
    String,
    /// Signed integer value. `--name 42` or `--name=42`.
    Int,
    /// Explicit boolean. `--name true|false|1|0|yes|no|on|off`.
    Bool,
}

/// Parsed value of a user-defined option, stored per-option after
/// [`crate::Installer::process_commandline`].
#[derive(Clone, Debug)]
pub enum OptionValue {
    Flag(bool),
    String(String),
    Int(i64),
    Bool(bool),
}

impl From<bool> for OptionValue {
    fn from(b: bool) -> Self {
        OptionValue::Bool(b)
    }
}
impl From<String> for OptionValue {
    fn from(s: String) -> Self {
        OptionValue::String(s)
    }
}
impl From<&str> for OptionValue {
    fn from(s: &str) -> Self {
        OptionValue::String(s.to_string())
    }
}
impl From<&String> for OptionValue {
    fn from(s: &String) -> Self {
        OptionValue::String(s.clone())
    }
}
impl From<i64> for OptionValue {
    fn from(n: i64) -> Self {
        OptionValue::Int(n)
    }
}
impl From<i32> for OptionValue {
    fn from(n: i32) -> Self {
        OptionValue::Int(n as i64)
    }
}

/// Types that can be pulled out of an [`OptionValue`] via
/// [`crate::Installer::get_option`]. Implemented for `bool`, `String`, `i64`,
/// `i32`, `u64`, `u32`.
pub trait FromOptionValue: Sized {
    fn from_option_value(v: &OptionValue) -> Option<Self>;
}

impl FromOptionValue for bool {
    fn from_option_value(v: &OptionValue) -> Option<Self> {
        match v {
            OptionValue::Flag(b) | OptionValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}
impl FromOptionValue for String {
    fn from_option_value(v: &OptionValue) -> Option<Self> {
        match v {
            OptionValue::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}
impl FromOptionValue for i64 {
    fn from_option_value(v: &OptionValue) -> Option<Self> {
        match v {
            OptionValue::Int(n) => Some(*n),
            _ => None,
        }
    }
}
impl FromOptionValue for i32 {
    fn from_option_value(v: &OptionValue) -> Option<Self> {
        match v {
            OptionValue::Int(n) => i32::try_from(*n).ok(),
            _ => None,
        }
    }
}
impl FromOptionValue for u64 {
    fn from_option_value(v: &OptionValue) -> Option<Self> {
        match v {
            OptionValue::Int(n) if *n >= 0 => Some(*n as u64),
            _ => None,
        }
    }
}
impl FromOptionValue for u32 {
    fn from_option_value(v: &OptionValue) -> Option<Self> {
        match v {
            OptionValue::Int(n) if *n >= 0 => u32::try_from(*n as u64).ok(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CmdOption {
    pub(crate) name: String,
    pub(crate) kind: OptionKind,
    /// One-line description for future `--help` output.
    pub(crate) help: String,
}

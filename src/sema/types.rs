// Type definitions for the Ferro type system.

/// The types that Ferro currently supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I64,
    Bool,
    Str,
    Void,
}

impl Type {
    /// Parse a type name string into a Type.
    pub fn from_name(name: &str) -> Option<Type> {
        match name {
            "i64"  => Some(Type::I64),
            "bool" => Some(Type::Bool),
            "str"  => Some(Type::Str),
            _      => None,
        }
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::I64  => write!(f, "i64"),
            Type::Bool => write!(f, "bool"),
            Type::Str  => write!(f, "str"),
            Type::Void => write!(f, "void"),
        }
    }
}

/// Source location tracking and error reporting.

/// A position in the source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

/// A span of source code (start..end).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

impl Span {
    pub fn new(start: Pos, end: Pos) -> Self {
        Self { start, end }
    }
}

/// A compiler error with a message and source location.
#[derive(Debug)]
pub struct CompileError {
    pub message: String,
    pub span: Option<Span>,
}

impl CompileError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
        }
    }

    pub fn no_span(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
        }
    }

    /// Pretty-print the error with source context.
    pub fn report(&self, source: &str, filename: &str) {
        if let Some(span) = &self.span {
            eprintln!(
                "error: {} --> {}:{}:{}",
                self.message, filename, span.start.line, span.start.column
            );
            // Show the offending line
            if let Some(line) = source.lines().nth(span.start.line - 1) {
                eprintln!("  |");
                eprintln!("  | {}", line);
                eprintln!(
                    "  | {}{}",
                    " ".repeat(span.start.column - 1),
                    "^".repeat((span.end.column - span.start.column).max(1))
                );
            }
        } else {
            eprintln!("error: {}", self.message);
        }
    }
}

pub type Result<T> = std::result::Result<T, CompileError>;

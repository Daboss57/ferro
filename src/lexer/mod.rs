// Lexer module — tokenizes Ferro source code.

pub mod token;

use crate::error::{CompileError, Pos, Span};
use token::{Token, TokenKind};

/// The lexer: walks through source text and produces tokens.
pub struct Lexer {
    /// The full source code as a vector of characters.
    chars: Vec<char>,
    /// Current position in the chars vector.
    pos: usize,
    /// Current line number (1-based).
    line: usize,
    /// Current column number (1-based).
    column: usize,
}

impl Lexer {
    /// Create a new lexer for the given source code.
    pub fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    /// Tokenize the entire source code into a list of tokens.
    pub fn tokenize(&mut self) -> Result<Vec<Token>, CompileError> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    // -- Character helpers --

    /// Peek at the current character without consuming it.
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Peek at the next character (one ahead of current).
    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    /// Consume the current character and advance the cursor.
    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    /// Get the current source position.
    fn current_pos(&self) -> Pos {
        Pos {
            offset: self.pos,
            line: self.line,
            column: self.column,
        }
    }

    // -- Whitespace & comments --

    /// Skip whitespace and comments, return when we hit real content.
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while let Some(ch) = self.peek() {
                if ch.is_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }

            // Skip line comments: // ...
            if self.peek() == Some('/') && self.peek_next() == Some('/') {
                while let Some(ch) = self.peek() {
                    if ch == '\n' {
                        break;
                    }
                    self.advance();
                }
                continue; // loop back to skip more whitespace after the comment
            }

            break;
        }
    }

    // -- Token readers --

    /// Read a number literal (integers only for now).
    fn read_number(&mut self, start: Pos) -> Result<Token, CompileError> {
        let mut num_str = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        let end = self.current_pos();
        let value: i64 = num_str.parse().map_err(|_| {
            CompileError::new(
                format!("invalid integer literal '{}'", num_str),
                Span::new(start, end),
            )
        })?;
        Ok(Token {
            kind: TokenKind::Int(value),
            span: Span::new(start, end),
        })
    }

    /// Read an identifier or keyword.
    fn read_identifier(&mut self, start: Pos) -> Token {
        let mut word = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                word.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        let end = self.current_pos();
        // Check if the word is a keyword
        let kind = token::lookup_keyword(&word).unwrap_or(TokenKind::Ident(word));
        Token {
            kind,
            span: Span::new(start, end),
        }
    }

    /// Read a string literal (everything between double quotes).
    fn read_string(&mut self, start: Pos) -> Result<Token, CompileError> {
        let mut value = String::new();
        loop {
            match self.advance() {
                Some('"') => {
                    let end = self.current_pos();
                    return Ok(Token {
                        kind: TokenKind::StringLit(value),
                        span: Span::new(start, end),
                    });
                }
                Some('\\') => {
                    // Handle escape sequences
                    match self.advance() {
                        Some('n') => value.push('\n'),
                        Some('t') => value.push('\t'),
                        Some('\\') => value.push('\\'),
                        Some('"') => value.push('"'),
                        Some(ch) => {
                            let end = self.current_pos();
                            return Err(CompileError::new(
                                format!("unknown escape sequence '\\{}'", ch),
                                Span::new(start, end),
                            ));
                        }
                        None => {
                            let end = self.current_pos();
                            return Err(CompileError::new(
                                "unexpected end of file in escape sequence",
                                Span::new(start, end),
                            ));
                        }
                    }
                }
                Some(ch) => value.push(ch),
                None => {
                    let end = self.current_pos();
                    return Err(CompileError::new(
                        "unterminated string literal",
                        Span::new(start, end),
                    ));
                }
            }
        }
    }

    // -- Main tokenizer --

    /// Read the next token from the source.
    fn next_token(&mut self) -> Result<Token, CompileError> {
        self.skip_whitespace_and_comments();

        let start = self.current_pos();

        let ch = match self.peek() {
            Some(ch) => ch,
            None => {
                // End of file
                return Ok(Token {
                    kind: TokenKind::Eof,
                    span: Span::new(start, start),
                });
            }
        };

        // Numbers
        if ch.is_ascii_digit() {
            return self.read_number(start);
        }

        // Identifiers and keywords
        if ch.is_alphabetic() || ch == '_' {
            return Ok(self.read_identifier(start));
        }

        // String literals
        if ch == '"' {
            self.advance(); // consume opening quote
            return self.read_string(start);
        }

        // Operators and punctuation
        self.advance(); // consume the character
        let kind = match ch {
            '+' => TokenKind::Plus,
            '*' => TokenKind::Star,
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            ';' => TokenKind::Semicolon,
            ':' => TokenKind::Colon,
            ',' => TokenKind::Comma,

            // Two-character operators: need to peek ahead
            '-' => {
                if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Arrow      // ->
                } else {
                    TokenKind::Minus      // -
                }
            }
            '/' => TokenKind::Slash, // comments already handled above
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::EqualEqual // ==
                } else {
                    TokenKind::Equals     // =
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::BangEqual  // !=
                } else {
                    TokenKind::Bang       // !
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::LessEqual  // <=
                } else {
                    TokenKind::Less       // <
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::GreaterEqual // >=
                } else {
                    TokenKind::Greater      // >
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.advance();
                    TokenKind::AmpAmp     // &&
                } else {
                    let end = self.current_pos();
                    return Err(CompileError::new(
                        "unexpected character '&' (did you mean '&&'?)",
                        Span::new(start, end),
                    ));
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    TokenKind::PipePipe   // ||
                } else {
                    let end = self.current_pos();
                    return Err(CompileError::new(
                        "unexpected character '|' (did you mean '||'?)",
                        Span::new(start, end),
                    ));
                }
            }

            _ => {
                let end = self.current_pos();
                return Err(CompileError::new(
                    format!("unexpected character '{}'", ch),
                    Span::new(start, end),
                ));
            }
        };

        let end = self.current_pos();
        Ok(Token {
            kind,
            span: Span::new(start, end),
        })
    }
}

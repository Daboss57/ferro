// Parser module — parses tokens into an AST.
//
// Uses recursive descent for statements and Pratt parsing for expressions.
// Pratt parsing handles operator precedence: * and / bind tighter than + and -.

use crate::ast::*;
use crate::error::{CompileError, Span};
use crate::lexer::token::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ── Helpers ──────────────────────────────────────────────

    /// Peek at the current token's kind.
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    /// Get the current token's span.
    fn span(&self) -> Span {
        self.tokens[self.pos].span
    }

    /// Advance and return the consumed token.
    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    /// Expect the current token to be a specific kind, consume it, or error.
    fn expect(&mut self, expected: &TokenKind) -> Result<Span, CompileError> {
        if self.peek() == expected {
            let span = self.span();
            self.advance();
            Ok(span)
        } else {
            Err(CompileError::new(
                format!("expected {:?}, found {:?}", expected, self.peek()),
                self.span(),
            ))
        }
    }

    /// Expect an identifier, consume it, and return its name.
    fn expect_ident(&mut self) -> Result<(String, Span), CompileError> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                let span = self.span();
                self.advance();
                Ok((name, span))
            }
            _ => Err(CompileError::new(
                format!("expected identifier, found {:?}", self.peek()),
                self.span(),
            )),
        }
    }

    // ── Program ─────────────────────────────────────────────

    /// Parse a full program: a list of function definitions.
    pub fn parse_program(&mut self) -> Result<Program, CompileError> {
        let mut items = Vec::new();
        while *self.peek() != TokenKind::Eof {
            items.push(self.parse_function()?);
        }
        Ok(Program { items })
    }

    // ── Functions ───────────────────────────────────────────

    /// Parse: `fn name(params) -> return_type { body }`
    fn parse_function(&mut self) -> Result<Function, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Fn)?;

        let (name, _) = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;

        // Parse parameter list
        let mut params = Vec::new();
        while *self.peek() != TokenKind::RParen {
            if !params.is_empty() {
                self.expect(&TokenKind::Comma)?;
            }
            let param_start = self.span();
            let (param_name, _) = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let (type_name, _) = self.expect_ident()?;
            params.push(Param {
                name: param_name,
                type_name,
                span: Span::new(param_start.start, self.span().start),
            });
        }
        self.expect(&TokenKind::RParen)?;

        // Optional return type: `-> type`
        let return_type = if *self.peek() == TokenKind::Arrow {
            self.advance();
            let (type_name, _) = self.expect_ident()?;
            Some(type_name)
        } else {
            None
        };

        let body = self.parse_block()?;
        let end = self.span();

        Ok(Function {
            name,
            params,
            return_type,
            body,
            span: Span::new(start.start, end.start),
        })
    }

    // ── Blocks ──────────────────────────────────────────────

    /// Parse: `{ stmt1; stmt2; ... }`
    fn parse_block(&mut self) -> Result<Block, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::LBrace)?;

        let mut stmts = Vec::new();
        while *self.peek() != TokenKind::RBrace {
            stmts.push(self.parse_stmt()?);
        }

        self.expect(&TokenKind::RBrace)?;
        let end = self.span();

        Ok(Block {
            stmts,
            span: Span::new(start.start, end.start),
        })
    }

    // ── Statements ──────────────────────────────────────────

    /// Parse a single statement.
    fn parse_stmt(&mut self) -> Result<Stmt, CompileError> {
        match self.peek() {
            TokenKind::Let => self.parse_let(),
            TokenKind::Return => self.parse_return(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            _ => self.parse_expr_or_assign_stmt(),
        }
    }

    /// Parse: `let [mut] name [: type] = expr;`
    fn parse_let(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Let)?;

        let mutable = if *self.peek() == TokenKind::Mut {
            self.advance();
            true
        } else {
            false
        };

        let (name, _) = self.expect_ident()?;

        // Optional type annotation: `: type`
        let type_name = if *self.peek() == TokenKind::Colon {
            self.advance();
            let (t, _) = self.expect_ident()?;
            Some(t)
        } else {
            None
        };

        self.expect(&TokenKind::Equals)?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon)?;

        Ok(Stmt::Let {
            name,
            mutable,
            type_name,
            value,
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse: `return [expr];`
    fn parse_return(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Return)?;

        let value = if *self.peek() != TokenKind::Semicolon {
            Some(self.parse_expr()?)
        } else {
            None
        };

        self.expect(&TokenKind::Semicolon)?;

        Ok(Stmt::Return {
            value,
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse: `if condition { ... } [else { ... }]`
    fn parse_if(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::If)?;

        let condition = self.parse_expr()?;
        let then_block = self.parse_block()?;

        let else_block = if *self.peek() == TokenKind::Else {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            then_block,
            else_block,
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse: `while condition { ... }`
    fn parse_while(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::While)?;

        let condition = self.parse_expr()?;
        let body = self.parse_block()?;

        Ok(Stmt::While {
            condition,
            body,
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse an expression-statement or assignment: `expr;` or `name = expr;`
    fn parse_expr_or_assign_stmt(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        let expr = self.parse_expr()?;

        // Check if this is an assignment: `name = expr;`
        if *self.peek() == TokenKind::Equals {
            self.advance();
            if let Expr::Ident { name, .. } = expr {
                let value = self.parse_expr()?;
                self.expect(&TokenKind::Semicolon)?;
                return Ok(Stmt::Assign {
                    name,
                    value,
                    span: Span::new(start.start, self.span().start),
                });
            } else {
                return Err(CompileError::new(
                    "invalid assignment target",
                    Span::new(start.start, self.span().start),
                ));
            }
        }

        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Expr {
            expr,
            span: Span::new(start.start, self.span().start),
        })
    }

    // ── Expressions (Pratt parsing) ─────────────────────────
    //
    // Pratt parsing uses "binding power" to handle precedence.
    // Higher binding power = binds tighter.
    //   ||           → 1
    //   &&           → 2
    //   == !=        → 3
    //   < > <= >=    → 4
    //   + -          → 5
    //   * /          → 6
    //   unary - !    → 7 (prefix)

    /// Parse an expression with the given minimum binding power.
    pub fn parse_expr(&mut self) -> Result<Expr, CompileError> {
        self.parse_expr_bp(0)
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Result<Expr, CompileError> {
        // Parse the left-hand side (prefix: literals, identifiers, unary ops, parens)
        let mut lhs = self.parse_prefix()?;

        // Now handle binary operators: keep going as long as the next operator
        // binds tighter than our minimum.
        loop {
            let (op, bp) = match self.peek() {
                TokenKind::PipePipe   => (BinOp::Or,  1),
                TokenKind::AmpAmp    => (BinOp::And, 2),
                TokenKind::EqualEqual => (BinOp::Eq,  3),
                TokenKind::BangEqual  => (BinOp::Neq, 3),
                TokenKind::Less       => (BinOp::Lt,  4),
                TokenKind::Greater    => (BinOp::Gt,  4),
                TokenKind::LessEqual  => (BinOp::Lte, 4),
                TokenKind::GreaterEqual => (BinOp::Gte, 4),
                TokenKind::Plus       => (BinOp::Add, 5),
                TokenKind::Minus      => (BinOp::Sub, 5),
                TokenKind::Star       => (BinOp::Mul, 6),
                TokenKind::Slash      => (BinOp::Div, 6),
                _ => break, // not a binary operator — stop
            };

            // Left-associative: only continue if binding power is strictly greater
            if bp <= min_bp {
                break;
            }

            self.advance(); // consume the operator

            let start_span = lhs.span();
            let rhs = self.parse_expr_bp(bp)?;
            let end_span = rhs.span();

            lhs = Expr::BinaryOp {
                op,
                left: Box::new(lhs),
                right: Box::new(rhs),
                span: Span::new(start_span.start, end_span.end),
            };
        }

        Ok(lhs)
    }

    /// Parse a prefix expression: literals, identifiers, calls, unary ops, parens.
    fn parse_prefix(&mut self) -> Result<Expr, CompileError> {
        match self.peek().clone() {
            TokenKind::Int(value) => {
                let span = self.span();
                self.advance();
                Ok(Expr::IntLit { value, span })
            }
            TokenKind::True => {
                let span = self.span();
                self.advance();
                Ok(Expr::BoolLit { value: true, span })
            }
            TokenKind::False => {
                let span = self.span();
                self.advance();
                Ok(Expr::BoolLit { value: false, span })
            }
            TokenKind::StringLit(value) => {
                let span = self.span();
                self.advance();
                Ok(Expr::StringLit { value, span })
            }
            TokenKind::Ident(name) => {
                let span = self.span();
                self.advance();

                // Check if it's a function call: `name(`
                if *self.peek() == TokenKind::LParen {
                    self.advance(); // consume '('
                    let mut args = Vec::new();
                    while *self.peek() != TokenKind::RParen {
                        if !args.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                        }
                        args.push(self.parse_expr()?);
                    }
                    let end = self.span();
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::Call {
                        name,
                        args,
                        span: Span::new(span.start, end.end),
                    })
                } else {
                    Ok(Expr::Ident { name, span })
                }
            }
            // Unary minus: `-expr`
            TokenKind::Minus => {
                let start = self.span();
                self.advance();
                let operand = self.parse_expr_bp(7)?; // 7 = highest precedence (prefix)
                let end = operand.span();
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                    span: Span::new(start.start, end.end),
                })
            }
            // Unary not: `!expr`
            TokenKind::Bang => {
                let start = self.span();
                self.advance();
                let operand = self.parse_expr_bp(7)?;
                let end = operand.span();
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                    span: Span::new(start.start, end.end),
                })
            }
            // Parenthesized expression: `(expr)`
            TokenKind::LParen => {
                self.advance(); // consume '('
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            _ => Err(CompileError::new(
                format!("expected expression, found {:?}", self.peek()),
                self.span(),
            )),
        }
    }
}

// Helper to get span from an Expr
impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit { span, .. }
            | Expr::BoolLit { span, .. }
            | Expr::StringLit { span, .. }
            | Expr::Ident { span, .. }
            | Expr::BinaryOp { span, .. }
            | Expr::UnaryOp { span, .. }
            | Expr::Call { span, .. } => *span,
        }
    }
}

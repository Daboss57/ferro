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

    /// Peek at token at offset from current position.
    fn peek_at(&self, offset: usize) -> &TokenKind {
        let idx = self.pos + offset;
        if idx < self.tokens.len() {
            &self.tokens[idx].kind
        } else {
            &TokenKind::Eof
        }
    }

    /// Check if current `{` starts a struct literal (next tokens: `Ident :` or `}`).
    fn is_struct_literal(&self) -> bool {
        // Current token is `{`, check tokens after it
        if let TokenKind::Ident(_) = self.peek_at(1) {
            *self.peek_at(2) == TokenKind::Colon
        } else {
            *self.peek_at(1) == TokenKind::RBrace // empty struct `Name {}`
        }
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

    /// Parse a type name: `i64`, `str`, `bool`, or `[elem; N]` for arrays.
    fn parse_type_name(&mut self) -> Result<(String, Span), CompileError> {
        if *self.peek() == TokenKind::LBracket {
            let start = self.span();
            self.advance(); // eat [
            let (elem, _) = self.expect_ident()?;
            self.expect(&TokenKind::Semicolon)?;
            let size = match self.peek().clone() {
                TokenKind::Int(n) => {
                    self.advance();
                    n
                }
                _ => {
                    return Err(CompileError::new(
                        format!("expected array size, found {:?}", self.peek()),
                        self.span(),
                    ));
                }
            };
            let end = self.span();
            self.expect(&TokenKind::RBracket)?;
            Ok((format!("[{}; {}]", elem, size), Span::new(start.start, end.end)))
        } else {
            self.expect_ident()
        }
    }

    // ── Program ─────────────────────────────────────────────

    /// Parse a full program: a list of top-level items (functions and enums).
    pub fn parse_program(&mut self) -> Result<Program, CompileError> {
        let mut imports = Vec::new();
        let mut functions = Vec::new();
        let mut enums = Vec::new();
        let mut structs = Vec::new();
        let mut comptimes = Vec::new();

        while *self.peek() != TokenKind::Eof {
            // Check for `priv` modifier
            let is_private = if *self.peek() == TokenKind::Priv {
                self.advance();
                true
            } else {
                false
            };

            match self.peek() {
                TokenKind::Import => {
                    if is_private {
                        return Err(CompileError::new(
                            "import declarations cannot be private",
                            self.span(),
                        ));
                    }
                    imports.push(self.parse_import()?);
                }
                TokenKind::Enum => {
                    let mut e = self.parse_enum_def()?;
                    e.is_private = is_private;
                    enums.push(e);
                }
                TokenKind::Struct => {
                    let mut s = self.parse_struct_def()?;
                    s.is_private = is_private;
                    structs.push(s);
                }
                TokenKind::Comptime => {
                    let mut c = self.parse_comptime_def()?;
                    c.is_private = is_private;
                    comptimes.push(c);
                }
                _ => {
                    let mut f = self.parse_function()?;
                    f.is_private = is_private;
                    functions.push(f);
                }
            }
        }
        Ok(Program { imports, functions, enums, structs, comptimes })
    }

    // ── Functions ───────────────────────────────────────────

    /// Parse: `import "path.ferro";`
    fn parse_import(&mut self) -> Result<ImportDecl, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Import)?;
        // Expect a string literal for the path
        let path = match self.peek().clone() {
            TokenKind::StringLit(s) => {
                self.advance();
                s
            }
            _ => {
                return Err(CompileError::new(
                    format!("expected string path after 'import', found {:?}", self.peek()),
                    self.span(),
                ));
            }
        };
        self.expect(&TokenKind::Semicolon)?;
        Ok(ImportDecl {
            path,
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse: `comptime let NAME = expr;`
    fn parse_comptime_def(&mut self) -> Result<ComptimeDef, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Comptime)?;
        self.expect(&TokenKind::Let)?;
        let (name, _) = self.expect_ident()?;
        self.expect(&TokenKind::Equals)?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(ComptimeDef {
            name,
            value,
            is_private: false, // set by parse_program
            span: Span::new(start.start, self.span().start),
        })
    }

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
            let (type_name, _) = self.parse_type_name()?;
            params.push(Param {
                name: param_name,
                type_name,
                span: Span::new(param_start.start, self.span().start),
            });
        }
        self.expect(&TokenKind::RParen)?;

        // Optional return type: `-> type` or `-> type ! str`
        let mut return_type = None;
        let mut can_fail = false;
        if *self.peek() == TokenKind::Arrow {
            self.advance();
            let (type_name, _) = self.parse_type_name()?;
            return_type = Some(type_name);
            // Check for `! str` (failable)
            if *self.peek() == TokenKind::Bang {
                self.advance();
                // Expect `str` after `!`
                let (err_type, _) = self.expect_ident()?;
                if err_type != "str" {
                    return Err(CompileError::new(
                        format!("expected 'str' after '!' in return type, got '{}'", err_type),
                        self.span(),
                    ));
                }
                can_fail = true;
            }
        }

        let body = self.parse_block()?;
        let end = self.span();

        Ok(Function {
            name,
            params,
            return_type,
            can_fail,
            is_private: false, // set by parse_program
            body,
            span: Span::new(start.start, end.start),
        })
    }

    /// Parse: `enum Name { Variant1, Variant2, ... }`
    fn parse_enum_def(&mut self) -> Result<EnumDef, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Enum)?;
        let (name, _) = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;

        let mut variants = Vec::new();
        while *self.peek() != TokenKind::RBrace {
            if !variants.is_empty() {
                self.expect(&TokenKind::Comma)?;
                // Allow trailing comma
                if *self.peek() == TokenKind::RBrace {
                    break;
                }
            }
            let (variant_name, _) = self.expect_ident()?;
            variants.push(variant_name);
        }
        let end = self.span();
        self.expect(&TokenKind::RBrace)?;

        Ok(EnumDef {
            name,
            variants,
            is_private: false, // set by parse_program
            span: Span::new(start.start, end.start),
        })
    }

    /// Parse: `struct Name { field: type, ... }`
    fn parse_struct_def(&mut self) -> Result<StructDef, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Struct)?;
        let (name, _) = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while *self.peek() != TokenKind::RBrace {
            if !fields.is_empty() {
                self.expect(&TokenKind::Comma)?;
                if *self.peek() == TokenKind::RBrace {
                    break; // trailing comma
                }
            }
            let field_start = self.span();
            let (field_name, _) = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let (type_name, _) = self.parse_type_name()?;
            fields.push(StructField {
                name: field_name,
                type_name,
                span: Span::new(field_start.start, self.span().start),
            });
        }
        let end = self.span();
        self.expect(&TokenKind::RBrace)?;

        Ok(StructDef {
            name,
            fields,
            is_private: false, // set by parse_program
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
            TokenKind::Defer => self.parse_defer(),
            TokenKind::Fail => self.parse_fail(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Break => self.parse_break(),
            TokenKind::Continue => self.parse_continue(),
            TokenKind::Match => self.parse_match(),
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
            let (t, _) = self.parse_type_name()?;
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

    /// Parse: `defer expr;`
    fn parse_defer(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Defer)?;
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Defer {
            expr,
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse: `fail expr;`
    fn parse_fail(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Fail)?;
        let message = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Fail {
            message,
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

    /// Parse: `for var in start..end { body }`
    fn parse_for(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::For)?;
        let (var, _) = self.expect_ident()?;
        self.expect(&TokenKind::In)?;
        let start_expr = self.parse_expr_bp(5)?; // parse before `..` (higher than comparison)
        self.expect(&TokenKind::DotDot)?;
        let end_expr = self.parse_expr_bp(5)?;
        let body = self.parse_block()?;

        Ok(Stmt::For {
            var,
            start: start_expr,
            end: end_expr,
            body,
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse: `break;`
    fn parse_break(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Break)?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Break {
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse: `continue;`
    fn parse_continue(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Continue)?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Continue {
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse: `match expr { pattern => { body } ... }`
    fn parse_match(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        self.expect(&TokenKind::Match)?;
        let subject = self.parse_expr()?;
        self.expect(&TokenKind::LBrace)?;

        let mut arms = Vec::new();
        while *self.peek() != TokenKind::RBrace {
            let arm_start = self.span();
            let pattern = self.parse_pattern()?;
            self.expect(&TokenKind::FatArrow)?;
            let body = self.parse_block()?;
            arms.push(MatchArm {
                pattern,
                body,
                span: Span::new(arm_start.start, self.span().start),
            });
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(Stmt::Match {
            subject,
            arms,
            span: Span::new(start.start, self.span().start),
        })
    }

    /// Parse a match pattern: integer, boolean, `_` wildcard, or `Enum::Variant`.
    fn parse_pattern(&mut self) -> Result<Pattern, CompileError> {
        match self.peek().clone() {
            TokenKind::Int(v) => {
                let span = self.span();
                self.advance();
                Ok(Pattern::IntLit(v, span))
            }
            TokenKind::True => {
                let span = self.span();
                self.advance();
                Ok(Pattern::BoolLit(true, span))
            }
            TokenKind::False => {
                let span = self.span();
                self.advance();
                Ok(Pattern::BoolLit(false, span))
            }
            TokenKind::Ident(ref name) if name == "_" => {
                let span = self.span();
                self.advance();
                Ok(Pattern::Wildcard(span))
            }
            TokenKind::Ident(name) => {
                let start = self.span();
                self.advance();
                // Check for `::` — enum variant pattern
                if *self.peek() == TokenKind::ColonColon {
                    self.advance(); // consume ::
                    let (variant, end_span) = self.expect_ident()?;
                    Ok(Pattern::EnumVariant(name, variant, Span::new(start.start, end_span.end)))
                } else {
                    Err(CompileError::new(
                        format!("expected pattern (integer, bool, _, or Enum::Variant), found identifier '{}'", name),
                        start,
                    ))
                }
            }
            _ => Err(CompileError::new(
                format!("expected pattern (integer, bool, _, or Enum::Variant), found {:?}", self.peek()),
                self.span(),
            )),
        }
    }

    /// Parse an expression-statement, assignment, or tail expression.
    /// `expr;` = statement, `name = expr;` = assignment, `expr` (no `;`, before `}`) = tail/implicit return
    fn parse_expr_or_assign_stmt(&mut self) -> Result<Stmt, CompileError> {
        let start = self.span();
        let expr = self.parse_expr()?;

        // Check if this is an assignment: `name = expr;` or `name[i] = expr;`
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
            } else if let Expr::Index { object, index, .. } = expr {
                if let Expr::Ident { name, .. } = *object {
                    let value = self.parse_expr()?;
                    self.expect(&TokenKind::Semicolon)?;
                    return Ok(Stmt::IndexAssign {
                        object: name,
                        index: *index,
                        value,
                        span: Span::new(start.start, self.span().start),
                    });
                } else {
                    return Err(CompileError::new(
                        "invalid index assignment target",
                        Span::new(start.start, self.span().start),
                    ));
                }
            } else if let Expr::FieldAccess { object, field, .. } = expr {
                if let Expr::Ident { name, .. } = *object {
                    let value = self.parse_expr()?;
                    self.expect(&TokenKind::Semicolon)?;
                    return Ok(Stmt::FieldAssign {
                        object: name,
                        field,
                        value,
                        span: Span::new(start.start, self.span().start),
                    });
                } else {
                    return Err(CompileError::new(
                        "invalid field assignment target",
                        Span::new(start.start, self.span().start),
                    ));
                }
            } else {
                return Err(CompileError::new(
                    "invalid assignment target",
                    Span::new(start.start, self.span().start),
                ));
            }
        }

        // If next token is `}` (end of block) and no semicolon → tail expression (implicit return)
        if *self.peek() == TokenKind::RBrace {
            return Ok(Stmt::TailExpr {
                expr,
                span: Span::new(start.start, self.span().start),
            });
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
    //   |>           → 0 (pipe, lowest — handled specially)
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
            // Postfix: indexing `expr[index]`
            if *self.peek() == TokenKind::LBracket {
                let start_span = lhs.span();
                self.advance(); // eat [
                let index = self.parse_expr()?;
                let end = self.span();
                self.expect(&TokenKind::RBracket)?;
                lhs = Expr::Index {
                    object: Box::new(lhs),
                    index: Box::new(index),
                    span: Span::new(start_span.start, end.end),
                };
                continue;
            }

            // Postfix: field access `expr.field`
            if *self.peek() == TokenKind::Dot {
                let start_span = lhs.span();
                self.advance(); // eat .
                let (field, end_span) = self.expect_ident()?;
                lhs = Expr::FieldAccess {
                    object: Box::new(lhs),
                    field,
                    span: Span::new(start_span.start, end_span.end),
                };
                continue;
            }

            // Postfix: type cast `expr as type`
            if *self.peek() == TokenKind::As {
                let start_span = lhs.span();
                self.advance(); // eat `as`
                let (target, end_span) = self.expect_ident()?;
                lhs = Expr::Cast {
                    expr: Box::new(lhs),
                    target,
                    span: Span::new(start_span.start, end_span.end),
                };
                continue;
            }

            // Special case: pipe operator |>
            // Desugars: `a |> f` → `f(a)`, `a |> f(b, c)` → `f(a, b, c)`
            if *self.peek() == TokenKind::PipeArrow {
                let pipe_bp: u8 = 1; // lowest precedence
                if pipe_bp <= min_bp {
                    break;
                }
                self.advance(); // consume |>

                let start_span = lhs.span();
                // The right side must be a function call or identifier
                let rhs = self.parse_prefix()?;

                lhs = match rhs {
                    // `a |> f(b, c)` → `f(a, b, c)` — prepend lhs as first arg
                    Expr::Call { name, mut args, span } => {
                        args.insert(0, lhs);
                        Expr::Call {
                            name,
                            args,
                            span: Span::new(start_span.start, span.end),
                        }
                    }
                    // `a |> f` → `f(a)` — bare identifier becomes a call
                    Expr::Ident { name, span } => {
                        Expr::Call {
                            name,
                            args: vec![lhs],
                            span: Span::new(start_span.start, span.end),
                        }
                    }
                    _ => {
                        return Err(CompileError::new(
                            "right side of |> must be a function name or call",
                            self.span(),
                        ));
                    }
                };
                continue;
            }

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
                TokenKind::Percent    => (BinOp::Mod, 6),
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

                // Check for enum variant: `Name::Variant`
                if *self.peek() == TokenKind::ColonColon {
                    self.advance(); // consume ::
                    let (variant, end_span) = self.expect_ident()?;
                    return Ok(Expr::EnumVariant {
                        enum_name: name,
                        variant,
                        span: Span::new(span.start, end_span.end),
                    });
                }

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
                }
                // Check for struct literal: `Name { field: expr, ... }`
                // Disambiguate from block by checking if Ident followed by Colon after {
                else if *self.peek() == TokenKind::LBrace && self.is_struct_literal() {
                    self.advance(); // consume {
                    let mut fields = Vec::new();
                    while *self.peek() != TokenKind::RBrace {
                        if !fields.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                            if *self.peek() == TokenKind::RBrace {
                                break; // trailing comma
                            }
                        }
                        let (field_name, _) = self.expect_ident()?;
                        self.expect(&TokenKind::Colon)?;
                        let value = self.parse_expr()?;
                        fields.push((field_name, value));
                    }
                    let end = self.span();
                    self.expect(&TokenKind::RBrace)?;
                    Ok(Expr::StructLit {
                        name,
                        fields,
                        span: Span::new(span.start, end.end),
                    })
                }
                else {
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
            // Try expression: `try expr`
            TokenKind::Try => {
                let start = self.span();
                self.advance();
                let expr = self.parse_expr_bp(7)?; // high precedence prefix
                let end = expr.span();
                Ok(Expr::Try {
                    expr: Box::new(expr),
                    span: Span::new(start.start, end.end),
                })
            }
            // Array literal: `[expr, expr, ...]`
            TokenKind::LBracket => {
                let start = self.span();
                self.advance(); // consume '['
                let mut elements = Vec::new();
                while *self.peek() != TokenKind::RBracket {
                    if !elements.is_empty() {
                        self.expect(&TokenKind::Comma)?;
                    }
                    elements.push(self.parse_expr()?);
                }
                let end = self.span();
                self.expect(&TokenKind::RBracket)?;
                Ok(Expr::ArrayLit {
                    elements,
                    span: Span::new(start.start, end.end),
                })
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
            | Expr::Call { span, .. }
            | Expr::ArrayLit { span, .. }
            | Expr::Index { span, .. }
            | Expr::EnumVariant { span, .. }
            | Expr::StructLit { span, .. }
            | Expr::FieldAccess { span, .. }
            | Expr::Try { span, .. }
            | Expr::Cast { span, .. } => *span,
        }
    }
}

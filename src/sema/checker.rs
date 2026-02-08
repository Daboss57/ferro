// Semantic analysis — name resolution and type checking.
//
// Walks the AST and checks:
// 1. Every variable is declared before use
// 2. Every function called exists and gets the right number/types of args
// 3. Types match in expressions (can't add bool + i64)
// 4. Conditions in if/while are booleans
// 5. Return types match the function signature

use std::collections::HashMap;

use crate::ast::*;
use crate::error::{CompileError, Span};
use crate::sema::types::Type;

/// Information about a declared variable.
#[derive(Debug, Clone)]
struct VarInfo {
    ty: Type,
    #[allow(dead_code)]
    mutable: bool,
}

/// Information about a declared function.
#[derive(Debug, Clone)]
struct FuncInfo {
    param_types: Vec<Type>,
    return_type: Type,
}

/// A scope is a mapping from names to variable info.
/// We use a stack of scopes to handle nested blocks.
pub struct Checker {
    scopes: Vec<HashMap<String, VarInfo>>,
    functions: HashMap<String, FuncInfo>,
    current_return_type: Type,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            current_return_type: Type::Void,
        }
    }

    // ── Scope management ────────────────────────────────

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Define a variable in the current (innermost) scope.
    fn define_var(&mut self, name: &str, ty: Type, mutable: bool) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), VarInfo { ty, mutable });
        }
    }

    /// Look up a variable, searching from innermost scope outward.
    fn lookup_var(&self, name: &str) -> Option<&VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    // ── Resolve a type name ─────────────────────────────

    fn resolve_type(&self, name: &str, span: Span) -> Result<Type, CompileError> {
        Type::from_name(name).ok_or_else(|| {
            CompileError::new(format!("unknown type '{}'", name), span)
        })
    }

    // ── Program-level checking ──────────────────────────

    /// Check an entire program.
    pub fn check_program(&mut self, program: &Program) -> Result<(), CompileError> {
        // First pass: register all function signatures (so functions can call each other)
        for func in &program.items {
            let mut param_types = Vec::new();
            for param in &func.params {
                let ty = self.resolve_type(&param.type_name, param.span)?;
                param_types.push(ty);
            }
            let return_type = match &func.return_type {
                Some(name) => self.resolve_type(name, func.span)?,
                None => Type::Void,
            };
            self.functions.insert(
                func.name.clone(),
                FuncInfo { param_types, return_type },
            );
        }

        // Register built-in function: print(value) -> void
        // print accepts any type, we'll handle it specially in check_call
        self.functions.insert(
            "print".to_string(),
            FuncInfo {
                param_types: vec![Type::I64], // placeholder, check_call handles it
                return_type: Type::Void,
            },
        );

        // Second pass: check each function body
        for func in &program.items {
            self.check_function(func)?;
        }

        Ok(())
    }

    fn check_function(&mut self, func: &Function) -> Result<(), CompileError> {
        // Set the expected return type for this function
        self.current_return_type = match &func.return_type {
            Some(name) => self.resolve_type(name, func.span)?,
            None => Type::Void,
        };

        self.push_scope();

        // Add parameters to scope
        for param in &func.params {
            let ty = self.resolve_type(&param.type_name, param.span)?;
            self.define_var(&param.name, ty, false);
        }

        self.check_block(&func.body)?;
        self.pop_scope();
        Ok(())
    }

    fn check_block(&mut self, block: &Block) -> Result<(), CompileError> {
        self.push_scope();
        for stmt in &block.stmts {
            self.check_stmt(stmt)?;
        }
        self.pop_scope();
        Ok(())
    }

    // ── Statement checking ──────────────────────────────

    fn check_stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        match stmt {
            Stmt::Let { name, mutable, type_name, value, span } => {
                let value_ty = self.check_expr(value)?;

                if let Some(type_str) = type_name {
                    let declared_ty = self.resolve_type(type_str, *span)?;
                    if declared_ty != value_ty {
                        return Err(CompileError::new(
                            format!(
                                "type mismatch: declared type '{}' but value has type '{}'",
                                declared_ty, value_ty
                            ),
                            *span,
                        ));
                    }
                    self.define_var(name, declared_ty, *mutable);
                } else {
                    // Type inference: use the value's type
                    self.define_var(name, value_ty, *mutable);
                }
                Ok(())
            }

            Stmt::Assign { name, value, span } => {
                // Check variable exists
                let var_ty = match self.lookup_var(name) {
                    Some(info) => info.ty.clone(),
                    None => {
                        return Err(CompileError::new(
                            format!("undeclared variable '{}'", name),
                            *span,
                        ));
                    }
                };

                let value_ty = self.check_expr(value)?;
                if var_ty != value_ty {
                    return Err(CompileError::new(
                        format!(
                            "type mismatch in assignment: '{}' has type '{}' but got '{}'",
                            name, var_ty, value_ty
                        ),
                        *span,
                    ));
                }
                Ok(())
            }

            Stmt::Return { value, span } => {
                let return_ty = match value {
                    Some(expr) => self.check_expr(expr)?,
                    None => Type::Void,
                };
                if return_ty != self.current_return_type {
                    return Err(CompileError::new(
                        format!(
                            "return type mismatch: function returns '{}' but got '{}'",
                            self.current_return_type, return_ty
                        ),
                        *span,
                    ));
                }
                Ok(())
            }

            Stmt::Expr { expr, .. } => {
                self.check_expr(expr)?;
                Ok(())
            }

            Stmt::TailExpr { expr, span } => {
                // Tail expression acts as implicit return
                let return_ty = self.check_expr(expr)?;
                if return_ty != self.current_return_type {
                    return Err(CompileError::new(
                        format!(
                            "implicit return type mismatch: function returns '{}' but tail expression has type '{}'",
                            self.current_return_type, return_ty
                        ),
                        *span,
                    ));
                }
                Ok(())
            }

            Stmt::If { condition, then_block, else_block, span } => {
                let cond_ty = self.check_expr(condition)?;
                if cond_ty != Type::Bool {
                    return Err(CompileError::new(
                        format!("if condition must be 'bool', got '{}'", cond_ty),
                        *span,
                    ));
                }
                self.check_block(then_block)?;
                if let Some(else_b) = else_block {
                    self.check_block(else_b)?;
                }
                Ok(())
            }

            Stmt::While { condition, body, span } => {
                let cond_ty = self.check_expr(condition)?;
                if cond_ty != Type::Bool {
                    return Err(CompileError::new(
                        format!("while condition must be 'bool', got '{}'", cond_ty),
                        *span,
                    ));
                }
                self.check_block(body)?;
                Ok(())
            }
        }
    }

    // ── Expression checking (returns the type of the expression) ──

    fn check_expr(&mut self, expr: &Expr) -> Result<Type, CompileError> {
        match expr {
            Expr::IntLit { .. } => Ok(Type::I64),
            Expr::BoolLit { .. } => Ok(Type::Bool),
            Expr::StringLit { .. } => Ok(Type::Str),

            Expr::Ident { name, span } => {
                match self.lookup_var(name) {
                    Some(info) => Ok(info.ty.clone()),
                    None => Err(CompileError::new(
                        format!("undeclared variable '{}'", name),
                        *span,
                    )),
                }
            }

            Expr::UnaryOp { op, operand, span } => {
                let operand_ty = self.check_expr(operand)?;
                match op {
                    UnaryOp::Neg => {
                        if operand_ty != Type::I64 {
                            return Err(CompileError::new(
                                format!("cannot negate type '{}'", operand_ty),
                                *span,
                            ));
                        }
                        Ok(Type::I64)
                    }
                    UnaryOp::Not => {
                        if operand_ty != Type::Bool {
                            return Err(CompileError::new(
                                format!("cannot apply '!' to type '{}'", operand_ty),
                                *span,
                            ));
                        }
                        Ok(Type::Bool)
                    }
                }
            }

            Expr::BinaryOp { op, left, right, span } => {
                let left_ty = self.check_expr(left)?;
                let right_ty = self.check_expr(right)?;

                match op {
                    // Arithmetic: i64 op i64 -> i64
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                        if left_ty != Type::I64 || right_ty != Type::I64 {
                            return Err(CompileError::new(
                                format!(
                                    "cannot apply '{:?}' to '{}' and '{}'",
                                    op, left_ty, right_ty
                                ),
                                *span,
                            ));
                        }
                        Ok(Type::I64)
                    }
                    // Comparison: i64 op i64 -> bool
                    BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => {
                        if left_ty != Type::I64 || right_ty != Type::I64 {
                            return Err(CompileError::new(
                                format!(
                                    "cannot compare '{}' and '{}'",
                                    left_ty, right_ty
                                ),
                                *span,
                            ));
                        }
                        Ok(Type::Bool)
                    }
                    // Equality: same type -> bool
                    BinOp::Eq | BinOp::Neq => {
                        if left_ty != right_ty {
                            return Err(CompileError::new(
                                format!(
                                    "cannot compare '{}' and '{}' for equality",
                                    left_ty, right_ty
                                ),
                                *span,
                            ));
                        }
                        Ok(Type::Bool)
                    }
                    // Logical: bool op bool -> bool
                    BinOp::And | BinOp::Or => {
                        if left_ty != Type::Bool || right_ty != Type::Bool {
                            return Err(CompileError::new(
                                format!(
                                    "cannot apply '{:?}' to '{}' and '{}'",
                                    op, left_ty, right_ty
                                ),
                                *span,
                            ));
                        }
                        Ok(Type::Bool)
                    }
                }
            }

            Expr::Call { name, args, span } => {
                // Special handling for print — accepts any single argument
                if name == "print" {
                    if args.len() != 1 {
                        return Err(CompileError::new(
                            format!("print() takes 1 argument, got {}", args.len()),
                            *span,
                        ));
                    }
                    self.check_expr(&args[0])?;
                    return Ok(Type::Void);
                }

                let func_info = match self.functions.get(name) {
                    Some(info) => info.clone(),
                    None => {
                        return Err(CompileError::new(
                            format!("undeclared function '{}'", name),
                            *span,
                        ));
                    }
                };

                if args.len() != func_info.param_types.len() {
                    return Err(CompileError::new(
                        format!(
                            "function '{}' takes {} argument(s), got {}",
                            name,
                            func_info.param_types.len(),
                            args.len()
                        ),
                        *span,
                    ));
                }

                for (i, (arg, expected_ty)) in
                    args.iter().zip(func_info.param_types.iter()).enumerate()
                {
                    let arg_ty = self.check_expr(arg)?;
                    if arg_ty != *expected_ty {
                        return Err(CompileError::new(
                            format!(
                                "argument {} of '{}': expected '{}', got '{}'",
                                i + 1, name, expected_ty, arg_ty
                            ),
                            *span,
                        ));
                    }
                }

                Ok(func_info.return_type.clone())
            }
        }
    }
}

// Semantic analysis — name resolution and type checking.
//
// Walks the AST and checks:
// 1. Every variable is declared before use
// 2. Every function called exists and gets the right number/types of args
// 3. Types match in expressions (can't add bool + i64)
// 4. Conditions in if/while are booleans
// 5. Return types match the function signature

use std::collections::{HashMap, HashSet};

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
    can_fail: bool,
}

/// Information about a declared enum.
#[derive(Debug, Clone)]
struct EnumInfo {
    variants: Vec<String>,
}

/// Information about a declared struct.
#[derive(Debug, Clone)]
struct StructInfo {
    fields: Vec<(String, Type)>, // (name, type) pairs in order
}

/// A scope is a mapping from names to variable info.
/// We use a stack of scopes to handle nested blocks.
pub struct Checker {
    scopes: Vec<HashMap<String, VarInfo>>,
    functions: HashMap<String, FuncInfo>,
    enums: HashMap<String, EnumInfo>,
    structs: HashMap<String, StructInfo>,
    comptimes: HashMap<String, (Type, i64)>, // name → (type, evaluated value)
    private_items: HashSet<String>,           // names that are priv in imported modules
    imported_items: HashSet<String>,          // all names from imported modules
    check_privacy: bool,                     // true when checking main-module code
    current_return_type: Type,
    current_can_fail: bool,
    loop_depth: usize,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            enums: HashMap::new(),
            structs: HashMap::new(),
            comptimes: HashMap::new(),
            private_items: HashSet::new(),
            imported_items: HashSet::new(),
            check_privacy: false,
            current_return_type: Type::Void,
            current_can_fail: false,
            loop_depth: 0,
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

    // ── Compile-time evaluation ────────────────────────

    fn eval_comptime(&self, expr: &Expr, span: Span) -> Result<i64, CompileError> {
        match expr {
            Expr::IntLit { value, .. } => Ok(*value),
            Expr::BoolLit { value, .. } => Ok(if *value { 1 } else { 0 }),
            Expr::Ident { name, .. } => {
                if let Some((_, val)) = self.comptimes.get(name) {
                    Ok(*val)
                } else {
                    Err(CompileError::new(
                        format!("'{}' is not a comptime constant", name),
                        span,
                    ))
                }
            }
            Expr::UnaryOp { op, operand, .. } => {
                let v = self.eval_comptime(operand, span)?;
                match op {
                    UnaryOp::Neg => Ok(-v),
                    UnaryOp::Not => Ok(if v == 0 { 1 } else { 0 }),
                }
            }
            Expr::BinaryOp { op, left, right, .. } => {
                let l = self.eval_comptime(left, span)?;
                let r = self.eval_comptime(right, span)?;
                match op {
                    BinOp::Add => Ok(l + r),
                    BinOp::Sub => Ok(l - r),
                    BinOp::Mul => Ok(l * r),
                    BinOp::Div => {
                        if r == 0 {
                            Err(CompileError::new("division by zero in comptime", span))
                        } else {
                            Ok(l / r)
                        }
                    }
                    BinOp::Mod => {
                        if r == 0 {
                            Err(CompileError::new("modulo by zero in comptime", span))
                        } else {
                            Ok(l % r)
                        }
                    }
                    BinOp::Eq  => Ok(if l == r { 1 } else { 0 }),
                    BinOp::Neq => Ok(if l != r { 1 } else { 0 }),
                    BinOp::Lt  => Ok(if l < r  { 1 } else { 0 }),
                    BinOp::Gt  => Ok(if l > r  { 1 } else { 0 }),
                    BinOp::Lte => Ok(if l <= r { 1 } else { 0 }),
                    BinOp::Gte => Ok(if l >= r { 1 } else { 0 }),
                    BinOp::And => Ok(if l != 0 && r != 0 { 1 } else { 0 }),
                    BinOp::Or  => Ok(if l != 0 || r != 0 { 1 } else { 0 }),
                }
            }
            _ => Err(CompileError::new(
                "comptime expressions must be constant (literals, arithmetic, other comptime values)",
                span,
            )),
        }
    }

    // ── Resolve a type name ─────────────────────────────

    fn resolve_type(&self, name: &str, span: Span) -> Result<Type, CompileError> {
        // Handle array types: "[elem; N]"
        if name.starts_with('[') && name.ends_with(']') {
            let inner = &name[1..name.len() - 1];
            let parts: Vec<&str> = inner.split(';').collect();
            if parts.len() != 2 {
                return Err(CompileError::new(format!("invalid array type '{}'", name), span));
            }
            let elem_name = parts[0].trim();
            let size: usize = parts[1].trim().parse().map_err(|_| {
                CompileError::new(format!("invalid array size in '{}'", name), span)
            })?;
            let elem_type = self.resolve_type(elem_name, span)?;
            return Ok(Type::Array(Box::new(elem_type), size));
        }
        Type::from_name(name).ok_or_else(|| {
            // Check if it's an enum type name
            if self.enums.contains_key(name) {
                return CompileError::new("internal: should not reach here", span);
            }
            CompileError::new(format!("unknown type '{}'", name), span)
        })
    }

    fn resolve_type_or_enum(&self, name: &str, span: Span) -> Result<Type, CompileError> {
        if name.starts_with('[') && name.ends_with(']') {
            return self.resolve_type(name, span);
        }
        if self.enums.contains_key(name) {
            return Ok(Type::Enum(name.to_string()));
        }
        if self.structs.contains_key(name) {
            return Ok(Type::Struct(name.to_string()));
        }
        self.resolve_type(name, span)
    }

    // ── Program-level checking ──────────────────────────

    /// Mark items as private (from imported modules). Must be called before check_program.
    pub fn set_imported_privates(&mut self, privates: &[(String, bool)]) {
        for (name, is_private) in privates {
            self.imported_items.insert(name.clone());
            if *is_private {
                self.private_items.insert(name.clone());
            }
        }
    }

    /// Check an entire program.
    pub fn check_program(&mut self, program: &Program) -> Result<(), CompileError> {
        // Register enum definitions
        for enum_def in &program.enums {
            self.enums.insert(
                enum_def.name.clone(),
                EnumInfo { variants: enum_def.variants.clone() },
            );
        }

        // Register struct definitions
        for struct_def in &program.structs {
            let mut fields = Vec::new();
            for field in &struct_def.fields {
                let ty = self.resolve_type_or_enum(&field.type_name, field.span)?;
                fields.push((field.name.clone(), ty));
            }
            self.structs.insert(
                struct_def.name.clone(),
                StructInfo { fields },
            );
        }

        // Evaluate comptime constants
        for ct in &program.comptimes {
            let val = self.eval_comptime(&ct.value, ct.span)?;
            let ty = match &ct.value {
                Expr::BoolLit { .. } => Type::Bool,
                _ => Type::I64,
            };
            self.comptimes.insert(ct.name.clone(), (ty.clone(), val));
            // Also make comptime constants available as variables in all scopes
            self.define_var(&ct.name, ty, false);
        }

        // First pass: register all function signatures
        for func in &program.functions {
            let mut param_types = Vec::new();
            for param in &func.params {
                let ty = self.resolve_type_or_enum(&param.type_name, param.span)?;
                param_types.push(ty);
            }
            let return_type = match &func.return_type {
                Some(name) => self.resolve_type_or_enum(name, func.span)?,
                None => Type::Void,
            };
            self.functions.insert(
                func.name.clone(),
                FuncInfo { param_types, return_type, can_fail: func.can_fail },
            );
        }

        // Register built-in function: print(value) -> void
        // print accepts any type, we'll handle it specially in check_call
        self.functions.insert(
            "print".to_string(),
            FuncInfo {
                param_types: vec![Type::I64], // placeholder, check_call handles it
                return_type: Type::Void,
                can_fail: false,
            },
        );

        // Register built-in function: len(s: str) -> i64
        self.functions.insert(
            "len".to_string(),
            FuncInfo {
                param_types: vec![Type::Str],
                return_type: Type::I64,
                can_fail: false,
            },
        );

        // ── Math builtins ────────────────────────────────
        self.functions.insert("abs".to_string(), FuncInfo {
            param_types: vec![Type::I64], return_type: Type::I64, can_fail: false,
        });
        self.functions.insert("min".to_string(), FuncInfo {
            param_types: vec![Type::I64, Type::I64], return_type: Type::I64, can_fail: false,
        });
        self.functions.insert("max".to_string(), FuncInfo {
            param_types: vec![Type::I64, Type::I64], return_type: Type::I64, can_fail: false,
        });
        self.functions.insert("pow".to_string(), FuncInfo {
            param_types: vec![Type::I64, Type::I64], return_type: Type::I64, can_fail: false,
        });
        self.functions.insert("rand".to_string(), FuncInfo {
            param_types: vec![], return_type: Type::I64, can_fail: false,
        });

        // ── String builtins ──────────────────────────────
        self.functions.insert("to_str".to_string(), FuncInfo {
            param_types: vec![Type::I64], return_type: Type::Str, can_fail: false,
        });
        self.functions.insert("parse_int".to_string(), FuncInfo {
            param_types: vec![Type::Str], return_type: Type::I64, can_fail: true,
        });
        self.functions.insert("char_at".to_string(), FuncInfo {
            param_types: vec![Type::Str, Type::I64], return_type: Type::I64, can_fail: false,
        });
        self.functions.insert("contains".to_string(), FuncInfo {
            param_types: vec![Type::Str, Type::Str], return_type: Type::Bool, can_fail: false,
        });
        self.functions.insert("starts_with".to_string(), FuncInfo {
            param_types: vec![Type::Str, Type::Str], return_type: Type::Bool, can_fail: false,
        });
        self.functions.insert("substr".to_string(), FuncInfo {
            param_types: vec![Type::Str, Type::I64, Type::I64], return_type: Type::Str, can_fail: false,
        });
        self.functions.insert("trim".to_string(), FuncInfo {
            param_types: vec![Type::Str], return_type: Type::Str, can_fail: false,
        });

        // ── I/O builtins ─────────────────────────────────
        self.functions.insert("read_line".to_string(), FuncInfo {
            param_types: vec![], return_type: Type::Str, can_fail: false,
        });
        self.functions.insert("read_file".to_string(), FuncInfo {
            param_types: vec![Type::Str], return_type: Type::Str, can_fail: true,
        });
        self.functions.insert("write_file".to_string(), FuncInfo {
            param_types: vec![Type::Str, Type::Str], return_type: Type::Void, can_fail: true,
        });
        self.functions.insert("file_exists".to_string(), FuncInfo {
            param_types: vec![Type::Str], return_type: Type::Bool, can_fail: false,
        });
        self.functions.insert("eprint".to_string(), FuncInfo {
            param_types: vec![Type::Str], return_type: Type::Void, can_fail: false,
        });

        // ── System builtins ──────────────────────────────
        self.functions.insert("exit".to_string(), FuncInfo {
            param_types: vec![Type::I64], return_type: Type::Void, can_fail: false,
        });
        self.functions.insert("time".to_string(), FuncInfo {
            param_types: vec![], return_type: Type::I64, can_fail: false,
        });
        self.functions.insert("sleep".to_string(), FuncInfo {
            param_types: vec![Type::I64], return_type: Type::Void, can_fail: false,
        });

        // ── Memory builtins ─────────────────────────────
        self.functions.insert("alloc".to_string(), FuncInfo {
            param_types: vec![Type::I64], return_type: Type::I64, can_fail: false,
        });
        self.functions.insert("free".to_string(), FuncInfo {
            param_types: vec![Type::I64], return_type: Type::Void, can_fail: false,
        });

        // Second pass: check each function body
        for func in &program.functions {
            // Only enforce privacy when checking main-module functions
            self.check_privacy = !self.imported_items.contains(&func.name);
            self.check_function(func)?;
        }
        self.check_privacy = false;

        Ok(())
    }

    fn check_function(&mut self, func: &Function) -> Result<(), CompileError> {
        self.current_return_type = match &func.return_type {
            Some(name) => self.resolve_type_or_enum(name, func.span)?,
            None => Type::Void,
        };
        self.current_can_fail = func.can_fail;

        self.push_scope();

        for param in &func.params {
            let ty = self.resolve_type_or_enum(&param.type_name, param.span)?;
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
                    let declared_ty = self.resolve_type_or_enum(type_str, *span)?;
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

            Stmt::IndexAssign { object, index, value, span } => {
                // Look up the array variable
                let arr_ty = match self.lookup_var(object) {
                    Some(info) => info.ty.clone(),
                    None => {
                        return Err(CompileError::new(
                            format!("undeclared variable '{}'", object),
                            *span,
                        ));
                    }
                };
                // Must be an array
                let elem_ty = match &arr_ty {
                    Type::Array(elem, _) => elem.as_ref().clone(),
                    _ => {
                        return Err(CompileError::new(
                            format!("cannot index into non-array type '{}'", arr_ty),
                            *span,
                        ));
                    }
                };
                // Index must be i64
                let idx_ty = self.check_expr(index)?;
                if idx_ty != Type::I64 {
                    return Err(CompileError::new(
                        format!("array index must be 'i64', got '{}'", idx_ty),
                        *span,
                    ));
                }
                // Value must match element type
                let val_ty = self.check_expr(value)?;
                if val_ty != elem_ty {
                    return Err(CompileError::new(
                        format!(
                            "type mismatch in index assignment: expected '{}', got '{}'",
                            elem_ty, val_ty
                        ),
                        *span,
                    ));
                }
                Ok(())
            }

            Stmt::FieldAssign { object, field, value, span } => {
                let obj_ty = match self.lookup_var(object) {
                    Some(info) => info.ty.clone(),
                    None => {
                        return Err(CompileError::new(
                            format!("undeclared variable '{}'", object),
                            *span,
                        ));
                    }
                };
                match &obj_ty {
                    Type::Struct(sname) => {
                        let info = self.structs.get(sname).unwrap().clone();
                        match info.fields.iter().find(|(n, _)| n == field) {
                            Some((_, expected_ty)) => {
                                let val_ty = self.check_expr(value)?;
                                if val_ty != *expected_ty {
                                    return Err(CompileError::new(
                                        format!(
                                            "field '{}' expected type '{}', got '{}'",
                                            field, expected_ty, val_ty
                                        ),
                                        *span,
                                    ));
                                }
                                Ok(())
                            }
                            None => Err(CompileError::new(
                                format!("struct '{}' has no field '{}'", sname, field),
                                *span,
                            )),
                        }
                    }
                    _ => Err(CompileError::new(
                        format!("field assignment on non-struct type '{}'", obj_ty),
                        *span,
                    )),
                }
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

            Stmt::Defer { expr, .. } => {
                self.check_expr(expr)?;
                Ok(())
            }

            Stmt::Fail { message, span } => {
                if !self.current_can_fail {
                    return Err(CompileError::new(
                        "'fail' can only be used in failable functions (-> T ! str)",
                        *span,
                    ));
                }
                let msg_ty = self.check_expr(message)?;
                if msg_ty != Type::Str {
                    return Err(CompileError::new(
                        format!("'fail' expects a string message, got '{}'", msg_ty),
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
                self.loop_depth += 1;
                self.check_block(body)?;
                self.loop_depth -= 1;
                Ok(())
            }

            Stmt::For { var, start, end, body, span } => {
                let start_ty = self.check_expr(start)?;
                let end_ty = self.check_expr(end)?;
                if start_ty != Type::I64 {
                    return Err(CompileError::new(
                        format!("for range start must be 'i64', got '{}'", start_ty),
                        *span,
                    ));
                }
                if end_ty != Type::I64 {
                    return Err(CompileError::new(
                        format!("for range end must be 'i64', got '{}'", end_ty),
                        *span,
                    ));
                }
                // The loop variable is scoped inside the for body
                self.push_scope();
                self.define_var(var, Type::I64, false);
                self.loop_depth += 1;
                for stmt in &body.stmts {
                    self.check_stmt(stmt)?;
                }
                self.loop_depth -= 1;
                self.pop_scope();
                Ok(())
            }

            Stmt::Break { span } => {
                if self.loop_depth == 0 {
                    return Err(CompileError::new(
                        "break outside of loop",
                        *span,
                    ));
                }
                Ok(())
            }

            Stmt::Continue { span } => {
                if self.loop_depth == 0 {
                    return Err(CompileError::new(
                        "continue outside of loop",
                        *span,
                    ));
                }
                Ok(())
            }

            Stmt::Match { subject, arms, span } => {
                let subject_ty = self.check_expr(subject)?;
                for arm in arms {
                    // Check pattern type matches subject
                    match &arm.pattern {
                        Pattern::IntLit(_, _) => {
                            if subject_ty != Type::I64 {
                                return Err(CompileError::new(
                                    format!("integer pattern in match on '{}'", subject_ty),
                                    *span,
                                ));
                            }
                        }
                        Pattern::BoolLit(_, _) => {
                            if subject_ty != Type::Bool {
                                return Err(CompileError::new(
                                    format!("boolean pattern in match on '{}'", subject_ty),
                                    *span,
                                ));
                            }
                        }
                        Pattern::Wildcard(_) => {} // matches anything
                        Pattern::EnumVariant(enum_name, variant, span) => {
                            // Check enum exists
                            let info = match self.enums.get(enum_name) {
                                Some(info) => info.clone(),
                                None => {
                                    return Err(CompileError::new(
                                        format!("unknown enum '{}'", enum_name),
                                        *span,
                                    ));
                                }
                            };
                            // Check variant exists
                            if !info.variants.contains(variant) {
                                return Err(CompileError::new(
                                    format!("unknown variant '{}::{}' ", enum_name, variant),
                                    *span,
                                ));
                            }
                            // Check subject is this enum type
                            if subject_ty != Type::Enum(enum_name.clone()) {
                                return Err(CompileError::new(
                                    format!(
                                        "enum pattern '{}::{}' in match on '{}'",
                                        enum_name, variant, subject_ty
                                    ),
                                    *span,
                                ));
                            }
                        }
                    }
                    self.check_block(&arm.body)?;
                }
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
                    // Arithmetic: i64 op i64 -> i64 (Add also supports str + str)
                    BinOp::Add => {
                        if left_ty == Type::Str && right_ty == Type::Str {
                            return Ok(Type::Str); // string concatenation
                        }
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
                    BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
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

                // Special handling for len — accepts str or array
                if name == "len" {
                    if args.len() != 1 {
                        return Err(CompileError::new(
                            format!("len() takes 1 argument, got {}", args.len()),
                            *span,
                        ));
                    }
                    let arg_ty = self.check_expr(&args[0])?;
                    match &arg_ty {
                        Type::Str | Type::Array(_, _) => return Ok(Type::I64),
                        _ => {
                            return Err(CompileError::new(
                                format!("len() expects 'str' or array, got '{}'", arg_ty),
                                *span,
                            ));
                        }
                    }
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

                // Check privacy — can't call priv functions from main module
                if self.check_privacy && self.private_items.contains(name) {
                    return Err(CompileError::new(
                        format!("function '{}' is private in its module", name),
                        *span,
                    ));
                }

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

            Expr::ArrayLit { elements, span } => {
                if elements.is_empty() {
                    return Err(CompileError::new(
                        "empty array literals are not allowed",
                        *span,
                    ));
                }
                let first_ty = self.check_expr(&elements[0])?;
                for (i, elem) in elements.iter().enumerate().skip(1) {
                    let elem_ty = self.check_expr(elem)?;
                    if elem_ty != first_ty {
                        return Err(CompileError::new(
                            format!(
                                "array element {} has type '{}', expected '{}'",
                                i, elem_ty, first_ty
                            ),
                            *span,
                        ));
                    }
                }
                Ok(Type::Array(Box::new(first_ty), elements.len()))
            }

            Expr::Index { object, index, span } => {
                let obj_ty = self.check_expr(object)?;
                let elem_ty = match &obj_ty {
                    Type::Array(elem, _) => elem.as_ref().clone(),
                    _ => {
                        return Err(CompileError::new(
                            format!("cannot index into non-array type '{}'", obj_ty),
                            *span,
                        ));
                    }
                };
                let idx_ty = self.check_expr(index)?;
                if idx_ty != Type::I64 {
                    return Err(CompileError::new(
                        format!("array index must be 'i64', got '{}'", idx_ty),
                        *span,
                    ));
                }
                Ok(elem_ty)
            }

            Expr::EnumVariant { enum_name, variant, span } => {
                let info = match self.enums.get(enum_name) {
                    Some(info) => info.clone(),
                    None => {
                        return Err(CompileError::new(
                            format!("unknown enum '{}'", enum_name),
                            *span,
                        ));
                    }
                };
                if self.check_privacy && self.private_items.contains(enum_name) {
                    return Err(CompileError::new(
                        format!("enum '{}' is private in its module", enum_name),
                        *span,
                    ));
                }
                if !info.variants.contains(variant) {
                    return Err(CompileError::new(
                        format!("unknown variant '{}::{}'", enum_name, variant),
                        *span,
                    ));
                }
                Ok(Type::Enum(enum_name.clone()))
            }

            Expr::StructLit { name, fields, span } => {
                let info = match self.structs.get(name) {
                    Some(info) => info.clone(),
                    None => {
                        return Err(CompileError::new(
                            format!("unknown struct '{}'", name),
                            *span,
                        ));
                    }
                };
                if self.check_privacy && self.private_items.contains(name) {
                    return Err(CompileError::new(
                        format!("struct '{}' is private in its module", name),
                        *span,
                    ));
                }
                // Check each provided field exists and types match
                for (fname, fexpr) in fields {
                    let field_info = info.fields.iter().find(|(n, _)| n == fname);
                    match field_info {
                        None => {
                            return Err(CompileError::new(
                                format!("struct '{}' has no field '{}'", name, fname),
                                *span,
                            ));
                        }
                        Some((_, expected_ty)) => {
                            let actual_ty = self.check_expr(fexpr)?;
                            if actual_ty != *expected_ty {
                                return Err(CompileError::new(
                                    format!(
                                        "field '{}' expected type '{}', got '{}'",
                                        fname, expected_ty, actual_ty
                                    ),
                                    *span,
                                ));
                            }
                        }
                    }
                }
                // Check all fields are provided
                for (fname, _) in &info.fields {
                    if !fields.iter().any(|(n, _)| n == fname) {
                        return Err(CompileError::new(
                            format!("missing field '{}' in struct '{}'", fname, name),
                            *span,
                        ));
                    }
                }
                Ok(Type::Struct(name.clone()))
            }

            Expr::FieldAccess { object, field, span } => {
                let obj_ty = self.check_expr(object)?;
                match &obj_ty {
                    Type::Struct(sname) => {
                        let info = self.structs.get(sname).unwrap().clone();
                        match info.fields.iter().find(|(n, _)| n == field) {
                            Some((_, ty)) => Ok(ty.clone()),
                            None => Err(CompileError::new(
                                format!("struct '{}' has no field '{}'", sname, field),
                                *span,
                            )),
                        }
                    }
                    _ => Err(CompileError::new(
                        format!("field access on non-struct type '{}'", obj_ty),
                        *span,
                    )),
                }
            }

            Expr::Try { expr, span } => {
                if !self.current_can_fail {
                    return Err(CompileError::new(
                        "'try' can only be used in failable functions (-> T ! str)",
                        *span,
                    ));
                }
                // The inner expression must be a call to a failable function
                if let Expr::Call { name, .. } = expr.as_ref() {
                    match self.functions.get(name) {
                        Some(info) => {
                            if !info.can_fail {
                                return Err(CompileError::new(
                                    format!("'try' used on non-failable function '{}'", name),
                                    *span,
                                ));
                            }
                            self.check_expr(expr)
                        }
                        None => {
                            self.check_expr(expr)
                        }
                    }
                } else {
                    Err(CompileError::new(
                        "'try' must be used with a function call",
                        *span,
                    ))
                }
            }

            Expr::Cast { expr, target, span } => {
                let from_ty = self.check_expr(expr)?;
                let to_ty = match Type::from_name(target) {
                    Some(t) => t,
                    None => {
                        return Err(CompileError::new(
                            format!("unknown cast target type '{}'", target),
                            *span,
                        ));
                    }
                };
                // Allowed casts: i64→str, i64→bool, bool→i64, str→i64
                let valid = matches!(
                    (&from_ty, &to_ty),
                    (Type::I64, Type::Str)
                    | (Type::I64, Type::Bool)
                    | (Type::Bool, Type::I64)
                    | (Type::Str, Type::I64)
                );
                if !valid {
                    return Err(CompileError::new(
                        format!("cannot cast '{}' to '{}'", from_ty, to_ty),
                        *span,
                    ));
                }
                Ok(to_ty)
            }
        }
    }
}

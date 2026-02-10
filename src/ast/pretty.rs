// Pretty-printer for the AST — useful for debugging.

use crate::ast::*;
use std::fmt::Write;

pub fn pretty_print(program: &Program) -> String {
    let mut out = String::new();
    for import in &program.imports {
        write!(out, "import \"{}\";\n", import.path).unwrap();
    }
    for enum_def in &program.enums {
        print_enum_def(&mut out, enum_def, 0);
    }
    for struct_def in &program.structs {
        print_struct_def(&mut out, struct_def, 0);
    }
    for ct in &program.comptimes {
        if ct.is_private {
            out.push_str("priv ");
        }
        write!(out, "comptime let {} = ", ct.name).unwrap();
        print_expr(&mut out, &ct.value);
        out.push_str(";\n");
    }
    for func in &program.functions {
        print_function(&mut out, func, 0);
    }
    out
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn print_enum_def(out: &mut String, enum_def: &EnumDef, level: usize) {
    indent(out, level);
    if enum_def.is_private {
        out.push_str("priv ");
    }
    write!(out, "enum {} {{ ", enum_def.name).unwrap();
    for (i, variant) in enum_def.variants.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(variant);
    }
    out.push_str(" }\n");
}

fn print_struct_def(out: &mut String, struct_def: &StructDef, level: usize) {
    indent(out, level);
    if struct_def.is_private {
        out.push_str("priv ");
    }
    write!(out, "struct {} {{ ", struct_def.name).unwrap();
    for (i, field) in struct_def.fields.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write!(out, "{}: {}", field.name, field.type_name).unwrap();
    }
    out.push_str(" }\n");
}

fn print_function(out: &mut String, func: &Function, level: usize) {
    indent(out, level);
    if func.is_private {
        out.push_str("priv ");
    }
    write!(out, "fn {}(", func.name).unwrap();
    for (i, param) in func.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write!(out, "{}: {}", param.name, param.type_name).unwrap();
    }
    out.push(')');
    if let Some(ret) = &func.return_type {
        write!(out, " -> {}", ret).unwrap();
        if func.can_fail {
            out.push_str(" ! str");
        }
    }
    out.push_str(" {\n");
    print_block(out, &func.body, level + 1);
    indent(out, level);
    out.push_str("}\n");
}

fn print_block(out: &mut String, block: &Block, level: usize) {
    for stmt in &block.stmts {
        print_stmt(out, stmt, level);
    }
}

fn print_stmt(out: &mut String, stmt: &Stmt, level: usize) {
    indent(out, level);
    match stmt {
        Stmt::Let { name, mutable, type_name, value, .. } => {
            out.push_str("let ");
            if *mutable {
                out.push_str("mut ");
            }
            out.push_str(name);
            if let Some(t) = type_name {
                write!(out, ": {}", t).unwrap();
            }
            out.push_str(" = ");
            print_expr(out, value);
            out.push_str(";\n");
        }
        Stmt::Assign { name, value, .. } => {
            out.push_str(name);
            out.push_str(" = ");
            print_expr(out, value);
            out.push_str(";\n");
        }
        Stmt::IndexAssign { object, index, value, .. } => {
            out.push_str(object);
            out.push('[');
            print_expr(out, index);
            out.push_str("] = ");
            print_expr(out, value);
            out.push_str(";\n");
        }
        Stmt::FieldAssign { object, field, value, .. } => {
            write!(out, "{}.{} = ", object, field).unwrap();
            print_expr(out, value);
            out.push_str(";\n");
        }
        Stmt::Return { value, .. } => {
            out.push_str("return");
            if let Some(v) = value {
                out.push(' ');
                print_expr(out, v);
            }
            out.push_str(";\n");
        }
        Stmt::Defer { expr, .. } => {
            out.push_str("defer ");
            print_expr(out, expr);
            out.push_str(";\n");
        }
        Stmt::Fail { message, .. } => {
            out.push_str("fail ");
            print_expr(out, message);
            out.push_str(";\n");
        }
        Stmt::Expr { expr, .. } => {
            print_expr(out, expr);
            out.push_str(";\n");
        }
        Stmt::TailExpr { expr, .. } => {
            print_expr(out, expr);
            out.push('\n');
        }
        Stmt::If { condition, then_block, else_block, .. } => {
            out.push_str("if ");
            print_expr(out, condition);
            out.push_str(" {\n");
            print_block(out, then_block, level + 1);
            indent(out, level);
            out.push('}');
            if let Some(else_b) = else_block {
                out.push_str(" else {\n");
                print_block(out, else_b, level + 1);
                indent(out, level);
                out.push('}');
            }
            out.push('\n');
        }
        Stmt::While { condition, body, .. } => {
            out.push_str("while ");
            print_expr(out, condition);
            out.push_str(" {\n");
            print_block(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::For { var, start, end, body, .. } => {
            write!(out, "for {} in ", var).unwrap();
            print_expr(out, start);
            out.push_str("..");
            print_expr(out, end);
            out.push_str(" {\n");
            print_block(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Break { .. } => {
            out.push_str("break;\n");
        }
        Stmt::Continue { .. } => {
            out.push_str("continue;\n");
        }
        Stmt::Match { subject, arms, .. } => {
            out.push_str("match ");
            print_expr(out, subject);
            out.push_str(" {\n");
            for arm in arms {
                indent(out, level + 1);
                match &arm.pattern {
                    Pattern::IntLit(v, _) => write!(out, "{}", v).unwrap(),
                    Pattern::BoolLit(v, _) => write!(out, "{}", v).unwrap(),
                    Pattern::Wildcard(_) => out.push('_'),
                    Pattern::EnumVariant(enum_name, variant, _) => {
                        write!(out, "{}::{}", enum_name, variant).unwrap()
                    }
                }
                out.push_str(" => {\n");
                print_block(out, &arm.body, level + 2);
                indent(out, level + 1);
                out.push_str("}\n");
            }
            indent(out, level);
            out.push_str("}\n");
        }
    }
}

fn print_expr(out: &mut String, expr: &Expr) {
    match expr {
        Expr::IntLit { value, .. } => write!(out, "{}", value).unwrap(),
        Expr::BoolLit { value, .. } => write!(out, "{}", value).unwrap(),
        Expr::StringLit { value, .. } => write!(out, "\"{}\"", value).unwrap(),
        Expr::Ident { name, .. } => out.push_str(name),
        Expr::BinaryOp { op, left, right, .. } => {
            out.push('(');
            print_expr(out, left);
            let op_str = match op {
                BinOp::Add => " + ",
                BinOp::Sub => " - ",
                BinOp::Mul => " * ",
                BinOp::Div => " / ",
                BinOp::Mod => " % ",
                BinOp::Eq  => " == ",
                BinOp::Neq => " != ",
                BinOp::Lt  => " < ",
                BinOp::Gt  => " > ",
                BinOp::Lte => " <= ",
                BinOp::Gte => " >= ",
                BinOp::And => " && ",
                BinOp::Or  => " || ",
            };
            out.push_str(op_str);
            print_expr(out, right);
            out.push(')');
        }
        Expr::UnaryOp { op, operand, .. } => {
            let op_str = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            out.push_str(op_str);
            print_expr(out, operand);
        }
        Expr::Call { name, args, .. } => {
            out.push_str(name);
            out.push('(');
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                print_expr(out, arg);
            }
            out.push(')');
        }
        Expr::ArrayLit { elements, .. } => {
            out.push('[');
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                print_expr(out, elem);
            }
            out.push(']');
        }
        Expr::Index { object, index, .. } => {
            print_expr(out, object);
            out.push('[');
            print_expr(out, index);
            out.push(']');
        }
        Expr::EnumVariant { enum_name, variant, .. } => {
            write!(out, "{}::{}", enum_name, variant).unwrap();
        }
        Expr::StructLit { name, fields, .. } => {
            write!(out, "{} {{ ", name).unwrap();
            for (i, (fname, fval)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write!(out, "{}: ", fname).unwrap();
                print_expr(out, fval);
            }
            out.push_str(" }");
        }
        Expr::FieldAccess { object, field, .. } => {
            print_expr(out, object);
            write!(out, ".{}", field).unwrap();
        }
        Expr::Try { expr, .. } => {
            out.push_str("try ");
            print_expr(out, expr);
        }
        Expr::Cast { expr, target, .. } => {
            print_expr(out, expr);
            out.push_str(" as ");
            out.push_str(target);
        }
    }
}

// Pretty-printer for the AST — useful for debugging.

use crate::ast::*;
use std::fmt::Write;

pub fn pretty_print(program: &Program) -> String {
    let mut out = String::new();
    for func in &program.items {
        print_function(&mut out, func, 0);
    }
    out
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn print_function(out: &mut String, func: &Function, level: usize) {
    indent(out, level);
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
        Stmt::Return { value, .. } => {
            out.push_str("return");
            if let Some(v) = value {
                out.push(' ');
                print_expr(out, v);
            }
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
    }
}

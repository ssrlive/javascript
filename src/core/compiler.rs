use crate::core::opcode::{Chunk, Opcode};
use crate::core::statement::{BinaryOp, Expr, Statement, StatementKind};
use crate::Value;

pub struct Compiler<'gc> {
    chunk: Chunk<'gc>,
}

impl<'gc> Compiler<'gc> {
    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
        }
    }

    pub fn compile(mut self, statements: &[Statement]) -> Result<Chunk<'gc>, String> {
        for (i, stmt) in statements.iter().enumerate() {
            let is_last = i == statements.len() - 1;
            self.compile_statement(stmt, is_last)?;
        }
        
        // Ensure returning at the end
        self.chunk.write_opcode(Opcode::Return);
        
        Ok(self.chunk)
    }

    fn compile_statement(&mut self, stmt: &Statement, is_last: bool) -> Result<(), String> {
        match &*stmt.kind {
            StatementKind::Expr(expr) => {
                self.compile_expr(expr)?;
                // Pop if it's not the last evaluated statement, to keep stack clean
                if !is_last{
                    self.chunk.write_opcode(Opcode::Pop);
                }
            }
            StatementKind::Let(decls) | StatementKind::Var(decls) => {
                for (name, init_opt) in decls {
                    if let Some(init) = init_opt {
                        self.compile_expr(init)?;
                    } else {
                        // Normally Push Undefined, but for now we push constant Undefined 
                        // Wait, creating a Constant pool entry for Undefined is fine.
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_byte(idx);
                    }
                    
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_byte(name_idx);
                }
                if is_last {
                    // Statements don't return values usually, but REPL likes Undefined
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_byte(idx);
                }
            }
            StatementKind::Assign(name, expr) => {
                // Wait, some assignments are parsed as statements?
                // compile rhs
                self.compile_expr(expr)?;
                let name_u16 = crate::unicode::utf8_to_utf16(name);
                let name_idx = self.chunk.add_constant(Value::String(name_u16));
                self.chunk.write_opcode(Opcode::SetGlobal);
                self.chunk.write_byte(name_idx);
                if !is_last{
                    self.chunk.write_opcode(Opcode::Pop);
                }
            }
            _ => return Err(format!("UnimplementedstatementkindforVM")),
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Number(n) => {
                let constant_index = self.chunk.add_constant(Value::Number(*n));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_byte(constant_index);
            }
            Expr::Var(name, ..) => {
                let name_u16 = crate::unicode::utf8_to_utf16(name);
                let name_idx = self.chunk.add_constant(Value::String(name_u16));
                self.chunk.write_opcode(Opcode::GetGlobal);
                self.chunk.write_byte(name_idx);
            }
            Expr::Assign(left, right) => {
                if let Expr::Var(name, ..) = &**left {
                    self.compile_expr(right)?;
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::SetGlobal);
                    self.chunk.write_byte(name_idx);
                } else {
                    return Err("Invalid assignment target for VM".to_string());
                }
            }
            Expr::Binary(left, op, right) => {
                // Evaluate left, then evaluate right 
                self.compile_expr(left)?;
                self.compile_expr(right)?;

                match op {
                    BinaryOp::Add => self.chunk.write_opcode(Opcode::Add),
                    BinaryOp::Sub => self.chunk.write_opcode(Opcode::Sub),
                    BinaryOp::Mul => self.chunk.write_opcode(Opcode::Mul),
                    BinaryOp::Div => self.chunk.write_opcode(Opcode::Div),
                    // We can add other opcodes easily later
                    _ => return Err(format!("UnimplementedbinaryoperatorforVM")),
                }
            }
            _ => return Err(format!("UnimplementedexpressiontypeforVM")),
        }
        Ok(())
    }
}

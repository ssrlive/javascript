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
        for stmt in statements {
            self.compile_statement(stmt)?;
        }
        
        // Ensure returning at the end
        self.chunk.write_opcode(Opcode::Return);
        
        Ok(self.chunk)
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), String> {
        match &*stmt.kind {
            StatementKind::Expr(expr) => {
                self.compile_expr(expr)?;
            }
            _ => return Err(format!("Unimplemented statement kind for VM: {:?}", stmt.kind)),
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
                    _ => return Err(format!("Unimplemented binary operator for VM")),
                }
            }
            _ => return Err(format!("Unimplemented expression type for VM")),
        }
        Ok(())
    }
}

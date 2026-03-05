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

    fn emit_jump(&mut self, opcode: Opcode) -> usize {
        self.chunk.write_opcode(opcode);
        // Write placeholder u16
        self.chunk.write_u16(0xffff);
        self.chunk.code.len() - 2 // Return the offset to the placeholder
    }

    fn patch_jump(&mut self, offset: usize) {
        let jump_target = self.chunk.code.len();
        if jump_target > u16::MAX as usize {
            panic!("Jump target too large");
        }
        self.chunk.code[offset] = (jump_target & 0xff) as u8;
        self.chunk.code[offset + 1] = ((jump_target >> 8) & 0xff) as u8;
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.chunk.write_opcode(Opcode::Jump);
        if loop_start > u16::MAX as usize {
            panic!("Loop start too large");
        }
        self.chunk.write_u16(loop_start as u16);
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
            }            StatementKind::Block(statements) => {
                for (i, s) in statements.iter().enumerate() {
                    let s_is_last = is_last && i == statements.len() - 1;
                    self.compile_statement(s, s_is_last)?;
                }
            }
            StatementKind::If(if_stmt) => {
                self.compile_expr(&if_stmt.condition)?;
                let then_jump = self.emit_jump(Opcode::JumpIfFalse);
                
                // Then branch
                for (i, s) in if_stmt.then_body.iter().enumerate() {
                    let s_is_last = is_last && i == if_stmt.then_body.len() - 1 && if_stmt.else_body.is_none();
                    self.compile_statement(s, s_is_last)?;
                }
                
                if let Some(else_body) = &if_stmt.else_body {
                    let else_jump = self.emit_jump(Opcode::Jump);
                    self.patch_jump(then_jump);
                    
                    for (i, s) in else_body.iter().enumerate() {
                        let s_is_last = is_last && i == else_body.len() - 1;
                        self.compile_statement(s, s_is_last)?;
                    }
                    self.patch_jump(else_jump);
                } else {
                    self.patch_jump(then_jump);
                }
            }
            StatementKind::While(cond, body) => {
                let loop_start = self.chunk.code.len();
                self.compile_expr(cond)?;
                let exit_jump = self.emit_jump(Opcode::JumpIfFalse);
                
                for s in body {
                    self.compile_statement(s, false)?; // Inside loops, rarely the definitive last val
                }
                
                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);
                
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_byte(idx);
                }
            }            _ => return Err(format!("UnimplementedstatementkindforVM")),
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
                    BinaryOp::Div => self.chunk.write_opcode(Opcode::Div),                    BinaryOp::LessThan => self.chunk.write_opcode(Opcode::LessThan),
                    BinaryOp::GreaterThan => self.chunk.write_opcode(Opcode::GreaterThan),
                    BinaryOp::Equal => self.chunk.write_opcode(Opcode::Equal),
                    BinaryOp::StrictEqual => self.chunk.write_opcode(Opcode::Equal), // rough approximation for demo                    // We can add other opcodes easily later
                    _ => return Err(format!("UnimplementedbinaryoperatorforVM")),
                }
            }
            _ => return Err(format!("UnimplementedexpressiontypeforVM")),
        }
        Ok(())
    }
}

use crate::core::opcode::{Chunk, Opcode};
use crate::core::statement::{BinaryOp, Expr, Statement, StatementKind};
use crate::Value;

pub struct Compiler<'gc> {
    chunk: Chunk<'gc>,
    locals: Vec<String>,
    scope_depth: i32, // 0 = top-level (global), > 0 = inside function
}

impl<'gc> Compiler<'gc> {
    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
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
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_byte(idx);
                    }

                    if self.scope_depth > 0 {
                        // Inside a function: value stays on stack as a local slot
                        self.locals.push(name.clone());
                    } else {
                        // Top-level: define as global
                        let name_u16 = crate::unicode::utf8_to_utf16(name);
                        let name_idx = self.chunk.add_constant(Value::String(name_u16));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_byte(name_idx);
                    }
                }
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_byte(idx);
                }
            }
            StatementKind::Assign(name, expr) => {
                self.compile_expr(expr)?;
                if let Some(pos) = self.locals.iter().position(|l| l == name) {
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::SetGlobal);
                    self.chunk.write_byte(name_idx);
                }
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
            }            StatementKind::Return(expr_opt) => {
                if let Some(expr) = expr_opt {
                    self.compile_expr(expr)?;
                } else {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_byte(idx);
                }
                self.chunk.write_opcode(Opcode::Return);
            }
            StatementKind::FunctionDeclaration(name, params, body, _is_gen, _is_async) => {
                // Jump over the function body in the main bytecode stream
                let jump_over = self.emit_jump(Opcode::Jump);
                let func_ip = self.chunk.code.len();

                // Save and reset locals/scope for function scope
                let old_locals = std::mem::take(&mut self.locals);
                let old_depth = self.scope_depth;
                self.scope_depth = 1;
                for param in params {
                    if let crate::core::statement::DestructuringElement::Variable(param_name, _) = param {
                        self.locals.push(param_name.clone());
                    }
                }

                for (i, s) in body.iter().enumerate() {
                    self.compile_statement(s, i == body.len() - 1)?;
                }

                // Implicit return undefined if no explicit return
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_byte(idx);
                self.chunk.write_opcode(Opcode::Return);

                self.patch_jump(jump_over);
                self.locals = old_locals;
                self.scope_depth = old_depth;

                // Push the VmFunction value and define it as a global
                let func_val = Value::VmFunction(func_ip, params.len() as u8);
                let func_idx = self.chunk.add_constant(func_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_byte(func_idx);

                let name_u16 = crate::unicode::utf8_to_utf16(name);
                let name_idx = self.chunk.add_constant(Value::String(name_u16));
                self.chunk.write_opcode(Opcode::DefineGlobal);
                self.chunk.write_byte(name_idx);
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
            Expr::StringLit(s) => {
                let idx = self.chunk.add_constant(Value::String(s.clone()));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_byte(idx);
            }
            Expr::Boolean(b) => {
                let idx = self.chunk.add_constant(Value::Boolean(*b));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_byte(idx);
            }
            Expr::Null => {
                let idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_byte(idx);
            }
            Expr::Undefined => {
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_byte(idx);
            }
            Expr::Var(name, ..) => {
                if let Some(pos) = self.locals.iter().position(|l| l == name) {
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_byte(name_idx);
                }
            }

            Expr::Assign(left, right) => {
                if let Expr::Var(name, ..) = &**left {
                    self.compile_expr(right)?;
                    if let Some(pos) = self.locals.iter().position(|l| l == name) {
                        self.chunk.write_opcode(Opcode::SetLocal);
                        self.chunk.write_byte(pos as u8);
                    } else {
                        let name_u16 = crate::unicode::utf8_to_utf16(name);
                        let name_idx = self.chunk.add_constant(Value::String(name_u16));
                        self.chunk.write_opcode(Opcode::SetGlobal);
                        self.chunk.write_byte(name_idx);
                    }
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
                    BinaryOp::Mod => self.chunk.write_opcode(Opcode::Mod),
                    BinaryOp::LessThan => self.chunk.write_opcode(Opcode::LessThan),
                    BinaryOp::GreaterThan => self.chunk.write_opcode(Opcode::GreaterThan),
                    BinaryOp::LessEqual => self.chunk.write_opcode(Opcode::LessEqual),
                    BinaryOp::GreaterEqual => self.chunk.write_opcode(Opcode::GreaterEqual),
                    BinaryOp::Equal => self.chunk.write_opcode(Opcode::Equal),
                    BinaryOp::StrictEqual => self.chunk.write_opcode(Opcode::Equal),
                    BinaryOp::NotEqual => self.chunk.write_opcode(Opcode::NotEqual),
                    BinaryOp::StrictNotEqual => self.chunk.write_opcode(Opcode::StrictNotEqual),
                    _ => return Err(format!("Unimplemented binary operator for VM: {:?}", op)),
                }
            }
            Expr::Call(callee, args) => {
                self.compile_expr(callee)?;
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.write_opcode(Opcode::Call);
                self.chunk.write_byte(args.len() as u8);
            }
            // Unary operators
            Expr::UnaryNeg(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Negate);
            }
            Expr::LogicalNot(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Not);
            }
            Expr::UnaryPlus(inner) => {
                // +x is just coerce to number, for now just compile inner
                self.compile_expr(inner)?;
            }
            Expr::TypeOf(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::TypeOf);
            }
            // Logical operators (short-circuit)
            Expr::LogicalAnd(left, right) => {
                // Evaluate left; if falsy, short-circuit (keep left value)
                self.compile_expr(left)?;
                let end_jump = self.emit_jump(Opcode::JumpIfFalse);
                // Left was truthy, discard it and evaluate right
                self.compile_expr(right)?;
                let skip = self.emit_jump(Opcode::Jump);
                self.patch_jump(end_jump);
                // Left was falsy, push false
                let idx = self.chunk.add_constant(Value::Boolean(false));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_byte(idx);
                self.patch_jump(skip);
            }
            Expr::LogicalOr(left, right) => {
                // Evaluate left; if truthy, short-circuit (keep left value)
                self.compile_expr(left)?;
                let end_jump = self.emit_jump(Opcode::JumpIfTrue);
                // Left was falsy, discard it and evaluate right
                self.compile_expr(right)?;
                let skip = self.emit_jump(Opcode::Jump);
                self.patch_jump(end_jump);
                // Left was truthy, push true
                let idx = self.chunk.add_constant(Value::Boolean(true));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_byte(idx);
                self.patch_jump(skip);
            }
            _ => return Err(format!("Unimplemented expression type for VM: {:?}", expr)),
        }
        Ok(())
    }
}

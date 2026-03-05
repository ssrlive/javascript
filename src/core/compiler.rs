use crate::core::opcode::{Chunk, Opcode};
use crate::core::statement::{BinaryOp, CatchParamPattern, ClassMember, DestructuringElement, Expr, Statement, StatementKind};
use crate::core::{JSError, Value};
use crate::raise_syntax_error;

pub struct Compiler<'gc> {
    chunk: Chunk<'gc>,
    locals: Vec<String>,
    scope_depth: i32, // 0 = top-level (global), > 0 = inside function
    loop_stack: Vec<LoopContext>,
    pending_label: Option<String>, // label to attach to the next loop
}

#[derive(Debug, Clone, Default)]
struct LoopContext {
    #[allow(dead_code)]
    loop_start: usize, // IP to jump back to (top of loop)
    label: Option<String>,        // optional label for labeled break/continue
    continue_patches: Vec<usize>, // offsets to patch with continue target
    break_patches: Vec<usize>,    // offsets to patch with post-loop address
}

impl<'gc> Compiler<'gc> {
    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
            loop_stack: Vec::new(),
            pending_label: None,
        }
    }

    pub fn compile(mut self, statements: &[Statement]) -> Result<Chunk<'gc>, JSError> {
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
        self.patch_jump_to(offset, jump_target);
    }

    fn patch_jump_to(&mut self, offset: usize, target: usize) {
        if target > u16::MAX as usize {
            panic!("Jump target too large");
        }
        self.chunk.code[offset] = (target & 0xff) as u8;
        self.chunk.code[offset + 1] = ((target >> 8) & 0xff) as u8;
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.chunk.write_opcode(Opcode::Jump);
        if loop_start > u16::MAX as usize {
            panic!("Loop start too large");
        }
        self.chunk.write_u16(loop_start as u16);
    }

    /// Create a LoopContext, consuming any pending label
    fn make_loop_context(&mut self, loop_start: usize) -> LoopContext {
        LoopContext {
            loop_start,
            label: self.pending_label.take(),
            ..LoopContext::default()
        }
    }

    fn compile_statement(&mut self, stmt: &Statement, is_last: bool) -> Result<(), JSError> {
        match &*stmt.kind {
            StatementKind::Expr(expr) => {
                self.compile_expr(expr)?;
                // Pop if it's not the last evaluated statement, to keep stack clean
                if !is_last {
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
                        self.chunk.write_u16(idx);
                    }

                    if self.scope_depth > 0 {
                        // Inside a function: check if var already exists (var is function-scoped)
                        if let Some(pos) = self.locals.iter().position(|l| l == name) {
                            // Re-declaration: assign to existing slot
                            self.chunk.write_opcode(Opcode::SetLocal);
                            self.chunk.write_byte(pos as u8);
                            self.chunk.write_opcode(Opcode::Pop);
                        } else {
                            // New local: value stays on stack as a local slot
                            self.locals.push(name.clone());
                        }
                    } else {
                        // Top-level: define as global
                        let name_u16 = crate::unicode::utf8_to_utf16(name);
                        let name_idx = self.chunk.add_constant(Value::String(name_u16));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(name_idx);
                    }
                }
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Const(decls) => {
                for (name, init) in decls {
                    self.compile_expr(init)?;
                    if self.scope_depth > 0 {
                        self.locals.push(name.clone());
                    } else {
                        let name_u16 = crate::unicode::utf8_to_utf16(name);
                        let name_idx = self.chunk.add_constant(Value::String(name_u16));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(name_idx);
                    }
                }
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
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
                    self.chunk.write_u16(name_idx);
                }
                if !is_last {
                    self.chunk.write_opcode(Opcode::Pop);
                }
            }
            StatementKind::Block(statements) => {
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
            StatementKind::DoWhile(body, cond) => {
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);

                for s in body {
                    self.compile_statement(s, false)?;
                }

                // continue target: the condition check (current IP)
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump(*cp);
                }

                self.compile_expr(cond)?;
                let continue_jump = self.emit_jump(Opcode::JumpIfTrue);
                // Patch: JumpIfTrue target is loop_start
                self.chunk.code[continue_jump] = (loop_start & 0xff) as u8;
                self.chunk.code[continue_jump + 1] = ((loop_start >> 8) & 0xff) as u8;

                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::While(cond, body) => {
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);

                self.compile_expr(cond)?;
                let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

                for s in body {
                    self.compile_statement(s, false)?;
                }

                // Patch continue → loop_start (condition)
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, loop_start);
                }

                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);

                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::For(for_stmt) => {
                // Compile init
                if let Some(init) = &for_stmt.init {
                    self.compile_statement(init, false)?;
                }
                // Loop start: test
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);

                let exit_jump = if let Some(test) = &for_stmt.test {
                    self.compile_expr(test)?;
                    Some(self.emit_jump(Opcode::JumpIfFalse))
                } else {
                    None
                };
                // Body
                for s in &for_stmt.body {
                    self.compile_statement(s, false)?;
                }
                // Update — continue jumps here (not to condition)
                let update_ip = self.chunk.code.len();
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }
                if let Some(update) = &for_stmt.update {
                    self.compile_statement(update, false)?;
                }
                // Jump back to test
                self.emit_loop(loop_start);
                if let Some(ej) = exit_jump {
                    self.patch_jump(ej);
                }

                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Return(expr_opt) => {
                if let Some(expr) = expr_opt {
                    self.compile_expr(expr)?;
                } else {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
                self.chunk.write_opcode(Opcode::Return);
            }
            StatementKind::Throw(expr) => {
                self.compile_expr(expr)?;
                self.chunk.write_opcode(Opcode::Throw);
            }
            StatementKind::Break(label_opt) => {
                let patch = self.emit_jump(Opcode::Jump);
                if let Some(label) = label_opt {
                    // Labeled break: find the matching loop context
                    if let Some(ctx) = self.loop_stack.iter_mut().rev().find(|c| c.label.as_deref() == Some(label)) {
                        ctx.break_patches.push(patch);
                    } else {
                        return Err(crate::raise_syntax_error!(format!("label '{}' not found for break", label)));
                    }
                } else if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.break_patches.push(patch);
                } else {
                    return Err(crate::raise_syntax_error!("break outside of loop"));
                }
            }
            StatementKind::Continue(label_opt) => {
                let patch = self.emit_jump(Opcode::Jump);
                if let Some(label) = label_opt {
                    if let Some(ctx) = self.loop_stack.iter_mut().rev().find(|c| c.label.as_deref() == Some(label)) {
                        ctx.continue_patches.push(patch);
                    } else {
                        return Err(crate::raise_syntax_error!(format!("label '{}' not found for continue", label)));
                    }
                } else if self.loop_stack.last().is_some() {
                    self.loop_stack.last_mut().unwrap().continue_patches.push(patch);
                } else {
                    return Err(crate::raise_syntax_error!("continue outside of loop"));
                }
            }
            StatementKind::Label(label, inner) => {
                // Set pending label so the next loop picks it up
                self.pending_label = Some(label.clone());
                self.compile_statement(inner, is_last)?;
                // Clear in case it wasn't consumed (non-loop label)
                self.pending_label = None;
            }
            StatementKind::ForIn(_decl_kind, var_name, obj_expr, body) => {
                // Compile: keys = GetKeys(obj); for (i=0; i<keys.length; i++) { var_name = keys[i]; body }
                self.compile_expr(obj_expr)?;
                self.chunk.write_opcode(Opcode::GetKeys);
                // keys array is now on stack; store as a local (or global)
                let _keys_local = if self.scope_depth > 0 {
                    self.locals.push("__keys__".to_string());
                    None
                } else {
                    let n = crate::unicode::utf8_to_utf16("__keys__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                    Some(ni)
                };
                // i = 0
                let zero_idx = self.chunk.add_constant(Value::Number(0.0));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(zero_idx);
                let _idx_local = if self.scope_depth > 0 {
                    self.locals.push("__idx__".to_string());
                    None
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                    Some(ni)
                };

                // Pre-allocate loop variable slot if it's a new local
                if self.scope_depth > 0 && !self.locals.iter().any(|l| l == var_name) {
                    let undef = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef);
                    self.locals.push(var_name.clone());
                }

                // Loop start: test i < keys.length
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);
                // push i
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__idx__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                // push keys.length
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__keys__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__keys__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                let len_key = crate::unicode::utf8_to_utf16("length");
                let len_idx = self.chunk.add_constant(Value::String(len_key));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(len_idx);
                self.chunk.write_opcode(Opcode::LessThan);
                let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

                // var_name = keys[i]
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__keys__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__keys__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__idx__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                self.chunk.write_opcode(Opcode::GetIndex);
                // Store in var_name (always emit SetLocal+Pop so it works on every iteration)
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == var_name).unwrap();
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                    self.chunk.write_opcode(Opcode::Pop);
                } else {
                    let vn = crate::unicode::utf8_to_utf16(var_name);
                    let vni = self.chunk.add_constant(Value::String(vn));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(vni);
                }

                // Body
                for s in body {
                    self.compile_statement(s, false)?;
                }

                // continue target: i++ update
                let update_ip = self.chunk.code.len();
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }

                // i++
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__idx__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                self.chunk.write_opcode(Opcode::Increment);
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__idx__").unwrap();
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::SetGlobal);
                    self.chunk.write_u16(ni);
                }
                self.chunk.write_opcode(Opcode::Pop);

                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);
                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }

                // Clean up synthetic locals
                if self.scope_depth > 0 {
                    self.locals.retain(|l| l != "__keys__" && l != "__idx__");
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::TryCatch(tc) => {
                // Determine catch binding name constant index (or 0xffff for none)
                let binding_idx: u16 = if let Some(_catch_body) = &tc.catch_body {
                    if let Some(CatchParamPattern::Identifier(ref name)) = tc.catch_param {
                        let name_u16 = crate::unicode::utf8_to_utf16(name);
                        self.chunk.add_constant(Value::String(name_u16))
                    } else {
                        0xffff
                    }
                } else {
                    0xffff
                };

                // SetupTry <catch_ip:u16> <binding_idx:u16>
                self.chunk.write_opcode(Opcode::SetupTry);
                let catch_placeholder = self.chunk.code.len();
                self.chunk.write_u16(0xffff); // placeholder for catch ip
                self.chunk.write_u16(binding_idx);

                // Try body
                for s in &tc.try_body {
                    self.compile_statement(s, false)?;
                }
                self.chunk.write_opcode(Opcode::TeardownTry);

                // Jump over catch block
                let jump_over_catch = self.emit_jump(Opcode::Jump);

                // Patch catch address to here
                let catch_start = self.chunk.code.len();
                self.chunk.code[catch_placeholder] = (catch_start & 0xff) as u8;
                self.chunk.code[catch_placeholder + 1] = ((catch_start >> 8) & 0xff) as u8;

                // Catch body
                if let Some(ref catch_body) = tc.catch_body {
                    for s in catch_body {
                        self.compile_statement(s, false)?;
                    }
                }

                self.patch_jump(jump_over_catch);

                // Finally body
                if let Some(ref finally_body) = tc.finally_body {
                    for s in finally_body {
                        self.compile_statement(s, false)?;
                    }
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::FunctionDeclaration(name, params, body, _is_gen, _is_async) => {
                // Jump over the function body in the main bytecode stream
                let jump_over = self.emit_jump(Opcode::Jump);
                let func_ip = self.chunk.code.len();

                // Save and reset locals/scope for function scope
                let old_locals = std::mem::take(&mut self.locals);
                let old_depth = self.scope_depth;
                let old_loops = std::mem::take(&mut self.loop_stack);
                self.scope_depth = 1;
                for param in params {
                    if let DestructuringElement::Variable(param_name, _) = param {
                        self.locals.push(param_name.clone());
                    }
                }

                for (i, s) in body.iter().enumerate() {
                    self.compile_statement(s, i == body.len() - 1)?;
                }

                // Implicit return undefined if no explicit return
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.chunk.write_opcode(Opcode::Return);

                self.patch_jump(jump_over);
                self.locals = old_locals;
                self.scope_depth = old_depth;
                self.loop_stack = old_loops;

                // Push the VmFunction value and define it as a global
                let func_val = Value::VmFunction(func_ip, params.len() as u8);
                let func_idx = self.chunk.add_constant(func_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(func_idx);

                let name_u16 = crate::unicode::utf8_to_utf16(name);
                let name_idx = self.chunk.add_constant(Value::String(name_u16));
                self.chunk.write_opcode(Opcode::DefineGlobal);
                self.chunk.write_u16(name_idx);
            }
            StatementKind::ForOf(_decl_kind, var_name, iterable_expr, body) => {
                // Desugar: arr = iterable; for (idx=0; idx<arr.length; idx++) { var_name = arr[idx]; body }
                self.compile_expr(iterable_expr)?;
                // Store iterable as __forofArr__
                if self.scope_depth > 0 {
                    self.locals.push("__forofArr__".to_string());
                } else {
                    let n = crate::unicode::utf8_to_utf16("__forofArr__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                }
                // idx = 0
                let zero_idx = self.chunk.add_constant(Value::Number(0.0));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(zero_idx);
                if self.scope_depth > 0 {
                    self.locals.push("__forofIdx__".to_string());
                } else {
                    let n = crate::unicode::utf8_to_utf16("__forofIdx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                }

                // Pre-allocate loop variable slot if it's a new local
                if self.scope_depth > 0 && !self.locals.iter().any(|l| l == var_name) {
                    let undef = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef);
                    self.locals.push(var_name.clone());
                }

                // Loop start: test idx < arr.length
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);
                self.emit_helper_get("__forofIdx__");
                self.emit_helper_get("__forofArr__");
                let len_key = crate::unicode::utf8_to_utf16("length");
                let len_idx = self.chunk.add_constant(Value::String(len_key));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(len_idx);
                self.chunk.write_opcode(Opcode::LessThan);
                let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

                // var_name = arr[idx]
                self.emit_helper_get("__forofArr__");
                self.emit_helper_get("__forofIdx__");
                self.chunk.write_opcode(Opcode::GetIndex);
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == var_name).unwrap();
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                    self.chunk.write_opcode(Opcode::Pop);
                } else {
                    let vn = crate::unicode::utf8_to_utf16(var_name);
                    let vni = self.chunk.add_constant(Value::String(vn));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(vni);
                }

                // Body
                for s in body {
                    self.compile_statement(s, false)?;
                }

                // continue target: idx++ update
                let update_ip = self.chunk.code.len();
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }

                // idx++
                self.emit_helper_get("__forofIdx__");
                self.chunk.write_opcode(Opcode::Increment);
                self.emit_helper_set("__forofIdx__");
                self.chunk.write_opcode(Opcode::Pop);

                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);
                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }

                // Clean up synthetic locals
                if self.scope_depth > 0 {
                    self.locals.retain(|l| l != "__forofArr__" && l != "__forofIdx__");
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Switch(sw) => {
                // Compile discriminant once, store in synthetic local/global
                self.compile_expr(&sw.expr)?;
                if self.scope_depth > 0 {
                    self.locals.push("__switch__".to_string());
                } else {
                    let n = crate::unicode::utf8_to_utf16("__switch__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                }

                // We need break patches
                let ctx = LoopContext::default();
                self.loop_stack.push(ctx);

                // For each case: test → jump to body if match, else next case
                let mut case_body_patches: Vec<(usize, usize)> = Vec::new(); // (body_start_idx in cases, jump_patch)
                let mut default_idx: Option<usize> = None;

                for (i, case) in sw.cases.iter().enumerate() {
                    match case {
                        crate::core::statement::SwitchCase::Case(val_expr, _body) => {
                            // push __switch__, push case value, compare
                            self.emit_helper_get("__switch__");
                            self.compile_expr(val_expr)?;
                            self.chunk.write_opcode(Opcode::Equal);
                            let body_jump = self.emit_jump(Opcode::JumpIfTrue);
                            case_body_patches.push((i, body_jump));
                        }
                        crate::core::statement::SwitchCase::Default(_body) => {
                            default_idx = Some(i);
                        }
                    }
                }

                // If no case matched, jump to default or end
                let default_jump = self.emit_jump(Opcode::Jump);

                // Emit bodies in order (fall-through semantics)
                let mut body_ips: Vec<usize> = Vec::new();
                for case in &sw.cases {
                    let body_ip = self.chunk.code.len();
                    body_ips.push(body_ip);
                    let body_stmts = match case {
                        crate::core::statement::SwitchCase::Case(_, body) => body,
                        crate::core::statement::SwitchCase::Default(body) => body,
                    };
                    for s in body_stmts {
                        self.compile_statement(s, false)?;
                    }
                }

                // Patch case jumps to their body IPs
                for (case_idx, patch) in &case_body_patches {
                    self.patch_jump_to(*patch, body_ips[*case_idx]);
                }

                // Patch default jump
                if let Some(di) = default_idx {
                    self.patch_jump_to(default_jump, body_ips[di]);
                } else {
                    self.patch_jump(default_jump);
                }

                // Patch break statements
                let end_ip = self.chunk.code.len();
                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump_to(bp, end_ip);
                }

                // Clean up synthetic local
                if self.scope_depth > 0 {
                    self.locals.retain(|l| l != "__switch__");
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Class(class_def) => {
                // Compile class constructor as a function
                let name = &class_def.name;
                let mut ctor_params = Vec::new();
                let mut ctor_body = Vec::new();
                for member in &class_def.members {
                    if let ClassMember::Constructor(params, body) = member {
                        ctor_params = params.clone();
                        ctor_body = body.clone();
                        break;
                    }
                }
                // Count simple params (DestructuringElement::Variable)
                let arity = ctor_params.len() as u8;

                // Emit jump over function body
                let jump_over = self.emit_jump(Opcode::Jump);
                let fn_start = self.chunk.code.len();

                // Push new scope for constructor body
                self.scope_depth += 1;
                // Register params as locals
                for p in &ctor_params {
                    if let DestructuringElement::Variable(pname, _) = p {
                        self.locals.push(pname.clone());
                    }
                }

                for (i, stmt) in ctor_body.iter().enumerate() {
                    let _is_last = i == ctor_body.len() - 1;
                    self.compile_statement(stmt, false)?;
                }
                // Constructor returns `this`
                self.chunk.write_opcode(Opcode::GetThis);
                self.chunk.write_opcode(Opcode::Return);

                // Clean up locals
                let locals_to_remove = ctor_params.len();
                for _ in 0..locals_to_remove {
                    self.locals.pop();
                }
                self.scope_depth -= 1;

                self.patch_jump(jump_over);

                // Define as global function
                let fn_val = Value::VmFunction(fn_start, arity);
                let fn_idx = self.chunk.add_constant(fn_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(fn_idx);

                let name_u16 = crate::unicode::utf8_to_utf16(name);
                let name_idx = self.chunk.add_constant(Value::String(name_u16));
                self.chunk.write_opcode(Opcode::DefineGlobal);
                self.chunk.write_u16(name_idx);
            }
            _ => {
                return Err(crate::raise_syntax_error!(format!(
                    "Unimplemented statement kind for VM: {:?}",
                    stmt.kind
                )));
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), JSError> {
        match expr {
            Expr::Number(n) => {
                let constant_index = self.chunk.add_constant(Value::Number(*n));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(constant_index);
            }
            Expr::StringLit(s) => {
                let idx = self.chunk.add_constant(Value::String(s.clone()));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::Boolean(b) => {
                let idx = self.chunk.add_constant(Value::Boolean(*b));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::Null => {
                let idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::Undefined => {
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::Var(name, ..) => {
                if let Some(pos) = self.locals.iter().position(|l| l == name) {
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(name_idx);
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
                    BinaryOp::In => self.chunk.write_opcode(Opcode::In),
                    BinaryOp::InstanceOf => self.chunk.write_opcode(Opcode::InstanceOf),
                    _ => {
                        return Err(crate::raise_syntax_error!(format!("Unimplemented binary operator for VM: {op:?}")));
                    }
                }
            }
            Expr::Call(callee, args) => {
                if let Expr::Property(obj, method_name) = &**callee {
                    // Method call: obj.method(args)
                    // Stack layout: [..., obj, method_fn, arg0, arg1, ...]
                    // GetMethod peeks at obj and pushes method on top
                    self.compile_expr(obj)?;
                    let key_u16 = crate::unicode::utf8_to_utf16(method_name);
                    let name_idx = self.chunk.add_constant(Value::String(key_u16));
                    self.chunk.write_opcode(Opcode::GetMethod);
                    self.chunk.write_u16(name_idx);
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.chunk.write_opcode(Opcode::Call);
                    // arg_count + 1 signals method call (the +128 flag)
                    self.chunk.write_byte(args.len() as u8 | 0x80);
                } else {
                    // Regular function call
                    self.compile_expr(callee)?;
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.chunk.write_opcode(Opcode::Call);
                    self.chunk.write_byte(args.len() as u8);
                }
            }
            Expr::This => {
                self.chunk.write_opcode(Opcode::GetThis);
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
            // Logical operators (short-circuit) — must return actual operand values
            Expr::LogicalAnd(left, right) => {
                // left && right: if left is falsy, return left; else return right
                self.compile_expr(left)?;
                self.chunk.write_opcode(Opcode::Dup);
                let end_jump = self.emit_jump(Opcode::JumpIfFalse);
                // Left was truthy: discard it, evaluate right
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(right)?;
                self.patch_jump(end_jump);
                // If left was falsy, the dup'd left value remains on stack
            }
            Expr::LogicalOr(left, right) => {
                // left || right: if left is truthy, return left; else return right
                self.compile_expr(left)?;
                self.chunk.write_opcode(Opcode::Dup);
                let end_jump = self.emit_jump(Opcode::JumpIfTrue);
                // Left was falsy: discard it, evaluate right
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(right)?;
                self.patch_jump(end_jump);
            }
            // Array literal: [a, b, c]
            Expr::Array(elements) => {
                for elem in elements {
                    if let Some(e) = elem {
                        self.compile_expr(e)?;
                    } else {
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                    }
                }
                self.chunk.write_opcode(Opcode::NewArray);
                self.chunk.write_byte(elements.len() as u8);
            }
            // Object literal: { key: val, ... }
            Expr::Object(props) => {
                let mut count = 0u8;
                for (key, val, _computed, _shorthand) in props {
                    self.compile_expr(key)?;
                    self.compile_expr(val)?;
                    count += 1;
                }
                self.chunk.write_opcode(Opcode::NewObject);
                self.chunk.write_byte(count);
            }
            // Property access: obj.key
            Expr::Property(obj, key) => {
                self.compile_expr(obj)?;
                let key_u16 = crate::unicode::utf8_to_utf16(key);
                let name_idx = self.chunk.add_constant(Value::String(key_u16));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(name_idx);
            }
            // Index access: obj[expr]
            Expr::Index(obj, index) => {
                self.compile_expr(obj)?;
                self.compile_expr(index)?;
                self.chunk.write_opcode(Opcode::GetIndex);
            }
            // Prefix increment: ++x
            Expr::Increment(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Increment);
                // Write back
                self.compile_store(inner)?;
            }
            // Prefix decrement: --x
            Expr::Decrement(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Decrement);
                self.compile_store(inner)?;
            }
            // Postfix increment: x++
            // Returns ToNumber(old_value), stores ToNumber(old_value)+1
            Expr::PostIncrement(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::ToNumber);
                self.chunk.write_opcode(Opcode::Dup);
                self.chunk.write_opcode(Opcode::Increment);
                self.compile_store(inner)?;
                self.chunk.write_opcode(Opcode::Pop);
            }
            // Postfix decrement: x--
            Expr::PostDecrement(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::ToNumber);
                self.chunk.write_opcode(Opcode::Dup);
                self.chunk.write_opcode(Opcode::Decrement);
                self.compile_store(inner)?;
                self.chunk.write_opcode(Opcode::Pop);
            }
            // Assignment to property: obj.key = val, obj[i] = val
            Expr::Assign(left, right) => match &**left {
                Expr::Var(name, ..) => {
                    self.compile_expr(right)?;
                    if let Some(pos) = self.locals.iter().position(|l| l == name) {
                        self.chunk.write_opcode(Opcode::SetLocal);
                        self.chunk.write_byte(pos as u8);
                    } else {
                        let name_u16 = crate::unicode::utf8_to_utf16(name);
                        let name_idx = self.chunk.add_constant(Value::String(name_u16));
                        self.chunk.write_opcode(Opcode::SetGlobal);
                        self.chunk.write_u16(name_idx);
                    }
                }
                Expr::Property(obj, key) => {
                    self.compile_expr(obj)?;
                    self.compile_expr(right)?;
                    let key_u16 = crate::unicode::utf8_to_utf16(key);
                    let name_idx = self.chunk.add_constant(Value::String(key_u16));
                    self.chunk.write_opcode(Opcode::SetProperty);
                    self.chunk.write_u16(name_idx);
                }
                Expr::Index(obj, idx) => {
                    self.compile_expr(obj)?;
                    self.compile_expr(idx)?;
                    self.compile_expr(right)?;
                    self.chunk.write_opcode(Opcode::SetIndex);
                }
                _ => {
                    return Err(crate::raise_syntax_error!("Invalid assignment target for VM"));
                }
            },
            // Arrow function: (params) => body
            Expr::ArrowFunction(params, body) => {
                let jump_over = self.emit_jump(Opcode::Jump);
                let func_ip = self.chunk.code.len();

                let old_locals = std::mem::take(&mut self.locals);
                let old_depth = self.scope_depth;
                let old_loops = std::mem::take(&mut self.loop_stack);
                self.scope_depth = 1;
                for param in params {
                    if let DestructuringElement::Variable(param_name, _) = param {
                        self.locals.push(param_name.clone());
                    }
                }

                if body.len() == 1 {
                    if let StatementKind::Expr(expr) = &*body[0].kind {
                        // Single expression body: implicitly return the value
                        self.compile_expr(expr)?;
                        self.chunk.write_opcode(Opcode::Return);
                    } else {
                        self.compile_statement(&body[0], true)?;
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                        self.chunk.write_opcode(Opcode::Return);
                    }
                } else {
                    for (i, s) in body.iter().enumerate() {
                        self.compile_statement(s, i == body.len() - 1)?;
                    }
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.chunk.write_opcode(Opcode::Return);
                }

                self.patch_jump(jump_over);
                self.locals = old_locals;
                self.scope_depth = old_depth;
                self.loop_stack = old_loops;

                let func_val = Value::VmFunction(func_ip, params.len() as u8);
                let func_idx = self.chunk.add_constant(func_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(func_idx);
            }
            // Anonymous function expression: function(params) { body }
            Expr::Function(_name, params, body) => {
                self.compile_function_body(params, body)?;
            }
            // new Constructor(args) — for now, special-case Error
            Expr::New(constructor, args) => {
                if let Expr::Var(name, ..) = &**constructor {
                    match name.as_str() {
                        "Error" | "TypeError" | "SyntaxError" | "RangeError" | "ReferenceError" => {
                            // Push error type name
                            let type_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(name)));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(type_idx);
                            // Push message
                            if let Some(arg) = args.first() {
                                self.compile_expr(arg)?;
                            } else {
                                let idx = self.chunk.add_constant(Value::String(Vec::new()));
                                self.chunk.write_opcode(Opcode::Constant);
                                self.chunk.write_u16(idx);
                            }
                            self.chunk.write_opcode(Opcode::NewError);
                        }
                        "Array" => {
                            // new Array("a","b","c") → NewArray
                            for a in args {
                                self.compile_expr(a)?;
                            }
                            self.chunk.write_opcode(Opcode::NewArray);
                            self.chunk.write_byte(args.len() as u8);
                        }
                        "Object" | "Number" | "Boolean" | "String" | "Date" => {
                            // Create typed wrapper: { __type__: "TypeName", __value__: arg }
                            let type_key = crate::unicode::utf8_to_utf16("__type__");
                            let type_key_idx = self.chunk.add_constant(Value::String(type_key));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(type_key_idx);
                            let type_val = crate::unicode::utf8_to_utf16(name);
                            let type_val_idx = self.chunk.add_constant(Value::String(type_val));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(type_val_idx);
                            if let Some(first_arg) = args.first() {
                                let val_key = crate::unicode::utf8_to_utf16("__value__");
                                let val_key_idx = self.chunk.add_constant(Value::String(val_key));
                                self.chunk.write_opcode(Opcode::Constant);
                                self.chunk.write_u16(val_key_idx);
                                self.compile_expr(first_arg)?;
                                self.chunk.write_opcode(Opcode::NewObject);
                                self.chunk.write_byte(2); // 2 key-value pairs
                            } else {
                                self.chunk.write_opcode(Opcode::NewObject);
                                self.chunk.write_byte(1); // 1 key-value pair
                            }
                        }
                        "Function" => {
                            // new Function(body) → compile to: push native_fn, push body, Call(1)
                            let fn_idx = self.chunk.add_constant(Value::VmNativeFunction(72));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(fn_idx);
                            if let Some(body_arg) = args.last() {
                                self.compile_expr(body_arg)?;
                            } else {
                                let idx = self.chunk.add_constant(Value::String(Vec::new()));
                                self.chunk.write_opcode(Opcode::Constant);
                                self.chunk.write_u16(idx);
                            }
                            self.chunk.write_opcode(Opcode::Call);
                            self.chunk.write_byte(1);
                        }
                        _ => {
                            // Generic constructor: create object, call constructor with this
                            self.compile_expr(constructor)?;
                            for a in args {
                                self.compile_expr(a)?;
                            }
                            self.chunk.write_opcode(Opcode::NewCall);
                            self.chunk.write_byte(args.len() as u8);
                        }
                    }
                } else {
                    // Dynamic constructor: create object, call constructor with this
                    self.compile_expr(constructor)?;
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    self.chunk.write_opcode(Opcode::NewCall);
                    self.chunk.write_byte(args.len() as u8);
                }
            }
            // Compound assignment: x += rhs
            Expr::AddAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Add);
                self.compile_store(lhs)?;
            }
            Expr::SubAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Sub);
                self.compile_store(lhs)?;
            }
            Expr::MulAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Mul);
                self.compile_store(lhs)?;
            }
            Expr::DivAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Div);
                self.compile_store(lhs)?;
            }
            Expr::ModAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Mod);
                self.compile_store(lhs)?;
            }
            // Ternary: cond ? a : b
            Expr::Conditional(cond, then_expr, else_expr) => {
                self.compile_expr(cond)?;
                let else_jump = self.emit_jump(Opcode::JumpIfFalse);
                self.compile_expr(then_expr)?;
                let end_jump = self.emit_jump(Opcode::Jump);
                self.patch_jump(else_jump);
                self.compile_expr(else_expr)?;
                self.patch_jump(end_jump);
            }
            // Comma: (a, b) → evaluate a (discard), evaluate b (keep)
            Expr::Comma(left, right) => {
                self.compile_expr(left)?;
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(right)?;
            }
            // Delete operator
            Expr::Delete(inner) => {
                match &**inner {
                    Expr::Var(..) => {
                        // delete variable → SyntaxError in strict mode
                        // Emit: push SyntaxError type name, push message, NewError, Throw
                        let type_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("SyntaxError")));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(type_idx);
                        let msg_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(
                            "Delete of an unqualified identifier in strict mode.",
                        )));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(msg_idx);
                        self.chunk.write_opcode(Opcode::NewError);
                        self.chunk.write_opcode(Opcode::Throw);
                    }
                    Expr::Property(obj, key) => {
                        self.compile_expr(obj)?;
                        let key_u16 = crate::unicode::utf8_to_utf16(key);
                        let name_idx = self.chunk.add_constant(Value::String(key_u16));
                        self.chunk.write_opcode(Opcode::DeleteProperty);
                        self.chunk.write_u16(name_idx);
                    }
                    Expr::Index(obj, idx_expr) => {
                        self.compile_expr(obj)?;
                        self.compile_expr(idx_expr)?;
                        self.chunk.write_opcode(Opcode::DeleteIndex);
                    }
                    _ => {
                        self.compile_expr(inner)?;
                        self.chunk.write_opcode(Opcode::Pop);
                        let idx = self.chunk.add_constant(Value::Boolean(true));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                    }
                }
            }
            // Void operator: evaluate expression, discard, push undefined
            Expr::Void(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Pop);
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            // Nullish coalescing: a ?? b
            Expr::NullishCoalescing(left, right) => {
                // If left is null/undefined, return right; else return left
                self.compile_expr(left)?;
                self.chunk.write_opcode(Opcode::Dup);
                // Check if value is null or undefined: dup, typeof, compare to "undefined", or use IsNullish
                // Simpler: dup, push null, equal → if null jump; dup, push undefined, equal → if undef jump
                // Even simpler: just use Dup + JumpIfFalse pattern but also jump on null...
                // Best approach: dup, check null; if not null dup check undefined; if neither, keep left
                // Actually the simplest: since null and undefined are both falsy, but 0/"" are also falsy,
                // we need a proper nullish check. Let's inline it:
                // push null, Equal → if true jump to rhs
                // else dup original, push undefined, Equal → if true jump to rhs
                // Hmm, we already consumed the dup. Let me use a different approach:
                // eval left → dup → dup → push null → equal → jumpIfTrue(use_right)
                //                          → push undefined → equal → jumpIfTrue(use_right)
                //                          → jump(end) → use_right: pop → eval right → end:
                self.chunk.write_opcode(Opcode::Dup);
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_null = self.emit_jump(Opcode::JumpIfTrue);
                // Not null — check undefined
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                // Not nullish — keep left, jump to end
                let end_jump = self.emit_jump(Opcode::Jump);
                // is_null / is_undef: pop left value, evaluate right
                self.patch_jump(is_null);
                self.patch_jump(is_undef);
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(right)?;
                self.patch_jump(end_jump);
            }
            // Optional chaining: obj?.prop
            Expr::OptionalProperty(obj, key) => {
                self.compile_expr(obj)?;
                self.chunk.write_opcode(Opcode::Dup);
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_null = self.emit_jump(Opcode::JumpIfTrue);
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                // Not nullish: do property access
                let key_u16 = crate::unicode::utf8_to_utf16(key);
                let name_idx = self.chunk.add_constant(Value::String(key_u16));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(name_idx);
                let end_jump = self.emit_jump(Opcode::Jump);
                // Nullish: pop obj, push undefined
                self.patch_jump(is_null);
                self.patch_jump(is_undef);
                self.chunk.write_opcode(Opcode::Pop);
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.patch_jump(end_jump);
            }
            // Optional index: obj?.[expr]
            Expr::OptionalIndex(obj, index_expr) => {
                self.compile_expr(obj)?;
                self.chunk.write_opcode(Opcode::Dup);
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_null = self.emit_jump(Opcode::JumpIfTrue);
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                self.compile_expr(index_expr)?;
                self.chunk.write_opcode(Opcode::GetIndex);
                let end_jump = self.emit_jump(Opcode::Jump);
                self.patch_jump(is_null);
                self.patch_jump(is_undef);
                self.chunk.write_opcode(Opcode::Pop);
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.patch_jump(end_jump);
            }
            // Optional call: fn?.()
            Expr::OptionalCall(callee, args) => {
                self.compile_expr(callee)?;
                self.chunk.write_opcode(Opcode::Dup);
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_null = self.emit_jump(Opcode::JumpIfTrue);
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.write_opcode(Opcode::Call);
                self.chunk.write_byte(args.len() as u8);
                let end_jump = self.emit_jump(Opcode::Jump);
                self.patch_jump(is_null);
                self.patch_jump(is_undef);
                self.chunk.write_opcode(Opcode::Pop);
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.patch_jump(end_jump);
            }
            // Getter/Setter in object literal: compile as the inner function
            Expr::Getter(inner) | Expr::Setter(inner) => {
                self.compile_expr(inner)?;
            }
            _ => return Err(raise_syntax_error!(format!("Unimplemented expression type for VM: {expr:?}"))),
        }
        Ok(())
    }

    /// Write-back helper for increment/decrement: store the top-of-stack value
    /// back into the variable that `expr` represents.
    fn compile_store(&mut self, expr: &Expr) -> Result<(), JSError> {
        match expr {
            Expr::Var(name, ..) => {
                if let Some(pos) = self.locals.iter().position(|l| l == name) {
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::SetGlobal);
                    self.chunk.write_u16(name_idx);
                }
            }
            Expr::Property(obj, key) => {
                // Stack has: [..., new_val]
                // Need: push obj, swap so stack = [..., obj, new_val], SetProperty
                self.compile_expr(obj)?;
                self.chunk.write_opcode(Opcode::Swap);
                let key_u16 = crate::unicode::utf8_to_utf16(key);
                let key_idx = self.chunk.add_constant(Value::String(key_u16));
                self.chunk.write_opcode(Opcode::SetProperty);
                self.chunk.write_u16(key_idx);
            }
            Expr::Index(_obj, _idx) => {
                // Index store needs 3-way rotate which is complex
                // For now, fall through to error — handle inline in inc/dec if needed
                return Err(crate::raise_syntax_error!("Index increment/decrement not yet supported in VM"));
            }
            _ => {
                return Err(crate::raise_syntax_error!("Invalid increment/decrement target for VM"));
            }
        }
        Ok(())
    }

    /// Emit get for a synthetic local/global variable
    fn emit_helper_get(&mut self, name: &str) {
        if self.scope_depth > 0 {
            let pos = self.locals.iter().position(|l| l == name).unwrap();
            self.chunk.write_opcode(Opcode::GetLocal);
            self.chunk.write_byte(pos as u8);
        } else {
            let n = crate::unicode::utf8_to_utf16(name);
            let ni = self.chunk.add_constant(Value::String(n));
            self.chunk.write_opcode(Opcode::GetGlobal);
            self.chunk.write_u16(ni);
        }
    }

    /// Emit set for a synthetic local/global variable (value already on TOS)
    fn emit_helper_set(&mut self, name: &str) {
        if self.scope_depth > 0 {
            let pos = self.locals.iter().position(|l| l == name).unwrap();
            self.chunk.write_opcode(Opcode::SetLocal);
            self.chunk.write_byte(pos as u8);
        } else {
            let n = crate::unicode::utf8_to_utf16(name);
            let ni = self.chunk.add_constant(Value::String(n));
            self.chunk.write_opcode(Opcode::SetGlobal);
            self.chunk.write_u16(ni);
        }
    }

    /// Compile a function body (shared between FunctionDeclaration, ArrowFunction, and Function expression)
    fn compile_function_body(&mut self, params: &[DestructuringElement], body: &[Statement]) -> Result<(), JSError> {
        let jump_over = self.emit_jump(Opcode::Jump);
        let func_ip = self.chunk.code.len();

        let old_locals = std::mem::take(&mut self.locals);
        let old_depth = self.scope_depth;
        let old_loops = std::mem::take(&mut self.loop_stack);
        let old_label = self.pending_label.take();
        self.scope_depth = 1;
        for param in params {
            if let DestructuringElement::Variable(param_name, _) = param {
                self.locals.push(param_name.clone());
            }
        }

        for (i, s) in body.iter().enumerate() {
            self.compile_statement(s, i == body.len() - 1)?;
        }

        let idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(idx);
        self.chunk.write_opcode(Opcode::Return);

        self.patch_jump(jump_over);
        self.locals = old_locals;
        self.scope_depth = old_depth;
        self.loop_stack = old_loops;
        self.pending_label = old_label;

        let func_val = Value::VmFunction(func_ip, params.len() as u8);
        let func_idx = self.chunk.add_constant(func_val);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(func_idx);
        Ok(())
    }
}

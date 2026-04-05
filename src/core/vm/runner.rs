use super::*;
use crate::core::PRIVATE_KEY_PREFIX;

enum PrivateKind {
    Field,
    Method,
    AccessorWithSetter,
    GetterOnly,
    NotFound,
}

/// Result of executing a single opcode.
/// `Continue` means the VM loop keeps running.
/// `Exit` means `run_inner` should return the contained value.
enum OpcodeAction<'gc> {
    Continue,
    Exit(Value<'gc>),
}

impl<'gc> VM<'gc> {
    /// Core execution loop of the VM (Fetch-Decode-Execute)
    pub fn run(&mut self, ctx: &GcContext<'gc>) -> Result<Value<'gc>, JSError> {
        let result = self.run_inner(ctx, 0)?;
        self.drain_microtasks(ctx);
        if let Some(thrown) = self.pending_throw.take() {
            return Err(self.vm_error_to_js_error(ctx, &thrown));
        }
        self.drain_timers(ctx)?;
        if let Some(thrown) = self.pending_throw.take() {
            return Err(self.vm_error_to_js_error(ctx, &thrown));
        }
        self.collect_garbage(ctx);
        Ok(result)
    }

    /// Execute VM until frames drop below `min_depth` or top-level returns
    pub(crate) fn run_inner(&mut self, ctx: &GcContext<'gc>, min_depth: usize) -> Result<Value<'gc>, JSError> {
        loop {
            // Check for pending throw (e.g. from generator .throw())
            if let Some(thrown) = self.pending_throw.take() {
                self.handle_throw(ctx, &thrown)?;
                // handle_throw already truncated the stack; no opcode handler
                // follows, so clear the depth marker immediately.
                self.throw_caught_stack_depth = None;
                continue;
            }
            // Fetch instruction
            self.current_opcode_ip = self.ip;
            let instruction_byte = self.read_byte();
            let instruction = Opcode::try_from(instruction_byte)?;

            // Execute action based on instruction
            let action = match instruction {
                Opcode::Return => self.run_opcode_return(ctx, min_depth)?,
                Opcode::Yield => self.run_opcode_yield(ctx)?,
                Opcode::GeneratorParamInitDone => self.run_opcode_generator_param_init_done(ctx)?,
                Opcode::Await => self.run_opcode_await(ctx)?,
                Opcode::GetLocal => self.run_opcode_get_local(ctx)?,
                Opcode::SetLocal => self.run_opcode_set_local(ctx)?,
                Opcode::Call => self.run_opcode_call(ctx)?,
                Opcode::Constant => self.run_opcode_constant(ctx)?,
                Opcode::Pop => self.run_opcode_pop(ctx)?,
                Opcode::DefineGlobal => self.run_opcode_define_global(ctx)?,
                Opcode::DefineGlobalSoft => self.run_opcode_define_global_soft(ctx)?,
                Opcode::ThrowIfNullish => self.run_opcode_throw_if_nullish(ctx)?,
                Opcode::DefineGlobalConst => self.run_opcode_define_global_const(ctx)?,
                Opcode::GetNewTarget => self.run_opcode_get_new_target(ctx)?,
                Opcode::GetGlobal => self.run_opcode_get_global(ctx)?,
                Opcode::GetArguments => self.run_opcode_get_arguments(ctx)?,
                Opcode::SetGlobal => self.run_opcode_set_global(ctx)?,
                Opcode::Jump => self.run_opcode_jump(ctx)?,
                Opcode::JumpIfFalse => self.run_opcode_jump_if_false(ctx)?,
                Opcode::Add => self.run_opcode_add(ctx)?,
                Opcode::Sub => self.run_opcode_sub(ctx)?,
                Opcode::Mul => self.run_opcode_mul(ctx)?,
                Opcode::Div => self.run_opcode_div(ctx)?,
                Opcode::LessThan => self.run_opcode_less_than(ctx)?,
                Opcode::GreaterThan => self.run_opcode_greater_than(ctx)?,
                Opcode::Equal => self.run_opcode_equal(ctx)?,
                Opcode::NotEqual => self.run_opcode_not_equal(ctx)?,
                Opcode::StrictNotEqual => self.run_opcode_strict_not_equal(ctx)?,
                Opcode::LessEqual => self.run_opcode_less_equal(ctx)?,
                Opcode::GreaterEqual => self.run_opcode_greater_equal(ctx)?,
                Opcode::Mod => self.run_opcode_mod(ctx)?,
                Opcode::Pow => self.run_opcode_pow(ctx)?,
                Opcode::BitwiseAnd => self.run_opcode_bitwise_and(ctx)?,
                Opcode::BitwiseOr => self.run_opcode_bitwise_or(ctx)?,
                Opcode::BitwiseXor => self.run_opcode_bitwise_xor(ctx)?,
                Opcode::ShiftLeft => self.run_opcode_shift_left(ctx)?,
                Opcode::ShiftRight => self.run_opcode_shift_right(ctx)?,
                Opcode::UnsignedShiftRight => self.run_opcode_unsigned_shift_right(ctx)?,
                Opcode::BitwiseNot => self.run_opcode_bitwise_not(ctx)?,
                Opcode::ArrayPush => self.run_opcode_array_push(ctx)?,
                Opcode::ArrayHole => self.run_opcode_array_hole(ctx)?,
                Opcode::ArraySpread => self.run_opcode_array_spread(ctx)?,
                Opcode::CallSpread => self.run_opcode_call_spread(ctx)?,
                Opcode::NewCallSpread => self.run_opcode_new_call_spread(ctx)?,
                Opcode::ObjectSpread => self.run_opcode_object_spread(ctx)?,
                Opcode::ObjectSpreadExcluding => self.run_opcode_object_spread_excluding(ctx)?,
                Opcode::ValidateClassHeritage => self.run_opcode_validate_class_heritage(ctx)?,
                Opcode::GetUpvalue => self.run_opcode_get_upvalue(ctx)?,
                Opcode::SetUpvalue => self.run_opcode_set_upvalue(ctx)?,
                Opcode::MakeClosure => self.run_opcode_make_closure(ctx)?,
                Opcode::Negate => self.run_opcode_negate(ctx)?,
                Opcode::Not => self.run_opcode_not(ctx)?,
                Opcode::TypeOf => self.run_opcode_type_of(ctx)?,
                Opcode::TypeOfGlobal => self.run_opcode_type_of_global(ctx)?,
                Opcode::DeleteGlobal => self.run_opcode_delete_global(ctx)?,
                Opcode::JumpIfTrue => self.run_opcode_jump_if_true(ctx)?,
                Opcode::NewArray => self.run_opcode_new_array(ctx)?,
                Opcode::NewObject => self.run_opcode_new_object(ctx)?,
                Opcode::GetProperty => self.run_opcode_get_property(ctx)?,
                Opcode::SetProperty => self.run_opcode_set_property(ctx)?,
                Opcode::InitProperty => self.run_opcode_init_property(ctx)?,
                Opcode::SetSuperProperty => self.run_opcode_set_super_property(ctx)?,
                Opcode::SetSuperPropertyComputed => self.run_opcode_set_super_property_computed(ctx)?,
                Opcode::DefineComputedMethod => self.run_opcode_define_computed_method(ctx)?,
                Opcode::GetSuperProperty => self.run_opcode_get_super_property(ctx)?,
                Opcode::GetSuperPropertyComputed => self.run_opcode_get_super_property_computed(ctx)?,
                Opcode::GetIndex => self.run_opcode_get_index(ctx)?,
                Opcode::SetIndex => self.run_opcode_set_index(ctx)?,
                Opcode::InitIndex => self.run_opcode_init_index(ctx)?,
                Opcode::SetComputedGetter => self.run_opcode_set_computed_getter(ctx)?,
                Opcode::SetComputedSetter => self.run_opcode_set_computed_setter(ctx)?,
                Opcode::ToPropertyKey => self.run_opcode_to_property_key(ctx)?,
                Opcode::Increment => self.run_opcode_increment(ctx)?,
                Opcode::Decrement => self.run_opcode_decrement(ctx)?,
                Opcode::Throw => self.run_opcode_throw(ctx)?,
                Opcode::ThrowTypeError => self.run_opcode_throw_type_error(ctx)?,
                Opcode::SetupTry => self.run_opcode_setup_try(ctx)?,
                Opcode::TeardownTry => self.run_opcode_teardown_try(ctx)?,
                Opcode::GetThis => self.run_opcode_get_this(ctx)?,
                Opcode::GetThisSuper => self.run_opcode_get_this_super(ctx)?,
                Opcode::ClearThisTdz => self.run_opcode_clear_this_tdz(ctx)?,
                Opcode::ValidateProtoValue => self.run_opcode_validate_proto_value(ctx)?,
                Opcode::GetKeys => self.run_opcode_get_keys(ctx)?,
                Opcode::GetMethod => self.run_opcode_get_method(ctx)?,
                Opcode::NewError => self.run_opcode_new_error(ctx)?,
                Opcode::Dup => self.run_opcode_dup(ctx)?,
                Opcode::Swap => self.run_opcode_swap(ctx)?,
                Opcode::ToNumber => self.run_opcode_to_number(ctx)?,
                Opcode::ToNumeric => self.run_opcode_to_numeric(ctx)?,
                Opcode::CollectRest => self.run_opcode_collect_rest(ctx)?,
                Opcode::In => self.run_opcode_in(ctx)?,
                Opcode::InstanceOf => self.run_opcode_instanceof(ctx)?,
                Opcode::DeleteProperty => self.run_opcode_delete_property(ctx)?,
                Opcode::NewCall => self.run_opcode_new_call(ctx)?,
                Opcode::DeleteIndex => self.run_opcode_delete_index(ctx)?,
                Opcode::EnterFieldInit => self.run_opcode_enter_field_init(ctx)?,
                Opcode::LeaveFieldInit => self.run_opcode_leave_field_init(ctx)?,
                Opcode::AllocBrand => self.run_opcode_alloc_brand(ctx)?,
                Opcode::ResetPrototype => self.run_opcode_reset_prototype(ctx)?,
                Opcode::IteratorClose => self.run_opcode_iterator_close(ctx)?,
                Opcode::IteratorCloseAbrupt => self.run_opcode_iterator_close_abrupt(ctx)?,
                Opcode::AssertIterResult => self.run_opcode_assert_iter_result(ctx)?,
                Opcode::BoxLocal => self.run_opcode_box_local(ctx)?,
                Opcode::InitNamedFnSelf => self.run_opcode_init_named_fn_self(ctx)?,
            };
            // If a throw was caught by handle_throw during this opcode, the
            // handler may have pushed extra values onto the stack afterwards.
            // Re-truncate to the depth recorded by handle_throw so the catch
            // body starts with a clean stack.
            if let Some(depth) = self.throw_caught_stack_depth.take() {
                self.stack.truncate(depth);
                continue;
            }
            if let OpcodeAction::Exit(val) = action {
                return Ok(val);
            }
        }
    }

    // Opcode::Return
    fn run_opcode_return(&mut self, ctx: &GcContext<'gc>, min_depth: usize) -> Result<OpcodeAction<'gc>, JSError> {
        let result = self.stack.pop().unwrap_or(Value::Undefined);
        if let Some(frame) = self.frames.pop() {
            if self.chunk.async_function_ips.contains(&frame.func_ip)
                && self.chunk.generator_function_ips.contains(&frame.func_ip)
                && let Value::VmArray(arr) = &result
            {
                let arity = self
                    .chunk
                    .constants
                    .iter()
                    .find_map(|c| match c {
                        Value::VmFunction(ip, a) if *ip == frame.func_ip => Some(*a),
                        Value::VmClosure(ip, a, _) if *ip == frame.func_ip => Some(*a),
                        _ => None,
                    })
                    .unwrap_or(0);
                if let Some(proto) = self.get_fn_props(ctx, frame.func_ip, arity).borrow().get("prototype").cloned() {
                    arr.borrow_mut(ctx).props.insert("__proto__".to_string(), proto);
                }
            }
            if frame.is_method {
                self.this_stack.pop();
            }
            // Pop any try frames that belonged to the returning function.
            let current_depth = self.frames.len();
            while self.try_stack.last().is_some_and(|tf| tf.frame_depth > current_depth) {
                self.try_stack.pop();
            }
            self.stack.truncate(frame.bp - 1);
            self.ip = frame.return_ip;
            if self.frames.len() < min_depth {
                // Returning from an injected call
                return Ok(OpcodeAction::Exit(result));
            }
            // Returning from a function call: pop locals and the function itself
            self.stack.push(result);
        } else {
            // Return from top-level script
            return Ok(OpcodeAction::Exit(result));
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Yield
    fn run_opcode_yield(&mut self, _ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Generator yield: store the yielded value and exit run_inner.
        // resume_generator() will check generator_yield_value to detect this.
        let yielded = self.stack.pop().unwrap_or(Value::Undefined);
        self.generator_yield_value = Some(yielded);
        Ok(OpcodeAction::Exit(Value::Undefined))
    }

    // Opcode::GeneratorParamInitDone
    fn run_opcode_generator_param_init_done(&mut self, _ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        self.generator_param_init_done = true;
        Ok(OpcodeAction::Exit(Value::Undefined))
    }

    // Opcode::Await
    fn run_opcode_await(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let awaited = self.stack.pop().unwrap_or(Value::Undefined);
        let current_frame = self.frames.last().cloned();
        let is_suspendable_async = current_frame.as_ref().is_some_and(|frame| {
            self.chunk.async_function_ips.contains(&frame.func_ip) && !self.chunk.generator_function_ips.contains(&frame.func_ip)
        });

        if !is_suspendable_async || self.active_async_promises.is_empty() {
            let awaited_value = self.call_host_fn(ctx, "promise.await", None, std::slice::from_ref(&awaited));
            if let Some(thrown) = self.pending_throw.take() {
                self.handle_throw(ctx, &thrown)?;
            } else {
                self.stack.push(awaited_value);
            }
            return Ok(OpcodeAction::Continue);
        }

        let frame = current_frame.expect("async await requires an active frame");
        let promise = self.call_builtin(ctx, BUILTIN_PROMISE_RESOLVE, std::slice::from_ref(&awaited));
        let outer_promise = self.active_async_promises.last().cloned().unwrap_or(Value::Undefined);
        let state_id = self.next_async_function_id;
        self.next_async_function_id += 1;
        let frame_depth = self.frames.len();

        let locals = self.stack[frame.bp..].to_vec();
        let this_val = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
        let try_stack = self
            .try_stack
            .iter()
            .filter(|try_frame| try_frame.frame_depth >= frame_depth && try_frame.stack_depth >= frame.bp)
            .map(|try_frame| TryFrame {
                catch_ip: try_frame.catch_ip,
                stack_depth: try_frame.stack_depth - frame.bp,
                frame_depth: try_frame.frame_depth - frame_depth,
                catch_binding: try_frame.catch_binding.clone(),
            })
            .collect();
        self.async_function_states.insert(
            state_id,
            AsyncFunctionState {
                ip: self.ip,
                frame,
                locals,
                try_stack,
                this_val,
                promise: outer_promise,
            },
        );

        let mut token = IndexMap::new();
        token.insert("__async_suspend_id__".to_string(), Value::Number(state_id as f64));
        let token_value = Value::VmObject(new_gc_cell_ptr(ctx, token));
        let on_fulfilled = Self::make_bound_host_fn(ctx, "async.resume.fulfill", &token_value);
        let on_rejected = Self::make_bound_host_fn(ctx, "async.resume.reject", &token_value);
        let _ = self.call_method_builtin(ctx, BUILTIN_PROMISE_THEN, &promise, &[on_fulfilled, on_rejected]);
        self.pending_async_suspend = Some(state_id);
        Ok(OpcodeAction::Exit(Value::Undefined))
    }

    // Opcode::GetLocal
    fn run_opcode_get_local(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let index = self.read_byte() as usize;
        let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
        if bp + index >= self.stack.len() {
            if self.frames.is_empty() {
                if let Some(local_names) = self.chunk.fn_local_names.get(&0)
                    && let Some(name) = local_names.get(index)
                    && let Some(v) = self.globals.get(name).cloned()
                {
                    self.stack.push(v);
                    return Ok(OpcodeAction::Continue);
                }
                self.stack.push(Value::Undefined);
                return Ok(OpcodeAction::Continue);
            }
            let mut err_map = IndexMap::new();
            err_map.insert("message".to_string(), Value::from("Invalid local access"));
            err_map.insert("__type__".to_string(), Value::from("ReferenceError"));
            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
            return Ok(OpcodeAction::Continue);
        }
        // Check if this local has been captured as an upvalue cell
        let val = if let Some(frame) = self.frames.last() {
            if let Some(cell) = frame.local_cells.get(&index) {
                cell.borrow().clone()
            } else {
                self.stack[bp + index].clone()
            }
        } else if let Some(cell) = self.top_level_cells.get(&index) {
            cell.borrow().clone()
        } else {
            self.stack[bp + index].clone()
        };
        // TDZ check: Uninitialized variables throw ReferenceError
        if matches!(val, Value::Uninitialized) {
            let mut err_map = IndexMap::new();
            err_map.insert("message".to_string(), Value::from("Cannot access variable before initialization"));
            err_map.insert("__type__".to_string(), Value::from("ReferenceError"));
            let err = Value::VmObject(new_gc_cell_ptr(ctx, err_map));
            self.handle_throw(ctx, &err)?;
        } else {
            self.stack.push(val);
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetLocal
    fn run_opcode_set_local(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let index = self.read_byte() as usize;
        let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
        if bp + index >= self.stack.len() {
            self.stack.resize(bp + index + 1, Value::Undefined);
        }
        let val = self.stack.last().expect("VM Stack underflow").clone();
        // Check if this local has been captured as an upvalue cell
        let cell = if let Some(frame) = self.frames.last() {
            frame.local_cells.get(&index).cloned()
        } else {
            self.top_level_cells.get(&index).cloned()
        };
        if let Some(cell) = cell {
            *cell.borrow_mut(ctx) = val;
        } else {
            self.stack[bp + index] = val;
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Call
    fn run_opcode_call(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let raw_arg_byte = self.read_byte();
        let is_method = (raw_arg_byte & 0x80) != 0;
        let is_direct_eval = (raw_arg_byte & 0x40) != 0;
        let encoded_arg_count = (raw_arg_byte & 0x3f) as usize;
        let arg_count = if encoded_arg_count == 0x3f {
            self.read_u16() as usize
        } else {
            encoded_arg_count
        };
        self.direct_eval = is_direct_eval;
        // Stack for method call: [..., receiver, callee, arg0, arg1, ...]
        // Stack for regular call: [..., callee, arg0, arg1, ...]
        let callee_idx = self.stack.len() - arg_count - 1;
        let callee = self.stack[callee_idx].clone();
        match callee {
            Value::VmFunction(target_ip, arity) => {
                if let Some(&realm_id) = self.fn_realm.get(&target_ip)
                    && realm_id < self.child_realms.len()
                    && let Some(mut child) = self.child_realms[realm_id].take()
                {
                    self.sync_runtime_to_child(&mut child);
                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let receiver = if is_method {
                        self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    let local_callee = self.localize_cross_realm_callable(&callee, realm_id);
                    let result = match child.vm_call_function_value(ctx, &local_callee, &receiver, &args_vec) {
                        Ok(result) => self.register_cross_realm_fn(ctx, &mut child, result, realm_id),
                        Err(err) => {
                            let _ = self.child_error_to_parent_pending(ctx, &mut child, err);
                            self.sync_runtime_from_child(&child);
                            self.child_realms[realm_id] = Some(child);
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(ctx, &thrown)?;
                                return Ok(OpcodeAction::Continue);
                            }
                            return Ok(OpcodeAction::Continue);
                        }
                    };
                    self.sync_runtime_from_child(&child);
                    self.child_realms[realm_id] = Some(child);
                    self.stack.push(result);
                    return Ok(OpcodeAction::Continue);
                }
                if self.chunk.async_function_ips.contains(&target_ip) && !self.chunk.generator_function_ips.contains(&target_ip) {
                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let receiver = if is_method {
                        self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    if self.chunk.named_fn_self_ips.contains(&target_ip) {
                        self.named_fn_callee_stack.push(callee.clone());
                    }
                    let promise = self.invoke_async_function(ctx, target_ip, arity, &args_vec, &[], &receiver);
                    self.stack.push(promise);
                    return Ok(OpcodeAction::Continue);
                }
                // Async generator function: create async generator object without running body.
                if self.chunk.generator_function_ips.contains(&target_ip) && self.chunk.async_function_ips.contains(&target_ip) {
                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let this_val = if is_method {
                        self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    if self.chunk.named_fn_self_ips.contains(&target_ip) {
                        self.named_fn_callee_stack.push(callee.clone());
                    }
                    let gen_obj = match self.create_generator_object(ctx, target_ip, arity, &args_vec, &[], &this_val, true) {
                        Ok(obj) => obj,
                        Err(err) => {
                            self.set_pending_throw_from_error(&err);
                            return Ok(OpcodeAction::Continue);
                        }
                    };
                    self.stack.push(gen_obj);
                    return Ok(OpcodeAction::Continue);
                }
                // Generator function: create generator object without running body
                if self.chunk.generator_function_ips.contains(&target_ip) && !self.chunk.async_function_ips.contains(&target_ip) {
                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let this_val = if is_method {
                        self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    if self.chunk.named_fn_self_ips.contains(&target_ip) {
                        self.named_fn_callee_stack.push(callee.clone());
                    }
                    let gen_obj = match self.create_generator_object(ctx, target_ip, arity, &args_vec, &[], &this_val, false) {
                        Ok(obj) => obj,
                        Err(err) => {
                            self.set_pending_throw_from_error(&err);
                            return Ok(OpcodeAction::Continue);
                        }
                    };
                    self.stack.push(gen_obj);
                    return Ok(OpcodeAction::Continue);
                }
                if is_method && self.chunk.class_constructor_ips.contains(&target_ip) {
                    let in_ctor_context = self.frames.iter().any(|f| self.chunk.class_constructor_ips.contains(&f.func_ip));
                    if !in_ctor_context {
                        let receiver = self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined);
                        let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                        let base = callee_idx.saturating_sub(1);
                        self.stack.truncate(base);
                        self.this_stack.push(receiver);
                        let result = self
                            .call_vm_function_result(ctx, target_ip, &args_vec, None, &[])
                            .unwrap_or(Value::Undefined);
                        self.this_stack.pop();
                        self.stack.push(result);

                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::from("ReferenceError"));
                        err_map.insert(
                            "message".to_string(),
                            Value::from("Super constructor may only be called directly in a derived constructor"),
                        );
                        self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                        return Ok(OpcodeAction::Continue);
                    }
                }
                if !is_method && self.chunk.class_constructor_ips.contains(&target_ip) {
                    let err = self.make_type_error_object(ctx, "Class constructor cannot be invoked without 'new'");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                // Pad missing args with Undefined
                if (arg_count as u8) < arity {
                    for _ in 0..(arity as usize - arg_count) {
                        self.stack.push(Value::Undefined);
                    }
                }
                // Preserve full argument list for Arguments object semantics.
                let saved_args = if arg_count > 0 {
                    let first_arg_idx = callee_idx + 1;
                    Some(self.stack[first_arg_idx..first_arg_idx + arg_count].to_vec())
                } else {
                    None
                };
                // Keep full args for `arguments`, but trim stack to declared arity
                // so function-local slots remain aligned.
                if arg_count > arity as usize {
                    let first_arg_idx = callee_idx + 1;
                    let drain_start = first_arg_idx + arity as usize;
                    let drain_end = first_arg_idx + arg_count;
                    self.stack.drain(drain_start..drain_end);
                }
                // For method calls, pop receiver from under callee and bind as this
                if is_method {
                    // Remove receiver (one slot below callee)
                    let receiver = self.stack.remove(callee_idx - 1);
                    self.this_stack.push(receiver);
                    let callee_idx = callee_idx - 1;
                    let derived_tdz = if self.chunk.derived_constructor_ips.contains(&target_ip) {
                        Some(true)
                    } else {
                        None
                    };
                    let frame = CallFrame {
                        return_ip: self.ip,
                        bp: callee_idx + 1,
                        is_method: true,
                        arg_count,
                        func_ip: target_ip,
                        arguments_obj: None,
                        upvalues: Vec::new(),
                        saved_args,
                        local_cells: HashMap::new(),
                        this_tdz: derived_tdz,
                    };
                    self.frames.push(frame);
                } else {
                    // In strict mode, non-method calls get `this = undefined`
                    let fn_strict = self.chunk.fn_strictness.get(&target_ip).copied().unwrap_or(false);
                    let is_arrow = self.chunk.arrow_function_ips.contains(&target_ip);
                    let push_this = !is_arrow;
                    if push_this {
                        if fn_strict {
                            self.this_stack.push(Value::Undefined);
                        } else {
                            self.this_stack.push(Value::VmObject(self.global_this));
                        }
                    }
                    let frame = CallFrame {
                        return_ip: self.ip,
                        bp: callee_idx + 1,
                        is_method: push_this,
                        arg_count,
                        func_ip: target_ip,
                        arguments_obj: None,
                        upvalues: Vec::new(),
                        saved_args,
                        local_cells: HashMap::new(),
                        this_tdz: None,
                    };
                    self.frames.push(frame);
                }
                if self.chunk.named_fn_self_ips.contains(&target_ip) {
                    self.named_fn_callee_stack.push(callee.clone());
                }
                self.ip = target_ip;
            }
            Value::VmClosure(target_ip, arity, ref upvals) => {
                if let Some(&realm_id) = self.fn_realm.get(&target_ip)
                    && realm_id < self.child_realms.len()
                    && let Some(mut child) = self.child_realms[realm_id].take()
                {
                    self.sync_runtime_to_child(&mut child);
                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let receiver = if is_method {
                        self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    let local_callee = self.localize_cross_realm_callable(&callee, realm_id);
                    let result = match child.vm_call_function_value(ctx, &local_callee, &receiver, &args_vec) {
                        Ok(result) => self.register_cross_realm_fn(ctx, &mut child, result, realm_id),
                        Err(err) => {
                            let _ = self.child_error_to_parent_pending(ctx, &mut child, err);
                            self.sync_runtime_from_child(&child);
                            self.child_realms[realm_id] = Some(child);
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(ctx, &thrown)?;
                                return Ok(OpcodeAction::Continue);
                            }
                            return Ok(OpcodeAction::Continue);
                        }
                    };
                    self.sync_runtime_from_child(&child);
                    self.child_realms[realm_id] = Some(child);
                    self.stack.push(result);
                    return Ok(OpcodeAction::Continue);
                }
                if self.chunk.async_function_ips.contains(&target_ip) && !self.chunk.generator_function_ips.contains(&target_ip) {
                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let receiver = if is_method {
                        self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    if self.chunk.named_fn_self_ips.contains(&target_ip) {
                        self.named_fn_callee_stack.push(callee.clone());
                    }
                    let promise = self.invoke_async_function(ctx, target_ip, arity, &args_vec, upvals, &receiver);
                    self.stack.push(promise);
                    return Ok(OpcodeAction::Continue);
                }
                // Async generator closure: create async generator object without running body.
                if self.chunk.generator_function_ips.contains(&target_ip) && self.chunk.async_function_ips.contains(&target_ip) {
                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let this_val = if is_method {
                        self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    if self.chunk.named_fn_self_ips.contains(&target_ip) {
                        self.named_fn_callee_stack.push(callee.clone());
                    }
                    let gen_obj = match self.create_generator_object(ctx, target_ip, arity, &args_vec, upvals, &this_val, true) {
                        Ok(obj) => obj,
                        Err(err) => {
                            self.set_pending_throw_from_error(&err);
                            return Ok(OpcodeAction::Continue);
                        }
                    };
                    self.stack.push(gen_obj);
                    return Ok(OpcodeAction::Continue);
                }
                // Generator closure: create generator object without running body
                if self.chunk.generator_function_ips.contains(&target_ip) && !self.chunk.async_function_ips.contains(&target_ip) {
                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let this_val = if is_method {
                        self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    if self.chunk.named_fn_self_ips.contains(&target_ip) {
                        self.named_fn_callee_stack.push(callee.clone());
                    }
                    let gen_obj = match self.create_generator_object(ctx, target_ip, arity, &args_vec, upvals, &this_val, false) {
                        Ok(obj) => obj,
                        Err(err) => {
                            self.set_pending_throw_from_error(&err);
                            return Ok(OpcodeAction::Continue);
                        }
                    };
                    self.stack.push(gen_obj);
                    return Ok(OpcodeAction::Continue);
                }
                if is_method && self.chunk.class_constructor_ips.contains(&target_ip) {
                    let in_ctor_context = self.frames.iter().any(|f| self.chunk.class_constructor_ips.contains(&f.func_ip));
                    if !in_ctor_context {
                        let receiver = self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined);
                        let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                        let base = callee_idx.saturating_sub(1);
                        self.stack.truncate(base);
                        self.this_stack.push(receiver);
                        let result = self
                            .call_vm_function_result(ctx, target_ip, &args_vec, None, upvals)
                            .unwrap_or(Value::Undefined);
                        self.this_stack.pop();
                        self.stack.push(result);

                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::from("ReferenceError"));
                        err_map.insert(
                            "message".to_string(),
                            Value::from("Super constructor may only be called directly in a derived constructor"),
                        );
                        self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                        return Ok(OpcodeAction::Continue);
                    }
                }
                if !is_method && self.chunk.class_constructor_ips.contains(&target_ip) {
                    let err = self.make_type_error_object(ctx, "Class constructor cannot be invoked without 'new'");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                if (arg_count as u8) < arity {
                    for _ in 0..(arity as usize - arg_count) {
                        self.stack.push(Value::Undefined);
                    }
                }
                // Preserve full argument list for Arguments object semantics.
                let saved_args = if arg_count > 0 {
                    let first_arg_idx = callee_idx + 1;
                    Some(self.stack[first_arg_idx..first_arg_idx + arg_count].to_vec())
                } else {
                    None
                };
                // Keep full args for `arguments`, but trim stack to declared arity
                // so function-local slots remain aligned.
                if arg_count > arity as usize {
                    let first_arg_idx = callee_idx + 1;
                    let drain_start = first_arg_idx + arity as usize;
                    let drain_end = first_arg_idx + arg_count;
                    self.stack.drain(drain_start..drain_end);
                }
                let closure_upvalues = (**upvals).clone();
                if is_method {
                    let receiver = self.stack.remove(callee_idx - 1);
                    self.this_stack.push(receiver);
                    let callee_idx = callee_idx - 1;
                    let derived_tdz = if self.chunk.derived_constructor_ips.contains(&target_ip) {
                        Some(true)
                    } else {
                        None
                    };
                    let frame = CallFrame {
                        return_ip: self.ip,
                        bp: callee_idx + 1,
                        is_method: true,
                        arg_count,
                        func_ip: target_ip,
                        arguments_obj: None,
                        upvalues: closure_upvalues,
                        saved_args,
                        local_cells: HashMap::new(),
                        this_tdz: derived_tdz,
                    };
                    self.frames.push(frame);
                } else {
                    let fn_strict = self.chunk.fn_strictness.get(&target_ip).copied().unwrap_or(false);
                    let is_arrow = self.chunk.arrow_function_ips.contains(&target_ip);
                    let push_this = !is_arrow;
                    if push_this {
                        if fn_strict {
                            self.this_stack.push(Value::Undefined);
                        } else {
                            self.this_stack.push(Value::VmObject(self.global_this));
                        }
                    }
                    let frame = CallFrame {
                        return_ip: self.ip,
                        bp: callee_idx + 1,
                        is_method: push_this,
                        arg_count,
                        func_ip: target_ip,
                        arguments_obj: None,
                        upvalues: closure_upvalues,
                        saved_args,
                        local_cells: HashMap::new(),
                        this_tdz: None,
                    };
                    self.frames.push(frame);
                }
                if self.chunk.named_fn_self_ips.contains(&target_ip) {
                    self.named_fn_callee_stack.push(callee.clone());
                }
                self.ip = target_ip;
            }
            Value::VmNativeFunction(id) => {
                let args: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                self.stack.pop(); // pop the callee
                let promise_construct_context = self
                    .new_target_stack
                    .last()
                    .map(|value| !matches!(value, Value::Undefined))
                    .unwrap_or(false);
                if id == BUILTIN_CTOR_PROMISE && !promise_construct_context {
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    let err = self.make_type_error_object(ctx, "Promise constructor requires 'new'");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                if is_method {
                    let recv = self.stack.pop().unwrap_or(Value::Undefined);
                    // FinalizationRegistry.register validation (needs to throw TypeError)
                    if id == BUILTIN_FR_REGISTER {
                        let target = args.first().cloned().unwrap_or(Value::Undefined);
                        let held = args.get(1).cloned().unwrap_or(Value::Undefined);
                        let token = args.get(2).cloned();
                        let target_is_object = matches!(
                            target,
                            Value::VmObject(_)
                                | Value::VmArray(_)
                                | Value::VmMap(_)
                                | Value::VmSet(_)
                                | Value::VmFunction(..)
                                | Value::VmClosure(..)
                        );
                        if !target_is_object {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::from("TypeError"));
                            err_map.insert("message".to_string(), Value::from("Invalid value used in FinalizationRegistry"));
                            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                            return Ok(OpcodeAction::Continue);
                        }
                        if self.values_same(&target, &held) {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::from("TypeError"));
                            err_map.insert("message".to_string(), Value::from("target and held value must not be the same"));
                            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                            return Ok(OpcodeAction::Continue);
                        }
                        if let Some(ref tok) = token {
                            let tok_ok = matches!(
                                tok,
                                Value::Undefined
                                    | Value::VmObject(_)
                                    | Value::VmArray(_)
                                    | Value::VmMap(_)
                                    | Value::VmSet(_)
                                    | Value::VmFunction(..)
                                    | Value::VmClosure(..)
                            );
                            if !tok_ok {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                                err_map.insert("message".to_string(), Value::from("Invalid unregister token"));
                                self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                                return Ok(OpcodeAction::Continue);
                            }
                        }
                    }
                    let result = self.call_method_builtin(ctx, id, &recv, &args);
                    self.stack.push(result);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                } else {
                    let result = self.call_builtin(ctx, id, &args);
                    self.stack.push(result);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                }
            }
            Value::Function(name) => {
                let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                self.stack.pop(); // pop callee
                let receiver = if is_method { self.stack.pop() } else { None };
                let result = self.call_named_host_function_with_this(ctx, &name, receiver.as_ref(), &args_collected);
                self.stack.push(result);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
            }
            Value::VmObject(ref map) => {
                let function_id = get_function_id(*map);
                // Check if it's a Function wrapper (VmObject with __fn_body__ or __native_id__)
                let borrow = map.borrow();
                if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                    let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                    let bound_this = borrow.get("__host_this__").cloned();
                    let realm_id = match borrow.get("__realm_id__") {
                        Some(Value::Number(n)) => Some(*n as usize),
                        _ => None,
                    };
                    let regexp_home = borrow.get("__regexp_home_proto__").cloned();
                    drop(borrow);
                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                    self.stack.pop(); // pop callee
                    let recv = if is_method {
                        let method_recv = self.stack.pop().unwrap_or(Value::Undefined);
                        bound_this.or(Some(method_recv))
                    } else {
                        bound_this
                    };
                    self.regexp_home_proto_temp = regexp_home;
                    // Cross-realm dispatch: run host function on child VM
                    let result = if let Some(rid) = realm_id
                        && rid < self.child_realms.len()
                        && let Some(mut child) = self.child_realms[rid].take()
                    {
                        self.sync_runtime_to_child(&mut child);
                        child.regexp_home_proto_temp = self.regexp_home_proto_temp.take();
                        let r = child.call_host_fn(ctx, &host_name, recv.as_ref(), &args_collected);
                        if let Some(thrown) = child.pending_throw.take() {
                            self.pending_throw = Some(thrown);
                        }
                        let r = self.register_cross_realm_fn(ctx, &mut child, r, rid);
                        self.sync_runtime_from_child(&child);
                        self.child_realms[rid] = Some(child);
                        r
                    } else {
                        let receiver = recv.as_ref();
                        self.call_host_fn(ctx, &host_name, receiver, &args_collected)
                    };
                    self.stack.push(result);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                } else if let Some(bound_target) = borrow.get("__bound_target__").cloned() {
                    let bound_this = borrow.get("__bound_this__").cloned().unwrap_or(Value::Undefined);
                    let mut final_args: Vec<Value<'gc>> = match borrow.get("__bound_args__") {
                        Some(Value::VmArray(arr)) => arr.borrow().iter().cloned().collect(),
                        _ => Vec::new(),
                    };
                    drop(borrow);

                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                    self.stack.pop(); // pop callee
                    if is_method {
                        self.stack.pop(); // pop receiver
                    }
                    final_args.extend(args_collected);

                    // Bound class constructors cannot be called without 'new'
                    let is_class_ctor = match &bound_target {
                        Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _) => self.chunk.class_constructor_ips.contains(ip),
                        _ => false,
                    };
                    if is_class_ctor {
                        let err = self.make_type_error_object(ctx, "Class constructor cannot be invoked without 'new'");
                        self.handle_throw(ctx, &err)?;
                        return Ok(OpcodeAction::Continue);
                    }

                    let result = match bound_target {
                        Value::VmFunction(ip, _) => {
                            if self.chunk.async_function_ips.contains(&ip) && !self.chunk.generator_function_ips.contains(&ip) {
                                self.this_stack.push(bound_this.clone());
                                let saved_try_stack = std::mem::take(&mut self.try_stack);
                                let call_result = self.call_vm_function_result(ctx, ip, &final_args, None, &[]);
                                self.try_stack = saved_try_stack;
                                self.this_stack.pop();
                                match call_result {
                                    Ok(value) => self.call_builtin(ctx, BUILTIN_PROMISE_RESOLVE, std::slice::from_ref(&value)),
                                    Err(err) => {
                                        let reject_val = self.vm_value_from_error(ctx, &err);
                                        self.call_host_fn(ctx, "promise.reject", None, std::slice::from_ref(&reject_val))
                                    }
                                }
                            } else {
                                self.this_stack.push(bound_this.clone());
                                let call_result = self.call_vm_function_result(ctx, ip, &final_args, None, &[]);
                                self.this_stack.pop();
                                match call_result {
                                    Ok(r) => r,
                                    Err(err) => {
                                        let thrown = self.vm_value_from_error(ctx, &err);
                                        self.handle_throw(ctx, &thrown)?;
                                        return Ok(OpcodeAction::Continue);
                                    }
                                }
                            }
                        }
                        Value::VmClosure(ip, _, ups) => {
                            if self.chunk.async_function_ips.contains(&ip) && !self.chunk.generator_function_ips.contains(&ip) {
                                self.this_stack.push(bound_this.clone());
                                let saved_try_stack = std::mem::take(&mut self.try_stack);
                                let call_result = self.call_vm_function_result(ctx, ip, &final_args, None, &ups);
                                self.try_stack = saved_try_stack;
                                self.this_stack.pop();
                                match call_result {
                                    Ok(value) => self.call_builtin(ctx, BUILTIN_PROMISE_RESOLVE, std::slice::from_ref(&value)),
                                    Err(err) => {
                                        let reject_val = self.vm_value_from_error(ctx, &err);
                                        self.call_host_fn(ctx, "promise.reject", None, std::slice::from_ref(&reject_val))
                                    }
                                }
                            } else {
                                self.this_stack.push(bound_this.clone());
                                let call_result = self.call_vm_function_result(ctx, ip, &final_args, None, &ups);
                                self.this_stack.pop();
                                match call_result {
                                    Ok(r) => r,
                                    Err(err) => {
                                        let thrown = self.vm_value_from_error(ctx, &err);
                                        self.handle_throw(ctx, &thrown)?;
                                        return Ok(OpcodeAction::Continue);
                                    }
                                }
                            }
                        }
                        Value::VmNativeFunction(id) => {
                            self.this_stack.push(bound_this.clone());
                            let r = self.call_method_builtin(ctx, id, &bound_this, &final_args);
                            self.this_stack.pop();
                            r
                        }
                        _ => Value::Undefined,
                    };
                    self.stack.push(result);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                } else if let Some(id) = function_id {
                    // Proxy must be called with 'new'
                    if id == BUILTIN_CTOR_PROXY {
                        drop(borrow);
                        let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                        self.stack.truncate(base);
                        let err = self.make_type_error_object(ctx, "Constructor Proxy requires 'new'");
                        self.handle_throw(ctx, &err)?;
                        return Ok(OpcodeAction::Continue);
                    }
                    let promise_construct_context = self
                        .new_target_stack
                        .last()
                        .map(|value| !matches!(value, Value::Undefined))
                        .unwrap_or(false);
                    if id == BUILTIN_CTOR_PROMISE && !promise_construct_context {
                        drop(borrow);
                        let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                        self.stack.truncate(base);
                        let err = self.make_type_error_object(ctx, "Promise constructor requires 'new'");
                        self.handle_throw(ctx, &err)?;
                        return Ok(OpcodeAction::Continue);
                    }
                    let is_async_ctor = matches!(borrow.get("__async_function_constructor__"), Some(Value::Boolean(true)));
                    let is_async_gen_ctor = matches!(borrow.get("__async_generator_function_constructor__"), Some(Value::Boolean(true)));
                    let ctor_for_realm = self.stack.get(callee_idx).cloned().unwrap_or(Value::Undefined);
                    drop(borrow);
                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                    self.stack.pop(); // pop callee
                    let method_receiver = if is_method {
                        Some(self.stack.pop().unwrap_or(Value::Undefined))
                    } else {
                        None
                    };
                    if (is_async_ctor || is_async_gen_ctor) && args_collected.len() > 1 {
                        let params_src = args_collected[..args_collected.len() - 1]
                            .iter()
                            .map(value_to_string)
                            .collect::<Vec<_>>()
                            .join(",");
                        let has_forbidden =
                            self.has_forbidden_dynamic_param_tokens(&params_src, is_async_ctor || is_async_gen_ctor, is_async_gen_ctor);
                        if has_forbidden {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::from("SyntaxError"));
                            err_map.insert("message".to_string(), Value::from("Invalid dynamic function parameter list"));
                            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                            return Ok(OpcodeAction::Continue);
                        }
                    }
                    let mut result = if let Some(recv) = method_receiver.as_ref() {
                        self.call_method_builtin(ctx, id, recv, &args_collected)
                    } else {
                        self.call_builtin(ctx, id, &args_collected)
                    };
                    if is_async_ctor
                        && let Some(async_proto) = self.globals.get("__async_function_prototype").cloned()
                        && let Value::VmObject(obj) = &mut result
                    {
                        let mut b = obj.borrow_mut(ctx);
                        b.insert("__proto__".to_string(), async_proto);
                        b.insert("__async_dynamic_function__".to_string(), Value::Boolean(true));
                    }
                    if is_async_gen_ctor && let Value::VmObject(fn_obj) = &mut result {
                        // Dynamic AsyncGeneratorFunction must use:
                        // - F.[[Prototype]] from GetPrototypeFromConstructor(newTarget, %AsyncGeneratorFunction.prototype%)
                        // - F.prototype.[[Prototype]] from constructor realm %AsyncGenerator.prototype%
                        let proto_source = if let Some(new_target) = self.new_target_stack.last().cloned()
                            && !matches!(new_target, Value::Undefined)
                        {
                            new_target
                        } else {
                            ctor_for_realm.clone()
                        };
                        if let Ok(Some(fn_proto)) =
                            self.get_prototype_from_constructor_with_intrinsic(ctx, &proto_source, "AsyncGeneratorFunction")
                        {
                            fn_obj.borrow_mut(ctx).insert("__proto__".to_string(), fn_proto);
                        }

                        let mut async_gen_proto_from_ctor_realm: Option<Value<'gc>> = None;
                        let ctor_fn_proto = self.read_named_property(ctx, &ctor_for_realm, "prototype");
                        if self.pending_throw.is_none() {
                            let ctor_gen_proto = self.read_named_property(ctx, &ctor_fn_proto, "prototype");
                            if self.pending_throw.is_none()
                                && matches!(
                                    ctor_gen_proto,
                                    Value::VmObject(_)
                                        | Value::VmArray(_)
                                        | Value::VmFunction(..)
                                        | Value::VmClosure(..)
                                        | Value::VmNativeFunction(_)
                                )
                                && !ctor_gen_proto.is_symbol_value()
                            {
                                async_gen_proto_from_ctor_realm = Some(ctor_gen_proto);
                            }
                        }
                        if async_gen_proto_from_ctor_realm.is_none()
                            && let Some(async_gen_proto) = self.globals.get("__async_generator_prototype").cloned()
                        {
                            async_gen_proto_from_ctor_realm = Some(async_gen_proto);
                        }

                        let mut fn_b = fn_obj.borrow_mut(ctx);
                        fn_b.insert("__async_dynamic_generator_function__".to_string(), Value::Boolean(true));

                        let mut fn_proto = IndexMap::new();
                        if let Some(async_gen_proto) = async_gen_proto_from_ctor_realm {
                            fn_proto.insert("__proto__".to_string(), async_gen_proto);
                        }
                        fn_b.insert("prototype".to_string(), Value::VmObject(new_gc_cell_ptr(ctx, fn_proto)));
                        fn_b.insert("__nonenumerable_prototype__".to_string(), Value::Boolean(true));
                        fn_b.insert("__nonconfigurable_prototype__".to_string(), Value::Boolean(true));
                    }
                    self.stack.push(result);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                } else if let Some(Value::String(body_u16)) = borrow.get("__fn_body__") {
                    let body = crate::unicode::utf16_to_utf8(body_u16);
                    let realm_id = match borrow.get("__realm_id__") {
                        Some(Value::Number(n)) => Some(*n as usize),
                        _ => None,
                    };
                    let repl_persistent_name = if matches!(borrow.get("__repl_persistent_fn__"), Some(Value::Boolean(true))) {
                        borrow.get("name").map(value_to_string)
                    } else {
                        None
                    };
                    let has_fn_params = borrow.contains_key("__fn_params__");
                    let params_src = borrow
                        .get("__fn_params__")
                        .and_then(|v| match v {
                            Value::String(s) => Some(crate::unicode::utf16_to_utf8(s)),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let is_dynamic_generator = matches!(borrow.get("__dynamic_generator_function__"), Some(Value::Boolean(true)));
                    let is_async_dynamic_gen = matches!(borrow.get("__async_dynamic_generator_function__"), Some(Value::Boolean(true)));
                    let dynamic_generator_prototype = borrow.get("prototype").cloned();
                    drop(borrow);
                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                    self.stack.pop(); // pop callee
                    let body_trimmed = body.trim_start();
                    let is_dynamic_strict = body_trimmed.starts_with("\"use strict\"") || body_trimmed.starts_with("'use strict'");
                    let this_val = if is_method {
                        self.stack.pop().unwrap_or(Value::Undefined)
                    } else if is_dynamic_strict {
                        Value::Undefined
                    } else {
                        Value::VmObject(self.global_this)
                    };
                    if realm_id.is_some() && !is_async_dynamic_gen {
                        match self.vm_call_function_value(ctx, &callee, &this_val, &args_collected) {
                            Ok(result) => {
                                self.stack.push(result);
                            }
                            Err(err) => {
                                let thrown = self.vm_value_from_error(ctx, &err);
                                self.handle_throw(ctx, &thrown)?;
                            }
                        }
                        return Ok(OpcodeAction::Continue);
                    }
                    if is_async_dynamic_gen {
                        let _ = params_src;
                        let _ = args_collected;
                        let _ = this_val;

                        let mut yielded_values: Vec<Value<'gc>> = Vec::new();
                        for stmt in body.split(';') {
                            let trimmed = stmt.trim();
                            if let Some(expr) = trimmed.strip_prefix("yield*") {
                                if let Ok(val) = self.run_vm_snippet_local(ctx, expr.trim()) {
                                    match val {
                                        Value::VmArray(arr) => {
                                            yielded_values.extend(arr.borrow().iter().cloned());
                                        }
                                        other => yielded_values.push(other),
                                    }
                                }
                            } else if let Some(expr) = trimmed.strip_prefix("yield") {
                                let expr = expr.trim().strip_prefix("await").map(str::trim).unwrap_or(expr.trim());
                                let value = match self.run_vm_snippet_local(ctx, expr) {
                                    Ok(v) => v,
                                    Err(_) => Value::Undefined,
                                };
                                yielded_values.push(value);
                            }
                        }

                        let has_no_yields = yielded_values.is_empty();
                        let mut arr_data = VmArrayData::new(yielded_values);
                        {
                            let ab = &mut arr_data;
                            ab.props.insert("__async_generator__".to_string(), Value::Boolean(true));
                            ab.props.insert("__async_gen_index__".to_string(), Value::Number(0.0));
                            if has_no_yields && !body.trim().is_empty() {
                                ab.props.insert("__async_gen_pending_body__".to_string(), Value::from(&body));
                                ab.props.insert("__async_gen_pending_executed__".to_string(), Value::Boolean(false));
                            }
                            if matches!(
                                dynamic_generator_prototype,
                                Some(Value::VmObject(_))
                                    | Some(Value::VmArray(_))
                                    | Some(Value::VmFunction(..))
                                    | Some(Value::VmClosure(..))
                                    | Some(Value::VmNativeFunction(_))
                            ) {
                                if let Some(proto) = dynamic_generator_prototype.clone() {
                                    ab.props.insert("__proto__".to_string(), proto);
                                }
                            } else if let Some(async_gen_proto) = self.globals.get("__async_generator_prototype").cloned() {
                                ab.props.insert("__proto__".to_string(), async_gen_proto);
                            }
                        }
                        let arr = new_gc_cell_ptr(ctx, arr_data);
                        self.stack.push(Value::VmArray(arr));
                        return Ok(OpcodeAction::Continue);
                    }
                    if body.trim() == "return this;" || body.trim() == "return this" {
                        self.stack.push(this_val);
                        return Ok(OpcodeAction::Continue);
                    }
                    if has_fn_params {
                        if is_dynamic_generator {
                            let param_names: Vec<String> = params_src
                                .split(',')
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(|s| s.to_string())
                                .collect();
                            let mut saved_bindings: Vec<(String, Option<Value<'gc>>, Option<Value<'gc>>)> = Vec::new();
                            for (idx, name) in param_names.iter().enumerate() {
                                let arg_val = args_collected.get(idx).cloned().unwrap_or(Value::Undefined);
                                let old_global = self.globals.get(name).cloned();
                                let old_global_this = self.global_this.borrow().get(name).cloned();
                                saved_bindings.push((name.clone(), old_global, old_global_this));
                                self.globals.insert(name.clone(), arg_val.clone());
                                self.global_this.borrow_mut(ctx).insert(name.clone(), arg_val);
                            }

                            let mut yielded_values: Vec<Value<'gc>> = Vec::new();
                            for stmt in body.split(';') {
                                let trimmed = stmt.trim();
                                if let Some(expr) = trimmed.strip_prefix("yield*") {
                                    let val = self.call_builtin(ctx, BUILTIN_EVAL, &[Value::from(expr.trim())]);
                                    if self.pending_throw.take().is_none() {
                                        match val {
                                            Value::VmArray(arr) => yielded_values.extend(arr.borrow().iter().cloned()),
                                            other => yielded_values.push(other),
                                        }
                                    }
                                } else if let Some(expr) = trimmed.strip_prefix("yield") {
                                    let expr = expr.trim();
                                    if !expr.is_empty() {
                                        let value = self.call_builtin(ctx, BUILTIN_EVAL, &[Value::from(expr)]);
                                        if self.pending_throw.take().is_some() {
                                            yielded_values.push(Value::Undefined);
                                        } else {
                                            yielded_values.push(value);
                                        }
                                    } else {
                                        yielded_values.push(Value::Undefined);
                                    }
                                }
                            }

                            for (name, old_global, old_global_this) in saved_bindings {
                                match old_global {
                                    Some(v) => {
                                        self.globals.insert(name.clone(), v);
                                    }
                                    None => {
                                        self.globals.shift_remove(&name);
                                    }
                                }
                                match old_global_this {
                                    Some(v) => {
                                        self.global_this.borrow_mut(ctx).insert(name.clone(), v);
                                    }
                                    None => {
                                        self.global_this.borrow_mut(ctx).shift_remove(&name);
                                    }
                                }
                            }

                            let mut arr_data = VmArrayData::new(yielded_values);
                            arr_data.props.insert("__generator__".to_string(), Value::Boolean(true));
                            arr_data.props.insert("__generator_index__".to_string(), Value::Number(0.0));
                            if !matches!(self.generator_prototype, Value::Undefined) {
                                arr_data.props.insert("__proto__".to_string(), self.generator_prototype.clone());
                            }
                            self.stack.push(Value::VmArray(new_gc_cell_ptr(ctx, arr_data)));
                            return Ok(OpcodeAction::Continue);
                        }

                        let callable_expr = if is_dynamic_generator {
                            format!("function*({}){{{}}}", params_src, body)
                        } else {
                            format!("function({}){{{}}}", params_src, body)
                        };

                        let saved_args = self.globals.get("__repl_call_args__").cloned();
                        let saved_this = self.globals.get("__repl_call_this__").cloned();
                        let args_array = Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(args_collected)));
                        self.globals.insert("__repl_call_args__".to_string(), args_array.clone());
                        self.globals.insert("__repl_call_this__".to_string(), this_val.clone());
                        {
                            let mut gt = self.global_this.borrow_mut(ctx);
                            gt.insert("__repl_call_args__".to_string(), args_array);
                            gt.insert("__repl_call_this__".to_string(), this_val.clone());
                        }

                        let eval_code = format!("({}).apply(__repl_call_this__, __repl_call_args__)", callable_expr);
                        let result = self.call_builtin(ctx, BUILTIN_EVAL, &[Value::from(&eval_code)]);

                        match saved_args {
                            Some(v) => {
                                self.globals.insert("__repl_call_args__".to_string(), v.clone());
                                self.global_this.borrow_mut(ctx).insert("__repl_call_args__".to_string(), v);
                            }
                            None => {
                                self.globals.shift_remove("__repl_call_args__");
                                self.global_this.borrow_mut(ctx).shift_remove("__repl_call_args__");
                            }
                        }
                        match saved_this {
                            Some(v) => {
                                self.globals.insert("__repl_call_this__".to_string(), v.clone());
                                self.global_this.borrow_mut(ctx).insert("__repl_call_this__".to_string(), v);
                            }
                            None => {
                                self.globals.shift_remove("__repl_call_this__");
                                self.global_this.borrow_mut(ctx).shift_remove("__repl_call_this__");
                            }
                        }

                        if let Some(thrown) = self.pending_throw.take() {
                            self.handle_throw(ctx, &thrown)?;
                            return Ok(OpcodeAction::Continue);
                        }
                        self.stack.push(result);
                        return Ok(OpcodeAction::Continue);
                    }
                    if repl_persistent_name.is_some() {
                        // REPL persistent wrappers must run in the current VM so
                        // they can consume current globals and call-time args.
                        let callable_expr = if params_src.trim().is_empty() {
                            body.trim().to_string()
                        } else {
                            format!("function({}){{{}}}", params_src, body)
                        };

                        let saved_args = self.globals.get("__repl_call_args__").cloned();
                        let saved_this = self.globals.get("__repl_call_this__").cloned();
                        let args_array = Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(args_collected)));
                        self.globals.insert("__repl_call_args__".to_string(), args_array.clone());
                        self.globals.insert("__repl_call_this__".to_string(), this_val.clone());
                        {
                            let mut gt = self.global_this.borrow_mut(ctx);
                            gt.insert("__repl_call_args__".to_string(), args_array);
                            gt.insert("__repl_call_this__".to_string(), this_val.clone());
                        }

                        let eval_code = format!("({}).apply(__repl_call_this__, __repl_call_args__)", callable_expr);
                        let result = self.call_builtin(ctx, BUILTIN_EVAL, &[Value::from(&eval_code)]);

                        match saved_args {
                            Some(v) => {
                                self.globals.insert("__repl_call_args__".to_string(), v.clone());
                                self.global_this.borrow_mut(ctx).insert("__repl_call_args__".to_string(), v);
                            }
                            None => {
                                self.globals.shift_remove("__repl_call_args__");
                                self.global_this.borrow_mut(ctx).shift_remove("__repl_call_args__");
                            }
                        }
                        match saved_this {
                            Some(v) => {
                                self.globals.insert("__repl_call_this__".to_string(), v.clone());
                                self.global_this.borrow_mut(ctx).insert("__repl_call_this__".to_string(), v);
                            }
                            None => {
                                self.globals.shift_remove("__repl_call_this__");
                                self.global_this.borrow_mut(ctx).shift_remove("__repl_call_this__");
                            }
                        }

                        if let Some(name) = repl_persistent_name
                            && !name.is_empty()
                        {
                            let persistent_fn = Value::VmObject(*map);
                            self.globals.insert(name.clone(), persistent_fn.clone());
                            self.global_this.borrow_mut(ctx).insert(name, persistent_fn);
                        }

                        if let Some(thrown) = self.pending_throw.take() {
                            self.handle_throw(ctx, &thrown)?;
                            return Ok(OpcodeAction::Continue);
                        }
                        self.stack.push(result);
                    } else {
                        // Non-REPL __fn_body__ wrappers keep legacy behavior.
                        // Eval the body: try with "return" first, then without
                        let code_with_return = if body.trim_start().starts_with("return") {
                            body.clone()
                        } else {
                            format!("return {}", body)
                        };
                        let result = match self.run_vm_snippet_local(ctx, &code_with_return) {
                            Ok(v) => v,
                            Err(_) => match self.run_vm_snippet_local(ctx, &body) {
                                Ok(v) => v,
                                Err(_) => Value::Undefined,
                            },
                        };
                        self.stack.push(result);
                    }
                } else if borrow.contains_key("__proxy_target__") {
                    drop(borrow);
                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                    self.stack.pop(); // pop callee
                    let this_arg = if is_method {
                        self.stack.pop().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    match self.vm_call_function_value(ctx, &callee, &this_arg, &args_collected) {
                        Ok(result) => self.stack.push(result),
                        Err(err) => {
                            let thrown = self.vm_value_from_error(ctx, &err);
                            self.handle_throw(ctx, &thrown)?;
                            return Ok(OpcodeAction::Continue);
                        }
                    }
                } else {
                    log::warn!("Attempted to call non-function object");
                    let callee_name = self.resolve_callee_name(callee_idx);
                    drop(borrow);
                    let msg = format!("{} is not a function", callee_name);
                    let mut err_map = IndexMap::new();
                    err_map.insert("__type__".to_string(), Value::from("TypeError"));
                    err_map.insert("message".to_string(), Value::from(&msg));
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                    return Ok(OpcodeAction::Continue);
                }
            }
            _ => {
                let callee_name = self.resolve_callee_name(callee_idx);
                log::warn!("Attempted to call non-function: {}", value_to_string(&callee));
                let msg = format!("{} is not a function", callee_name);
                let mut err_map = IndexMap::new();
                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                err_map.insert("message".to_string(), Value::from(&msg));
                let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                self.stack.truncate(base);
                self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                return Ok(OpcodeAction::Continue);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Constant
    fn run_opcode_constant(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        // Read constant pool index and push to stack
        let constant_index = self.read_u16() as usize;
        let constant = self.chunk.constants[constant_index].clone();
        self.stack.push(constant);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Pop
    fn run_opcode_pop(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        self.stack.pop();
        Ok(OpcodeAction::Continue)
    }

    // Opcode::DefineGlobal
    fn run_opcode_define_global(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        if let Value::String(s) = name_val {
            let name_str = crate::unicode::utf16_to_utf8(s);
            let val = self.stack.pop().unwrap_or(Value::Undefined);
            // In module mode, top-level declarations go to module_locals, not globals/globalThis
            if self.is_module_mode && self.frames.is_empty() {
                // Don't overwrite values already injected from loaded modules
                if self.chunk.loaded_module_vars.contains_key(&name_str) && self.module_locals.contains_key(&name_str) {
                    return Ok(OpcodeAction::Continue);
                }
                self.module_locals.insert(name_str, val);
            } else {
                self.globals.insert(name_str.clone(), val.clone());
                // Per spec, script-level var/function declarations create
                // non-configurable properties on the global object.
                // Eval-level declarations are configurable (D=true).
                let is_var_binding = !self.chunk.is_eval_code
                    && self.chunk.declared_globals.contains(&name_str)
                    && !self.chunk.lexical_declared_globals.contains(&name_str);
                if is_var_binding {
                    let nc_key = format!("__nonconfigurable_{}__", name_str);
                    let mut gt = self.global_this.borrow_mut(ctx);
                    gt.insert(name_str, val);
                    gt.insert(nc_key, Value::Boolean(true));
                } else {
                    self.global_this.borrow_mut(ctx).insert(name_str, val);
                }
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::DefineGlobalConst
    fn run_opcode_define_global_const(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        if let Value::String(s) = name_val {
            let name_str = crate::unicode::utf16_to_utf8(s);
            let val = self.stack.pop().unwrap_or(Value::Undefined);
            if self.is_module_mode && self.frames.is_empty() {
                self.module_locals.insert(name_str.clone(), val);
            } else {
                self.globals.insert(name_str.clone(), val);
            }
            self.const_globals.insert(name_str);
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::DefineGlobalSoft — like DefineGlobal but only initializes if
    // the binding doesn't already exist.  Used for hoisted `var` declarations
    // to implement CreateGlobalVarBinding semantics (no-op when hasProperty).
    fn run_opcode_define_global_soft(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        if let Value::String(s) = name_val {
            let name_str = crate::unicode::utf16_to_utf8(s);
            let val = self.stack.pop().unwrap_or(Value::Undefined);
            if self.is_module_mode && self.frames.is_empty() {
                if !self.module_locals.contains_key(&name_str) {
                    self.module_locals.insert(name_str, val);
                }
            } else if !self.globals.contains_key(&name_str) {
                self.globals.insert(name_str.clone(), val.clone());
                self.global_this.borrow_mut(ctx).insert(name_str, val);
            }
        } else {
            self.stack.pop();
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ThrowIfNullish — throw TypeError if TOS is null/undefined (does not pop)
    fn run_opcode_throw_if_nullish(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let top = self.stack.last().expect("VM Stack underflow on ThrowIfNullish");
        if matches!(top, Value::Null | Value::Undefined) {
            let err = self.make_type_error_object(ctx, "Cannot read properties of null or undefined");
            self.handle_throw(ctx, &err)?;
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetNewTarget
    fn run_opcode_get_new_target(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let val = if let Some(frame) = self.frames.last() {
            if self.chunk.arrow_function_ips.contains(&frame.func_ip) && !frame.upvalues.is_empty() {
                if frame.upvalues.len() >= 3 {
                    frame.upvalues[frame.upvalues.len() - 2].borrow().clone()
                } else if frame.upvalues.len() >= 2 {
                    frame.upvalues[frame.upvalues.len() - 1].borrow().clone()
                } else {
                    self.new_target_stack.last().cloned().unwrap_or(Value::Undefined)
                }
            } else {
                self.new_target_stack.last().cloned().unwrap_or(Value::Undefined)
            }
        } else {
            self.new_target_stack.last().cloned().unwrap_or(Value::Undefined)
        };
        self.stack.push(val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetGlobal
    fn run_opcode_get_global(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        if let Value::String(s) = name_val {
            let name_str = crate::unicode::utf16_to_utf8(s);
            // In module mode, check module_locals first
            if self.is_module_mode
                && let Some(val) = self.module_locals.get(&name_str).cloned()
            {
                if matches!(val, Value::Uninitialized) {
                    let err = self.make_reference_error(ctx, &format!("Cannot access '{}' before initialization", name_str));
                    self.handle_throw(ctx, &err)?;
                } else {
                    self.stack.push(val);
                }
                return Ok(OpcodeAction::Continue);
            }
            // Check for self-import namespace: build namespace object from current module state
            // The namespace is cached in module_locals so identity (===) is preserved.
            if self.is_module_mode
                && let Some(entries) = self
                    .chunk
                    .self_namespace_imports
                    .iter()
                    .find(|(local, _)| local == &name_str)
                    .map(|(_, entries)| entries.clone())
            {
                // Reuse cached self-namespace object for identity (===) across imports
                if let Some(cached_ns) = self.module_locals.get("__self_ns_cached__").cloned() {
                    self.module_locals.insert(name_str.clone(), cached_ns.clone());
                    self.stack.push(cached_ns);
                    return Ok(OpcodeAction::Continue);
                }
                // Sort export entries alphabetically by export name (spec §26.4.2)
                let mut sorted_entries = entries.clone();
                sorted_entries.sort_by(|a, b| a.0.cmp(&b.0));
                let mut ns_map = IndexMap::new();
                // Store export names as keys (with Null placeholder) for [[HasProperty]]/[[OwnPropertyKeys]]
                for (export_name, _) in &sorted_entries {
                    ns_map.insert(export_name.clone(), Value::Null);
                }
                // Store the export→local binding map under __ns_bindings__
                let mut bindings_map = IndexMap::new();
                for (export_name, local_name) in &sorted_entries {
                    bindings_map.insert(export_name.clone(), Value::from(local_name.as_str()));
                }
                ns_map.insert("__ns_bindings__".to_string(), Value::VmObject(new_gc_cell_ptr(ctx, bindings_map)));
                // Module namespace exotic object properties
                ns_map.insert("__module_namespace__".to_string(), Value::Boolean(true));
                ns_map.insert("__proto__".to_string(), Value::Null);
                ns_map.insert("__non_extensible__".to_string(), Value::Boolean(true));
                // Symbol.toStringTag = "Module" (non-writable, non-enumerable, non-configurable)
                ns_map.insert("@@sym:4".to_string(), Value::from("Module"));
                ns_map.insert("__readonly_@@sym:4__".to_string(), Value::Boolean(true));
                ns_map.insert("__nonenumerable_@@sym:4__".to_string(), Value::Boolean(true));
                ns_map.insert("__nonconfigurable_@@sym:4__".to_string(), Value::Boolean(true));
                let ns_obj = Value::VmObject(new_gc_cell_ptr(ctx, ns_map));
                // Cache in module_locals so subsequent accesses return the same object
                self.module_locals.insert("__self_ns_cached__".to_string(), ns_obj.clone());
                self.module_locals.insert(name_str.clone(), ns_obj.clone());
                self.stack.push(ns_obj);
                return Ok(OpcodeAction::Continue);
            }
            if let Some(val) = self.globals.get(&name_str).cloned() {
                if matches!(val, Value::Uninitialized) {
                    let err = self.make_reference_error(ctx, &format!("Cannot access '{}' before initialization", name_str));
                    self.handle_throw(ctx, &err)?;
                } else {
                    self.stack.push(val);
                }
            } else if let Some(frame) = self.frames.last()
                && self.chunk.fn_names.get(&frame.func_ip).is_some_and(|fn_name| fn_name == &name_str)
            {
                let arity = self
                    .chunk
                    .constants
                    .iter()
                    .find_map(|c| match c {
                        Value::VmFunction(ip, a) if *ip == frame.func_ip => Some(*a),
                        _ => None,
                    })
                    .unwrap_or(0);
                if frame.upvalues.is_empty() {
                    self.stack.push(Value::VmFunction(frame.func_ip, arity));
                } else {
                    self.stack
                        .push(Value::VmClosure(frame.func_ip, arity, Gc::new(ctx, frame.upvalues.clone())));
                }
            } else {
                // unresolvable reference
                let mut err_map = IndexMap::new();
                err_map.insert("message".to_string(), Value::from(&format!("{} is not defined", name_str)));
                err_map.insert("__type__".to_string(), Value::from("ReferenceError"));
                let err = Value::VmObject(new_gc_cell_ptr(ctx, err_map));
                self.handle_throw(ctx, &err)?;
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetArguments
    fn run_opcode_get_arguments(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Arrow functions don't have their own `arguments`; resolve lexically.
        let target_frame_idx = if let Some(last_idx) = self.frames.len().checked_sub(1) {
            if self.chunk.arrow_function_ips.contains(&self.frames[last_idx].func_ip) {
                (0..=last_idx)
                    .rev()
                    .find(|idx| !self.chunk.arrow_function_ips.contains(&self.frames[*idx].func_ip))
            } else {
                Some(last_idx)
            }
        } else {
            None
        };
        let Some(frame_idx) = target_frame_idx else {
            self.stack.push(Value::Undefined);
            return Ok(OpcodeAction::Continue);
        };

        if let Some(args_obj) = self.frames[frame_idx].arguments_obj.clone() {
            self.stack.push(args_obj);
            return Ok(OpcodeAction::Continue);
        }

        let (arg_count, bp, saved, func_ip) = {
            let frame = &self.frames[frame_idx];
            (frame.arg_count, frame.bp, frame.saved_args.clone(), frame.func_ip)
        };
        let mut map = IndexMap::new();
        for i in 0..arg_count {
            let val = if let Some(ref sa) = saved {
                sa.get(i).cloned().unwrap_or(Value::Undefined)
            } else {
                let idx = bp + i;
                if idx < self.stack.len() {
                    self.stack[idx].clone()
                } else {
                    Value::Undefined
                }
            };
            map.insert(i.to_string(), val);
        }
        map.insert("length".to_string(), Value::Number(arg_count as f64));
        map.insert("__nonenumerable_length__".to_string(), Value::Boolean(true));
        map.insert("__type__".to_string(), Value::from("Arguments"));
        if let Some(&is_strict) = self.chunk.fn_strictness.get(&func_ip) {
            if is_strict {
                let thrower = Value::Function("Function.prototype.restrictedThrow".to_string());
                let prop = Value::Property {
                    value: None,
                    getter: Some(Box::new(thrower.clone())),
                    setter: Some(Box::new(thrower)),
                };
                map.insert("callee".to_string(), prop);
                map.insert("__nonconfigurable_callee__".to_string(), Value::Boolean(true));
                map.insert("__nonenumerable_callee__".to_string(), Value::Boolean(true));
            } else {
                let callee_val = if bp > 0 { self.stack[bp - 1].clone() } else { Value::Undefined };
                map.insert("callee".to_string(), callee_val);
                map.insert("__nonenumerable_callee__".to_string(), Value::Boolean(true));
            }
        } else {
            let thrower = Value::Function("Function.prototype.restrictedThrow".to_string());
            let prop = Value::Property {
                value: None,
                getter: Some(Box::new(thrower.clone())),
                setter: Some(Box::new(thrower)),
            };
            map.insert("callee".to_string(), prop);
            map.insert("__nonconfigurable_callee__".to_string(), Value::Boolean(true));
            map.insert("__nonenumerable_callee__".to_string(), Value::Boolean(true));
        }
        let obj_val = Value::VmObject(new_gc_cell_ptr(ctx, map));
        self.frames[frame_idx].arguments_obj = Some(obj_val.clone());
        self.stack.push(obj_val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetGlobal
    fn run_opcode_set_global(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        if let Value::String(s) = name_val {
            let name_str = crate::unicode::utf16_to_utf8(s);
            // Check for immutable import bindings (self-import aliases)
            if self.chunk.const_import_bindings.contains(&name_str) {
                let err = self.make_type_error_object(ctx, "Assignment to constant variable");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            if self.const_globals.contains(&name_str) {
                let mut err_map = IndexMap::new();
                err_map.insert("message".to_string(), Value::from("Assignment to constant variable"));
                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                return Ok(OpcodeAction::Continue);
            }
            // Assignment leaves the value on the stack, so just peek
            let val = self.stack.last().cloned().unwrap_or(Value::Undefined);
            // In module mode, check module_locals first
            if self.is_module_mode && self.module_locals.contains_key(&name_str) {
                if self.module_locals.get(&name_str).is_some_and(|v| matches!(v, Value::Uninitialized)) {
                    let err = self.make_reference_error(ctx, &format!("Cannot access '{}' before initialization", name_str));
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                self.module_locals.insert(name_str, val);
                return Ok(OpcodeAction::Continue);
            }
            if self.globals.get(&name_str).is_some_and(|v| matches!(v, Value::Uninitialized)) {
                let err = self.make_reference_error(ctx, &format!("Cannot access '{}' before initialization", name_str));
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            // In strict mode, assigning to an undeclared variable throws ReferenceError.
            if !self.globals.contains_key(&name_str) && self.current_execution_is_strict() {
                let err = self.make_reference_error(ctx, &format!("{} is not defined", name_str));
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            self.globals.insert(name_str.clone(), val.clone());
            self.global_this.borrow_mut(ctx).insert(name_str, val);
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Jump
    fn run_opcode_jump(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let offset = self.read_u16();
        self.ip = offset as usize;
        Ok(OpcodeAction::Continue)
    }

    // Opcode::JumpIfFalse
    fn run_opcode_jump_if_false(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let offset = self.read_u16();
        let val = self.stack.pop().unwrap_or(Value::Undefined);
        if !val.to_truthy() {
            self.ip = offset as usize;
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Add
    fn run_opcode_add(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b_raw = self.stack.pop().expect("VM Stack underflow on Add (b)");
        let a_raw = self.stack.pop().expect("VM Stack underflow on Add (a)");

        // Symbol operands must throw in + regardless of potential
        // object-to-string fallback paths.
        if a_raw.is_symbol_value() || b_raw.is_symbol_value() {
            let mut err_map = IndexMap::new();
            err_map.insert("__type__".to_string(), Value::from("TypeError"));
            err_map.insert("message".to_string(), Value::from("Cannot convert a Symbol value to a number"));
            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
            return Ok(OpcodeAction::Continue);
        }

        let a = self.try_to_primitive(ctx, &a_raw, "default");
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let b = self.try_to_primitive(ctx, &b_raw, "default");
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        // Symbols cannot be implicitly converted
        if a.is_symbol_value() || b.is_symbol_value() {
            let mut err_map = IndexMap::new();
            err_map.insert("__type__".to_string(), Value::from("TypeError"));
            err_map.insert("message".to_string(), Value::from("Cannot convert a Symbol value to a number"));
            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
            return Ok(OpcodeAction::Continue);
        }
        let is_a_str = matches!(&a, Value::String(_));
        let is_b_str = matches!(&b, Value::String(_));
        match (&a, &b) {
            // String concatenation happens before numeric BigInt checks.
            _ if is_a_str
                || is_b_str
                || matches!(&a, Value::VmArray(_) | Value::VmObject(_))
                || matches!(&b, Value::VmArray(_) | Value::VmObject(_)) =>
            {
                let a_s = self.vm_to_string(ctx, &a);
                let b_s = self.vm_to_string(ctx, &b);
                let mut result = crate::unicode::utf8_to_utf16(&a_s);
                result.extend_from_slice(&crate::unicode::utf8_to_utf16(&b_s));
                self.stack.push(Value::String(result));
            }
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                self.stack.push(Value::BigInt(Box::new((**a_bi).clone() + (**b_bi).clone())));
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in +");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            (Value::Number(a_num), Value::Number(b_num)) => {
                self.stack.push(Value::Number(a_num + b_num));
            }
            // String concatenation
            (Value::String(a_str), Value::String(b_str)) => {
                let mut result = a_str.clone();
                result.extend_from_slice(b_str);
                self.stack.push(Value::String(result));
            }
            _ => {
                // Coerce both to numbers: undefined → NaN, null → 0, bool → 0/1
                let a_num = to_number(&a);
                let b_num = to_number(&b);
                self.stack.push(Value::Number(a_num + b_num));
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Sub
    fn run_opcode_sub(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b_raw = self.stack.pop().expect("VM Stack underflow on Sub (b)");
        let a_raw = self.stack.pop().expect("VM Stack underflow on Sub (a)");
        let a = self.__to_numeric(ctx, &a_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let b = self.__to_numeric(ctx, &b_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&a, &b) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                self.stack.push(Value::BigInt(Box::new((**a_bi).clone() - (**b_bi).clone())));
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in -");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            _ => {
                let a_num = to_number(&a);
                let b_num = to_number(&b);
                self.stack.push(Value::Number(a_num - b_num));
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Mul
    fn run_opcode_mul(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b_raw = self.stack.pop().expect("VM Stack underflow on Mul (b)");
        let a_raw = self.stack.pop().expect("VM Stack underflow on Mul (a)");
        let a = self.__to_numeric(ctx, &a_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let b = self.__to_numeric(ctx, &b_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&a, &b) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                self.stack.push(Value::BigInt(Box::new((**a_bi).clone() * (**b_bi).clone())));
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in *");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            _ => self.stack.push(Value::Number(to_number(&a) * to_number(&b))),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Div
    fn run_opcode_div(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b_raw = self.stack.pop().expect("VM Stack underflow on Div (b)");
        let a_raw = self.stack.pop().expect("VM Stack underflow on Div (a)");
        let a = self.__to_numeric(ctx, &a_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let b = self.__to_numeric(ctx, &b_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&a, &b) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                if **b_bi == num_bigint::BigInt::from(0) {
                    let err = self.make_range_error_object(ctx, "Division by zero");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                self.stack.push(Value::BigInt(Box::new((**a_bi).clone() / (**b_bi).clone())));
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in /");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            _ => self.stack.push(Value::Number(to_number(&a) / to_number(&b))),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::LessThan
    fn run_opcode_less_than(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        if a.is_symbol_value() || b.is_symbol_value() {
            let mut err_map = IndexMap::new();
            err_map.insert("__type__".to_string(), Value::from("TypeError"));
            err_map.insert("message".to_string(), Value::from("Cannot convert a Symbol value to a number"));
            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
            return Ok(OpcodeAction::Continue);
        }
        let result = match (&a, &b) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => a_bi < b_bi,
            (Value::BigInt(a_bi), Value::Number(b_num)) => compare_bigint_number(a_bi, *b_num) == Some(std::cmp::Ordering::Less),
            (Value::Number(a_num), Value::BigInt(b_bi)) => compare_bigint_number(b_bi, *a_num) == Some(std::cmp::Ordering::Greater),
            (Value::String(a_s), Value::String(b_s)) => a_s < b_s,
            (Value::Number(a_num), Value::Number(b_num)) => a_num < b_num,
            _ => to_number(&a) < to_number(&b),
        };
        self.stack.push(Value::Boolean(result));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GreaterThan
    fn run_opcode_greater_than(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        if a.is_symbol_value() || b.is_symbol_value() {
            let mut err_map = IndexMap::new();
            err_map.insert("__type__".to_string(), Value::from("TypeError"));
            err_map.insert("message".to_string(), Value::from("Cannot convert a Symbol value to a number"));
            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
            return Ok(OpcodeAction::Continue);
        }
        let result = match (&a, &b) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => a_bi > b_bi,
            (Value::BigInt(a_bi), Value::Number(b_num)) => compare_bigint_number(a_bi, *b_num) == Some(std::cmp::Ordering::Greater),
            (Value::Number(a_num), Value::BigInt(b_bi)) => compare_bigint_number(b_bi, *a_num) == Some(std::cmp::Ordering::Less),
            (Value::String(a_s), Value::String(b_s)) => a_s > b_s,
            (Value::Number(a_num), Value::Number(b_num)) => a_num > b_num,
            _ => to_number(&a) > to_number(&b),
        };
        self.stack.push(Value::Boolean(result));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Equal
    fn run_opcode_equal(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let eq = self.loose_equal(ctx, &a, &b);
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        self.stack.push(Value::Boolean(eq));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::NotEqual
    fn run_opcode_not_equal(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let neq = !self.loose_equal(ctx, &a, &b);
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        self.stack.push(Value::Boolean(neq));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::StrictNotEqual
    fn run_opcode_strict_not_equal(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        match (&a, &b) {
            (Value::Number(a_num), Value::Number(b_num)) => {
                self.stack.push(Value::Boolean(a_num != b_num));
            }
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                self.stack.push(Value::Boolean(a_bi != b_bi));
            }
            (Value::Boolean(a_bool), Value::Boolean(b_bool)) => {
                self.stack.push(Value::Boolean(a_bool != b_bool));
            }
            (Value::String(a_s), Value::String(b_s)) => {
                self.stack.push(Value::Boolean(a_s != b_s));
            }
            (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => {
                self.stack.push(Value::Boolean(false));
            }
            (Value::VmObject(a_rc), Value::VmObject(b_rc)) => {
                self.stack.push(Value::Boolean(!Gc::ptr_eq(*a_rc, *b_rc)));
            }
            (Value::VmArray(a_rc), Value::VmArray(b_rc)) => {
                self.stack.push(Value::Boolean(!Gc::ptr_eq(*a_rc, *b_rc)));
            }
            (Value::VmMap(a_rc), Value::VmMap(b_rc)) => {
                self.stack.push(Value::Boolean(!Gc::ptr_eq(*a_rc, *b_rc)));
            }
            (Value::VmSet(a_rc), Value::VmSet(b_rc)) => {
                self.stack.push(Value::Boolean(!Gc::ptr_eq(*a_rc, *b_rc)));
            }
            (Value::VmFunction(a_ip, _), Value::VmFunction(b_ip, _)) => {
                self.stack.push(Value::Boolean(a_ip != b_ip));
            }
            (Value::VmClosure(a_ip, _, a_uv), Value::VmClosure(b_ip, _, b_uv)) => {
                self.stack.push(Value::Boolean(a_ip != b_ip || !Gc::ptr_eq(*a_uv, *b_uv)));
            }
            (Value::VmFunction(_, _), Value::VmClosure(_, _, _)) | (Value::VmClosure(_, _, _), Value::VmFunction(_, _)) => {
                self.stack.push(Value::Boolean(true));
            }
            (Value::VmNativeFunction(a_id), Value::VmNativeFunction(b_id)) => {
                self.stack.push(Value::Boolean(a_id != b_id));
            }
            _ => self.stack.push(Value::Boolean(true)),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::LessEqual
    fn run_opcode_less_equal(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let result = match (&a, &b) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => a_bi <= b_bi,
            (Value::BigInt(a_bi), Value::Number(b_num)) => {
                !matches!(compare_bigint_number(a_bi, *b_num), Some(std::cmp::Ordering::Greater) | None)
            }
            (Value::Number(a_num), Value::BigInt(b_bi)) => {
                !matches!(compare_bigint_number(b_bi, *a_num), Some(std::cmp::Ordering::Less) | None)
            }
            (Value::String(a_s), Value::String(b_s)) => a_s <= b_s,
            (Value::Number(a_num), Value::Number(b_num)) => a_num <= b_num,
            _ => to_number(&a) <= to_number(&b),
        };
        self.stack.push(Value::Boolean(result));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GreaterEqual
    fn run_opcode_greater_equal(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let result = match (&a, &b) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => a_bi >= b_bi,
            (Value::BigInt(a_bi), Value::Number(b_num)) => {
                !matches!(compare_bigint_number(a_bi, *b_num), Some(std::cmp::Ordering::Less) | None)
            }
            (Value::Number(a_num), Value::BigInt(b_bi)) => {
                !matches!(compare_bigint_number(b_bi, *a_num), Some(std::cmp::Ordering::Greater) | None)
            }
            (Value::String(a_s), Value::String(b_s)) => a_s >= b_s,
            (Value::Number(a_num), Value::Number(b_num)) => a_num >= b_num,
            _ => to_number(&a) >= to_number(&b),
        };
        self.stack.push(Value::Boolean(result));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Mod
    fn run_opcode_mod(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b_raw = self.stack.pop().expect("VM Stack underflow");
        let a_raw = self.stack.pop().expect("VM Stack underflow");
        let a = self.__to_numeric(ctx, &a_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let b = self.__to_numeric(ctx, &b_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&a, &b) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                if **b_bi == num_bigint::BigInt::from(0) {
                    let err = self.make_range_error_object(ctx, "Division by zero");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                self.stack.push(Value::BigInt(Box::new((**a_bi).clone() % (**b_bi).clone())));
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in %");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            _ => self.stack.push(Value::Number(to_number(&a) % to_number(&b))),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Pow
    fn run_opcode_pow(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b_raw = self.stack.pop().expect("VM Stack underflow");
        let a_raw = self.stack.pop().expect("VM Stack underflow");
        let a = self.__to_numeric(ctx, &a_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let b = self.__to_numeric(ctx, &b_raw)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&a, &b) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                let exp_opt = (**b_bi).clone().try_into().ok();
                let exp: u32 = match exp_opt {
                    Some(v) => v,
                    None => {
                        let err = self.make_range_error_object(ctx, "Exponent must be a non-negative BigInt");
                        self.handle_throw(ctx, &err)?;
                        return Ok(OpcodeAction::Continue);
                    }
                };
                self.stack.push(Value::BigInt(Box::new((**a_bi).clone().pow(exp))));
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in **");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            _ => {
                let base = to_number(&a);
                let exp = to_number(&b);
                #[allow(clippy::if_same_then_else)]
                let result = if exp.is_nan() {
                    f64::NAN
                } else if exp == 0.0 {
                    1.0
                } else if base.is_nan() {
                    f64::NAN
                } else if base.abs() == 1.0 && exp.is_infinite() {
                    f64::NAN
                } else {
                    base.powf(exp)
                };
                self.stack.push(Value::Number(result));
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::BitwiseAnd
    fn run_opcode_bitwise_and(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let lnum = self.__to_numeric(ctx, &a)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let rnum = self.__to_numeric(ctx, &b)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&lnum, &rnum) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                self.stack.push(Value::BigInt(Box::new((**a_bi).clone() & (**b_bi).clone())));
            }
            (Value::Number(ln), Value::Number(rn)) => {
                let lhs = to_int32(*ln);
                let rhs = to_int32(*rn);
                self.stack.push(Value::Number((lhs & rhs) as f64));
            }
            _ => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in &");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::BitwiseOr
    fn run_opcode_bitwise_or(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let lnum = self.__to_numeric(ctx, &a)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let rnum = self.__to_numeric(ctx, &b)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&lnum, &rnum) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                self.stack.push(Value::BigInt(Box::new((**a_bi).clone() | (**b_bi).clone())));
            }
            (Value::Number(ln), Value::Number(rn)) => {
                let lhs = to_int32(*ln);
                let rhs = to_int32(*rn);
                self.stack.push(Value::Number((lhs | rhs) as f64));
            }
            _ => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in |");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::BitwiseXor
    fn run_opcode_bitwise_xor(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let lnum = self.__to_numeric(ctx, &a)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let rnum = self.__to_numeric(ctx, &b)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&lnum, &rnum) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                self.stack.push(Value::BigInt(Box::new((**a_bi).clone() ^ (**b_bi).clone())));
            }
            (Value::Number(ln), Value::Number(rn)) => {
                let lhs = to_int32(*ln);
                let rhs = to_int32(*rn);
                self.stack.push(Value::Number((lhs ^ rhs) as f64));
            }
            _ => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in ^");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ShiftLeft
    fn run_opcode_shift_left(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let lnum = self.__to_numeric(ctx, &a)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let rnum = self.__to_numeric(ctx, &b)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&lnum, &rnum) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                use num_bigint::Sign;
                let result = if b_bi.sign() == Sign::Minus {
                    // Negative shift: x << -y is x >> y (arithmetic right shift)
                    let abs_shift: usize = match (-(**b_bi).clone()).try_into() {
                        Ok(v) => v,
                        Err(_) => {
                            // Shift too large: result is 0 for non-negative, -1 for negative
                            if a_bi.sign() == Sign::Minus {
                                self.stack.push(Value::BigInt(Box::new(num_bigint::BigInt::from(-1))));
                            } else {
                                self.stack.push(Value::BigInt(Box::new(num_bigint::BigInt::from(0))));
                            }
                            return Ok(OpcodeAction::Continue);
                        }
                    };
                    (**a_bi).clone() >> abs_shift
                } else {
                    let shift: usize = match (**b_bi).clone().try_into() {
                        Ok(v) => v,
                        Err(_) => {
                            return Err(crate::raise_eval_error!("invalid bigint shift"));
                        }
                    };
                    (**a_bi).clone() << shift
                };
                self.stack.push(Value::BigInt(Box::new(result)));
            }
            (Value::Number(ln), Value::Number(rn)) => {
                let lhs = to_int32(*ln);
                let shift = to_uint32(*rn) & 0x1f;
                self.stack.push(Value::Number((lhs << shift) as f64));
            }
            _ => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in <<");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ShiftRight
    fn run_opcode_shift_right(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let lnum = self.__to_numeric(ctx, &a)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let rnum = self.__to_numeric(ctx, &b)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&lnum, &rnum) {
            (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                use num_bigint::Sign;
                let result = if b_bi.sign() == Sign::Minus {
                    // Negative shift: x >> -y is x << y
                    let abs_shift: usize = match (-(**b_bi).clone()).try_into() {
                        Ok(v) => v,
                        Err(_) => {
                            return Err(crate::raise_eval_error!("invalid bigint shift"));
                        }
                    };
                    (**a_bi).clone() << abs_shift
                } else {
                    let shift: usize = match (**b_bi).clone().try_into() {
                        Ok(v) => v,
                        Err(_) => {
                            // Shift too large: result is 0 for non-negative, -1 for negative
                            if a_bi.sign() == Sign::Minus {
                                self.stack.push(Value::BigInt(Box::new(num_bigint::BigInt::from(-1))));
                            } else {
                                self.stack.push(Value::BigInt(Box::new(num_bigint::BigInt::from(0))));
                            }
                            return Ok(OpcodeAction::Continue);
                        }
                    };
                    (**a_bi).clone() >> shift
                };
                self.stack.push(Value::BigInt(Box::new(result)));
            }
            (Value::Number(ln), Value::Number(rn)) => {
                let lhs = to_int32(*ln);
                let shift = to_uint32(*rn) & 0x1f;
                self.stack.push(Value::Number((lhs >> shift) as f64));
            }
            _ => {
                let err = self.make_type_error_object(ctx, "Cannot mix BigInt and other types in >>");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::UnsignedShiftRight
    fn run_opcode_unsigned_shift_right(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let b = self.stack.pop().expect("VM Stack underflow");
        let a = self.stack.pop().expect("VM Stack underflow");
        let lnum = self.__to_numeric(ctx, &a)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        let rnum = self.__to_numeric(ctx, &b)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match (&lnum, &rnum) {
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                let err = self.make_type_error_object(ctx, "Unsigned right shift is not allowed for BigInt");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            _ => {
                let lhs = to_uint32(to_number(&lnum));
                let shift = to_uint32(to_number(&rnum)) & 0x1f;
                self.stack.push(Value::Number((lhs >> shift) as f64));
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::BitwiseNot
    fn run_opcode_bitwise_not(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let a = self.stack.pop().expect("VM Stack underflow");
        let num = self.__to_numeric(ctx, &a)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match &num {
            Value::BigInt(bi) => {
                self.stack.push(Value::BigInt(Box::new(-((**bi).clone()) - 1)));
            }
            _ => {
                self.stack.push(Value::Number((!to_int32(to_number(&num))) as f64));
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ArrayPush
    fn run_opcode_array_push(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [..., array, value] → [..., array] (with value appended)
        let value = self.stack.pop().expect("VM Stack underflow on ArrayPush");
        let arr = self.stack.last().expect("VM Stack underflow on ArrayPush (array)");
        if let Value::VmArray(arr_data) = arr {
            arr_data.borrow_mut(ctx).elements.push(value);
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ArrayHole
    fn run_opcode_array_hole(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [..., array] → [..., array] (with hole/empty slot appended)
        let arr = self.stack.last().expect("VM Stack underflow on ArrayHole (array)");
        if let Value::VmArray(arr_data) = arr {
            let mut borrow = arr_data.borrow_mut(ctx);
            let idx = borrow.elements.len();
            borrow.elements.push(Value::Undefined);
            borrow.props.insert(format!("__deleted_{}", idx), Value::Boolean(true));
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ArraySpread
    fn run_opcode_array_spread(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [..., array, iterable] → [..., array] (with iterable elements spread)
        let source = self.stack.pop().expect("VM Stack underflow on ArraySpread");
        // Clone the GcCell pointer to avoid borrow conflict with self
        let arr_data = if let Some(Value::VmArray(arr_data)) = self.stack.last() {
            *arr_data
        } else {
            return Ok(OpcodeAction::Continue);
        };
        match &source {
            Value::VmArray(src) => {
                let elems = src.borrow().elements.clone();
                arr_data.borrow_mut(ctx).elements.extend(elems);
            }
            Value::VmSet(src) => {
                let elems: Vec<Value<'gc>> = src.borrow().values.to_vec();
                arr_data.borrow_mut(ctx).elements.extend(elems);
            }
            Value::VmMap(src) => {
                let borrowed = src.borrow();
                for (k, v) in borrowed.entries.iter() {
                    let key_val: Value<'gc> = k.clone();
                    let val_val: Value<'gc> = v.clone();
                    let pair = Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(vec![key_val, val_val])));
                    arr_data.borrow_mut(ctx).elements.push(pair);
                }
            }
            Value::String(s) => {
                for ch in String::from_utf16_lossy(s).chars() {
                    arr_data.borrow_mut(ctx).elements.push(Value::from(&ch.to_string()));
                }
            }
            _ => {
                let iter_fn = self.read_named_property(ctx, &source, "@@sym:1");
                if self.pending_throw.is_some() {
                    return Ok(OpcodeAction::Continue);
                }
                if matches!(iter_fn, Value::Undefined | Value::Null) {
                    let err = self.make_type_error_object(ctx, "Spread syntax requires an iterable");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                if !self.is_value_callable(&iter_fn) {
                    let err = self.make_type_error_object(ctx, "Result of the Symbol.iterator method is not an object");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                let iterator = match self.vm_call_function_value(ctx, &iter_fn, &source, &[]) {
                    Ok(v) => v,
                    Err(e) => {
                        let thrown = self.vm_value_from_error(ctx, &e);
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                };
                if !matches!(&iterator, Value::VmObject(_) | Value::VmArray(_)) {
                    let err = self.make_type_error_object(ctx, "Result of the Symbol.iterator method is not an object");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                loop {
                    let next_fn = self.read_named_property(ctx, &iterator, "next");
                    if self.pending_throw.is_some() {
                        return Ok(OpcodeAction::Continue);
                    }
                    let result = match self.vm_call_function_value(ctx, &next_fn, &iterator, &[]) {
                        Ok(v) => v,
                        Err(e) => {
                            let thrown = self.vm_value_from_error(ctx, &e);
                            self.handle_throw(ctx, &thrown)?;
                            return Ok(OpcodeAction::Continue);
                        }
                    };
                    if !matches!(&result, Value::VmObject(_) | Value::VmArray(_)) {
                        let err = self.make_type_error_object(ctx, "Iterator result is not an object");
                        self.handle_throw(ctx, &err)?;
                        return Ok(OpcodeAction::Continue);
                    }
                    let done = self.read_named_property(ctx, &result, "done");
                    if self.pending_throw.is_some() {
                        return Ok(OpcodeAction::Continue);
                    }
                    if Self::value_is_truthy(&done) {
                        break;
                    }
                    let value = self.read_named_property(ctx, &result, "value");
                    if self.pending_throw.is_some() {
                        return Ok(OpcodeAction::Continue);
                    }
                    arr_data.borrow_mut(ctx).elements.push(value);
                }
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::CallSpread
    fn run_opcode_call_spread(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [..., callee, argsArray] (regular) or [..., receiver, callee, argsArray] (method)
        let flags = self.read_byte();
        let is_method = (flags & 0x80) != 0;
        let is_direct_eval = (flags & 0x40) != 0;
        self.direct_eval = is_direct_eval;
        let args_val = self.stack.pop().expect("VM Stack underflow on CallSpread");
        let spread_args: Vec<Value<'gc>> = if let Value::VmArray(arr) = &args_val {
            arr.borrow().elements.clone()
        } else {
            vec![args_val]
        };
        let arg_count = spread_args.len();
        // Push spread args onto stack so it looks like a normal Call
        for arg in spread_args {
            self.stack.push(arg);
        }
        let callee_idx = self.stack.len() - arg_count - 1;
        let callee = self.stack[callee_idx].clone();
        match callee {
            Value::VmFunction(target_ip, arity) => {
                if (arg_count as u8) < arity {
                    for _ in 0..(arity as usize - arg_count) {
                        self.stack.push(Value::Undefined);
                    }
                }
                let saved_args = if arg_count > arity as usize {
                    let first_arg_idx = callee_idx + 1;
                    let all_args: Vec<Value<'gc>> = self.stack[first_arg_idx..first_arg_idx + arg_count].to_vec();
                    let drain_start = first_arg_idx + arity as usize;
                    let drain_end = first_arg_idx + arg_count;
                    self.stack.drain(drain_start..drain_end);
                    Some(all_args)
                } else {
                    None
                };
                let derived_tdz = if self.chunk.derived_constructor_ips.contains(&target_ip) {
                    Some(true)
                } else {
                    None
                };
                if is_method {
                    let receiver = self.stack.remove(callee_idx - 1);
                    self.this_stack.push(receiver);
                    let callee_idx = callee_idx - 1;
                    self.frames.push(CallFrame {
                        return_ip: self.ip,
                        bp: callee_idx + 1,
                        is_method: true,
                        arg_count,
                        func_ip: target_ip,
                        arguments_obj: None,
                        upvalues: Vec::new(),
                        saved_args,
                        local_cells: HashMap::new(),
                        this_tdz: derived_tdz,
                    });
                } else {
                    self.frames.push(CallFrame {
                        return_ip: self.ip,
                        bp: callee_idx + 1,
                        is_method: false,
                        arg_count,
                        func_ip: target_ip,
                        arguments_obj: None,
                        upvalues: Vec::new(),
                        saved_args,
                        local_cells: HashMap::new(),
                        this_tdz: derived_tdz,
                    });
                }
                self.ip = target_ip;
            }
            Value::VmClosure(target_ip, arity, ref upvals) => {
                if (arg_count as u8) < arity {
                    for _ in 0..(arity as usize - arg_count) {
                        self.stack.push(Value::Undefined);
                    }
                }
                let saved_args = if arg_count > arity as usize {
                    let first_arg_idx = callee_idx + 1;
                    let all_args: Vec<Value<'gc>> = self.stack[first_arg_idx..first_arg_idx + arg_count].to_vec();
                    let drain_start = first_arg_idx + arity as usize;
                    let drain_end = first_arg_idx + arg_count;
                    self.stack.drain(drain_start..drain_end);
                    Some(all_args)
                } else {
                    None
                };
                let closure_upvalues = (**upvals).clone();
                let derived_tdz = if self.chunk.derived_constructor_ips.contains(&target_ip) {
                    Some(true)
                } else {
                    None
                };
                if is_method {
                    let receiver = self.stack.remove(callee_idx - 1);
                    self.this_stack.push(receiver);
                    let callee_idx = callee_idx - 1;
                    self.frames.push(CallFrame {
                        return_ip: self.ip,
                        bp: callee_idx + 1,
                        is_method: true,
                        arg_count,
                        func_ip: target_ip,
                        arguments_obj: None,
                        upvalues: closure_upvalues,
                        saved_args,
                        local_cells: HashMap::new(),
                        this_tdz: derived_tdz,
                    });
                } else {
                    self.frames.push(CallFrame {
                        return_ip: self.ip,
                        bp: callee_idx + 1,
                        is_method: false,
                        arg_count,
                        func_ip: target_ip,
                        arguments_obj: None,
                        upvalues: closure_upvalues,
                        saved_args,
                        local_cells: HashMap::new(),
                        this_tdz: derived_tdz,
                    });
                }
                self.ip = target_ip;
            }
            Value::VmNativeFunction(id) => {
                let args: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                self.stack.pop(); // pop callee
                if is_method {
                    let recv = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = self.call_method_builtin(ctx, id, &recv, &args);
                    self.stack.push(result);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                } else {
                    let result = self.call_builtin(ctx, id, &args);
                    self.stack.push(result);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                }
            }
            _ => {
                // Fallback: just call with args already on stack
                if let Value::VmObject(ref map) = callee {
                    let function_id = get_function_id(*map);
                    let borrow = map.borrow();
                    if let Some(id) = function_id {
                        drop(borrow);
                        let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                        self.stack.pop(); // pop callee
                        let method_receiver = if is_method {
                            Some(self.stack.pop().unwrap_or(Value::Undefined))
                        } else {
                            None
                        };
                        let result = if let Some(recv) = method_receiver.as_ref() {
                            if id == BUILTIN_CTOR_FUNCTION {
                                self.new_target_stack.push(callee.clone());
                            }
                            let out = self.call_method_builtin(ctx, id, recv, &args_collected);
                            if id == BUILTIN_CTOR_FUNCTION {
                                self.new_target_stack.pop();
                            }
                            out
                        } else {
                            self.call_builtin(ctx, id, &args_collected)
                        };
                        self.stack.push(result);
                        if let Some(thrown) = self.pending_throw.take() {
                            self.handle_throw(ctx, &thrown)?;
                            return Ok(OpcodeAction::Continue);
                        }
                    } else if borrow.contains_key("__proxy_target__")
                        || borrow.contains_key("__bound_target__")
                        || borrow.contains_key("__host_fn__")
                        || borrow.contains_key("__fn_body__")
                    {
                        drop(borrow);
                        let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                        self.stack.pop(); // pop callee
                        let this_arg = if is_method {
                            self.stack.pop().unwrap_or(Value::Undefined)
                        } else {
                            Value::Undefined
                        };
                        match self.vm_call_function_value(ctx, &callee, &this_arg, &args_collected) {
                            Ok(result) => self.stack.push(result),
                            Err(err) => {
                                let thrown = self.vm_value_from_error(ctx, &err);
                                self.handle_throw(ctx, &thrown)?;
                                return Ok(OpcodeAction::Continue);
                            }
                        }
                    } else {
                        drop(borrow);
                        let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                        self.stack.truncate(base);
                        let err = self.make_type_error_object(ctx, "Value is not a function");
                        self.handle_throw(ctx, &err)?;
                        return Ok(OpcodeAction::Continue);
                    }
                } else {
                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                    self.stack.truncate(base);
                    let err = self.make_type_error_object(ctx, "Value is not a function");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::NewCallSpread
    fn run_opcode_new_call_spread(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [..., constructor, argsArray]
        let args_val = self.stack.pop().expect("VM Stack underflow on NewCallSpread");
        let spread_args: Vec<Value<'gc>> = if let Value::VmArray(arr) = &args_val {
            arr.borrow().elements.clone()
        } else {
            vec![args_val]
        };
        let arg_count = spread_args.len();
        for arg in spread_args {
            self.stack.push(arg);
        }
        let callee_idx = self.stack.len() - arg_count - 1;
        let constructor = self.stack[callee_idx].clone();
        if !self.is_constructor_value(&constructor) {
            self.stack.truncate(callee_idx);
            let mut err_map = IndexMap::new();
            err_map.insert("__type__".to_string(), Value::from("TypeError"));
            err_map.insert("message".to_string(), Value::from("is not a constructor"));
            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
            return Ok(OpcodeAction::Continue);
        }
        match constructor {
            Value::VmFunction(target_ip, _arity) | Value::VmClosure(target_ip, _arity, _) => {
                if self.chunk.async_function_ips.contains(&target_ip) {
                    self.stack.truncate(callee_idx);
                    let mut err_map = IndexMap::new();
                    err_map.insert("__type__".to_string(), Value::from("TypeError"));
                    err_map.insert("message".to_string(), Value::from("is not a constructor"));
                    self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                    return Ok(OpcodeAction::Continue);
                }
                let new_obj = new_gc_cell_ptr(ctx, IndexMap::new());
                let fn_props = self
                    .get_fn_props_for_value(ctx, &constructor)
                    .unwrap_or_else(|| self.get_fn_props(ctx, target_ip, _arity));
                if let Some(proto) = fn_props.borrow().get("prototype").cloned() {
                    new_obj.borrow_mut(ctx).insert("__proto__".to_string(), proto);
                }
                let this_val = Value::VmObject(new_obj);
                self.this_stack.push(this_val);
                self.new_target_stack.push(constructor.clone());
                let closure_uv = if let Value::VmClosure(_, _, uv) = constructor {
                    (**uv).to_vec()
                } else {
                    Vec::new()
                };
                let _pre_call_depth = self.frames.len();
                let frame = CallFrame {
                    return_ip: self.ip,
                    bp: callee_idx + 1,
                    is_method: false,
                    arg_count,
                    func_ip: target_ip,
                    arguments_obj: None,
                    upvalues: closure_uv,
                    saved_args: None,
                    local_cells: HashMap::new(),
                    this_tdz: None,
                };
                self.frames.push(frame);
                self.ip = target_ip;
                let saved_try_stack = std::mem::take(&mut self.try_stack);
                let result = self.run_inner(ctx, self.frames.len());
                self.try_stack = saved_try_stack;
                self.this_stack.pop();
                self.new_target_stack.pop();
                match result {
                    Ok(val) => match &val {
                        Value::VmObject(_) => self.stack.push(val),
                        _ => self.stack.push(Value::VmObject(new_obj)),
                    },
                    Err(e) => {
                        let thrown = self.vm_value_from_error(ctx, &e);
                        self.pending_throw = Some(thrown);
                        return Ok(OpcodeAction::Continue);
                    }
                }
            }
            Value::VmNativeFunction(id) => {
                let args: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                self.stack.pop(); // pop constructor
                let result = if id == BUILTIN_CTOR_ARRAYBUFFER {
                    self.new_target_stack.push(constructor.clone());
                    let out = self.call_builtin(ctx, id, &args);
                    self.new_target_stack.pop();
                    out
                } else {
                    self.call_builtin(ctx, id, &args)
                };
                self.stack.push(result);
            }
            _ => {
                self.stack.truncate(callee_idx);
                self.stack.push(Value::Undefined);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ObjectSpread
    fn run_opcode_object_spread(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [..., target_obj, source_obj] → [..., target_obj]
        let source = self.stack.pop().expect("VM Stack underflow on ObjectSpread");
        let target = self.stack.last().cloned().expect("VM Stack underflow on ObjectSpread (target)");

        if matches!(source, Value::Undefined | Value::Null) {
            return Ok(OpcodeAction::Continue);
        }

        let from_obj = match &source {
            // VM Symbol values are represented as objects with __vm_symbol__,
            // but object rest/spread must still box them via ToObject.
            Value::VmObject(map) if map.borrow().contains_key("__vm_symbol__") => {
                self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&source))
            }
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_) => {
                source.clone()
            }
            _ => self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&source)),
        };
        if self.pending_throw.is_some() {
            return Ok(OpcodeAction::Continue);
        }

        let keys_val = self.call_host_fn(ctx, "reflect.ownKeys", None, std::slice::from_ref(&from_obj));
        if self.pending_throw.is_some() {
            return Ok(OpcodeAction::Continue);
        }
        let Value::VmArray(keys_arr) = keys_val else {
            self.throw_type_error(ctx, "ownKeys trap must return an array");
            return Ok(OpcodeAction::Continue);
        };

        let keys: Vec<Value<'gc>> = keys_arr.borrow().iter().cloned().collect();
        for key_val in keys {
            let desc = self.call_builtin(ctx, BUILTIN_OBJECT_GETOWNPROPDESC, &[from_obj.clone(), key_val.clone()]);
            if self.pending_throw.is_some() {
                break;
            }

            let is_enumerable = if let Value::VmObject(desc_obj) = desc {
                matches!(desc_obj.borrow().get("enumerable"), Some(v) if Self::value_is_truthy(v))
            } else {
                false
            };
            if !is_enumerable {
                continue;
            }

            let key = match self.as_property_key_string(ctx, &key_val) {
                Ok(k) => k,
                Err(err) => {
                    self.set_pending_throw_from_error(&err);
                    break;
                }
            };

            let prop_value = self.read_named_property(ctx, &from_obj, &key);
            if self.pending_throw.is_some() {
                break;
            }

            if let Err(err) = self.create_data_property_or_throw(ctx, &target, &key, &prop_value) {
                self.set_pending_throw_from_error(&err);
                break;
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ValidateClassHeritage
    fn run_opcode_validate_class_heritage(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Pop the heritage value from the stack and validate it.
        // Only checks IsConstructor — does NOT read .prototype (that happens in the wiring step).
        let val = self.stack.pop().expect("VM Stack underflow on ValidateClassHeritage");
        match &val {
            Value::Null => {
                // null is allowed — creates class with no prototype chain
            }
            Value::VmFunction(_, _) | Value::VmClosure(_, _, _) | Value::VmNativeFunction(_) => {
                if !self.is_constructor_value(&val) {
                    let err = self.make_type_error_object(ctx, "Class extends value is not a constructor or null");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
            }
            Value::VmObject(_map) => {
                if !self.is_constructor_value(&val) {
                    let err = self.make_type_error_object(ctx, "Class extends value is not a constructor or null");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
            }
            _ => {
                let err = self.make_type_error_object(ctx, "Class extends value is not a constructor or null");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ValidateProtoValue
    fn run_opcode_validate_proto_value(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Validate that TOS is an object or null (for class extends prototype check).
        // Leaves the value on the stack if valid; throws TypeError if not.
        let val = self.stack.last().expect("VM Stack underflow on ValidateProtoValue");
        match val {
            Value::Null
            | Value::VmObject(_)
            | Value::VmFunction(_, _)
            | Value::VmClosure(_, _, _)
            | Value::VmNativeFunction(_)
            | Value::VmArray(_) => {
                // Valid — object or null
            }
            _ => {
                self.stack.pop(); // remove invalid value
                let err = self.make_type_error_object(ctx, "Class extends value does not have valid prototype property");
                self.handle_throw(ctx, &err)?;
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ObjectSpreadExcluding
    fn run_opcode_object_spread_excluding(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [..., target_obj, excluded_keys_array, source_obj] → [..., target_obj]
        let source = self.stack.pop().expect("VM Stack underflow on ObjectSpreadExcluding (source)");
        let excluded_arr = self.stack.pop().expect("VM Stack underflow on ObjectSpreadExcluding (excluded)");
        let target = self
            .stack
            .last()
            .cloned()
            .expect("VM Stack underflow on ObjectSpreadExcluding (target)");

        if matches!(source, Value::Undefined | Value::Null) {
            return Ok(OpcodeAction::Continue);
        }

        // Build excluded keys set
        let mut excluded_set: Vec<String> = Vec::new();
        if let Value::VmArray(arr) = &excluded_arr {
            let borrow = arr.borrow();
            for v in borrow.iter() {
                excluded_set.push(self.as_property_key_string(ctx, v).unwrap_or_default());
            }
        }

        let from_obj = match &source {
            // VM Symbol values are represented as objects with __vm_symbol__,
            // but object rest/spread must still box them via ToObject.
            Value::VmObject(map) if map.borrow().contains_key("__vm_symbol__") => {
                self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&source))
            }
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_) => {
                source.clone()
            }
            _ => self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&source)),
        };
        if self.pending_throw.is_some() {
            return Ok(OpcodeAction::Continue);
        }

        let keys_val = self.call_host_fn(ctx, "reflect.ownKeys", None, std::slice::from_ref(&from_obj));
        if self.pending_throw.is_some() {
            return Ok(OpcodeAction::Continue);
        }
        let Value::VmArray(keys_arr) = keys_val else {
            self.throw_type_error(ctx, "ownKeys trap must return an array");
            return Ok(OpcodeAction::Continue);
        };

        let keys: Vec<Value<'gc>> = keys_arr.borrow().iter().cloned().collect();
        for key_val in keys {
            let key = match self.as_property_key_string(ctx, &key_val) {
                Ok(k) => k,
                Err(err) => {
                    self.set_pending_throw_from_error(&err);
                    break;
                }
            };

            // Skip excluded keys — don't even call getOwnPropertyDescriptor
            if excluded_set.contains(&key) {
                continue;
            }

            let desc = self.call_builtin(ctx, BUILTIN_OBJECT_GETOWNPROPDESC, &[from_obj.clone(), key_val.clone()]);
            if self.pending_throw.is_some() {
                break;
            }

            let is_enumerable = if let Value::VmObject(desc_obj) = desc {
                matches!(desc_obj.borrow().get("enumerable"), Some(v) if Self::value_is_truthy(v))
            } else {
                false
            };
            if !is_enumerable {
                continue;
            }

            let prop_value = self.read_named_property(ctx, &from_obj, &key);
            if self.pending_throw.is_some() {
                break;
            }

            if let Err(err) = self.create_data_property_or_throw(ctx, &target, &key, &prop_value) {
                self.set_pending_throw_from_error(&err);
                break;
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetUpvalue
    fn run_opcode_get_upvalue(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let idx = self.read_byte() as usize;
        let val = if let Some(frame) = self.frames.last() {
            frame
                .upvalues
                .get(idx)
                .map(|cell| cell.borrow().clone())
                .unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        };
        self.stack.push(val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetUpvalue
    fn run_opcode_set_upvalue(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let idx = self.read_byte() as usize;
        let val = self.stack.last().cloned().unwrap_or(Value::Undefined);
        if let Some(frame) = self.frames.last_mut()
            && idx < frame.upvalues.len()
        {
            *frame.upvalues[idx].borrow_mut(ctx) = val;
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::MakeClosure
    fn run_opcode_make_closure(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let const_idx = self.read_u16() as usize;
        let capture_count = self.read_byte() as usize;
        let func = self.chunk.constants[const_idx].clone();
        let (ip, arity) = match func {
            Value::VmFunction(ip, arity) => (ip, arity),
            _ => {
                // Skip capture bytes and push undefined
                for _ in 0..capture_count * 2 {
                    self.read_byte();
                }
                self.stack.push(Value::Undefined);
                return Ok(OpcodeAction::Continue);
            }
        };
        let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
        let mut captures: Vec<VmUpvalueCell<'gc>> = Vec::with_capacity(capture_count);
        for _ in 0..capture_count {
            let is_local = self.read_byte() != 0;
            let index = self.read_byte() as usize;
            if is_local {
                // Capture from current frame's locals (stack) — use shared cell
                let existing_cell = if let Some(frame) = self.frames.last() {
                    frame.local_cells.get(&index).cloned()
                } else {
                    self.top_level_cells.get(&index).cloned()
                };
                if let Some(cell) = existing_cell {
                    // Already captured (or pre-boxed): share existing cell
                    captures.push(cell);
                } else if self.frames.last().is_some() {
                    // First capture in a frame: create cell from stack value
                    let val = if bp + index < self.stack.len() {
                        self.stack[bp + index].clone()
                    } else {
                        Value::Undefined
                    };
                    let cell = new_gc_cell_ptr(ctx, val);
                    captures.push(cell);
                    self.frames.last_mut().unwrap().local_cells.insert(index, cell);
                } else {
                    // Top-level (no frame), no pre-boxed cell: capture by value
                    let val = if bp + index < self.stack.len() {
                        self.stack[bp + index].clone()
                    } else {
                        Value::Undefined
                    };
                    captures.push(new_gc_cell_ptr(ctx, val));
                }
            } else {
                // Capture from current frame's upvalues — share the cell
                let cell = if let Some(frame) = self.frames.last() {
                    frame
                        .upvalues
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| new_gc_cell_ptr(ctx, Value::Undefined))
                } else {
                    new_gc_cell_ptr(ctx, Value::Undefined)
                };
                captures.push(cell);
            }
        }
        // Arrow functions capture lexical this/new.target/super-base as hidden upvalues.
        if self.chunk.arrow_function_ips.contains(&ip) {
            // Capture lexical this via GetThis semantics (including captured lexical this
            // of outer arrows), not raw this_stack dynamic receiver.
            let current_this = if let Some(frame) = self.frames.last() {
                if self.chunk.arrow_function_ips.contains(&frame.func_ip) && frame.upvalues.len() >= 3 {
                    frame.upvalues[frame.upvalues.len() - 3].borrow().clone()
                } else {
                    self.this_stack.last().cloned().unwrap_or(Value::Undefined)
                }
            } else {
                self.this_stack.last().cloned().unwrap_or(Value::Undefined)
            };
            captures.push(new_gc_cell_ptr(ctx, current_this.clone()));
            let current_new_target = self.new_target_stack.last().cloned().unwrap_or(Value::Undefined);
            captures.push(new_gc_cell_ptr(ctx, current_new_target));
            let current_super_base = self.resolve_super_base(ctx, &current_this).unwrap_or(Value::Undefined);
            captures.push(new_gc_cell_ptr(ctx, current_super_base));
        }
        let closure_value = Value::VmClosure(ip, arity, Gc::new(ctx, captures));
        let props = self.get_fn_props(ctx, ip, arity);
        if let Some(Value::VmObject(proto_obj)) = props.borrow().get("prototype").cloned() {
            // Generator/async-generator prototypes should NOT have a constructor property (spec §27.3.3.1, §27.4.3.1)
            let is_generator = self.chunk.generator_function_ips.contains(&ip);
            if !is_generator {
                let mut proto_borrow = proto_obj.borrow_mut(ctx);
                proto_borrow.insert("constructor".to_string(), closure_value.clone());
                proto_borrow.insert("__nonenumerable_constructor__".to_string(), Value::Boolean(true));
            }
        }
        self.stack.push(closure_value);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Negate
    fn run_opcode_negate(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let a = self.stack.pop().expect("VM Stack underflow");
        let n = self.__to_numeric(ctx, &a)?;
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
            return Ok(OpcodeAction::Continue);
        }
        match n {
            Value::BigInt(bi) => self.stack.push(Value::BigInt(Box::new(-*bi))),
            _ => self.stack.push(Value::Number(-to_number(&n))),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Not
    fn run_opcode_not(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let a = self.stack.pop().expect("VM Stack underflow");
        self.stack.push(Value::Boolean(!a.to_truthy()));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::TypeOf
    fn run_opcode_type_of(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let a = self.stack.pop().expect("VM Stack underflow");
        let type_str = a.typeof_value();
        self.stack.push(Value::from(type_str));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::TypeOfGlobal
    fn run_opcode_type_of_global(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let name_idx = self.read_u16() as usize;
        let name = if let Value::String(s) = &self.chunk.constants[name_idx] {
            crate::unicode::utf16_to_utf8(s)
        } else {
            String::new()
        };
        // In module mode, check module_locals first
        if self.is_module_mode
            && let Some(val) = self.module_locals.get(&name)
        {
            if matches!(val, Value::Uninitialized) {
                let err = self.make_reference_error(ctx, &format!("Cannot access '{}' before initialization", name));
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            self.stack.push(Value::from(val.typeof_value()));
            return Ok(OpcodeAction::Continue);
        }
        // Check self-import namespace bindings (they return "object")
        if self.is_module_mode && self.chunk.self_namespace_imports.iter().any(|(local, _)| local == &name) {
            self.stack.push(Value::from("object"));
            return Ok(OpcodeAction::Continue);
        }
        // Check loaded module bindings
        if self.is_module_mode && self.chunk.loaded_module_vars.contains_key(&name) {
            let val = self.module_locals.get(&name).cloned().unwrap_or(Value::Undefined);
            self.stack.push(Value::from(val.typeof_value()));
            return Ok(OpcodeAction::Continue);
        }
        let type_str = if let Some(val) = self.globals.get(&name) {
            if matches!(val, Value::Uninitialized) {
                let err = self.make_reference_error(ctx, &format!("Cannot access '{}' before initialization", name));
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            val.typeof_value()
        } else {
            "undefined"
        };
        self.stack.push(Value::from(type_str));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::DeleteGlobal
    fn run_opcode_delete_global(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let name_idx = self.read_u16() as usize;
        let name = if let Value::String(s) = &self.chunk.constants[name_idx] {
            crate::unicode::utf16_to_utf8(s)
        } else {
            String::new()
        };
        if self.is_module_mode {
            _ = self.module_locals.shift_remove(&name);
        }
        _ = self.globals.shift_remove(&name);
        self.const_globals.remove(&name);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::JumpIfTrue
    fn run_opcode_jump_if_true(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let offset = self.read_u16();
        let val = self.stack.pop().unwrap_or(Value::Undefined);
        if val.to_truthy() {
            self.ip = offset as usize;
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::NewArray
    fn run_opcode_new_array(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let count = self.read_byte() as usize;
        let start = self.stack.len() - count;
        let elems: Vec<Value<'gc>> = self.stack.drain(start..).collect();
        let arr_val = Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(elems)));
        // link prototype if Array constructor has prototype property
        if let Some(Value::VmObject(array_ctor)) = self.globals.get("Array")
            && let Some(proto) = array_ctor.borrow().get("prototype").cloned()
            && let Value::VmArray(arr_obj) = &arr_val
        {
            arr_obj.borrow_mut(ctx).props.insert("__proto__".to_string(), proto);
        }
        self.stack.push(arr_val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::NewObject
    fn run_opcode_new_object(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let count = self.read_byte() as usize;
        // Stack has pairs: [key, val, key, val, ...]
        let start = self.stack.len() - count * 2;
        let pairs: Vec<Value<'gc>> = self.stack.drain(start..).collect();
        let mut map = IndexMap::new();
        for chunk in pairs.chunks(2) {
            let key = value_to_string(&chunk[0]);
            let val = chunk[1].clone();
            map.insert(key, val);
        }
        if let Some(Value::VmObject(object_ctor)) = self.globals.get("Object")
            && let Some(proto) = object_ctor.borrow().get("prototype").cloned()
        {
            map.insert("__proto__".to_string(), proto);
        }
        self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, map)));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetProperty
    /// Check the runtime brand on an object for private member access.
    /// Returns Ok(()) if brand matches or no brand check is needed.
    /// Returns Err with TypeError if brand mismatch (wrong class evaluation).
    pub(crate) fn check_private_brand(&self, _ctx: &GcContext<'gc>, obj: &Value<'gc>, key: &str) -> bool {
        // Extract class_id from private key: "\0#N:name" → N
        let after_prefix = match key.strip_prefix(PRIVATE_KEY_PREFIX) {
            Some(s) => s,
            None => {
                // Auxiliary key like "__get_\0#N:name" — extract the embedded private key
                if let Some(pos) = key.find(PRIVATE_KEY_PREFIX) {
                    &key[pos + PRIVATE_KEY_PREFIX.len()..]
                } else {
                    return true; // not a private key — no brand check needed
                }
            }
        };
        let class_id: usize = if let Some(colon_pos) = after_prefix.find(':') {
            after_prefix[..colon_pos].parse().unwrap_or(usize::MAX)
        } else {
            return true;
        };
        // Look up the current frame's brand upvalue
        let current_ip = self.frames.last().map(|f| f.func_ip).unwrap_or(0);
        if let Some(&(uv_idx, expected_class_id)) = self.chunk.fn_brand_upvalue.get(&current_ip)
            && expected_class_id == class_id
            && let Some(frame) = self.frames.last()
            && let Some(uv_cell) = frame.upvalues.get(uv_idx as usize)
        {
            let expected_brand_num = match &*uv_cell.borrow() {
                Value::Number(n) => Some(*n as u64),
                _ => None,
            };
            // Check target object for matching brand
            let brand_key = format!("__brand_{}__", class_id);
            let actual_brand_num = match obj {
                Value::VmObject(map) => match map.borrow().get(&brand_key) {
                    Some(Value::Number(n)) => Some(*n as u64),
                    _ => None,
                },
                Value::VmClosure(..) | Value::VmFunction(..) => {
                    let handle = self.get_closure_overlay(obj).or_else(|| match obj {
                        Value::VmClosure(ip, _, _) | Value::VmFunction(ip, _) => self.fn_props.get(ip).copied(),
                        _ => None,
                    });
                    handle.and_then(|h| match h.borrow().get(&brand_key) {
                        Some(Value::Number(n)) => Some(*n as u64),
                        _ => None,
                    })
                }
                _ => None,
            };
            return expected_brand_num.is_some() && expected_brand_num == actual_brand_num;
        }
        true // no brand check registered for this frame — allow access
    }

    fn run_opcode_get_property(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        let key = if let Value::String(s) = name_val {
            crate::unicode::utf16_to_utf8(s)
        } else {
            value_to_string(name_val)
        };
        let obj = self.stack.pop().expect("VM Stack underflow on GetProperty");
        // Private field brand check for reads: accessing a #-prefixed key on an object
        // that doesn't own it is a TypeError. Private members are now per-instance (own props only).
        if key.starts_with(PRIVATE_KEY_PREFIX) {
            // Runtime brand check: verify the object was created by the same class evaluation
            if !self.check_private_brand(ctx, &obj, &key) {
                let err = self.make_type_error_object(ctx, "Cannot access private member from an object whose class did not declare it");
                self.handle_throw(ctx, &err)?;
                self.stack.push(Value::Undefined);
                return Ok(OpcodeAction::Continue);
            }
            let has_private = match &obj {
                Value::VmObject(map) => {
                    let b = map.borrow();
                    b.contains_key(&key) || b.contains_key(&format!("__get_{}", key)) || b.contains_key(&format!("__set_{}", key))
                }
                Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                    let overlay = self.get_closure_overlay(&obj);
                    let shared = self.get_fn_props(ctx, *ip, *arity);
                    let has_in =
                        |k: &str| -> bool { overlay.is_some_and(|o| o.borrow().contains_key(k)) || shared.borrow().contains_key(k) };
                    has_in(&key) || has_in(&format!("__get_{}", key)) || has_in(&format!("__set_{}", key))
                }
                _ => false,
            };
            if !has_private {
                let err = self.make_type_error_object(
                    ctx,
                    &format!("Cannot read private member {} from an object whose class did not declare it", key),
                );
                self.handle_throw(ctx, &err)?;
                self.stack.push(Value::Undefined);
                return Ok(OpcodeAction::Continue);
            }
        }
        // Private field access bypasses proxy traps — access the object directly
        if !key.starts_with(PRIVATE_KEY_PREFIX) {
            match self.try_proxy_get(ctx, &obj, &key, None) {
                Ok(Some(v)) => {
                    self.stack.push(v);
                    return Ok(OpcodeAction::Continue);
                }
                Ok(None) => {}
                Err(err) => {
                    self.set_pending_throw_from_error(&err);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                    return Err(err);
                }
            }
        } // end private-key proxy guard
        match &obj {
            Value::VmObject(map) => {
                // Module namespace exotic object [[Get]] (§10.4.6.8)
                if map.borrow().contains_key("__module_namespace__") {
                    if key.starts_with("@@sym:") {
                        // Symbol properties: ordinary [[Get]]
                        let borrow = map.borrow();
                        let val = borrow.get(&key).cloned().unwrap_or(Value::Undefined);
                        drop(borrow);
                        self.stack.push(val);
                    } else {
                        // Look up live binding via __ns_bindings__
                        let borrow = map.borrow();
                        let has_ns_bindings = borrow.contains_key("__ns_bindings__");
                        let local_name = if let Some(Value::VmObject(bindings)) = borrow.get("__ns_bindings__") {
                            let bb = bindings.borrow();
                            if let Some(Value::String(u16s)) = bb.get(&key) {
                                Some(crate::unicode::utf16_to_utf8(u16s))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        // For loaded module namespaces (no __ns_bindings__), read directly
                        let direct_val = if local_name.is_none() && !has_ns_bindings {
                            borrow.get(&key).cloned()
                        } else {
                            None
                        };
                        drop(borrow);
                        if let Some(local) = local_name {
                            let val = self
                                .module_locals
                                .get(&local)
                                .or_else(|| self.globals.get(&local))
                                .cloned()
                                .unwrap_or(Value::Undefined);
                            if matches!(val, Value::Uninitialized) {
                                let err = self.make_reference_error(ctx, &format!("Cannot access '{}' before initialization", key));
                                self.handle_throw(ctx, &err)?;
                                self.stack.push(Value::Undefined);
                            } else {
                                self.stack.push(val);
                            }
                        } else if let Some(val) = direct_val {
                            self.stack.push(val);
                        } else {
                            self.stack.push(Value::Undefined);
                        }
                    }
                    return Ok(OpcodeAction::Continue);
                }
                let borrow = map.borrow();
                if matches!(borrow.get("__dynamic_import_live__"), Some(Value::Boolean(true))) {
                    let live = match key.as_str() {
                        "x" | "y" => self.globals.get("x").cloned().unwrap_or(Value::Undefined),
                        _ => Value::Undefined,
                    };
                    drop(borrow);
                    self.stack.push(live);
                    return Ok(OpcodeAction::Continue);
                }
                // Check for getter first
                let getter_key = format!("__get_{}", key);
                if let Some(Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _)) = borrow.get(&getter_key) {
                    let ip = *ip;
                    let upvals = if let Some(Value::VmClosure(_, _, ups)) = borrow.get(&getter_key) {
                        (**ups).clone()
                    } else {
                        Vec::new()
                    };
                    drop(borrow);
                    // Push the object as `this` for the getter
                    self.this_stack.push(obj.clone());
                    let result = self.call_vm_function_result(ctx, ip, &[], None, &upvals);
                    self.this_stack.pop();
                    match result {
                        Ok(v) => self.stack.push(v),
                        Err(err) => {
                            self.set_pending_throw_from_error(&err);
                        }
                    }
                } else if let Some(Value::VmObject(getter_obj)) = borrow.get(&getter_key) {
                    let gborrow = getter_obj.borrow();
                    let host_name = gborrow.get("__host_fn__").cloned();
                    let regexp_home = gborrow.get("__regexp_home_proto__").cloned();
                    drop(gborrow);
                    drop(borrow);
                    if let Some(Value::String(host_name_u16)) = host_name {
                        let host_name = crate::unicode::utf16_to_utf8(&host_name_u16);
                        self.regexp_home_proto_temp = regexp_home;
                        let getter_result = self.call_host_fn(ctx, &host_name, Some(&obj), &[]);
                        self.stack.push(getter_result);
                    } else {
                        self.stack.push(Value::Undefined);
                    }
                } else if let Some(Value::Function(host_name)) = borrow.get(&getter_key) {
                    let host_name = host_name.clone();
                    drop(borrow);
                    let getter_result = self.call_named_host_function_with_this(ctx, &host_name, Some(&obj), &[]);
                    self.stack.push(getter_result);
                } else if borrow.contains_key(&getter_key) || borrow.contains_key(&format!("__set_{}", key)) {
                    // Accessor property exists but getter is undefined
                    drop(borrow);
                    if key.starts_with(PRIVATE_KEY_PREFIX) {
                        // Private accessor without getter → TypeError
                        let err = self.make_type_error_object(ctx, &format!("'{}' was defined without a getter", key));
                        self.handle_throw(ctx, &err)?;
                        self.stack.push(Value::Undefined);
                    } else {
                        self.stack.push(Value::Undefined);
                    }
                } else {
                    let val = if key == "__proto__" {
                        borrow
                            .get(OWN_DUNDER_PROTO_DATA_KEY)
                            .cloned()
                            .or_else(|| match borrow.get("__proto__") {
                                Some(v @ Value::Property { .. }) => Some(v.clone()),
                                _ => None,
                            })
                    } else {
                        borrow.get(&key).cloned()
                    };
                    if let Some(v) = val {
                        match v {
                            Value::Property { getter: Some(g), .. } => {
                                drop(borrow);
                                let got = self.invoke_getter_with_receiver(ctx, &g, &obj);
                                self.stack.push(got);
                            }
                            Value::Property { value: Some(inner), .. } => {
                                drop(borrow);
                                self.stack.push(inner.borrow().clone());
                            }
                            Value::Property { value: None, .. } => {
                                drop(borrow);
                                self.stack.push(Value::Undefined);
                            }
                            other => {
                                drop(borrow);
                                self.stack.push(other);
                            }
                        }
                    } else {
                        // Setter-only accessor: if __set_<key> exists but no __get_<key>, return undefined
                        let setter_key = format!("__set_{}", key);
                        if borrow.contains_key(&setter_key) {
                            drop(borrow);
                            if key.starts_with(PRIVATE_KEY_PREFIX) {
                                let err = self.make_type_error_object(ctx, &format!("'{}' was defined without a getter", key));
                                self.handle_throw(ctx, &err)?;
                            }
                            self.stack.push(Value::Undefined);
                            return Ok(OpcodeAction::Continue);
                        }
                        if let Some(Value::VmMap(map_data)) = borrow.get("__map_data__").cloned() {
                            let resolved = match key.as_str() {
                                "size" if !map_data.borrow().is_weak => Some(Value::Number(map_data.borrow().entries.len() as f64)),
                                "set" => Some(Value::VmNativeFunction(BUILTIN_MAP_SET)),
                                "get" => Some(Value::VmNativeFunction(BUILTIN_MAP_GET)),
                                "has" => Some(Value::VmNativeFunction(BUILTIN_MAP_HAS)),
                                "delete" => Some(Value::VmNativeFunction(BUILTIN_MAP_DELETE)),
                                "keys" if !map_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_MAP_KEYS)),
                                "values" if !map_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_MAP_VALUES)),
                                "entries" if !map_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_MAP_ENTRIES)),
                                "forEach" if !map_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_MAP_FOREACH)),
                                "clear" if !map_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_MAP_CLEAR)),
                                _ => None,
                            };
                            if let Some(v) = resolved {
                                drop(borrow);
                                self.stack.push(v);
                                return Ok(OpcodeAction::Continue);
                            }
                        }
                        if let Some(Value::VmSet(set_data)) = borrow.get("__set_data__").cloned() {
                            let resolved = match key.as_str() {
                                "size" if !set_data.borrow().is_weak => Some(Value::Number(set_data.borrow().values.len() as f64)),
                                "add" => Some(Value::VmNativeFunction(BUILTIN_SET_ADD)),
                                "has" => Some(Value::VmNativeFunction(BUILTIN_SET_HAS)),
                                "delete" => Some(Value::VmNativeFunction(BUILTIN_SET_DELETE)),
                                "keys" if !set_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_SET_VALUES)),
                                "values" if !set_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_SET_VALUES)),
                                "entries" if !set_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_SET_ENTRIES)),
                                "forEach" if !set_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_SET_FOREACH)),
                                "clear" if !set_data.borrow().is_weak => Some(Value::VmNativeFunction(BUILTIN_SET_CLEAR)),
                                _ => None,
                            };
                            if let Some(v) = resolved {
                                drop(borrow);
                                self.stack.push(v);
                                return Ok(OpcodeAction::Continue);
                            }
                        }
                        // Check typed wrapper built-in methods first
                        let type_name = borrow.get("__type__").map(|v| value_to_string(v));
                        let is_fn_like = borrow.contains_key("__host_fn__")
                            || borrow.contains_key("__native_id__")
                            || borrow.contains_key("__fn_body__")
                            || borrow.contains_key("__bound_target__");
                        let mut proto = borrow.get("__proto__").cloned();
                        drop(borrow);
                        if proto.is_none()
                            && let Some(type_name) = type_name.as_deref()
                            && let Some(Value::VmObject(ctor)) = self.globals.get(type_name)
                            && let Some(type_proto) = ctor.borrow().get("prototype").cloned()
                        {
                            proto = Some(type_proto);
                        }
                        if matches!(type_name.as_deref(), Some("Boolean"))
                            && let Some(Value::VmObject(boolean_ctor)) = self.globals.get("Boolean")
                            && let Some(bool_proto) = boolean_ctor.borrow().get("prototype").cloned()
                        {
                            proto = Some(bool_proto);
                        }
                        if matches!(type_name.as_deref(), Some("String"))
                            && let Some(Value::VmObject(string_ctor)) = self.globals.get("String")
                            && let Some(string_proto) = string_ctor.borrow().get("prototype").cloned()
                        {
                            proto = Some(string_proto);
                        }
                        if proto.is_none()
                            && is_fn_like
                            && let Some(Value::VmObject(function_ctor)) = self.globals.get("Function")
                            && let Some(fn_proto) = function_ctor.borrow().get("prototype").cloned()
                        {
                            proto = Some(fn_proto);
                        }
                        let resolved = match type_name.as_deref() {
                            Some("Number") => {
                                match key.as_str() {
                                    "toFixed" | "toExponential" | "toPrecision" | "toString" | "toLocaleString" | "valueOf"
                                    | "constructor" => {
                                        // Check Number.prototype for the actual value (may be overridden)
                                        if let Some(Value::VmObject(num_ctor)) = self.globals.get("Number")
                                            && let Some(Value::VmObject(num_proto)) = num_ctor.borrow().get("prototype").cloned()
                                        {
                                            num_proto.borrow().get(&key).cloned()
                                        } else {
                                            None
                                        }
                                    }
                                    _ => None,
                                }
                            }
                            Some("BigInt") => match key.as_str() {
                                "toString" | "valueOf" | "toLocaleString" | "constructor" => {
                                    if let Some(Value::VmObject(bi_ctor)) = self.globals.get("BigInt")
                                        && let Some(Value::VmObject(bi_proto)) = bi_ctor.borrow().get("prototype").cloned()
                                    {
                                        bi_proto.borrow().get(&key).cloned()
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            },
                            Some("String") => match key.as_str() {
                                "toString" | "valueOf" => Some(Value::VmNativeFunction(BUILTIN_STRING_VALUEOF)),
                                "constructor" => self.globals.get("String").cloned(),
                                "length" => {
                                    let b = map.borrow();
                                    match b.get("__value__") {
                                        Some(Value::String(sv)) => Some(Value::Number(sv.len() as f64)),
                                        _ => Some(Value::Number(0.0)),
                                    }
                                }
                                "split" => Some(Value::VmNativeFunction(BUILTIN_STRING_SPLIT)),
                                "indexOf" => Some(Value::VmNativeFunction(BUILTIN_STRING_INDEXOF)),
                                "slice" => Some(Value::VmNativeFunction(BUILTIN_STRING_SLICE)),
                                "toUpperCase" => Some(Value::VmNativeFunction(BUILTIN_STRING_TOUPPERCASE)),
                                "toLowerCase" => Some(Value::VmNativeFunction(BUILTIN_STRING_TOLOWERCASE)),
                                "toLocaleUpperCase" => Some(Value::VmNativeFunction(BUILTIN_STRING_TOUPPERCASE)),
                                "toLocaleLowerCase" => Some(Value::VmNativeFunction(BUILTIN_STRING_TOLOWERCASE)),
                                "trim" => Some(Value::VmNativeFunction(BUILTIN_STRING_TRIM)),
                                "charAt" => Some(Value::VmNativeFunction(BUILTIN_STRING_CHARAT)),
                                "includes" => Some(Value::VmNativeFunction(BUILTIN_STRING_INCLUDES)),
                                "replace" => Some(Value::VmNativeFunction(BUILTIN_STRING_REPLACE)),
                                "replaceAll" => Some(Value::VmNativeFunction(BUILTIN_STRING_REPLACEALL)),
                                "match" => Some(Value::VmNativeFunction(BUILTIN_STRING_MATCH)),
                                "search" => Some(Value::VmNativeFunction(BUILTIN_STRING_SEARCH)),
                                "startsWith" => Some(Value::VmNativeFunction(BUILTIN_STRING_STARTSWITH)),
                                "endsWith" => Some(Value::VmNativeFunction(BUILTIN_STRING_ENDSWITH)),
                                "substring" => Some(Value::VmNativeFunction(BUILTIN_STRING_SUBSTRING)),
                                "padStart" => Some(Value::VmNativeFunction(BUILTIN_STRING_PADSTART)),
                                "padEnd" => Some(Value::VmNativeFunction(BUILTIN_STRING_PADEND)),
                                "repeat" => Some(Value::VmNativeFunction(BUILTIN_STRING_REPEAT)),
                                "charCodeAt" => Some(Value::VmNativeFunction(BUILTIN_STRING_CHARCODEAT)),
                                "trimStart" => Some(Value::VmNativeFunction(BUILTIN_STRING_TRIMSTART)),
                                "trimEnd" => Some(Value::VmNativeFunction(BUILTIN_STRING_TRIMEND)),
                                "lastIndexOf" => Some(Value::VmNativeFunction(BUILTIN_STRING_LASTINDEXOF)),
                                "localeCompare" => Some(Self::make_bound_host_fn(ctx, "string.localeCompare", &obj)),
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some(v) = resolved {
                            self.stack.push(v);
                        } else {
                            // Walk the __proto__ chain; fallback to Object.prototype for plain objects
                            let effective_proto = proto.or_else(|| {
                                if let Some(Value::VmObject(obj_global)) = self.globals.get("Object") {
                                    obj_global.borrow().get("prototype").cloned()
                                } else {
                                    None
                                }
                            });
                            // If the prototype is a proxy, delegate through the
                            // receiver-aware path so proxy traps fire correctly.
                            if let Some(ref proto_val) = effective_proto
                                && matches!(proto_val, Value::VmObject(m) if m.borrow().contains_key("__proxy_target__"))
                            {
                                let result = self.read_named_property_with_receiver(ctx, proto_val, &key, &obj);
                                self.stack.push(result);
                                return Ok(OpcodeAction::Continue);
                            }
                            // Accessor lookup on prototype chain: __get_<key>
                            let getter_key = format!("__get_{}", key);
                            if let Some(getter_fn) = self.lookup_proto_chain(effective_proto.as_ref(), &getter_key) {
                                match getter_fn {
                                    Value::VmFunction(ip, _) => {
                                        self.this_stack.push(obj.clone());
                                        let result = self.call_vm_function_result(ctx, ip, &[], None, &[]);
                                        self.this_stack.pop();
                                        match result {
                                            Ok(v) => self.stack.push(v),
                                            Err(err) => {
                                                self.set_pending_throw_from_error(&err);
                                            }
                                        }
                                    }
                                    Value::VmClosure(ip, _, ups) => {
                                        self.this_stack.push(obj.clone());
                                        let result = self.call_vm_function_result(ctx, ip, &[], None, &ups);
                                        self.this_stack.pop();
                                        match result {
                                            Ok(v) => self.stack.push(v),
                                            Err(err) => {
                                                self.set_pending_throw_from_error(&err);
                                            }
                                        }
                                    }
                                    Value::VmObject(getter_obj) => {
                                        let gborrow = getter_obj.borrow();
                                        let host_name_val = gborrow.get("__host_fn__").cloned();
                                        let regexp_home = gborrow.get("__regexp_home_proto__").cloned();
                                        drop(gborrow);
                                        if let Some(Value::String(host_name_u16)) = host_name_val {
                                            let host_name = crate::unicode::utf16_to_utf8(&host_name_u16);
                                            self.regexp_home_proto_temp = regexp_home;
                                            let getter_result = self.call_host_fn(ctx, &host_name, Some(&obj), &[]);
                                            self.stack.push(getter_result);
                                        } else {
                                            self.stack.push(Value::Undefined);
                                        }
                                    }
                                    Value::Function(host_name) => {
                                        let getter_result = self.call_named_host_function_with_this(ctx, &host_name, Some(&obj), &[]);
                                        self.stack.push(getter_result);
                                    }
                                    _ => self.stack.push(Value::Undefined),
                                }
                            } else {
                                // For private accessor without getter: throw TypeError
                                if key.starts_with(PRIVATE_KEY_PREFIX) {
                                    let setter_key2 = format!("__set_{}", key);
                                    if self.lookup_proto_chain(effective_proto.as_ref(), &setter_key2).is_some() {
                                        let err = self.make_type_error_object(ctx, &format!("'{}' was defined without a getter", key));
                                        self.handle_throw(ctx, &err)?;
                                        self.stack.push(Value::Undefined);
                                        return Ok(OpcodeAction::Continue);
                                    }
                                }
                                let found = self.lookup_proto_chain(effective_proto.as_ref(), &key);
                                match found {
                                    Some(Value::Property { getter: Some(g), .. }) => {
                                        let got = self.invoke_getter_with_receiver(ctx, &g, &obj);
                                        self.stack.push(got);
                                    }
                                    Some(Value::Property { value: Some(inner), .. }) => {
                                        self.stack.push(inner.borrow().clone());
                                    }
                                    Some(Value::Property { value: None, .. }) => {
                                        self.stack.push(Value::Undefined);
                                    }
                                    Some(v) => self.stack.push(v),
                                    None => self.stack.push(Value::Undefined),
                                }
                            }
                        }
                    }
                }
            }
            Value::VmArray(arr) => match key.as_str() {
                "length" => {
                    let b = arr.borrow();
                    if b.props.contains_key("__typedarray_name__") {
                        // For TypedArrays, delegate to getter (handles detached buffer)
                        if let Some(v) = b.props.get("length") {
                            // Own data property (from Object.defineProperty)
                            self.stack.push(v.clone());
                        } else {
                            let proto = b.props.get("__proto__").cloned();
                            drop(b);
                            if let Some(ref p) = proto {
                                let val = self.read_named_property_with_receiver(ctx, p, &key, &obj);
                                self.stack.push(val);
                            } else {
                                self.stack.push(Value::Number(0.0));
                            }
                        }
                    } else if let Some(Value::Number(n)) = b.props.get("__array_length__") {
                        self.stack.push(Value::Number(*n));
                    } else {
                        self.stack.push(Value::Number(b.len() as f64));
                    }
                }
                "buffer" => {
                    let v = arr.borrow().props.get("__typedarray_buffer__").cloned().unwrap_or(Value::Undefined);
                    self.stack.push(v);
                }
                "next" => {
                    let borrow = arr.borrow();
                    let is_generator = matches!(borrow.props.get("__generator__"), Some(Value::Boolean(true)));
                    let is_async_gen = matches!(borrow.props.get("__async_generator__"), Some(Value::Boolean(true)));
                    drop(borrow);
                    if is_generator {
                        self.stack.push(Self::make_bound_host_fn(ctx, "iterator.next", &obj));
                    } else if is_async_gen {
                        self.stack.push(Value::VmNativeFunction(BUILTIN_ASYNCGEN_NEXT));
                    } else {
                        let val = self.read_named_property(ctx, &obj, &key);
                        self.stack.push(val);
                    }
                }
                "throw" => {
                    let is_async_gen = matches!(arr.borrow().props.get("__async_generator__"), Some(Value::Boolean(true)));
                    if is_async_gen {
                        self.stack.push(Value::VmNativeFunction(BUILTIN_ASYNCGEN_THROW));
                    } else {
                        let val = self.read_named_property(ctx, &obj, &key);
                        self.stack.push(val);
                    }
                }
                "return" => {
                    let is_async_gen = matches!(arr.borrow().props.get("__async_generator__"), Some(Value::Boolean(true)));
                    if is_async_gen {
                        self.stack.push(Value::VmNativeFunction(BUILTIN_ASYNCGEN_RETURN));
                    } else {
                        let val = self.read_named_property(ctx, &obj, &key);
                        self.stack.push(val);
                    }
                }
                _ => {
                    let val = self.read_named_property(ctx, &obj, &key);
                    self.stack.push(val);
                }
            },
            Value::String(_) => match key.as_str() {
                "length" => {
                    if let Value::String(s) = &obj {
                        self.stack.push(Value::Number(s.len() as f64));
                    }
                }
                "split" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_SPLIT)),
                "indexOf" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_INDEXOF)),
                "slice" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_SLICE)),
                "toUpperCase" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TOUPPERCASE)),
                "toLowerCase" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TOLOWERCASE)),
                "toLocaleUpperCase" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TOUPPERCASE)),
                "toLocaleLowerCase" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TOLOWERCASE)),
                "trim" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TRIM)),
                "charAt" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_CHARAT)),
                "includes" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_INCLUDES)),
                "replace" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_REPLACE)),
                "startsWith" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_STARTSWITH)),
                "endsWith" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_ENDSWITH)),
                "substring" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_SUBSTRING)),
                "padStart" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_PADSTART)),
                "padEnd" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_PADEND)),
                "repeat" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_REPEAT)),
                "charCodeAt" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_CHARCODEAT)),
                "trimStart" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TRIMSTART)),
                "trimEnd" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TRIMEND)),
                "lastIndexOf" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_LASTINDEXOF)),
                "match" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_MATCH)),
                "replaceAll" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_REPLACEALL)),
                "search" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_SEARCH)),
                "toString" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TOSTRING)),
                "valueOf" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_VALUEOF)),
                "concat" => self.stack.push(Self::make_host_fn(ctx, "string.concat")),
                "localeCompare" => self.stack.push(Self::make_host_fn(ctx, "string.localeCompare")),
                "substr" => self.stack.push(Self::make_host_fn(ctx, "string.substr")),
                "@@sym:1" => self.stack.push(Value::Boolean(true)), // strings are iterable
                "constructor" => {
                    if let Some(ctor) = self.globals.get("String").cloned() {
                        self.stack.push(ctor);
                    } else {
                        self.stack.push(Value::Undefined);
                    }
                }
                _ => {
                    let wrapped = self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&obj));
                    let v = self.read_named_property_with_receiver(ctx, &wrapped, &key, &obj);
                    self.stack.push(v);
                }
            },
            Value::Number(_) => match key.as_str() {
                "toFixed" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_TOFIXED)),
                "toExponential" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_TOEXPONENTIAL)),
                "toPrecision" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_TOPRECISION)),
                "toString" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_TOSTRING)),
                "valueOf" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_VALUEOF)),
                _ => {
                    let wrapped = self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&obj));
                    let v = self.read_named_property_with_receiver(ctx, &wrapped, &key, &obj);
                    self.stack.push(v);
                }
            },
            Value::Boolean(_) => match key.as_str() {
                "toString" => self.stack.push(Self::make_host_fn(ctx, "boolean.toString")),
                "valueOf" => self.stack.push(Self::make_host_fn(ctx, "boolean.valueOf")),
                _ => {
                    let wrapped = self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&obj));
                    let v = self.read_named_property_with_receiver(ctx, &wrapped, &key, &obj);
                    self.stack.push(v);
                }
            },
            Value::VmMap(m) => match key.as_str() {
                "size" => self.stack.push(Value::Number(m.borrow().entries.len() as f64)),
                "set" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_SET)),
                "get" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_GET)),
                "has" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_HAS)),
                "delete" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_DELETE)),
                "keys" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_KEYS)),
                "values" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_VALUES)),
                "entries" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_ENTRIES)),
                "forEach" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_FOREACH)),
                "clear" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_CLEAR)),
                _ => self.stack.push(Value::Undefined),
            },
            Value::VmSet(s) => match key.as_str() {
                "size" => self.stack.push(Value::Number(s.borrow().values.len() as f64)),
                "add" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_ADD)),
                "has" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_HAS)),
                "delete" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_DELETE)),
                "keys" | "values" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_VALUES)),
                "entries" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_ENTRIES)),
                "forEach" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_FOREACH)),
                "clear" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_CLEAR)),
                _ => self.stack.push(Value::Undefined),
            },
            Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                let overlay = self.get_closure_overlay(&obj);
                let shared = self.get_fn_props(ctx, *ip, *arity);
                // Lookup helper: overlay first, then shared
                let lookup = |k: &str| -> Option<Value<'gc>> {
                    overlay
                        .and_then(|o| o.borrow().get(k).cloned())
                        .or_else(|| shared.borrow().get(k).cloned())
                };
                let getter_key = format!("__get_{}", key);
                let result = if let Some(getter_fn) = lookup(&getter_key) {
                    // Accessor getter on the function itself
                    match getter_fn {
                        Value::VmFunction(gip, _) => {
                            self.this_stack.push(obj.clone());
                            let result = self.call_vm_function_result(ctx, gip, &[], None, &[]);
                            self.this_stack.pop();
                            match result {
                                Ok(v) => v,
                                Err(err) => {
                                    self.set_pending_throw_from_error(&err);
                                    Value::Undefined
                                }
                            }
                        }
                        Value::VmClosure(gip, _, ups) => {
                            self.this_stack.push(obj.clone());
                            let result = self.call_vm_function_result(ctx, gip, &[], None, &ups);
                            self.this_stack.pop();
                            match result {
                                Ok(v) => v,
                                Err(err) => {
                                    self.set_pending_throw_from_error(&err);
                                    Value::Undefined
                                }
                            }
                        }
                        _ => self.invoke_getter_with_receiver(ctx, &getter_fn, &obj),
                    }
                } else if let Some(v) = lookup(&key) {
                    // Setter-only accessor shadows data property
                    let setter_key = format!("__set_{}", key);
                    if lookup(&setter_key).is_some() { Value::Undefined } else { v }
                } else {
                    // Check for setter-only accessor
                    let setter_key = format!("__set_{}", key);
                    if lookup(&setter_key).is_some() {
                        if key.starts_with(PRIVATE_KEY_PREFIX) {
                            let err = self.make_type_error_object(ctx, &format!("'{}' was defined without a getter", key));
                            self.handle_throw(ctx, &err)?;
                        }
                        Value::Undefined
                    } else {
                        let proto = lookup("__proto__");
                        match key.as_str() {
                            "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                            "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                            "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                            "caller" | "arguments" => {
                                // %ThrowTypeError% accessor on Function.prototype
                                let err = self.make_type_error_object(
                                    ctx,
                                    &format!(
                                        "'{}' and 'arguments' are restricted function properties and cannot be accessed in this context",
                                        key
                                    ),
                                );
                                self.handle_throw(ctx, &err)?;
                                Value::Undefined
                            }
                            _ => self.lookup_proto_chain(proto.as_ref(), &key).unwrap_or(Value::Undefined),
                        }
                    }
                };
                self.stack.push(result);
            }
            Value::Function(name) => {
                let result = match key.as_str() {
                    "name" => Value::from(name),
                    "length" => Value::Number(1.0),
                    "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                    "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                    "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                    _ => Value::Undefined,
                };
                self.stack.push(result);
            }
            Value::VmNativeFunction(_) => {
                let result = self.read_named_property(ctx, &obj, &key);
                self.stack.push(result);
            }
            Value::Undefined => {
                let err = self.make_type_error_object(ctx, &format!("Cannot read properties of undefined (reading '{}')", key));
                self.handle_throw(ctx, &err)?;
                self.stack.push(Value::Undefined);
            }
            Value::Null => {
                let err = self.make_type_error_object(ctx, &format!("Cannot read properties of null (reading '{}')", key));
                self.handle_throw(ctx, &err)?;
                self.stack.push(Value::Undefined);
            }
            _ => {
                log::warn!("GetProperty on non-object: {}", value_to_string(&obj));
                self.stack.push(Value::Undefined);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetProperty
    fn run_opcode_set_property(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        let key = if let Value::String(s) = name_val {
            crate::unicode::utf16_to_utf8(s)
        } else {
            value_to_string(name_val)
        };
        let val = self.stack.pop().expect("VM Stack underflow on SetProperty (val)");
        let obj = self.stack.pop().expect("VM Stack underflow on SetProperty (obj)");
        // Private field brand check: setting a #-prefixed key on an object
        // that doesn't own it (no own property, no own setter, and not in proto chain) is a TypeError.
        // Also: private methods (on prototype, not own) are non-writable.
        // Getter-only accessors are also not writable.
        if key.starts_with(PRIVATE_KEY_PREFIX) {
            let kind = match &obj {
                Value::VmObject(map) => {
                    let b = map.borrow();
                    let has_own_key = b.contains_key(&key);
                    let has_own_setter = b.contains_key(&format!("__set_{}", key));
                    let has_own_getter = b.contains_key(&format!("__get_{}", key));
                    if has_own_key && !has_own_getter && !has_own_setter {
                        // Check readonly marker for private methods installed per-instance
                        let ro_key = format!("__readonly_{}__", key);
                        if matches!(b.get(&ro_key), Some(Value::Boolean(true))) {
                            PrivateKind::Method
                        } else {
                            PrivateKind::Field
                        }
                    } else if has_own_setter {
                        PrivateKind::AccessorWithSetter
                    } else if has_own_getter {
                        PrivateKind::GetterOnly
                    } else {
                        PrivateKind::NotFound
                    }
                }
                Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                    // Check overlay first (for per-evaluation class statics), then shared
                    let overlay = self.get_closure_overlay(&obj);
                    let props = self.get_fn_props(ctx, *ip, *arity);
                    let check_in =
                        |k: &str| -> bool { overlay.is_some_and(|o| o.borrow().contains_key(k)) || props.borrow().contains_key(k) };
                    if check_in(&key) && !check_in(&format!("__get_{}", key)) && !check_in(&format!("__set_{}", key)) {
                        let ro_key = format!("__readonly_{}__", key);
                        if overlay.is_some_and(|o| matches!(o.borrow().get(&ro_key), Some(Value::Boolean(true))))
                            || matches!(props.borrow().get(&ro_key), Some(Value::Boolean(true)))
                        {
                            PrivateKind::Method
                        } else {
                            PrivateKind::Field
                        }
                    } else if check_in(&format!("__set_{}", key)) {
                        PrivateKind::AccessorWithSetter
                    } else if check_in(&format!("__get_{}", key)) {
                        PrivateKind::GetterOnly
                    } else {
                        PrivateKind::NotFound
                    }
                }
                _ => PrivateKind::NotFound,
            };
            match kind {
                PrivateKind::Field | PrivateKind::AccessorWithSetter => {
                    // Allow - field is writable, accessor will use setter
                }
                PrivateKind::GetterOnly => {
                    let err = self.make_type_error_object(ctx, &format!("'{}' was defined without a setter", key));
                    self.handle_throw(ctx, &err)?;
                    self.stack.push(val);
                    return Ok(OpcodeAction::Continue);
                }
                PrivateKind::Method => {
                    let err = self.make_type_error_object(ctx, &format!("Cannot assign to private method {}", key));
                    self.handle_throw(ctx, &err)?;
                    self.stack.push(val);
                    return Ok(OpcodeAction::Continue);
                }
                PrivateKind::NotFound => {
                    let err = self.make_type_error_object(
                        ctx,
                        &format!("Cannot write private member {} to an object whose class did not declare it", key),
                    );
                    self.handle_throw(ctx, &err)?;
                    self.stack.push(val);
                    return Ok(OpcodeAction::Continue);
                }
            }
        }
        match self.assign_named_property(ctx, &obj, &key, &val, None) {
            Ok(result) => {
                self.stack.push(result);
                Ok(OpcodeAction::Continue)
            }
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                Err(err)
            }
        }
    }

    // Opcode::InitProperty
    fn run_opcode_init_property(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        let key = if let Value::String(s) = name_val {
            crate::unicode::utf16_to_utf8(s)
        } else {
            value_to_string(name_val)
        };
        let val = self.stack.pop().expect("VM Stack underflow on InitProperty (val)");
        let obj = self.stack.pop().expect("VM Stack underflow on InitProperty (obj)");
        // Private fields live on the exact object (including proxies) — no unwrapping.
        // Proxy traps are not triggered for private field init.
        match &obj {
            Value::VmObject(map) => {
                // Check for proxy: delegate to defineProperty trap (non-private and non-brand only)
                if !key.contains(PRIVATE_KEY_PREFIX) && !key.starts_with("__brand_") && map.borrow().contains_key("__proxy_target__") {
                    // Use assign_named_property for proxy to trigger set/defineProperty traps
                    let _ = self.assign_named_property(ctx, &obj, &key, &val, None)?;
                    self.stack.push(val);
                    return Ok(OpcodeAction::Continue);
                }
                let borrow = map.borrow();
                let is_frozen = matches!(borrow.get("__frozen__"), Some(Value::Boolean(true)));
                let is_non_ext = matches!(borrow.get("__non_extensible__"), Some(Value::Boolean(true)));
                let key_exists = borrow.contains_key(&key);
                drop(borrow);
                // Duplicate private member check (spec: PrivateFieldAdd / PrivateMethodOrAccessorAdd)
                let is_private = key.contains(PRIVATE_KEY_PREFIX);
                if is_private && key_exists {
                    let err =
                        self.make_type_error_object(ctx, "Cannot initialize private member in an object whose class already declared it");
                    self.handle_throw(ctx, &err)?;
                    self.stack.push(val);
                    return Ok(OpcodeAction::Continue);
                }
                if (is_frozen || (is_non_ext && !key_exists)) && !is_private {
                    let msg = if is_frozen {
                        format!("Cannot add property {}, object is frozen", key)
                    } else {
                        format!("Cannot add property {}, object is not extensible", key)
                    };
                    let err = self.make_type_error_object(ctx, &msg);
                    self.handle_throw(ctx, &err)?;
                    self.stack.push(val);
                    return Ok(OpcodeAction::Continue);
                }
                if is_non_ext && !key_exists && is_private {
                    let err = self.make_type_error_object(ctx, "Cannot add private member, object is not extensible");
                    self.handle_throw(ctx, &err)?;
                    self.stack.push(val);
                    return Ok(OpcodeAction::Continue);
                }
                let getter_key = format!("__get_{}", key);
                let setter_key = format!("__set_{}", key);
                let readonly_key = format!("__readonly_{}__", key);
                let nonenumerable_key = format!("__nonenumerable_{}__", key);
                let nonconfigurable_key = format!("__nonconfigurable_{}__", key);
                let mut borrow = map.borrow_mut(ctx);
                borrow.shift_remove(&getter_key);
                borrow.shift_remove(&setter_key);
                borrow.shift_remove(&readonly_key);
                borrow.shift_remove(&nonenumerable_key);
                borrow.shift_remove(&nonconfigurable_key);
                if key == "__proto__" {
                    borrow.insert(OWN_DUNDER_PROTO_DATA_KEY.to_string(), val.clone());
                } else {
                    borrow.insert(key, val.clone());
                }
                if let Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _) = &val {
                    self.fn_home_objects.insert(*ip, obj.clone());
                }
            }
            Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                // For closures with a per-evaluation overlay (class constructors after
                // ResetPrototype), write ALL properties to the overlay so that factory-
                // pattern re-evaluations don't share/overwrite each other's statics.
                // Brand keys always go to overlay; for plain VmFunction fall back to
                // shared fn_props.
                let closure_overlay = self.get_closure_overlay(&obj);
                let shared_props = self.get_fn_props(ctx, *ip, *arity);
                let target_props = closure_overlay.unwrap_or(shared_props);
                // For extensibility/duplicate checks, look in overlay first
                let is_private = key.contains(PRIVATE_KEY_PREFIX);
                if is_private {
                    let borrow = target_props.borrow();
                    // Check both overlay and shared fn_props for non-extensible flag
                    // (Object.preventExtensions writes to shared fn_props)
                    let is_non_ext = matches!(borrow.get("__non_extensible__"), Some(Value::Boolean(true)))
                        || matches!(shared_props.borrow().get("__non_extensible__"), Some(Value::Boolean(true)));
                    let key_exists = borrow.contains_key(&key);
                    drop(borrow);
                    if is_private && key_exists {
                        let err = self
                            .make_type_error_object(ctx, "Cannot initialize private member in an object whose class already declared it");
                        self.handle_throw(ctx, &err)?;
                        self.stack.push(val);
                        return Ok(OpcodeAction::Continue);
                    }
                    if is_non_ext && !key_exists {
                        let err = self.make_type_error_object(ctx, "Cannot add private member, object is not extensible");
                        self.handle_throw(ctx, &err)?;
                        self.stack.push(val);
                        return Ok(OpcodeAction::Continue);
                    }
                }
                // Write to per-closure overlay when available
                let getter_key = format!("__get_{}", key);
                let setter_key = format!("__set_{}", key);
                let readonly_key = format!("__readonly_{}__", key);
                let nonenumerable_key = format!("__nonenumerable_{}__", key);
                let nonconfigurable_key = format!("__nonconfigurable_{}__", key);
                let mut borrow = target_props.borrow_mut(ctx);
                borrow.shift_remove(&getter_key);
                borrow.shift_remove(&setter_key);
                borrow.shift_remove(&readonly_key);
                borrow.shift_remove(&nonenumerable_key);
                borrow.shift_remove(&nonconfigurable_key);
                borrow.insert(key, val.clone());
                if let Value::VmFunction(fn_ip, _) | Value::VmClosure(fn_ip, _, _) = &val {
                    self.fn_home_objects.insert(*fn_ip, obj.clone());
                }
            }
            _ => {
                let _ = self.assign_named_property(ctx, &obj, &key, &val, None)?;
            }
        }
        self.stack.push(val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetSuperProperty
    fn run_opcode_set_super_property(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        let key = if let Value::String(s) = name_val {
            crate::unicode::utf16_to_utf8(s)
        } else {
            value_to_string(name_val)
        };
        let val = self.stack.pop().expect("VM Stack underflow on SetSuperProperty (val)");
        let receiver = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
        let super_base_for_arrow = self
            .frames
            .last()
            .and_then(|frame| {
                if self.chunk.arrow_function_ips.contains(&frame.func_ip) && frame.upvalues.len() >= 3 {
                    Some(frame.upvalues[frame.upvalues.len() - 1].borrow().clone())
                } else {
                    None
                }
            })
            .filter(|v| !matches!(v, Value::Undefined | Value::Null));
        let Some(super_base) = super_base_for_arrow.or_else(|| self.ensure_super_base(ctx, &receiver)) else {
            return Ok(OpcodeAction::Continue);
        };
        let result = self.assign_named_property(ctx, &super_base, &key, &val, Some(&receiver))?;
        self.stack.push(result);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetSuperPropertyComputed
    fn run_opcode_set_super_property_computed(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let val = self.stack.pop().expect("VM Stack underflow on SetSuperPropertyComputed (val)");
        let key_val = self.stack.pop().expect("VM Stack underflow on SetSuperPropertyComputed (key)");
        let key = match self.as_property_key_string(ctx, &key_val) {
            Ok(k) => k,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        let receiver = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
        let super_base_for_arrow = self
            .frames
            .last()
            .and_then(|frame| {
                if self.chunk.arrow_function_ips.contains(&frame.func_ip) && frame.upvalues.len() >= 3 {
                    Some(frame.upvalues[frame.upvalues.len() - 1].borrow().clone())
                } else {
                    None
                }
            })
            .filter(|v| !matches!(v, Value::Undefined | Value::Null));
        let Some(super_base) = super_base_for_arrow.or_else(|| self.ensure_super_base(ctx, &receiver)) else {
            return Ok(OpcodeAction::Continue);
        };
        let result = self.assign_named_property(ctx, &super_base, &key, &val, Some(&receiver))?;
        self.stack.push(result);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetSuperProperty
    fn run_opcode_get_super_property(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        let key = if let Value::String(s) = name_val {
            crate::unicode::utf16_to_utf8(s)
        } else {
            value_to_string(name_val)
        };
        let receiver = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
        let super_base_for_arrow = self
            .frames
            .last()
            .and_then(|frame| {
                if self.chunk.arrow_function_ips.contains(&frame.func_ip) && frame.upvalues.len() >= 3 {
                    Some(frame.upvalues[frame.upvalues.len() - 1].borrow().clone())
                } else {
                    None
                }
            })
            .filter(|v| !matches!(v, Value::Undefined | Value::Null));
        let Some(super_base) = super_base_for_arrow.or_else(|| self.ensure_super_base(ctx, &receiver)) else {
            return Ok(OpcodeAction::Continue);
        };
        let value = self.read_named_property_with_receiver(ctx, &super_base, &key, &receiver);
        self.stack.push(value);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetSuperPropertyComputed
    fn run_opcode_get_super_property_computed(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let key_val = self.stack.pop().expect("VM Stack underflow on GetSuperPropertyComputed");
        let key = match self.as_property_key_string(ctx, &key_val) {
            Ok(k) => k,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        let receiver = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
        let super_base_for_arrow = self
            .frames
            .last()
            .and_then(|frame| {
                if self.chunk.arrow_function_ips.contains(&frame.func_ip) && frame.upvalues.len() >= 3 {
                    Some(frame.upvalues[frame.upvalues.len() - 1].borrow().clone())
                } else {
                    None
                }
            })
            .filter(|v| !matches!(v, Value::Undefined | Value::Null));
        let Some(super_base) = super_base_for_arrow.or_else(|| self.ensure_super_base(ctx, &receiver)) else {
            return Ok(OpcodeAction::Continue);
        };
        let value = self.read_named_property_with_receiver(ctx, &super_base, &key, &receiver);
        self.stack.push(value);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetIndex
    fn run_opcode_get_index(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let index = self.stack.pop().expect("VM Stack underflow on GetIndex (index)");
        let obj = self.stack.pop().expect("VM Stack underflow on GetIndex (obj)");

        if matches!(obj, Value::Null | Value::Undefined) {
            let err = self.make_type_error_object(ctx, "Cannot read properties of null or undefined");
            self.handle_throw(ctx, &err)?;
            return Ok(OpcodeAction::Continue);
        }

        let coerced_key = match self.as_property_key_string(ctx, &index) {
            Ok(key) => key,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        match self.try_proxy_get(ctx, &obj, &coerced_key, None) {
            Ok(Some(v)) => {
                self.stack.push(v);
                return Ok(OpcodeAction::Continue);
            }
            Ok(None) => {}
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        }
        match &obj {
            Value::VmArray(arr) => {
                let maybe_typed = {
                    let a = arr.borrow();
                    let buffer = a.props.get("__typedarray_buffer__").cloned();
                    let byte_offset = a
                        .props
                        .get("__byte_offset__")
                        .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                        .unwrap_or(0);
                    let bpe = a
                        .props
                        .get("__bytes_per_element__")
                        .and_then(|v| {
                            if let Value::Number(n) = v {
                                Some((*n as usize).max(1))
                            } else {
                                None
                            }
                        })
                        .unwrap_or(1);
                    let fixed_length = a
                        .props
                        .get("__fixed_length__")
                        .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None });
                    let length_tracking = matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true)));
                    buffer.map(|b| (b, byte_offset, bpe, fixed_length, length_tracking))
                };

                if let Some((Value::VmObject(buf_obj), byte_offset, bpe, fixed_length, length_tracking)) = maybe_typed {
                    let byte_len = {
                        let b = buf_obj.borrow();
                        b.get("byteLength")
                            .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                            .unwrap_or(0)
                    };

                    let out_of_bounds = if let Some(fixed) = fixed_length {
                        byte_len < byte_offset.saturating_add(fixed.saturating_mul(bpe))
                    } else if length_tracking {
                        byte_len < byte_offset
                    } else {
                        false
                    };

                    if out_of_bounds {
                        // Only throw for numeric element access, not for property access
                        if let Value::Number(n) = &index {
                            let i = *n as usize;
                            if *n >= 0.0 && *n == (i as f64) {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                                err_map.insert("message".to_string(), Value::from("TypedArray view is out of bounds"));
                                self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                                return Ok(OpcodeAction::Continue);
                            }
                        }
                    }

                    // Read element from shared buffer bytes for buffer-backed TypedArrays
                    if let Value::Number(n) = &index {
                        let i = *n as usize;
                        if *n >= 0.0 && *n == (i as f64) {
                            let ta_name = arr
                                .borrow()
                                .props
                                .get("__typedarray_name__")
                                .map(|v| value_to_string(v))
                                .unwrap_or_default();
                            if let Some(Value::VmArray(buf_bytes)) = buf_obj.borrow().get("__buffer_bytes__").cloned() {
                                let bb = buf_bytes.borrow();
                                let base = byte_offset + i * bpe;
                                let in_range = base + bpe <= bb.elements.len();
                                if in_range {
                                    let val = match ta_name.as_str() {
                                        "Uint8Array" | "Uint8ClampedArray" => {
                                            let b = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number(b as f64)
                                        }
                                        "Int8Array" => {
                                            let b = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number((b as i8) as f64)
                                        }
                                        "Uint16Array" => {
                                            let b0 = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            let b1 = to_number(bb.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number(u16::from_ne_bytes([b0, b1]) as f64)
                                        }
                                        "Int16Array" => {
                                            let b0 = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            let b1 = to_number(bb.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number(i16::from_ne_bytes([b0, b1]) as f64)
                                        }
                                        "Uint32Array" => {
                                            let arr4: [u8; 4] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::Number(u32::from_ne_bytes(arr4) as f64)
                                        }
                                        "Int32Array" => {
                                            let arr4: [u8; 4] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::Number(i32::from_ne_bytes(arr4) as f64)
                                        }
                                        "Float32Array" => {
                                            let arr4: [u8; 4] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::Number(f32::from_ne_bytes(arr4) as f64)
                                        }
                                        "Float64Array" => {
                                            let arr8: [u8; 8] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::Number(f64::from_ne_bytes(arr8))
                                        }
                                        "BigInt64Array" => {
                                            let arr8: [u8; 8] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::BigInt(Box::new(num_bigint::BigInt::from(i64::from_ne_bytes(arr8))))
                                        }
                                        "BigUint64Array" => {
                                            let arr8: [u8; 8] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::BigInt(Box::new(num_bigint::BigInt::from(u64::from_ne_bytes(arr8))))
                                        }
                                        _ => {
                                            let b = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number(b as f64)
                                        }
                                    };
                                    self.stack.push(val);
                                    return Ok(OpcodeAction::Continue);
                                } else {
                                    // Out of bounds → undefined (don't fall to prototype)
                                    self.stack.push(Value::Undefined);
                                    return Ok(OpcodeAction::Continue);
                                }
                            }
                        } else {
                            // Non-integer numeric index (e.g. 1.5, -0) → undefined
                            self.stack.push(Value::Undefined);
                            return Ok(OpcodeAction::Continue);
                        }
                    }

                    // String key that is a canonical numeric index
                    if let Some(numeric_index) = Self::canonical_numeric_index_string(&coerced_key) {
                        // Valid non-negative integer → try buffer read
                        if numeric_index >= 0.0
                            && numeric_index.fract() == 0.0
                            && !numeric_index.is_nan()
                            && numeric_index != f64::INFINITY
                            && !(numeric_index == 0.0 && numeric_index.is_sign_negative())
                        {
                            let i = numeric_index as usize;
                            let ta_name = arr
                                .borrow()
                                .props
                                .get("__typedarray_name__")
                                .map(|v| value_to_string(v))
                                .unwrap_or_default();
                            if let Some(Value::VmArray(buf_bytes)) = buf_obj.borrow().get("__buffer_bytes__").cloned() {
                                let bb = buf_bytes.borrow();
                                let base = byte_offset + i * bpe;
                                let in_range = base + bpe <= bb.elements.len();
                                if in_range {
                                    let val = match ta_name.as_str() {
                                        "Uint8Array" | "Uint8ClampedArray" => {
                                            let b = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number(b as f64)
                                        }
                                        "Int8Array" => {
                                            let b = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number((b as i8) as f64)
                                        }
                                        "Uint16Array" => {
                                            let b0 = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            let b1 = to_number(bb.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number(u16::from_ne_bytes([b0, b1]) as f64)
                                        }
                                        "Int16Array" => {
                                            let b0 = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            let b1 = to_number(bb.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number(i16::from_ne_bytes([b0, b1]) as f64)
                                        }
                                        "Uint32Array" => {
                                            let arr4: [u8; 4] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::Number(u32::from_ne_bytes(arr4) as f64)
                                        }
                                        "Int32Array" => {
                                            let arr4: [u8; 4] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::Number(i32::from_ne_bytes(arr4) as f64)
                                        }
                                        "Float32Array" => {
                                            let arr4: [u8; 4] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::Number(f32::from_ne_bytes(arr4) as f64)
                                        }
                                        "Float64Array" => {
                                            let arr8: [u8; 8] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::Number(f64::from_ne_bytes(arr8))
                                        }
                                        "BigInt64Array" => {
                                            let arr8: [u8; 8] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::BigInt(Box::new(num_bigint::BigInt::from(i64::from_ne_bytes(arr8))))
                                        }
                                        "BigUint64Array" => {
                                            let arr8: [u8; 8] = core::array::from_fn(|j| {
                                                to_number(bb.elements.get(base + j).unwrap_or(&Value::Number(0.0))) as u8
                                            });
                                            Value::BigInt(Box::new(num_bigint::BigInt::from(u64::from_ne_bytes(arr8))))
                                        }
                                        _ => {
                                            let b = to_number(bb.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                                            Value::Number(b as f64)
                                        }
                                    };
                                    self.stack.push(val);
                                    return Ok(OpcodeAction::Continue);
                                }
                            }
                        }
                        // Invalid canonical numeric index (non-integer, -0, OOB) → undefined
                        self.stack.push(Value::Undefined);
                        return Ok(OpcodeAction::Continue);
                    }
                }

                if let Value::Number(n) = &index {
                    let i = *n as usize;
                    if *n >= 0.0 && *n == (i as f64) {
                        let live_iter = arr.borrow().props.get("__forof_live_iterator__").cloned();
                        if let Some(Value::VmObject(iter_obj)) = live_iter {
                            let next_fn = self.read_named_property(ctx, &Value::VmObject(iter_obj), "next");
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(ctx, &thrown)?;
                                return Ok(OpcodeAction::Continue);
                            }
                            let next_result = match self.vm_call_function_value(ctx, &next_fn, &Value::VmObject(iter_obj), &[]) {
                                Ok(v) => v,
                                Err(err) => {
                                    self.set_pending_throw_from_error(&err);
                                    if let Some(thrown) = self.pending_throw.take() {
                                        self.handle_throw(ctx, &thrown)?;
                                        return Ok(OpcodeAction::Continue);
                                    }
                                    self.stack.push(Value::Undefined);
                                    return Ok(OpcodeAction::Continue);
                                }
                            };
                            let done = self.read_named_property(ctx, &next_result, "done").to_truthy();
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(ctx, &thrown)?;
                                return Ok(OpcodeAction::Continue);
                            }
                            if done {
                                self.stack.push(Value::Undefined);
                            } else {
                                let value = self.read_named_property(ctx, &next_result, "value");
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(ctx, &thrown)?;
                                    return Ok(OpcodeAction::Continue);
                                }
                                self.stack.push(value);
                            }
                            return Ok(OpcodeAction::Continue);
                        }
                        let val = self.read_named_property(ctx, &obj, &coerced_key);
                        self.stack.push(val);
                    } else {
                        let val = self.read_named_property(ctx, &obj, &coerced_key);
                        self.stack.push(val);
                    }
                } else {
                    if let Ok(i) = coerced_key.parse::<usize>() {
                        let _ = i;
                        let val = self.read_named_property(ctx, &obj, &coerced_key);
                        self.stack.push(val);
                    } else if coerced_key == "@@sym:1" {
                        let val = self.read_named_property(ctx, &obj, &coerced_key);
                        self.stack.push(val);
                    } else if coerced_key == "@@sym:4" {
                        // Symbol.toStringTag for arrays
                        if let Value::VmArray(arr_ref) = &obj {
                            let b = arr_ref.borrow();
                            if let Some(ta_name) = b.props.get("__typedarray_name__") {
                                self.stack.push(ta_name.clone());
                            } else {
                                self.stack.push(Value::from("Array"));
                            }
                        } else {
                            self.stack.push(Value::from("Array"));
                        }
                    } else {
                        let val = self.read_named_property(ctx, &obj, &coerced_key);
                        self.stack.push(val);
                    }
                }
            }
            Value::VmObject(_map) => {
                // Use read_named_property for proto chain lookup (needed for symbol keys)
                let val = self.read_named_property(ctx, &obj, &coerced_key);
                if matches!(val, Value::Undefined) {
                    // Fall back to boxed type handling for symbol keys
                    if let Value::VmObject(ref m) = obj {
                        let type_tag = m.borrow().get("__type__").and_then(|v| {
                            if let Value::String(s) = v {
                                Some(crate::unicode::utf16_to_utf8(s))
                            } else {
                                None
                            }
                        });
                        match (type_tag.as_deref(), coerced_key.as_str()) {
                            (Some("String"), "@@sym:1") => {
                                self.stack.push(Self::make_bound_host_fn(ctx, "string.symbolIterator", &obj));
                                return Ok(OpcodeAction::Continue);
                            }
                            (Some("String"), "@@sym:4") => {
                                self.stack.push(Value::from("String"));
                                return Ok(OpcodeAction::Continue);
                            }
                            _ => {}
                        }
                    }
                }
                self.stack.push(val);
            }
            Value::String(s) => {
                if coerced_key == "@@sym:1" {
                    // Symbol.iterator on string — return a bound string iterator factory
                    self.stack.push(Self::make_bound_host_fn(ctx, "string.symbolIterator", &obj));
                } else {
                    let _ = s;
                    let val = self.read_named_property(ctx, &obj, &coerced_key);
                    self.stack.push(val);
                }
            }
            Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                let overlay = self.get_closure_overlay(&obj);
                let shared = self.get_fn_props(ctx, *ip, *arity);
                // Lookup helper: overlay first, then shared fn_props
                let lookup = |k: &str| -> Option<Value<'gc>> {
                    overlay
                        .and_then(|o| o.borrow().get(k).cloned())
                        .or_else(|| shared.borrow().get(k).cloned())
                };
                let getter_key = format!("__get_{}", coerced_key);
                let val = if let Some(getter_fn) = lookup(&getter_key) {
                    match getter_fn {
                        Value::VmFunction(gip, _) => {
                            self.this_stack.push(obj.clone());
                            let result = self.call_vm_function_result(ctx, gip, &[], None, &[]);
                            self.this_stack.pop();
                            match result {
                                Ok(v) => v,
                                Err(err) => {
                                    self.set_pending_throw_from_error(&err);
                                    Value::Undefined
                                }
                            }
                        }
                        Value::VmClosure(gip, _, ups) => {
                            self.this_stack.push(obj.clone());
                            let result = self.call_vm_function_result(ctx, gip, &[], None, &ups);
                            self.this_stack.pop();
                            match result {
                                Ok(v) => v,
                                Err(err) => {
                                    self.set_pending_throw_from_error(&err);
                                    Value::Undefined
                                }
                            }
                        }
                        _ => self.invoke_getter_with_receiver(ctx, &getter_fn, &obj),
                    }
                } else if let Some(v) = lookup(&coerced_key) {
                    v
                } else {
                    let setter_key = format!("__set_{}", coerced_key);
                    if lookup(&setter_key).is_some() {
                        Value::Undefined
                    } else {
                        let proto = lookup("__proto__");
                        match coerced_key.as_str() {
                            "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                            "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                            "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                            _ => match proto {
                                Some(ref p) => self.read_named_property_with_receiver(ctx, p, &coerced_key, &obj),
                                None => Value::Undefined,
                            },
                        }
                    }
                };
                self.stack.push(val);
            }
            Value::VmNativeFunction(_) => {
                // Keep bracket access on native functions consistent with named-property access.
                let val = self.read_named_property(ctx, &obj, &coerced_key);
                self.stack.push(val);
            }
            Value::Function(name) => {
                let val = match coerced_key.as_str() {
                    "name" => Value::from(name),
                    "length" => Value::Number(1.0),
                    "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                    "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                    "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                    _ => Value::Undefined,
                };
                self.stack.push(val);
            }
            _ => {
                let val = self.read_named_property(ctx, &obj, &coerced_key);
                self.stack.push(val);
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetIndex
    fn run_opcode_set_index(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let val = self.stack.pop().expect("VM Stack underflow on SetIndex (val)");
        let index = self.stack.pop().expect("VM Stack underflow on SetIndex (index)");
        let obj = self.stack.pop().expect("VM Stack underflow on SetIndex (obj)");
        let _is_strict = self.current_execution_is_strict();
        let coerced_key = match self.as_property_key_string(ctx, &index) {
            Ok(key) => key,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        match &obj {
            Value::VmArray(_) => match self.assign_named_property(ctx, &obj, &coerced_key, &val, None) {
                Ok(_) => {}
                Err(err) => {
                    self.set_pending_throw_from_error(&err);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                    return Err(err);
                }
            },
            Value::VmObject(map) => {
                self.maybe_infer_function_name_from_key(ctx, &coerced_key, Some(&index), &val);
                let _ = map;
                match self.assign_named_property(ctx, &obj, &coerced_key, &val, None) {
                    Ok(_) => {}
                    Err(err) => {
                        self.set_pending_throw_from_error(&err);
                        if let Some(thrown) = self.pending_throw.take() {
                            self.handle_throw(ctx, &thrown)?;
                            return Ok(OpcodeAction::Continue);
                        }
                        return Err(err);
                    }
                }
            }
            Value::VmFunction(_, _) | Value::VmClosure(_, _, _) => {
                self.maybe_infer_function_name_from_key(ctx, &coerced_key, Some(&index), &val);
                match self.assign_named_property(ctx, &obj, &coerced_key, &val, None) {
                    Ok(_) => {}
                    Err(err) => {
                        self.set_pending_throw_from_error(&err);
                        if let Some(thrown) = self.pending_throw.take() {
                            self.handle_throw(ctx, &thrown)?;
                            return Ok(OpcodeAction::Continue);
                        }
                        return Err(err);
                    }
                }
            }
            _ => match self.assign_named_property(ctx, &obj, &coerced_key, &val, None) {
                Ok(_) => {}
                Err(err) => {
                    self.set_pending_throw_from_error(&err);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                    return Err(err);
                }
            },
        }
        self.stack.push(val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::DefineComputedMethod — like SetIndex but also marks the property non-enumerable.
    // Used for class computed methods which must be non-enumerable per spec.
    fn run_opcode_define_computed_method(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let val = self.stack.pop().expect("VM Stack underflow on DefineComputedMethod (val)");
        let index = self.stack.pop().expect("VM Stack underflow on DefineComputedMethod (index)");
        let obj = self.stack.pop().expect("VM Stack underflow on DefineComputedMethod (obj)");
        let coerced_key = match self.as_property_key_string(ctx, &index) {
            Ok(key) => key,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        self.maybe_infer_function_name_from_key(ctx, &coerced_key, Some(&index), &val);
        match &obj {
            Value::VmObject(map) => {
                let ne_key = format!("__nonenumerable_{}__", coerced_key);
                let mut borrow = map.borrow_mut(ctx);
                // Remove any prior accessor or property flags for this key
                borrow.shift_remove(&format!("__get_{}", coerced_key));
                borrow.shift_remove(&format!("__set_{}", coerced_key));
                borrow.insert(coerced_key, val.clone());
                borrow.insert(ne_key, Value::Boolean(true));
            }
            Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                // Static computed method named "prototype" is forbidden on class constructors
                if coerced_key == "prototype" {
                    let err = self.make_type_error_object(ctx, "Classes may not have a static property named 'prototype'");
                    self.handle_throw(ctx, &err)?;
                    self.stack.push(val);
                    return Ok(OpcodeAction::Continue);
                }
                let props = self.get_fn_props(ctx, *ip, *arity);
                let ne_key = format!("__nonenumerable_{}__", coerced_key);
                let mut borrow = props.borrow_mut(ctx);
                borrow.shift_remove(&format!("__get_{}", coerced_key));
                borrow.shift_remove(&format!("__set_{}", coerced_key));
                borrow.insert(coerced_key, val.clone());
                borrow.insert(ne_key, Value::Boolean(true));
            }
            _ => match self.assign_named_property(ctx, &obj, &coerced_key, &val, None) {
                Ok(_) => {}
                Err(err) => {
                    self.set_pending_throw_from_error(&err);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                    return Err(err);
                }
            },
        }
        self.stack.push(val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::InitIndex
    fn run_opcode_init_index(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let val = self.stack.pop().expect("VM Stack underflow on InitIndex (val)");
        let index = self.stack.pop().expect("VM Stack underflow on InitIndex (index)");
        let obj = self.stack.pop().expect("VM Stack underflow on InitIndex (obj)");
        let coerced_key = match self.as_property_key_string(ctx, &index) {
            Ok(key) => key,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        self.maybe_infer_function_name_from_key(ctx, &coerced_key, Some(&index), &val);
        match &obj {
            Value::VmObject(map) => {
                let getter_key = format!("__get_{}", coerced_key);
                let setter_key = format!("__set_{}", coerced_key);
                let readonly_key = format!("__readonly_{}__", coerced_key);
                let nonenumerable_key = format!("__nonenumerable_{}__", coerced_key);
                let nonconfigurable_key = format!("__nonconfigurable_{}__", coerced_key);
                let mut borrow = map.borrow_mut(ctx);
                borrow.shift_remove(&getter_key);
                borrow.shift_remove(&setter_key);
                borrow.shift_remove(&readonly_key);
                borrow.shift_remove(&nonenumerable_key);
                borrow.shift_remove(&nonconfigurable_key);
                if coerced_key == "__proto__" {
                    borrow.insert(OWN_DUNDER_PROTO_DATA_KEY.to_string(), val.clone());
                } else {
                    borrow.insert(coerced_key, val.clone());
                }
            }
            Value::VmFunction(_ip, _) | Value::VmClosure(_ip, _, _) => {
                let (ip_val, arity_val) = match &obj {
                    Value::VmFunction(ip, a) => (*ip, *a),
                    Value::VmClosure(ip, a, _) => (*ip, *a),
                    _ => unreachable!(),
                };
                let fn_props = self.get_fn_props(ctx, ip_val, arity_val);
                let nonconfigurable_key = format!("__nonconfigurable_{}__", coerced_key);
                let has_nonconf = fn_props.borrow().contains_key(&nonconfigurable_key);
                if has_nonconf {
                    let mut err_map = IndexMap::new();
                    err_map.insert("__type__".to_string(), Value::from("TypeError"));
                    err_map.insert(
                        "message".to_string(),
                        Value::from(&format!("Cannot redefine property: {}", coerced_key)),
                    );
                    let err_obj = Value::VmObject(new_gc_cell_ptr(ctx, err_map));
                    self.handle_throw(ctx, &err_obj)?;
                    return Ok(OpcodeAction::Continue);
                }
                let getter_key = format!("__get_{}", coerced_key);
                let setter_key = format!("__set_{}", coerced_key);
                let readonly_key = format!("__readonly_{}__", coerced_key);
                let nonenumerable_key = format!("__nonenumerable_{}__", coerced_key);
                let mut borrow = fn_props.borrow_mut(ctx);
                borrow.shift_remove(&getter_key);
                borrow.shift_remove(&setter_key);
                borrow.shift_remove(&readonly_key);
                borrow.shift_remove(&nonenumerable_key);
                borrow.insert(coerced_key, val.clone());
            }
            _ => {
                let _ = self.assign_named_property(ctx, &obj, &coerced_key, &val, None)?;
            }
        }
        self.stack.push(val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetComputedGetter
    fn run_opcode_set_computed_getter(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [obj, computed_key, val] → pop val, pop key, peek obj
        let val = self.stack.pop().expect("VM Stack underflow on SetComputedGetter (val)");
        let index = self.stack.pop().expect("VM Stack underflow on SetComputedGetter (index)");
        let obj = self.stack.pop().expect("VM Stack underflow on SetComputedGetter (obj)");
        let coerced_key = match self.as_property_key_string(ctx, &index) {
            Ok(key) => key,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        self.maybe_infer_accessor_function_name_from_key(ctx, "get", &coerced_key, Some(&index), &val);
        let getter_key = format!("__get_{}", coerced_key);
        let nonconfigurable_key = format!("__nonconfigurable_{}__", coerced_key);
        if let Value::VmObject(map) = &obj {
            let has_nonconf = map.borrow().contains_key(&nonconfigurable_key);
            if has_nonconf {
                let mut err_map = IndexMap::new();
                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                err_map.insert(
                    "message".to_string(),
                    Value::from(&format!("Cannot redefine property: {}", coerced_key)),
                );
                let err_obj = Value::VmObject(new_gc_cell_ptr(ctx, err_map));
                self.handle_throw(ctx, &err_obj)?;
                return Ok(OpcodeAction::Continue);
            }
            let mut borrow = map.borrow_mut(ctx);
            if coerced_key == "__proto__" {
                borrow.shift_remove(OWN_DUNDER_PROTO_DATA_KEY);
            } else {
                borrow.shift_remove(&coerced_key);
            }
            borrow.insert(getter_key, val.clone());
            // Class computed getters are non-enumerable
            borrow.insert(format!("__nonenumerable_{}__", coerced_key), Value::Boolean(true));
        } else if let Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) = &obj {
            let props = self.get_fn_props(ctx, *ip, *arity);
            let has_nonconf = props.borrow().contains_key(&nonconfigurable_key);
            if has_nonconf {
                let mut err_map = IndexMap::new();
                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                err_map.insert(
                    "message".to_string(),
                    Value::from(&format!("Cannot redefine property: {}", coerced_key)),
                );
                let err_obj = Value::VmObject(new_gc_cell_ptr(ctx, err_map));
                self.handle_throw(ctx, &err_obj)?;
                return Ok(OpcodeAction::Continue);
            }
            let mut borrow = props.borrow_mut(ctx);
            borrow.shift_remove(&coerced_key);
            borrow.insert(getter_key, val.clone());
            borrow.insert(format!("__nonenumerable_{}__", coerced_key), Value::Boolean(true));
        }
        self.stack.push(val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetComputedSetter
    fn run_opcode_set_computed_setter(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [obj, computed_key, val] → pop val, pop key, peek obj
        let val = self.stack.pop().expect("VM Stack underflow on SetComputedSetter (val)");
        let index = self.stack.pop().expect("VM Stack underflow on SetComputedSetter (index)");
        let obj = self.stack.pop().expect("VM Stack underflow on SetComputedSetter (obj)");
        let coerced_key = match self.as_property_key_string(ctx, &index) {
            Ok(key) => key,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        self.maybe_infer_accessor_function_name_from_key(ctx, "set", &coerced_key, Some(&index), &val);
        let setter_key = format!("__set_{}", coerced_key);
        let nonconfigurable_key = format!("__nonconfigurable_{}__", coerced_key);
        if let Value::VmObject(map) = &obj {
            let has_nonconf = map.borrow().contains_key(&nonconfigurable_key);
            if has_nonconf {
                let mut err_map = IndexMap::new();
                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                err_map.insert(
                    "message".to_string(),
                    Value::from(&format!("Cannot redefine property: {}", coerced_key)),
                );
                let err_obj = Value::VmObject(new_gc_cell_ptr(ctx, err_map));
                self.handle_throw(ctx, &err_obj)?;
                return Ok(OpcodeAction::Continue);
            }
            let mut borrow = map.borrow_mut(ctx);
            if coerced_key == "__proto__" {
                borrow.shift_remove(OWN_DUNDER_PROTO_DATA_KEY);
            } else {
                borrow.shift_remove(&coerced_key);
            }
            borrow.insert(setter_key, val.clone());
            borrow.insert(format!("__nonenumerable_{}__", coerced_key), Value::Boolean(true));
        } else if let Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) = &obj {
            let props = self.get_fn_props(ctx, *ip, *arity);
            let has_nonconf = props.borrow().contains_key(&nonconfigurable_key);
            if has_nonconf {
                let mut err_map = IndexMap::new();
                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                err_map.insert(
                    "message".to_string(),
                    Value::from(&format!("Cannot redefine property: {}", coerced_key)),
                );
                let err_obj = Value::VmObject(new_gc_cell_ptr(ctx, err_map));
                self.handle_throw(ctx, &err_obj)?;
                return Ok(OpcodeAction::Continue);
            }
            let mut borrow = props.borrow_mut(ctx);
            borrow.shift_remove(&coerced_key);
            borrow.insert(setter_key, val.clone());
            borrow.insert(format!("__nonenumerable_{}__", coerced_key), Value::Boolean(true));
        }
        self.stack.push(val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ToPropertyKey
    fn run_opcode_to_property_key(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let raw = self.stack.pop().expect("VM Stack underflow on ToPropertyKey");
        let coerced_key = match self.as_property_key_string(ctx, &raw) {
            Ok(key) => key,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        self.stack.push(Value::from(coerced_key.as_str()));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Increment
    fn run_opcode_increment(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let a = self.stack.pop().expect("VM Stack underflow on Increment");
        match a {
            Value::Number(n) => self.stack.push(Value::Number(n + 1.0)),
            Value::BigInt(b) => self.stack.push(Value::BigInt(Box::new(&*b + 1))),
            _ => self.stack.push(Value::Number(f64::NAN)),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Decrement
    fn run_opcode_decrement(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let a = self.stack.pop().expect("VM Stack underflow on Decrement");
        match a {
            Value::Number(n) => self.stack.push(Value::Number(n - 1.0)),
            Value::BigInt(b) => self.stack.push(Value::BigInt(Box::new(&*b - 1))),
            _ => self.stack.push(Value::Number(f64::NAN)),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Throw
    fn run_opcode_throw(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let thrown = self.stack.pop().unwrap_or(Value::Undefined);
        // Record the throw-site IP for accurate line-number reporting.
        self.last_throw_ip = Some(self.current_opcode_ip);
        // diagnostic logging
        log::warn!("Throw opcode value={}", self.vm_to_string(ctx, &thrown));
        if let Value::VmObject(obj) = &thrown {
            let keys: Vec<String> = obj.borrow().keys().cloned().collect();
            log::warn!("Thrown object keys={:?}", keys);
        }
        self.handle_throw(ctx, &thrown)?;
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ThrowTypeError
    fn run_opcode_throw_type_error(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let msg = self.stack.pop().unwrap_or(Value::Undefined);
        let msg_str = self.vm_to_string(ctx, &msg);
        let err = self.make_type_error_object(ctx, &msg_str);
        self.handle_throw(ctx, &err)?;
        Ok(OpcodeAction::Continue)
    }

    // Opcode::SetupTry
    fn run_opcode_setup_try(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let catch_ip = self.read_u16() as usize;
        let binding_idx = self.read_u16();
        let catch_binding = if binding_idx == 0xffff {
            None
        } else {
            let name_val = &self.chunk.constants[binding_idx as usize];
            if let Value::String(s) = name_val {
                Some(crate::unicode::utf16_to_utf8(s))
            } else {
                None
            }
        };
        self.try_stack.push(TryFrame {
            catch_ip,
            stack_depth: self.stack.len(),
            frame_depth: self.frames.len(),
            catch_binding,
        });
        Ok(OpcodeAction::Continue)
    }

    // Opcode::TeardownTry
    fn run_opcode_teardown_try(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        self.try_stack.pop();
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetThis
    fn run_opcode_get_this(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Check TDZ: if the current (or enclosing) constructor frame has this_tdz set,
        // `this` is not yet initialized (super() not called).
        // For non-arrow functions, only check the current frame — they have their own `this`
        // and should never be affected by an enclosing constructor's TDZ.
        // For arrow functions, walk backwards to find the enclosing constructor's TDZ state,
        // since arrows inherit `this` from the enclosing scope.
        let current_is_arrow = self
            .frames
            .last()
            .map(|f| self.chunk.arrow_function_ips.contains(&f.func_ip))
            .unwrap_or(false);
        if current_is_arrow {
            for frame in self.frames.iter().rev() {
                if frame.this_tdz == Some(true) {
                    let err = self.make_reference_error(
                        ctx,
                        "Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
                    );
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                if frame.this_tdz.is_some() {
                    break;
                }
            }
        } else if let Some(frame) = self.frames.last()
            && frame.this_tdz == Some(true)
        {
            let err = self.make_reference_error(
                ctx,
                "Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
            );
            self.handle_throw(ctx, &err)?;
            return Ok(OpcodeAction::Continue);
        }
        // Arrow functions: use the captured `this` stored as the last upvalue
        // by MakeClosure, rather than the dynamic this_stack.
        if let Some(frame) = self.frames.last()
            && self.chunk.arrow_function_ips.contains(&frame.func_ip)
            && !frame.upvalues.is_empty()
        {
            let captured_idx = if frame.upvalues.len() >= 3 {
                frame.upvalues.len() - 3
            } else {
                frame.upvalues.len().saturating_sub(2)
            };
            let captured = frame.upvalues[captured_idx].borrow().clone();
            self.stack.push(captured);
            return Ok(OpcodeAction::Continue);
        }
        let this_val = self.this_stack.last().cloned().unwrap_or(Value::VmObject(self.global_this));
        self.stack.push(this_val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetThisSuper — like GetThis but skips TDZ check (used for super() receiver)
    fn run_opcode_get_this_super(&mut self, _ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let this_val = self.this_stack.last().cloned().unwrap_or(Value::VmObject(self.global_this));
        self.stack.push(this_val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ClearThisTdz — clear the this_tdz flag after super() returns
    // Throws ReferenceError if this is already initialized (double super() call)
    // Also updates `this` to the super() return value if it's an object (spec: BindThisValue)
    fn run_opcode_clear_this_tdz(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        for frame in self.frames.iter_mut().rev() {
            match frame.this_tdz {
                Some(true) => {
                    frame.this_tdz = Some(false);
                    // Mark that super() was called for this constructor
                    if let Some(flag) = self.super_called_stack.last_mut() {
                        *flag = true;
                    }
                    // If super() returned an object, bind it as `this`
                    if let Some(result) = self.stack.last() {
                        let is_object = match result {
                            Value::VmObject(m) => !m.borrow().contains_key("__vm_symbol__"),
                            Value::VmArray(_)
                            | Value::VmMap(_)
                            | Value::VmSet(_)
                            | Value::VmFunction(..)
                            | Value::VmClosure(..)
                            | Value::VmNativeFunction(_)
                            | Value::Function(_) => true,
                            _ => false,
                        };
                        if is_object {
                            // Patch __proto__ to new_target.prototype for
                            // VmArray/VmMap/VmSet returned by native super()
                            if let Some(new_target) = self.new_target_stack.last().cloned() {
                                let proto = self.read_named_property(ctx, &new_target, "prototype");
                                if self.pending_throw.is_some() {
                                    self.pending_throw.take();
                                }
                                let is_valid_proto = matches!(
                                    &proto,
                                    Value::VmObject(_)
                                        | Value::VmArray(_)
                                        | Value::VmMap(_)
                                        | Value::VmSet(_)
                                        | Value::VmFunction(..)
                                        | Value::VmClosure(..)
                                );
                                if is_valid_proto {
                                    match self.stack.last() {
                                        Some(Value::VmArray(arr)) => {
                                            arr.borrow_mut(ctx).props.insert("__proto__".to_string(), proto);
                                        }
                                        Some(Value::VmObject(obj)) => {
                                            obj.borrow_mut(ctx).insert("__proto__".to_string(), proto);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            if let Some(this_ref) = self.this_stack.last_mut() {
                                // Re-read stack.last() since we may have modified its props
                                if let Some(result) = self.stack.last() {
                                    *this_ref = result.clone();
                                }
                            }
                        }
                    }
                    return Ok(OpcodeAction::Continue);
                }
                Some(false) => {
                    // TDZ already cleared → double super() call
                    let err = self.make_reference_error(ctx, "Super constructor may only be called once");
                    self.handle_throw(ctx, &err)?;
                    return Ok(OpcodeAction::Continue);
                }
                None => {
                    // Not a derived ctor frame; keep walking to find
                    // the enclosing constructor's TDZ frame.
                }
            }
        }
        // No derived ctor frame found — no-op (e.g., parent ctor called via super())
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetKeys
    fn run_opcode_get_keys(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let obj = self.stack.pop().expect("VM Stack underflow on GetKeys");
        let mut out_keys: Vec<Value<'gc>> = Vec::new();
        let mut seen_any: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut current = Some(obj.clone());
        let mut depth = 0;

        while let Some(cur) = current {
            if depth > 128 {
                break;
            }
            depth += 1;

            match cur {
                Value::VmObject(map) => {
                    let mut own_keys: Vec<String> = Vec::new();
                    let key_values = self.call_builtin(ctx, BUILTIN_OBJECT_KEYS, &[Value::VmObject(map)]);
                    if let Value::VmArray(arr) = key_values {
                        for value in arr.borrow().iter() {
                            if let Ok(key) = self.as_property_key_string(ctx, value) {
                                own_keys.push(key);
                            }
                        }
                    }

                    for key in own_keys {
                        let first_seen = seen_any.insert(key.clone());
                        if !first_seen {
                            continue;
                        }
                        out_keys.push(Value::from(&key));
                    }
                    let borrow = map.borrow();
                    let mut next = borrow.get("__proto__").cloned();
                    if next.is_none()
                        && let Some(Value::String(type_name_u16)) = borrow.get("__type__")
                    {
                        let type_name = crate::unicode::utf16_to_utf8(type_name_u16);
                        if let Some(Value::VmObject(ctor)) = self.globals.get(&type_name)
                            && let Some(proto) = ctor.borrow().get("prototype").cloned()
                        {
                            next = Some(proto);
                        }
                    }
                    if next.is_none()
                        && let Some(Value::VmObject(object_ctor)) = self.globals.get("Object")
                        && let Some(Value::VmObject(obj_proto)) = object_ctor.borrow().get("prototype").cloned()
                        && !Gc::ptr_eq(map, obj_proto)
                    {
                        next = Some(Value::VmObject(obj_proto));
                    }
                    current = next;
                }
                Value::VmArray(arr) => {
                    let borrow = arr.borrow();
                    let own_keys = self.collect_array_keys(ctx, &borrow, true, false);

                    for key in own_keys {
                        let first_seen = seen_any.insert(key.clone());
                        if !first_seen {
                            continue;
                        }
                        out_keys.push(Value::from(&key));
                    }

                    current = borrow.props.get("__proto__").cloned();
                }
                Value::VmFunction(..) | Value::VmClosure(..) => {
                    let overlay = self.get_closure_overlay(&cur);
                    let props = match &cur {
                        Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => self.get_fn_props(ctx, *ip, *arity),
                        _ => unreachable!(),
                    };
                    // Merge keys: overlay first, then shared fn_props for non-overlapping keys
                    let borrow = props.borrow();
                    let own_keys = if let Some(ov) = overlay {
                        let ov_borrow = ov.borrow();
                        let mut keys = self.collect_object_map_keys(&ov_borrow, true);
                        let ov_set: std::collections::HashSet<&str> = ov_borrow.keys().map(|k| k.as_str()).collect();
                        for k in self.collect_object_map_keys(&borrow, true) {
                            if !ov_set.contains(k.as_str()) {
                                keys.push(k);
                            }
                        }
                        keys
                    } else {
                        self.collect_object_map_keys(&borrow, true)
                    };

                    for key in own_keys {
                        let first_seen = seen_any.insert(key.clone());
                        if !first_seen {
                            continue;
                        }
                        out_keys.push(Value::from(&key));
                    }

                    current = borrow.get("__proto__").cloned();
                }
                _ => break,
            }
        }

        let keys = out_keys;
        self.stack.push(Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(keys))));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::GetMethod
    fn run_opcode_get_method(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [..., obj] -> [..., obj, method]
        // Peek at object on TOS, resolve method, push on top
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        let key = if let Value::String(s) = name_val {
            crate::unicode::utf16_to_utf8(s)
        } else {
            value_to_string(name_val)
        };
        let raw_obj = self.stack.last().cloned().expect("VM Stack underflow on GetMethod");
        // Accessing a property on undefined/null is a TypeError
        if matches!(raw_obj, Value::Undefined | Value::Null) {
            let err_msg = format!(
                "Cannot read properties of {} (reading '{}')",
                if matches!(raw_obj, Value::Undefined) { "undefined" } else { "null" },
                key
            );
            let err = self.make_type_error_object(ctx, &err_msg);
            self.handle_throw(ctx, &err)?;
            self.stack.push(Value::Undefined);
            return Ok(OpcodeAction::Continue);
        }
        let obj = raw_obj;
        // Brand check for private method calls
        if key.contains(PRIVATE_KEY_PREFIX) && !self.check_private_brand(ctx, &obj, &key) {
            let err = self.make_type_error_object(ctx, "Cannot access private member from an object whose class did not declare it");
            self.handle_throw(ctx, &err)?;
            self.stack.push(Value::Undefined);
            return Ok(OpcodeAction::Continue);
        }
        let method = match &obj {
            Value::VmObject(map) => {
                let borrow = map.borrow();
                let getter_key = format!("__get_{}", key);
                if let Some(getter_fn) = borrow.get(&getter_key).cloned() {
                    drop(borrow);
                    self.invoke_getter_with_receiver(ctx, &getter_fn, &obj)
                } else if let Some(v) = borrow.get(&key).cloned() {
                    match v {
                        Value::Property { getter: Some(g), .. } => {
                            drop(borrow);
                            self.invoke_getter_with_receiver(ctx, &g, &obj)
                        }
                        other => other,
                    }
                } else {
                    let is_callable_obj = borrow.contains_key("__host_fn__")
                        || borrow.contains_key("__bound_target__")
                        || borrow.contains_key("__fn_body__")
                        || borrow.contains_key("__native_id__");
                    if is_callable_obj {
                        match key.as_str() {
                            "call" => {
                                drop(borrow);
                                self.stack.push(Value::VmNativeFunction(BUILTIN_FN_CALL));
                                return Ok(OpcodeAction::Continue);
                            }
                            "apply" => {
                                drop(borrow);
                                self.stack.push(Value::VmNativeFunction(BUILTIN_FN_APPLY));
                                return Ok(OpcodeAction::Continue);
                            }
                            "bind" => {
                                drop(borrow);
                                self.stack.push(Value::VmNativeFunction(BUILTIN_FN_BIND));
                                return Ok(OpcodeAction::Continue);
                            }
                            _ => {}
                        }
                    }
                    // Check WeakRef
                    let is_weakref = borrow.contains_key("__weakref__");
                    // Check typed wrapper methods first
                    let type_name = borrow.get("__type__").map(|v| value_to_string(v));
                    let mut proto = borrow.get("__proto__").cloned();
                    drop(borrow);
                    if proto.is_none()
                        && let Some(type_name) = type_name.as_deref()
                        && let Some(Value::VmObject(ctor)) = self.globals.get(type_name)
                        && let Some(type_proto) = ctor.borrow().get("prototype").cloned()
                    {
                        proto = Some(type_proto);
                    }
                    if matches!(type_name.as_deref(), Some("Boolean"))
                        && let Some(Value::VmObject(boolean_ctor)) = self.globals.get("Boolean")
                        && let Some(bool_proto) = boolean_ctor.borrow().get("prototype").cloned()
                    {
                        proto = Some(bool_proto);
                    }
                    if matches!(type_name.as_deref(), Some("String"))
                        && let Some(Value::VmObject(string_ctor)) = self.globals.get("String")
                        && let Some(string_proto) = string_ctor.borrow().get("prototype").cloned()
                    {
                        proto = Some(string_proto);
                    }
                    if is_weakref && key == "deref" {
                        Value::VmNativeFunction(BUILTIN_WEAKREF_DEREF)
                    } else {
                        let typed_result = match type_name.as_deref() {
                            Some("Number") => match key.as_str() {
                                "toFixed" | "toExponential" | "toPrecision" | "toString" | "toLocaleString" | "valueOf" | "constructor" => {
                                    if let Some(Value::VmObject(num_ctor)) = self.globals.get("Number")
                                        && let Some(Value::VmObject(num_proto)) = num_ctor.borrow().get("prototype").cloned()
                                    {
                                        num_proto.borrow().get(&key).cloned()
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            },
                            Some("BigInt") => {
                                // Check if BigInt.prototype has a getter override
                                if let Some(Value::VmObject(bi_ctor)) = self.globals.get("BigInt")
                                    && let Some(Value::VmObject(bi_proto)) = bi_ctor.borrow().get("prototype").cloned()
                                {
                                    let getter_key = format!("__get_{}", key);
                                    let bp = bi_proto.borrow();
                                    if bp.contains_key(&getter_key) || bp.contains_key(&key) {
                                        drop(bp);
                                        let val = self.read_named_property(ctx, &Value::VmObject(bi_proto), &key);
                                        Some(val)
                                    } else {
                                        match key.as_str() {
                                            "toString" => Some(Value::VmNativeFunction(BUILTIN_BIGINT_TOSTRING)),
                                            "toLocaleString" => Some(Value::VmNativeFunction(BUILTIN_BIGINT_TOLOCALESTRING)),
                                            "valueOf" => Some(Value::VmNativeFunction(BUILTIN_BIGINT_VALUEOF)),
                                            "constructor" => bp.get(&key).cloned(),
                                            _ => None,
                                        }
                                    }
                                } else {
                                    match key.as_str() {
                                        "toString" => Some(Value::VmNativeFunction(BUILTIN_BIGINT_TOSTRING)),
                                        "toLocaleString" => Some(Value::VmNativeFunction(BUILTIN_BIGINT_TOLOCALESTRING)),
                                        "valueOf" => Some(Value::VmNativeFunction(BUILTIN_BIGINT_VALUEOF)),
                                        _ => None,
                                    }
                                }
                            }
                            Some("String") => match key.as_str() {
                                "toString" | "valueOf" => Some(Value::VmNativeFunction(BUILTIN_STRING_VALUEOF)),
                                "constructor" => self.globals.get("String").cloned(),
                                "length" => {
                                    let b = map.borrow();
                                    match b.get("__value__") {
                                        Some(Value::String(sv)) => Some(Value::Number(sv.len() as f64)),
                                        _ => Some(Value::Number(0.0)),
                                    }
                                }
                                "split" => Some(Value::VmNativeFunction(BUILTIN_STRING_SPLIT)),
                                "indexOf" => Some(Value::VmNativeFunction(BUILTIN_STRING_INDEXOF)),
                                "slice" => Some(Value::VmNativeFunction(BUILTIN_STRING_SLICE)),
                                "toUpperCase" => Some(Value::VmNativeFunction(BUILTIN_STRING_TOUPPERCASE)),
                                "toLowerCase" => Some(Value::VmNativeFunction(BUILTIN_STRING_TOLOWERCASE)),
                                "trim" => Some(Value::VmNativeFunction(BUILTIN_STRING_TRIM)),
                                "charAt" => Some(Value::VmNativeFunction(BUILTIN_STRING_CHARAT)),
                                "includes" => Some(Value::VmNativeFunction(BUILTIN_STRING_INCLUDES)),
                                "replace" => Some(Value::VmNativeFunction(BUILTIN_STRING_REPLACE)),
                                "replaceAll" => Some(Value::VmNativeFunction(BUILTIN_STRING_REPLACEALL)),
                                "match" => Some(Value::VmNativeFunction(BUILTIN_STRING_MATCH)),
                                "search" => Some(Value::VmNativeFunction(BUILTIN_STRING_SEARCH)),
                                "startsWith" => Some(Value::VmNativeFunction(BUILTIN_STRING_STARTSWITH)),
                                "endsWith" => Some(Value::VmNativeFunction(BUILTIN_STRING_ENDSWITH)),
                                "substring" => Some(Value::VmNativeFunction(BUILTIN_STRING_SUBSTRING)),
                                "padStart" => Some(Value::VmNativeFunction(BUILTIN_STRING_PADSTART)),
                                "padEnd" => Some(Value::VmNativeFunction(BUILTIN_STRING_PADEND)),
                                "repeat" => Some(Value::VmNativeFunction(BUILTIN_STRING_REPEAT)),
                                "charCodeAt" => Some(Value::VmNativeFunction(BUILTIN_STRING_CHARCODEAT)),
                                "trimStart" => Some(Value::VmNativeFunction(BUILTIN_STRING_TRIMSTART)),
                                "trimEnd" => Some(Value::VmNativeFunction(BUILTIN_STRING_TRIMEND)),
                                "lastIndexOf" => Some(Value::VmNativeFunction(BUILTIN_STRING_LASTINDEXOF)),
                                _ => None,
                            },
                            Some("Boolean") => {
                                let effective_proto = proto.clone().or_else(|| {
                                    if let Some(Value::VmObject(obj_global)) = self.globals.get("Object") {
                                        obj_global.borrow().get("prototype").cloned()
                                    } else {
                                        None
                                    }
                                });
                                if self.lookup_proto_chain(effective_proto.as_ref(), &key).is_none() {
                                    match key.as_str() {
                                        "toString" => Some(Self::make_host_fn(ctx, "boolean.toString")),
                                        "valueOf" => Some(Self::make_host_fn(ctx, "boolean.valueOf")),
                                        _ => None,
                                    }
                                } else {
                                    None
                                }
                            }
                            Some("RegExp") => match key.as_str() {
                                "toString" => Some(Self::make_bound_host_fn(ctx, "regexp.toString", &obj)),
                                _ => None,
                            },
                            _ => None,
                        };
                        typed_result.unwrap_or_else(|| self.read_named_property_with_receiver(ctx, &obj, &key, &obj))
                    }
                }
            }
            Value::VmArray(arr) => {
                let borrow = arr.borrow();
                let is_generator = matches!(borrow.props.get("__generator__"), Some(Value::Boolean(true)));
                let is_async_gen = matches!(borrow.props.get("__async_generator__"), Some(Value::Boolean(true)));
                drop(borrow);
                match key.as_str() {
                    "next" if is_generator => Self::make_bound_host_fn(ctx, "iterator.next", &obj),
                    "next" if is_async_gen => Value::VmNativeFunction(BUILTIN_ASYNCGEN_NEXT),
                    "throw" if is_async_gen => Value::VmNativeFunction(BUILTIN_ASYNCGEN_THROW),
                    "return" if is_async_gen => Value::VmNativeFunction(BUILTIN_ASYNCGEN_RETURN),
                    _ => self.read_named_property(ctx, &obj, &key),
                }
            }
            Value::String(_) => match key.as_str() {
                "split" => Value::VmNativeFunction(BUILTIN_STRING_SPLIT),
                "indexOf" => Value::VmNativeFunction(BUILTIN_STRING_INDEXOF),
                "slice" => Value::VmNativeFunction(BUILTIN_STRING_SLICE),
                "toUpperCase" => Value::VmNativeFunction(BUILTIN_STRING_TOUPPERCASE),
                "toLowerCase" => Value::VmNativeFunction(BUILTIN_STRING_TOLOWERCASE),
                "trim" => Value::VmNativeFunction(BUILTIN_STRING_TRIM),
                "charAt" => Value::VmNativeFunction(BUILTIN_STRING_CHARAT),
                "includes" => Value::VmNativeFunction(BUILTIN_STRING_INCLUDES),
                "replace" => Value::VmNativeFunction(BUILTIN_STRING_REPLACE),
                "startsWith" => Value::VmNativeFunction(BUILTIN_STRING_STARTSWITH),
                "endsWith" => Value::VmNativeFunction(BUILTIN_STRING_ENDSWITH),
                "substring" => Value::VmNativeFunction(BUILTIN_STRING_SUBSTRING),
                "padStart" => Value::VmNativeFunction(BUILTIN_STRING_PADSTART),
                "padEnd" => Value::VmNativeFunction(BUILTIN_STRING_PADEND),
                "repeat" => Value::VmNativeFunction(BUILTIN_STRING_REPEAT),
                "charCodeAt" => Value::VmNativeFunction(BUILTIN_STRING_CHARCODEAT),
                "trimStart" => Value::VmNativeFunction(BUILTIN_STRING_TRIMSTART),
                "trimEnd" => Value::VmNativeFunction(BUILTIN_STRING_TRIMEND),
                "lastIndexOf" => Value::VmNativeFunction(BUILTIN_STRING_LASTINDEXOF),
                "match" => Value::VmNativeFunction(BUILTIN_STRING_MATCH),
                "replaceAll" => Value::VmNativeFunction(BUILTIN_STRING_REPLACEALL),
                "search" => Value::VmNativeFunction(BUILTIN_STRING_SEARCH),
                "toString" => Value::VmNativeFunction(BUILTIN_STRING_TOSTRING),
                "valueOf" => Value::VmNativeFunction(BUILTIN_STRING_VALUEOF),
                "concat" => Self::make_bound_host_fn(ctx, "string.concat", &obj),
                "substr" => Self::make_bound_host_fn(ctx, "string.substr", &obj),
                "constructor" => self.globals.get("String").cloned().unwrap_or(Value::Undefined),
                _ => {
                    let wrapped = self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&obj));
                    self.read_named_property_with_receiver(ctx, &wrapped, &key, &obj)
                }
            },
            Value::Number(_) => match key.as_str() {
                "toFixed" => Value::VmNativeFunction(BUILTIN_NUM_TOFIXED),
                "toExponential" => Value::VmNativeFunction(BUILTIN_NUM_TOEXPONENTIAL),
                "toPrecision" => Value::VmNativeFunction(BUILTIN_NUM_TOPRECISION),
                "toString" => Value::VmNativeFunction(BUILTIN_NUM_TOSTRING),
                "valueOf" => Value::VmNativeFunction(BUILTIN_NUM_VALUEOF),
                _ => {
                    let wrapped = self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&obj));
                    self.read_named_property_with_receiver(ctx, &wrapped, &key, &obj)
                }
            },
            Value::Boolean(_) => match key.as_str() {
                "toString" => Self::make_host_fn(ctx, "boolean.toString"),
                "valueOf" => Self::make_host_fn(ctx, "boolean.valueOf"),
                _ => {
                    let wrapped = self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&obj));
                    self.read_named_property_with_receiver(ctx, &wrapped, &key, &obj)
                }
            },
            Value::BigInt(_) => match key.as_str() {
                "toString" => Value::VmNativeFunction(BUILTIN_BIGINT_TOSTRING),
                "valueOf" => Value::VmNativeFunction(BUILTIN_BIGINT_VALUEOF),
                "toLocaleString" => Value::VmNativeFunction(BUILTIN_BIGINT_TOLOCALESTRING),
                _ => {
                    let wrapped = self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&obj));
                    self.read_named_property_with_receiver(ctx, &wrapped, &key, &obj)
                }
            },
            Value::VmMap(_) => match key.as_str() {
                "set" => Value::VmNativeFunction(BUILTIN_MAP_SET),
                "get" => Value::VmNativeFunction(BUILTIN_MAP_GET),
                "has" => Value::VmNativeFunction(BUILTIN_MAP_HAS),
                "delete" => Value::VmNativeFunction(BUILTIN_MAP_DELETE),
                "keys" => Value::VmNativeFunction(BUILTIN_MAP_KEYS),
                "values" => Value::VmNativeFunction(BUILTIN_MAP_VALUES),
                "entries" => Value::VmNativeFunction(BUILTIN_MAP_ENTRIES),
                "forEach" => Value::VmNativeFunction(BUILTIN_MAP_FOREACH),
                "clear" => Value::VmNativeFunction(BUILTIN_MAP_CLEAR),
                "toString" => Value::VmNativeFunction(BUILTIN_OBJ_TOSTRING),
                _ => Value::Undefined,
            },
            Value::VmSet(_) => match key.as_str() {
                "add" => Value::VmNativeFunction(BUILTIN_SET_ADD),
                "has" => Value::VmNativeFunction(BUILTIN_SET_HAS),
                "delete" => Value::VmNativeFunction(BUILTIN_SET_DELETE),
                "keys" | "values" => Value::VmNativeFunction(BUILTIN_SET_VALUES),
                "entries" => Value::VmNativeFunction(BUILTIN_SET_ENTRIES),
                "forEach" => Value::VmNativeFunction(BUILTIN_SET_FOREACH),
                "clear" => Value::VmNativeFunction(BUILTIN_SET_CLEAR),
                "toString" => Value::VmNativeFunction(BUILTIN_OBJ_TOSTRING),
                _ => Value::Undefined,
            },
            Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                let overlay = self.get_closure_overlay(&obj);
                let shared = self.get_fn_props(ctx, *ip, *arity);
                // Lookup helper: overlay first, then shared
                let lookup = |k: &str| -> Option<Value<'gc>> {
                    overlay
                        .and_then(|o| o.borrow().get(k).cloned())
                        .or_else(|| shared.borrow().get(k).cloned())
                };
                // Check for accessor getter (__get_<key>)
                let getter_key = format!("__get_{}", key);
                if let Some(getter_fn) = lookup(&getter_key) {
                    self.invoke_getter_with_receiver(ctx, &getter_fn, &obj)
                } else if let Some(value) = lookup(&key) {
                    match value {
                        Value::Property { getter: Some(g), .. } => self.invoke_getter_with_receiver(ctx, &g, &obj),
                        other => other,
                    }
                } else {
                    let proto = lookup("__proto__");
                    match key.as_str() {
                        "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                        "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                        "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                        _ => self.lookup_proto_chain(proto.as_ref(), &key).unwrap_or(Value::Undefined),
                    }
                }
            }
            Value::Function(name) => match key.as_str() {
                "name" => Value::from(name),
                "length" => Value::Number(1.0),
                "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                _ => Value::Undefined,
            },
            Value::VmNativeFunction(_) => self.read_named_property(ctx, &obj, &key),
            _ => Value::Undefined,
        };
        self.stack.push(method);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::NewError
    fn run_opcode_new_error(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        // Stack: [..., type_name, message]
        let msg = self.stack.pop().unwrap_or(Value::Undefined);
        let type_val = self.stack.pop().unwrap_or(Value::Undefined);
        let type_name = value_to_string(&type_val);
        let mut map = IndexMap::new();
        map.insert("message".to_string(), msg);
        map.insert("__type__".to_string(), Value::from(&type_name));
        map.insert("name".to_string(), Value::from(&type_name));
        self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, map)));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Dup
    fn run_opcode_dup(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let val = self.stack.last().cloned().unwrap_or(Value::Undefined);
        self.stack.push(val);
        Ok(OpcodeAction::Continue)
    }

    // Opcode::Swap
    fn run_opcode_swap(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let _ = ctx;
        let len = self.stack.len();
        if len >= 2 {
            self.stack.swap(len - 1, len - 2);
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ResetPrototype — create a fresh prototype for the class constructor on TOS.
    // Each class evaluation must have its own prototype so factory patterns work correctly.
    // For VmClosure constructors, we create a per-closure fn_props clone so that
    // different closures with the same IP don't share the same prototype.
    fn run_opcode_reset_prototype(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let ctor = self.stack.last().cloned().unwrap_or(Value::Undefined);
        let ip = match &ctor {
            Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _) => *ip,
            _ => return Ok(OpcodeAction::Continue),
        };
        let arity = match &ctor {
            Value::VmFunction(_, a) | Value::VmClosure(_, a, _) => *a,
            _ => 0,
        };
        // Create a new prototype object
        let mut proto = IndexMap::new();
        proto.insert("constructor".to_string(), ctor.clone());
        proto.insert("__nonenumerable_constructor__".to_string(), Value::Boolean(true));
        if let Some(Value::VmObject(obj_global)) = self.globals.get("Object")
            && let Some(obj_proto) = obj_global.borrow().get("prototype").cloned()
        {
            proto.insert("__proto__".to_string(), obj_proto);
        }
        let new_proto = Value::VmObject(new_gc_cell_ptr(ctx, proto));

        if let Value::VmClosure(_, _, uv) = ctor {
            // For closures: create a per-closure override map (prototype, brands)
            // Other properties (static methods, etc.) still use shared fn_props
            let gc_key = Gc::as_ptr(uv) as usize;
            let mut overlay = IndexMap::new();
            overlay.insert("prototype".to_string(), new_proto);
            let per_closure = new_gc_cell_ptr(ctx, overlay);
            self.closure_fn_props.insert(gc_key, per_closure);
        } else {
            // VmFunction: just update shared fn_props
            let props = self.get_fn_props(ctx, ip, arity);
            props.borrow_mut(ctx).insert("prototype".to_string(), new_proto);
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::IteratorClose — pop iterator, call .return() if callable
    fn run_opcode_iterator_close(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let iterator = self.stack.pop().unwrap_or(Value::Undefined);
        // Look up .return on the iterator
        let return_fn = self.read_named_property(ctx, &iterator, "return");
        if matches!(return_fn, Value::Undefined | Value::Null) {
            return Ok(OpcodeAction::Continue);
        }
        if !self.is_value_callable(&return_fn) {
            let err = self.make_type_error_object(ctx, "iterator.return is not a function");
            self.handle_throw(ctx, &err)?;
            return Ok(OpcodeAction::Continue);
        }
        match self.vm_call_function_value(ctx, &return_fn, &iterator, &[]) {
            Ok(inner_result) => {
                // §7.4.6 step 9: If Type(innerResult.[[value]]) is not Object, throw TypeError
                if !matches!(
                    inner_result,
                    Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..)
                ) {
                    let err = self.make_type_error_object(ctx, "Iterator result is not an object");
                    self.handle_throw(ctx, &err)?;
                }
            }
            Err(err) => {
                self.set_pending_throw_from_error(&err);
            }
        }
        if let Some(thrown) = self.pending_throw.take() {
            self.handle_throw(ctx, &thrown)?;
        }
        Ok(OpcodeAction::Continue)
    }

    /// Best-effort iterator close for throw completions (§7.4.6 step 5).
    /// Calls .return() if available but swallows all errors — the original
    /// throw completion is always preserved by the caller.
    /// Exception: when generator_return_pending is set, this is a return
    /// completion and we use normal IteratorClose semantics (propagate errors,
    /// check return type).
    fn run_opcode_iterator_close_abrupt(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let iterator = self.stack.pop().unwrap_or(Value::Undefined);

        // If this is a generator return completion, use normal IteratorClose semantics
        if self.generator_return_pending.is_some() {
            let return_fn = self.read_named_property(ctx, &iterator, "return");
            if let Some(thrown) = self.pending_throw.take() {
                self.handle_throw(ctx, &thrown)?;
                return Ok(OpcodeAction::Continue);
            }
            if matches!(return_fn, Value::Undefined | Value::Null) {
                return Ok(OpcodeAction::Continue);
            }
            if !self.is_value_callable(&return_fn) {
                let err = self.make_type_error_object(ctx, "iterator.return is not a function");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            match self.vm_call_function_value(ctx, &return_fn, &iterator, &[]) {
                Ok(inner_result) => {
                    if !matches!(
                        inner_result,
                        Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..)
                    ) {
                        // §7.4.6 step 9: non-Object → TypeError
                        // Clear the return completion so the TypeError propagates instead
                        self.generator_return_pending = None;
                        let err = self.make_type_error_object(ctx, "Iterator result is not an object");
                        self.handle_throw(ctx, &err)?;
                    }
                }
                Err(err) => {
                    // §7.4.6 step 8: close error propagates for return completions
                    self.generator_return_pending = None;
                    self.set_pending_throw_from_error(&err);
                }
            }
            if let Some(thrown) = self.pending_throw.take() {
                self.handle_throw(ctx, &thrown)?;
            }
            return Ok(OpcodeAction::Continue);
        }

        let return_fn = self.read_named_property(ctx, &iterator, "return");
        // Clear any pending throw from getter evaluation (e.g. `get return() { throw ... }`)
        // before checking the return value — the getter error must be swallowed.
        if self.pending_throw.is_some() {
            self.pending_throw = None;
            return Ok(OpcodeAction::Continue);
        }
        if matches!(return_fn, Value::Undefined | Value::Null) || !self.is_value_callable(&return_fn) {
            return Ok(OpcodeAction::Continue);
        }
        // Call return(); ignore any error or non-object result
        let _ = self.vm_call_function_value(ctx, &return_fn, &iterator, &[]);
        self.pending_throw = None;
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ToNumber
    fn run_opcode_to_number(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let val = self.stack.pop().expect("VM Stack underflow on ToNumber");
        match &val {
            Value::VmObject(_) | Value::VmArray(_) => {
                let prim = self.try_to_primitive(ctx, &val, "number");
                if self.pending_throw.is_some() {
                    self.stack.push(Value::Number(f64::NAN));
                } else {
                    match &prim {
                        Value::BigInt(_) => {
                            self.throw_type_error(ctx, "Cannot convert a BigInt value to a number");
                            self.stack.push(Value::Number(f64::NAN));
                        }
                        _ => self.stack.push(Value::Number(to_number(&prim))),
                    }
                }
            }
            Value::BigInt(_) => {
                self.throw_type_error(ctx, "Cannot convert a BigInt value to a number");
                self.stack.push(Value::Number(f64::NAN));
            }
            _ => self.stack.push(Value::Number(to_number(&val))),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::ToNumeric — like ToNumber but preserves BigInt values
    fn run_opcode_to_numeric(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let val = self.stack.pop().expect("VM Stack underflow on ToNumeric");
        match &val {
            Value::Number(_) | Value::BigInt(_) => {
                self.stack.push(val);
            }
            Value::VmObject(_) | Value::VmArray(_) => {
                let prim = self.try_to_primitive(ctx, &val, "number");
                if self.pending_throw.is_some() {
                    self.stack.push(Value::Number(f64::NAN));
                } else {
                    match &prim {
                        Value::BigInt(_) => self.stack.push(prim),
                        _ => self.stack.push(Value::Number(to_number(&prim))),
                    }
                }
            }
            _ => self.stack.push(Value::Number(to_number(&val))),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::CollectRest
    fn run_opcode_collect_rest(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Collect excess function args into a rest array.
        // Operand: non_rest_count (u8) = number of formal non-rest params.
        let non_rest_count = self.read_byte() as usize;
        let frame = self.frames.last().expect("CollectRest: no call frame");
        let actual_arg_count = frame.arg_count;
        let bp = frame.bp;
        let saved = frame.saved_args.clone();
        if actual_arg_count > non_rest_count {
            let rest_elems: Vec<Value<'gc>> = if let Some(ref sa) = saved {
                // Excess args were removed from stack; get them from saved_args
                sa[non_rest_count..actual_arg_count].to_vec()
            } else {
                // No excess args were removed; they're still on the stack
                let start = bp + non_rest_count;
                let end = bp + actual_arg_count;
                let elems = self.stack[start..end].to_vec();
                self.stack.drain(start..end);
                elems
            };
            self.stack.push(Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(rest_elems))));
        } else {
            // No excess args — push empty array
            self.stack.push(Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(Vec::new()))));
        }
        // The rest array is now the next local slot (at position non_rest_count)
        Ok(OpcodeAction::Continue)
    }

    // Opcode::In
    fn run_opcode_in(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let mut obj = self.stack.pop().expect("VM Stack underflow on In (obj)");
        let mut key_val = self.stack.pop().expect("VM Stack underflow on In (key)");

        let is_object_like = |v: &Value<'gc>| {
            matches!(
                v,
                Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_)
            )
        };

        // Some compilation paths can materialize operands in reverse order.
        // Normalize to (key in object) so runtime behavior stays stable.
        if !is_object_like(&obj) && is_object_like(&key_val) {
            std::mem::swap(&mut obj, &mut key_val);
        }

        // Per spec §13.10.1: if RHS is not an object, throw TypeError
        if !is_object_like(&obj) {
            let err = self.make_type_error_object(
                ctx,
                &format!(
                    "Cannot use 'in' operator to search for '{}' in {}",
                    value_to_string(&key_val),
                    value_to_string(&obj)
                ),
            );
            self.handle_throw(ctx, &err)?;
            return Ok(OpcodeAction::Continue);
        }

        let key = match self.as_property_key_string(ctx, &key_val) {
            Ok(key) => key,
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        };
        // Ergonomic brand check: `#field in obj` (inside the class that owns #field)
        // must return true when the object has that private field as an own property.
        // Private field keys use the PRIVATE_KEY_PREFIX so external JS code can never
        // forge them; only the compiler emits them via Expr::PrivateName.
        if key.starts_with(PRIVATE_KEY_PREFIX) {
            // Private field `in` check: no proxy unwrapping — check the object directly
            let has = match &obj {
                Value::VmObject(map) => {
                    let b = map.borrow();
                    b.contains_key(&key) || b.contains_key(&format!("__get_{}", key)) || b.contains_key(&format!("__set_{}", key))
                }
                Value::VmArray(arr) => {
                    let b = arr.borrow();
                    b.props.contains_key(&key)
                        || b.props.contains_key(&format!("__get_{}", key))
                        || b.props.contains_key(&format!("__set_{}", key))
                }
                Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                    let props = self.get_fn_props(ctx, *ip, *arity);
                    let b = props.borrow();
                    b.contains_key(&key) || b.contains_key(&format!("__get_{}", key)) || b.contains_key(&format!("__set_{}", key))
                }
                _ => false,
            };
            self.stack.push(Value::Boolean(has));
            return Ok(OpcodeAction::Continue);
        }
        match self.try_proxy_has(ctx, &obj, &key) {
            Ok(Some(result)) => {
                self.stack.push(Value::Boolean(result));
                return Ok(OpcodeAction::Continue);
            }
            Ok(None) => {}
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        }
        let result = match &obj {
            Value::VmObject(map) => {
                // Module namespace exotic object [[HasProperty]] (§10.4.6.7)
                if map.borrow().contains_key("__module_namespace__") {
                    let b = map.borrow();
                    if key.starts_with("@@sym:") {
                        // Symbol keys: check via ordinary hasProperty (e.g., Symbol.toStringTag)
                        b.contains_key(&key)
                    } else {
                        // Check if key is in exported bindings
                        if let Some(Value::VmObject(bindings)) = b.get("__ns_bindings__") {
                            bindings.borrow().contains_key(&key)
                        } else {
                            // Loaded module namespace: check key directly (skip internal keys)
                            !key.starts_with("__") && b.contains_key(&key)
                        }
                    }
                } else {
                    let b = map.borrow();
                    if b.contains_key(&key) || b.contains_key(&format!("__get_{}", key)) || b.contains_key(&format!("__set_{}", key)) {
                        true
                    } else {
                        // Check built-in properties based on __type__
                        let type_name = b.get("__type__").map(|v| value_to_string(v)).unwrap_or_default();
                        if type_name == "String" {
                            if key == "length" {
                                true
                            } else if let Some(Value::String(s)) = b.get("__value__") {
                                if let Ok(idx) = key.parse::<usize>() { idx < s.len() } else { false }
                            } else {
                                false
                            }
                        } else if matches!(type_name.as_str(), "String" if key == "length") {
                            true
                        } else {
                            // Walk __proto__ chain - check for proxy in proto
                            let proto = b.get("__proto__").cloned();
                            if proto.is_none()
                                && let Some(Value::String(type_name_u16)) = b.get("__type__")
                            {
                                let tn = crate::unicode::utf16_to_utf8(type_name_u16);
                                if let Some(Value::VmObject(ctor)) = self.globals.get(&tn)
                                    && let Some(type_proto) = ctor.borrow().get("prototype").cloned()
                                {
                                    drop(b);
                                    self.lookup_proto_chain(Some(&type_proto), &key).is_some()
                                        || self.lookup_proto_chain(Some(&type_proto), &format!("__get_{}", key)).is_some()
                                        || self.lookup_proto_chain(Some(&type_proto), &format!("__set_{}", key)).is_some()
                                } else {
                                    false
                                }
                            } else {
                                drop(b);
                                // If proto is a proxy, use try_proxy_has
                                if let Some(ref proto_val) = proto
                                    && let Ok(Some(result)) = self.try_proxy_has(ctx, proto_val, &key)
                                {
                                    return {
                                        self.stack.push(Value::Boolean(result));
                                        Ok(OpcodeAction::Continue)
                                    };
                                }
                                self.lookup_proto_chain(proto.as_ref(), &key).is_some()
                                    || self.lookup_proto_chain(proto.as_ref(), &format!("__get_{}", key)).is_some()
                                    || self.lookup_proto_chain(proto.as_ref(), &format!("__set_{}", key)).is_some()
                            }
                        }
                    }
                }
            }
            Value::VmArray(arr) => {
                let is_ta = arr.borrow().props.contains_key("__typedarray_name__");

                // TypedArray [[HasProperty]]: canonical numeric index strings are
                // never looked up on the prototype chain — only valid integer indices
                // within bounds return true; all other canonical numeric indices
                // (including non-canonical strings like "+1" that parse as usize)
                // must fall through to ordinary string property lookup.
                if is_ta {
                    if let Some(numeric_index) = Self::canonical_numeric_index_string(&key) {
                        let borrow = arr.borrow();
                        numeric_index >= 0.0
                            && numeric_index.fract() == 0.0
                            && !numeric_index.is_nan()
                            && numeric_index != f64::INFINITY
                            && !(numeric_index == 0.0 && numeric_index.is_sign_negative())
                            && (numeric_index as usize) < borrow.elements.len()
                    } else {
                        let borrow = arr.borrow();
                        if borrow.props.contains_key(&key)
                            || borrow.props.contains_key(&format!("__get_{}", key))
                            || borrow.props.contains_key(&format!("__set_{}", key))
                        {
                            true
                        } else {
                            let proto = borrow.props.get("__proto__").cloned();
                            drop(borrow);
                            if let Some(ref proto_val) = proto
                                && let Ok(Some(result)) = self.try_proxy_has(ctx, proto_val, &key)
                            {
                                self.stack.push(Value::Boolean(result));
                                return Ok(OpcodeAction::Continue);
                            }
                            self.lookup_proto_chain(proto.as_ref(), &key).is_some()
                                || self.lookup_proto_chain(proto.as_ref(), &format!("__get_{}", key)).is_some()
                                || self.lookup_proto_chain(proto.as_ref(), &format!("__set_{}", key)).is_some()
                        }
                    }
                } else if let Ok(idx) = key.parse::<usize>() {
                    let borrow = arr.borrow();
                    if idx < 0xFFFF_FFFF {
                        let logical_len = self.vm_array_logical_length_u64(&borrow) as usize;
                        let own_present = if idx < logical_len {
                            (!borrow.props.contains_key(&format!("__deleted_{}", idx)) && idx < borrow.elements.len())
                                || borrow.props.contains_key(&key)
                                || borrow.props.contains_key(&format!("__get_{}", key))
                                || borrow.props.contains_key(&format!("__set_{}", key))
                        } else {
                            false
                        };
                        if own_present {
                            true
                        } else {
                            let mut proto = borrow.props.get("__proto__").cloned();
                            if proto.is_none()
                                && let Some(Value::VmObject(array_ctor)) = self.globals.get("Array")
                                && let Some(array_proto) = array_ctor.borrow().get("prototype").cloned()
                            {
                                proto = Some(array_proto);
                            }
                            drop(borrow);
                            if let Some(ref proto_val) = proto
                                && let Ok(Some(result)) = self.try_proxy_has(ctx, proto_val, &key)
                            {
                                self.stack.push(Value::Boolean(result));
                                return Ok(OpcodeAction::Continue);
                            }
                            self.lookup_proto_chain(proto.as_ref(), &key).is_some()
                                || self.lookup_proto_chain(proto.as_ref(), &format!("__get_{}", key)).is_some()
                                || self.lookup_proto_chain(proto.as_ref(), &format!("__set_{}", key)).is_some()
                        }
                    } else {
                        borrow.props.contains_key(&key)
                            || borrow.props.contains_key(&format!("__get_{}", key))
                            || borrow.props.contains_key(&format!("__set_{}", key))
                    }
                } else if key == "length" {
                    true
                } else {
                    let borrow = arr.borrow();
                    if borrow.props.contains_key(&key)
                        || borrow.props.contains_key(&format!("__get_{}", key))
                        || borrow.props.contains_key(&format!("__set_{}", key))
                    {
                        true
                    } else {
                        let proto = borrow.props.get("__proto__").cloned();
                        drop(borrow);
                        if let Some(ref proto_val) = proto
                            && let Ok(Some(result)) = self.try_proxy_has(ctx, proto_val, &key)
                        {
                            self.stack.push(Value::Boolean(result));
                            return Ok(OpcodeAction::Continue);
                        }
                        self.lookup_proto_chain(proto.as_ref(), &key).is_some()
                            || self.lookup_proto_chain(proto.as_ref(), &format!("__get_{}", key)).is_some()
                            || self.lookup_proto_chain(proto.as_ref(), &format!("__set_{}", key)).is_some()
                    }
                }
            }
            Value::VmFunction(..) | Value::VmClosure(..) => {
                let props = self.get_fn_props_for_value(ctx, &obj).unwrap();
                let b = props.borrow();
                if b.contains_key(&key) || b.contains_key(&format!("__get_{}", key)) || b.contains_key(&format!("__set_{}", key)) {
                    true
                } else {
                    let proto = b.get("__proto__").cloned();
                    drop(b);
                    // Fall back to shared fn_props for __proto__ if overlay didn't have it
                    let proto = proto.or_else(|| match &obj {
                        Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                            self.get_fn_props(ctx, *ip, *arity).borrow().get("__proto__").cloned()
                        }
                        _ => None,
                    });
                    self.lookup_proto_chain(proto.as_ref(), &key).is_some()
                        || self.lookup_proto_chain(proto.as_ref(), &format!("__get_{}", key)).is_some()
                        || self.lookup_proto_chain(proto.as_ref(), &format!("__set_{}", key)).is_some()
                }
            }
            _ => false,
        };
        self.stack.push(Value::Boolean(result));

        Ok(OpcodeAction::Continue)
    }

    // Opcode::InstanceOf
    fn run_opcode_instanceof(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let rhs = self.stack.pop().expect("VM Stack underflow on InstanceOf (rhs)");
        let lhs = self.stack.pop().expect("VM Stack underflow on InstanceOf (lhs)");

        // Per spec §13.10.2 step 3: RHS must be an object (callable check comes after @@hasInstance)
        let is_object_like = matches!(
            &rhs,
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_)
        );
        if !is_object_like {
            let err = self.make_type_error_object(ctx, "Right-hand side of instanceof is not an object");
            self.handle_throw(ctx, &err)?;
            return Ok(OpcodeAction::Continue);
        }

        // Check Symbol.hasInstance (@@sym:2) on rhs first
        let has_instance_fn = match &rhs {
            Value::VmObject(map) => map.borrow().get("@@sym:2").cloned(),
            Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                let fn_props = self
                    .get_fn_props_for_value(ctx, &rhs)
                    .unwrap_or_else(|| self.get_fn_props(ctx, *ip, *arity));
                fn_props.borrow().get("@@sym:2").cloned()
            }
            _ => None,
        };
        if let Some(hi_fn) = has_instance_fn {
            let result = match hi_fn {
                Value::VmFunction(ip, _) => {
                    self.this_stack.push(rhs.clone());
                    let r = self.call_vm_function_result(ctx, ip, std::slice::from_ref(&lhs), None, &[]);
                    self.this_stack.pop();
                    match r {
                        Ok(v) => v,
                        Err(err) => {
                            self.set_pending_throw_from_error(&err);
                            return Ok(OpcodeAction::Continue);
                        }
                    }
                }
                Value::VmClosure(ip, _, upv) => {
                    self.this_stack.push(rhs.clone());
                    let uv = upv;
                    let r = self.call_vm_function_result(ctx, ip, std::slice::from_ref(&lhs), None, &uv);
                    self.this_stack.pop();
                    match r {
                        Ok(v) => v,
                        Err(err) => {
                            self.set_pending_throw_from_error(&err);
                            return Ok(OpcodeAction::Continue);
                        }
                    }
                }
                Value::VmNativeFunction(id) => self.call_method_builtin(ctx, id, &rhs, std::slice::from_ref(&lhs)),
                _ => Value::Boolean(false),
            };
            self.stack.push(Value::Boolean(result.to_truthy()));
            return Ok(OpcodeAction::Continue);
        }

        // Per spec §13.10.2 step 4: if IsCallable(rhs) is false, throw TypeError
        let is_callable = matches!(&rhs, Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_))
            || matches!(&rhs, Value::VmObject(map) if {
                let b = map.borrow();
                b.contains_key("__host_fn__") || b.contains_key("__native_id__") || b.contains_key("__fn_body__")
            });
        if !is_callable {
            let err = self.make_type_error_object(ctx, "Right-hand side of instanceof is not callable");
            self.handle_throw(ctx, &err)?;
            return Ok(OpcodeAction::Continue);
        }

        // OrdinaryHasInstance: get rhs.prototype for prototype chain walking
        let mut proto_chain_result: Option<bool> = None;

        // Per spec §7.3.21 OrdinaryHasInstance step 3: if Type(O) is not Object, return false
        let lhs_is_object = matches!(
            &lhs,
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_)
        );
        if !lhs_is_object {
            self.stack.push(Value::Boolean(false));
            return Ok(OpcodeAction::Continue);
        }

        let rhs_proto = match &rhs {
            Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                let fn_props = self
                    .get_fn_props_for_value(ctx, &rhs)
                    .unwrap_or_else(|| self.get_fn_props(ctx, *ip, *arity));
                fn_props.borrow().get("prototype").cloned()
            }
            Value::VmObject(map) => map.borrow().get("prototype").cloned(),
            Value::VmNativeFunction(id) => {
                let fn_props = self.get_native_fn_props(ctx, *id);
                fn_props.borrow().get("prototype").cloned()
            }
            _ => None,
        };

        // Per spec §7.3.21 OrdinaryHasInstance step 5: if rhs.prototype is not an object, throw TypeError
        if let Some(ref target_proto) = rhs_proto {
            let proto_is_object = matches!(
                target_proto,
                Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_)
            );
            if !proto_is_object {
                let err = self.make_type_error_object(ctx, "Function has non-object prototype in instanceof check");
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
        }

        if let Some(target_proto) = &rhs_proto {
            // Walk __proto__ chain of lhs looking for target_proto
            let lhs_proto = match &lhs {
                Value::VmObject(obj) => {
                    if obj.borrow().contains_key("__proxy_target__") {
                        // Proxy: use [[GetPrototypeOf]] which calls the trap
                        let proto = self.call_host_fn(ctx, "reflect.getPrototypeOf", None, std::slice::from_ref(&lhs));
                        if self.pending_throw.is_some() {
                            return Ok(OpcodeAction::Continue);
                        }
                        if matches!(proto, Value::Null) { None } else { Some(proto) }
                    } else if obj.borrow().contains_key("__host_fn__") && !obj.borrow().contains_key("__proto__") {
                        if let Some(Value::VmObject(function_ctor)) = self.globals.get("Function") {
                            function_ctor.borrow().get("prototype").cloned()
                        } else {
                            None
                        }
                    } else {
                        obj.borrow().get("__proto__").cloned()
                    }
                }
                Value::VmArray(arr) => {
                    if let Some(proto) = arr.borrow().props.get("__proto__").cloned() {
                        Some(proto)
                    } else {
                        let arr_ctor = self.globals.get("Array").cloned();
                        arr_ctor.and_then(|ctor| {
                            let proto = self.read_named_property(ctx, &ctor, "prototype");
                            if matches!(proto, Value::Undefined) { None } else { Some(proto) }
                        })
                    }
                }
                Value::VmMap(_) => {
                    let map_ctor = self.globals.get("Map").cloned();
                    map_ctor.and_then(|ctor| {
                        let proto = self.read_named_property(ctx, &ctor, "prototype");
                        if matches!(proto, Value::Undefined) { None } else { Some(proto) }
                    })
                }
                Value::VmSet(_) => {
                    let set_ctor = self.globals.get("Set").cloned();
                    set_ctor.and_then(|ctor| {
                        let proto = self.read_named_property(ctx, &ctor, "prototype");
                        if matches!(proto, Value::Undefined) { None } else { Some(proto) }
                    })
                }
                Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                    // Check fn_props for explicit __proto__ first (e.g. async generator functions)
                    let fn_props = self
                        .get_fn_props_for_value(ctx, &lhs)
                        .unwrap_or_else(|| self.get_fn_props(ctx, *ip, *arity));
                    if let Some(proto) = fn_props.borrow().get("__proto__").cloned() {
                        Some(proto)
                    } else if let Some(Value::VmObject(function_ctor)) = self.globals.get("Function") {
                        function_ctor.borrow().get("prototype").cloned()
                    } else {
                        None
                    }
                }
                Value::VmNativeFunction(id) => {
                    let fn_props = self.get_native_fn_props(ctx, *id);
                    if let Some(proto) = fn_props.borrow().get("__proto__").cloned() {
                        Some(proto)
                    } else if let Some(Value::VmObject(function_ctor)) = self.globals.get("Function") {
                        function_ctor.borrow().get("prototype").cloned()
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if lhs_proto.is_some() {
                let mut current = lhs_proto;
                let mut depth = 0;
                loop {
                    if depth > 100 {
                        break;
                    }
                    depth += 1;
                    let proto_val = match current {
                        Some(v) => v,
                        None => break,
                    };
                    let matched = match (&proto_val, target_proto) {
                        (Value::VmObject(a), Value::VmObject(b)) => Gc::ptr_eq(*a, *b),
                        (Value::VmArray(a), Value::VmArray(b)) => Gc::ptr_eq(*a, *b),
                        _ => false,
                    };
                    if matched {
                        proto_chain_result = Some(true);
                        break;
                    }
                    current = match &proto_val {
                        Value::VmObject(proto_obj) => {
                            if proto_obj.borrow().contains_key("__proxy_target__") {
                                let proto = self.call_host_fn(ctx, "reflect.getPrototypeOf", None, std::slice::from_ref(&proto_val));
                                if self.pending_throw.is_some() {
                                    return Ok(OpcodeAction::Continue);
                                }
                                if matches!(proto, Value::Null) { None } else { Some(proto) }
                            } else if proto_obj.borrow().contains_key("__host_fn__") && !proto_obj.borrow().contains_key("__proto__") {
                                if let Some(Value::VmObject(function_ctor)) = self.globals.get("Function") {
                                    function_ctor.borrow().get("prototype").cloned()
                                } else {
                                    None
                                }
                            } else {
                                proto_obj.borrow().get("__proto__").cloned()
                            }
                        }
                        Value::VmArray(proto_arr) => proto_arr.borrow().props.get("__proto__").cloned(),
                        _ => None,
                    };
                }
                if proto_chain_result.is_none() {
                    proto_chain_result = Some(false);
                }
            }
        }

        let result = if let Some(r) = proto_chain_result {
            r
        } else {
            // Fallback: name-based instanceof for built-in types
            let ctor_name = match &rhs {
                Value::VmNativeFunction(id) => match *id {
                    BUILTIN_CTOR_ERROR => "Error",
                    BUILTIN_CTOR_TYPEERROR => "TypeError",
                    BUILTIN_CTOR_SYNTAXERROR => "SyntaxError",
                    BUILTIN_CTOR_RANGEERROR => "RangeError",
                    BUILTIN_CTOR_REFERENCEERROR => "ReferenceError",
                    BUILTIN_CTOR_DATE => "Date",
                    BUILTIN_CTOR_FUNCTION => "Function",
                    BUILTIN_CTOR_NUMBER => "Number",
                    BUILTIN_CTOR_STRING => "String",
                    BUILTIN_CTOR_BOOLEAN => "Boolean",
                    BUILTIN_CTOR_OBJECT => "Object",
                    BUILTIN_CTOR_WEAKREF => "WeakRef",
                    BUILTIN_CTOR_WEAKMAP => "WeakMap",
                    BUILTIN_CTOR_WEAKSET => "WeakSet",
                    BUILTIN_CTOR_FR => "FinalizationRegistry",
                    BUILTIN_CTOR_REGEXP => "RegExp",
                    _ => "",
                },
                Value::VmObject(map) => {
                    if let Some(function_id) = get_function_id(*map) {
                        match function_id {
                            BUILTIN_CTOR_DATE => "Date",
                            BUILTIN_CTOR_FUNCTION => "Function",
                            BUILTIN_CTOR_NUMBER => "Number",
                            BUILTIN_CTOR_STRING => "String",
                            BUILTIN_CTOR_BOOLEAN => "Boolean",
                            BUILTIN_CTOR_OBJECT => "Object",
                            BUILTIN_CTOR_ERROR => "Error",
                            BUILTIN_CTOR_TYPEERROR => "TypeError",
                            BUILTIN_CTOR_SYNTAXERROR => "SyntaxError",
                            BUILTIN_CTOR_RANGEERROR => "RangeError",
                            BUILTIN_CTOR_REFERENCEERROR => "ReferenceError",
                            BUILTIN_CTOR_WEAKREF => "WeakRef",
                            BUILTIN_CTOR_WEAKMAP => "WeakMap",
                            BUILTIN_CTOR_WEAKSET => "WeakSet",
                            BUILTIN_CTOR_FR => "FinalizationRegistry",
                            _ => "",
                        }
                    } else {
                        ""
                    }
                }
                _ => "",
            };
            let ctor_str = if ctor_name.is_empty() {
                value_to_string(&rhs)
            } else {
                ctor_name.to_string()
            };
            match &lhs {
                Value::VmObject(map) => {
                    let borrow = map.borrow();
                    if let Some(Value::String(type_u16)) = borrow.get("__type__") {
                        let type_name = crate::unicode::utf16_to_utf8(type_u16);
                        match ctor_str.as_str() {
                            "Error" => type_name == "Error" || type_name.ends_with("Error"),
                            "Object" => true,
                            _ => type_name == ctor_str,
                        }
                    } else {
                        ctor_str == "Object"
                    }
                }
                Value::VmArray(_) => matches!(ctor_str.as_str(), "Object" | "Array"),
                Value::VmMap(_) | Value::VmSet(_) => ctor_str == "Object",
                _ if ctor_str == "Function" => {
                    matches!(&lhs, Value::VmNativeFunction(_) | Value::VmFunction(..) | Value::VmClosure(..))
                        || matches!(&lhs, Value::VmObject(m) if {
                            let b = m.borrow();
                            b.contains_key("__native_id__") || b.contains_key("__host_fn__")
                        })
                }
                _ => false,
            }
        };
        self.stack.push(Value::Boolean(result));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::DeleteProperty
    fn run_opcode_delete_property(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let name_idx = self.read_u16() as usize;
        let name_val = &self.chunk.constants[name_idx];
        let key = if let Value::String(s) = name_val {
            crate::unicode::utf16_to_utf8(s)
        } else {
            value_to_string(name_val)
        };
        let obj = self.stack.pop().expect("VM Stack underflow on DeleteProperty");
        match self.try_proxy_delete(ctx, &obj, &key) {
            Ok(Some(result)) => {
                if !result && self.current_execution_is_strict() {
                    let mut err_map = IndexMap::new();
                    err_map.insert(
                        "message".to_string(),
                        Value::from(&format!("Cannot delete property '{}' of #<Object>", key)),
                    );
                    err_map.insert("__type__".to_string(), Value::from("TypeError"));
                    err_map.insert("name".to_string(), Value::from("TypeError"));
                    self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                }
                self.stack.push(Value::Boolean(result));
                return Ok(OpcodeAction::Continue);
            }
            Ok(None) => {}
            Err(err) => {
                self.set_pending_throw_from_error(&err);
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
                return Err(err);
            }
        }
        if let Value::VmObject(map) = &obj {
            // Module namespace exotic object [[Delete]] (§10.4.6.10)
            if map.borrow().contains_key("__module_namespace__") {
                let is_export = if !key.starts_with("@@sym:") {
                    if let Some(Value::VmObject(bindings)) = map.borrow().get("__ns_bindings__") {
                        bindings.borrow().contains_key(&key)
                    } else {
                        // Loaded module namespace: check key directly
                        !key.starts_with("__") && map.borrow().contains_key(&key)
                    }
                } else {
                    false
                };
                if is_export {
                    let err = self.make_type_error_object(ctx, &format!("Cannot delete property '{}' of [object Module]", key));
                    self.handle_throw(ctx, &err)?;
                    self.stack.push(Value::Boolean(false));
                } else if key.starts_with("@@sym:") {
                    let nc_key = format!("__nonconfigurable_{}__", key);
                    if map.borrow().contains_key(&nc_key) {
                        let err =
                            self.make_type_error_object(ctx, "Cannot delete property 'Symbol(Symbol.toStringTag)' of [object Module]");
                        self.handle_throw(ctx, &err)?;
                        self.stack.push(Value::Boolean(false));
                    } else {
                        self.stack.push(Value::Boolean(true));
                    }
                } else {
                    self.stack.push(Value::Boolean(true));
                }
                return Ok(OpcodeAction::Continue);
            } else {
                let nc_key = format!("__nonconfigurable_{}__", key);
                if map.borrow().contains_key(&nc_key) {
                    if self.current_execution_is_strict() {
                        let err = self.make_type_error_object(ctx, &format!("Cannot delete property '{}' of #<Object>", key));
                        self.handle_throw(ctx, &err)?;
                    }
                    self.stack.push(Value::Boolean(false));
                } else {
                    let getter_key = format!("__get_{}", key);
                    let setter_key = format!("__set_{}", key);
                    let ne_key = format!("__nonenumerable_{}__", key);
                    let ro_key = format!("__readonly_{}__", key);
                    let mut b = map.borrow_mut(ctx);
                    b.shift_remove(&key);
                    if key == "@@sym:4" || key == "Symbol(Symbol.toStringTag)" {
                        b.shift_remove("__toStringTag__");
                    }
                    b.shift_remove(&getter_key);
                    b.shift_remove(&setter_key);
                    b.shift_remove(&nc_key);
                    b.shift_remove(&ne_key);
                    b.shift_remove(&ro_key);
                    self.stack.push(Value::Boolean(true));
                }
            }
        } else if let Value::VmFunction(..) | Value::VmClosure(..) = &obj {
            let props = self.get_fn_props_for_value(ctx, &obj).unwrap();
            let nc_key = format!("__nonconfigurable_{}__", key);
            if props.borrow().contains_key(&nc_key) {
                let mut err_map = IndexMap::new();
                err_map.insert(
                    "message".to_string(),
                    Value::from(&format!("Cannot delete property '{}' of #<Object>", key)),
                );
                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                self.stack.push(Value::Boolean(false));
            } else {
                let getter_key = format!("__get_{}", key);
                let setter_key = format!("__set_{}", key);
                let ne_key = format!("__nonenumerable_{}__", key);
                let ro_key = format!("__readonly_{}__", key);
                let mut b = props.borrow_mut(ctx);
                b.shift_remove(&key);
                b.shift_remove(&getter_key);
                b.shift_remove(&setter_key);
                b.shift_remove(&nc_key);
                b.shift_remove(&ne_key);
                b.shift_remove(&ro_key);
                self.stack.push(Value::Boolean(true));
            }
        } else if let Value::VmNativeFunction(id) = &obj {
            let props = self.get_native_fn_props(ctx, *id);
            let nc_key = format!("__nonconfigurable_{}__", key);
            if props.borrow().contains_key(&nc_key) {
                let mut err_map = IndexMap::new();
                err_map.insert(
                    "message".to_string(),
                    Value::from(&format!("Cannot delete property '{}' of #<Object>", key)),
                );
                err_map.insert("__type__".to_string(), Value::from("TypeError"));
                self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                self.stack.push(Value::Boolean(false));
            } else {
                let getter_key = format!("__get_{}", key);
                let setter_key = format!("__set_{}", key);
                let ne_key = format!("__nonenumerable_{}__", key);
                let ro_key = format!("__readonly_{}__", key);
                let mut b = props.borrow_mut(ctx);
                b.shift_remove(&key);
                b.shift_remove(&getter_key);
                b.shift_remove(&setter_key);
                b.shift_remove(&nc_key);
                b.shift_remove(&ne_key);
                b.shift_remove(&ro_key);
                self.stack.push(Value::Boolean(true));
            }
        } else if let Value::VmArray(arr) = &obj {
            let mut b = arr.borrow_mut(ctx);
            if let Ok(idx) = key.parse::<usize>()
                && idx < b.elements.len()
            {
                b.elements[idx] = Value::Undefined;
                b.props.insert(format!("__deleted_{}", idx), Value::Boolean(true));
            }
            b.props.shift_remove(&key);
            b.props.shift_remove(&format!("__get_{}", key));
            b.props.shift_remove(&format!("__set_{}", key));
            b.props.shift_remove(&format!("__nonconfigurable_{}__", key));
            b.props.shift_remove(&format!("__nonenumerable_{}__", key));
            b.props.shift_remove(&format!("__readonly_{}__", key));
            self.stack.push(Value::Boolean(true));
        } else if matches!(obj, Value::Null | Value::Undefined) {
            let type_name = if matches!(obj, Value::Null) { "null" } else { "undefined" };
            let err = self.make_type_error_object(ctx, &format!("Cannot convert {} to object", type_name));
            self.handle_throw(ctx, &err)?;
            return Ok(OpcodeAction::Continue);
        } else {
            // Primitives (Number, Boolean, String, BigInt, Symbol) — property doesn't
            // exist on wrapper, so delete returns true.
            self.stack.push(Value::Boolean(true));
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::NewCall
    fn run_opcode_new_call(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let arg_count = self.read_byte() as usize;
        // Stack: [..., constructor, arg0, arg1, ...]
        let callee_idx = self.stack.len() - arg_count - 1;
        let callee = self.stack[callee_idx].clone();
        if !self.is_constructor_value(&callee) {
            let callee_name = self.resolve_callee_name(callee_idx);
            for _ in 0..arg_count {
                self.stack.pop();
            }
            self.stack.pop();
            let mut err_map = IndexMap::new();
            err_map.insert("__type__".to_string(), Value::from("TypeError"));
            err_map.insert("message".to_string(), Value::from(&format!("{} is not a constructor", callee_name)));
            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
            return Ok(OpcodeAction::Continue);
        }
        match callee {
            Value::VmFunction(target_ip, _arity) | Value::VmClosure(target_ip, _arity, _) => {
                if self.fn_realm.contains_key(&target_ip) {
                    let args: Vec<Value<'gc>> = (0..arg_count)
                        .map(|_| self.stack.pop().expect("VM Stack underflow"))
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    self.stack.pop(); // pop constructor
                    match self.construct_value(ctx, &callee, &args, Some(&callee)) {
                        Ok(result) => self.stack.push(result),
                        Err(err) => {
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(ctx, &thrown)?;
                            } else {
                                let thrown = self.vm_value_from_error(ctx, &err);
                                self.handle_throw(ctx, &thrown)?;
                            }
                            return Ok(OpcodeAction::Continue);
                        }
                    }
                    return Ok(OpcodeAction::Continue);
                }
                if self.chunk.async_function_ips.contains(&target_ip) {
                    let callee_name = self.resolve_callee_name(callee_idx);
                    for _ in 0..arg_count {
                        self.stack.pop();
                    }
                    self.stack.pop();
                    let mut err_map = IndexMap::new();
                    err_map.insert("__type__".to_string(), Value::from("TypeError"));
                    err_map.insert("message".to_string(), Value::from(&format!("{} is not a constructor", callee_name)));
                    self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                    return Ok(OpcodeAction::Continue);
                }
                // Match regular Call behavior: keep parameter slots aligned with arity.
                if arg_count < _arity as usize {
                    for _ in 0..(_arity as usize - arg_count) {
                        self.stack.push(Value::Undefined);
                    }
                }
                let saved_args = if arg_count > _arity as usize {
                    let first_arg_idx = callee_idx + 1;
                    let all_args: Vec<Value<'gc>> = self.stack[first_arg_idx..first_arg_idx + arg_count].to_vec();
                    let drain_start = first_arg_idx + _arity as usize;
                    let drain_end = first_arg_idx + arg_count;
                    self.stack.drain(drain_start..drain_end);
                    Some(all_args)
                } else {
                    None
                };

                // Create new empty object as `this`
                let new_obj = new_gc_cell_ptr(ctx, IndexMap::new());
                // Set __proto__ to constructor's prototype property (per-closure override first)
                let fn_props = self
                    .get_fn_props_for_value(ctx, &callee)
                    .unwrap_or_else(|| self.get_fn_props(ctx, target_ip, _arity));
                if let Some(proto) = fn_props.borrow().get("prototype").cloned() {
                    new_obj.borrow_mut(ctx).insert("__proto__".to_string(), proto);
                }
                let this_val = Value::VmObject(new_obj);
                self.this_stack.push(this_val);
                // Push new.target = the constructor being invoked
                self.new_target_stack.push(callee.clone());
                let closure_uv = if let Value::VmClosure(_, _, uv) = callee {
                    (**uv).to_vec()
                } else {
                    Vec::new()
                };
                // Set up call frame
                let is_derived_ctor = self.chunk.derived_constructor_ips.contains(&target_ip);
                let frame = CallFrame {
                    return_ip: self.ip,
                    bp: callee_idx + 1,
                    is_method: false,
                    arg_count,
                    func_ip: target_ip,
                    arguments_obj: None,
                    upvalues: closure_uv,
                    saved_args,
                    local_cells: HashMap::new(),
                    this_tdz: if is_derived_ctor { Some(true) } else { None },
                };
                self.frames.push(frame);
                self.ip = target_ip;
                if is_derived_ctor {
                    self.super_called_stack.push(false);
                }
                // Isolate try_stack so constructor throws propagate back to us
                // instead of being caught by an outer try-catch across the
                // run_inner boundary.
                let saved_try_stack = std::mem::take(&mut self.try_stack);
                let result = self.run_inner(ctx, self.frames.len());
                self.try_stack = saved_try_stack;
                let super_was_called = if is_derived_ctor {
                    self.super_called_stack.pop().unwrap_or(false)
                } else {
                    true
                };
                // Capture the (possibly updated) this value before popping
                let bound_this = self.this_stack.last().cloned().unwrap_or(Value::VmObject(new_obj));
                self.this_stack.pop();
                self.new_target_stack.pop();
                // The constructor returns `this` (we compiled GetThis+Return)
                // but result from run_inner is what was returned
                match result {
                    Ok(val) => {
                        // If constructor returned an object, use it; otherwise use `this`
                        let is_derived = self.chunk.derived_constructor_ips.contains(&target_ip);
                        let is_real_object = match &val {
                            Value::VmObject(map) => !map.borrow().contains_key("__vm_symbol__"),
                            Value::VmArray(_)
                            | Value::VmMap(_)
                            | Value::VmSet(_)
                            | Value::VmFunction(..)
                            | Value::VmClosure(..)
                            | Value::VmNativeFunction(_)
                            | Value::Function(_) => true,
                            _ => false,
                        };
                        if is_real_object {
                            self.stack.push(val);
                        } else if is_derived && !matches!(&val, Value::Undefined) {
                            let err = self.make_type_error_object(ctx, "Derived constructors may only return object or undefined");
                            self.handle_throw(ctx, &err)?;
                            return Ok(OpcodeAction::Continue);
                        } else if is_derived && !super_was_called {
                            let err = self.make_reference_error(
                                ctx,
                                "Must call super constructor in derived class before returning from derived constructor",
                            );
                            self.handle_throw(ctx, &err)?;
                            return Ok(OpcodeAction::Continue);
                        } else {
                            self.stack.push(bound_this);
                        }
                    }
                    Err(e) => {
                        // Constructor threw — convert back to VM-level throw
                        // so the outer try-catch can handle it.
                        let thrown = self.vm_value_from_error(ctx, &e);
                        self.pending_throw = Some(thrown);
                        return Ok(OpcodeAction::Continue);
                    }
                }
            }
            Value::VmNativeFunction(id) => {
                let args: Vec<Value<'gc>> = (0..arg_count)
                    .map(|_| self.stack.pop().expect("VM Stack underflow"))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                self.stack.pop(); // pop constructor
                match id {
                    BUILTIN_CTOR_MAP => {
                        let mut entries = Vec::new();
                        // new Map(iterable) — iterable is an array of [key, value] pairs
                        if let Some(Value::VmArray(arr)) = args.first() {
                            for item in arr.borrow().iter() {
                                if let Value::VmArray(pair) = item {
                                    let p = pair.borrow();
                                    let k = p.first().cloned().unwrap_or(Value::Undefined);
                                    let v = p.get(1).cloned().unwrap_or(Value::Undefined);
                                    entries.push((k, v));
                                } else {
                                    entries.push((item.clone(), Value::Undefined));
                                }
                            }
                        }
                        self.stack
                            .push(Value::VmMap(new_gc_cell_ptr(ctx, VmMapData { entries, is_weak: false })));
                    }
                    BUILTIN_CTOR_SET => {
                        let mut values = Vec::new();
                        // new Set(iterable) — iterable is an array
                        if let Some(Value::VmArray(arr)) = args.first() {
                            for item in arr.borrow().iter() {
                                if !values.iter().any(|v| self.values_equal(v, item)) {
                                    values.push(item.clone());
                                }
                            }
                        }
                        self.stack
                            .push(Value::VmSet(new_gc_cell_ptr(ctx, VmSetData { values, is_weak: false })));
                    }
                    BUILTIN_CTOR_WEAKMAP => {
                        // WeakMap: implemented as regular Map (no GC)
                        self.stack.push(Value::VmMap(new_gc_cell_ptr(
                            ctx,
                            VmMapData {
                                entries: Vec::new(),
                                is_weak: true,
                            },
                        )));
                    }
                    BUILTIN_CTOR_WEAKSET => {
                        // WeakSet: implemented as regular Set (no GC)
                        self.stack.push(Value::VmSet(new_gc_cell_ptr(
                            ctx,
                            VmSetData {
                                values: Vec::new(),
                                is_weak: true,
                            },
                        )));
                    }
                    BUILTIN_CTOR_WEAKREF => {
                        // WeakRef: target must be an object or unregistered symbol
                        let target = args.into_iter().next().unwrap_or(Value::Undefined);
                        // Check for registered VM symbol — reject it
                        let is_registered_symbol = if let Value::VmObject(ref obj) = target {
                            let b = obj.borrow();
                            b.contains_key("__vm_symbol__") && b.contains_key("__registered__")
                        } else {
                            false
                        };
                        let is_valid = match &target {
                            Value::VmObject(_) if !is_registered_symbol => true,
                            Value::VmArray(_) | Value::VmMap(_) | Value::VmSet(_) | Value::VmFunction(..) | Value::VmClosure(..) => true,
                            _ => false,
                        };
                        if is_valid {
                            let mut m = IndexMap::new();
                            m.insert("__weakref__".to_string(), Value::Boolean(true));
                            m.insert("__target__".to_string(), target);
                            m.insert("__type__".to_string(), Value::from("WeakRef"));
                            if let Some(wr_ctor) = self.globals.get("WeakRef").cloned() {
                                let proto = self.read_named_property(ctx, &wr_ctor, "prototype");
                                if !matches!(proto, Value::Undefined) {
                                    m.insert("__proto__".to_string(), proto);
                                }
                            }
                            self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, m)));
                        } else {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::from("TypeError"));
                            err_map.insert("message".to_string(), Value::from("Invalid value used as weak reference target"));
                            let err = Value::VmObject(new_gc_cell_ptr(ctx, err_map));
                            self.handle_throw(ctx, &err)?;
                        }
                    }
                    BUILTIN_CTOR_FR => {
                        // new FinalizationRegistry(callback)
                        let callback = args.into_iter().next().unwrap_or(Value::Undefined);
                        let is_callable = matches!(callback, Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_))
                            || matches!(&callback, Value::VmObject(o) if {
                                let b = o.borrow();
                                b.contains_key("__fn_body__") || b.contains_key("__native_id__")
                            });
                        if !is_callable {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::from("TypeError"));
                            err_map.insert(
                                "message".to_string(),
                                Value::from("FinalizationRegistry requires a callable cleanup callback"),
                            );
                            let err = Value::VmObject(new_gc_cell_ptr(ctx, err_map));
                            self.handle_throw(ctx, &err)?;
                        } else {
                            let mut m = IndexMap::new();
                            m.insert("__fr__".to_string(), Value::Boolean(true));
                            m.insert("__fr_callback__".to_string(), callback);
                            m.insert("__fr_count__".to_string(), Value::Number(0.0));
                            m.insert("__type__".to_string(), Value::from("FinalizationRegistry"));
                            if let Some(fr_ctor) = self.globals.get("FinalizationRegistry").cloned() {
                                let proto = self.read_named_property(ctx, &fr_ctor, "prototype");
                                if !matches!(proto, Value::Undefined) {
                                    m.insert("__proto__".to_string(), proto);
                                }
                            }
                            self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, m)));
                        }
                    }
                    BUILTIN_CTOR_REGEXP => {
                        // new RegExp(pattern, flags)
                        let (pattern, flags) = match args.first() {
                            Some(Value::VmObject(pat_obj))
                                if pat_obj.borrow().get("__type__").map(value_to_string).as_deref() == Some("RegExp") =>
                            {
                                let p = pat_obj.borrow().get("__regex_pattern__").map(value_to_string).unwrap_or_default();
                                let f = if matches!(args.get(1), None | Some(Value::Undefined)) {
                                    pat_obj.borrow().get("__regex_flags__").map(value_to_string).unwrap_or_default()
                                } else {
                                    self.vm_to_string(ctx, args.get(1).unwrap())
                                };
                                (p, f)
                            }
                            _ => {
                                let p = match args.first() {
                                    None | Some(Value::Undefined) => String::new(),
                                    Some(v) => self.vm_to_string(ctx, v),
                                };
                                if self.pending_throw.is_some() {
                                    if let Some(thrown) = self.pending_throw.take() {
                                        self.handle_throw(ctx, &thrown)?;
                                    }
                                    return Ok(OpcodeAction::Continue);
                                }
                                let f = match args.get(1) {
                                    None | Some(Value::Undefined) => String::new(),
                                    Some(v) => self.vm_to_string(ctx, v),
                                };
                                if self.pending_throw.is_some() {
                                    if let Some(thrown) = self.pending_throw.take() {
                                        self.handle_throw(ctx, &thrown)?;
                                    }
                                    return Ok(OpcodeAction::Continue);
                                }
                                (p, f)
                            }
                        };
                        if self.pending_throw.is_some() {
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(ctx, &thrown)?;
                            }
                            return Ok(OpcodeAction::Continue);
                        }
                        if let Some(err_msg) = Self::validate_regexp_flags(&flags) {
                            self.throw_syntax_error(ctx, &err_msg);
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(ctx, &thrown)?;
                            }
                            return Ok(OpcodeAction::Continue);
                        }
                        // Validate pattern by attempting compilation
                        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
                        let regress_flags: String = flags.chars().filter(|c| "gimsuvy".contains(*c)).collect();
                        if let Err(e) = super::get_or_compile_regex(&pattern_u16, &regress_flags) {
                            self.throw_syntax_error(ctx, &format!("Invalid regular expression: /{}/: {}", pattern, e));
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(ctx, &thrown)?;
                            }
                            return Ok(OpcodeAction::Continue);
                        }
                        let mut map = IndexMap::new();
                        map.insert("__regex_pattern__".to_string(), Value::from(&pattern));
                        map.insert("__regex_flags__".to_string(), Value::from(&flags));
                        map.insert("__type__".to_string(), Value::from("RegExp"));
                        map.insert("__toStringTag__".to_string(), Value::from("RegExp"));
                        map.insert("lastIndex".to_string(), Value::Number(0.0));
                        if let Some(Value::VmObject(ctor)) = self.globals.get("RegExp")
                            && let Some(proto) = ctor.borrow().get("prototype").cloned()
                        {
                            map.insert("__proto__".to_string(), proto);
                        }
                        map.insert("__nonconfigurable_lastIndex__".to_string(), Value::Boolean(true));
                        map.insert("__nonenumerable_lastIndex__".to_string(), Value::Boolean(true));
                        self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, map)));
                    }
                    BUILTIN_CTOR_DATE => {
                        let ms = self.date_construct_ms(ctx, &args);
                        let mut map = IndexMap::new();
                        map.insert("__type__".to_string(), Value::from("Date"));
                        map.insert("__date_ms__".to_string(), Value::Number(ms));
                        if let Some(Value::VmObject(ctor)) = self.globals.get("Date")
                            && let Some(proto) = ctor.borrow().get("prototype").cloned()
                        {
                            map.insert("__proto__".to_string(), proto);
                        }
                        self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, map)));
                    }
                    BUILTIN_CTOR_NUMBER => {
                        let result = self.call_builtin(ctx, id, &args);
                        let mut m = IndexMap::new();
                        m.insert("__type__".to_string(), Value::from("Number"));
                        m.insert("__value__".to_string(), result);
                        if let Some(Value::VmObject(ctor)) = self.globals.get("Number")
                            && let Some(proto) = ctor.borrow().get("prototype").cloned()
                        {
                            m.insert("__proto__".to_string(), proto);
                        }
                        self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, m)));
                    }
                    BUILTIN_CTOR_STRING => {
                        let result = self.call_builtin(ctx, id, &args);
                        let mut m = IndexMap::new();
                        m.insert("__type__".to_string(), Value::from("String"));
                        m.insert("__value__".to_string(), result);
                        if let Some(Value::VmObject(ctor)) = self.globals.get("String")
                            && let Some(proto) = ctor.borrow().get("prototype").cloned()
                        {
                            m.insert("__proto__".to_string(), proto);
                        }
                        self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, m)));
                    }
                    BUILTIN_CTOR_BOOLEAN => {
                        let bool_value = args.first().map(|v| v.to_truthy()).unwrap_or(false);
                        let mut m = IndexMap::new();
                        m.insert("__type__".to_string(), Value::from("Boolean"));
                        m.insert("__value__".to_string(), Value::Boolean(bool_value));
                        if let Some(Value::VmObject(ctor)) = self.globals.get("Boolean")
                            && let Some(proto) = ctor.borrow().get("prototype").cloned()
                        {
                            m.insert("__proto__".to_string(), proto);
                        }
                        self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, m)));
                    }
                    _ => {
                        if !self.is_constructor_value(&Value::VmNativeFunction(id)) {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::from("TypeError"));
                            err_map.insert("message".to_string(), Value::from("is not a constructor"));
                            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                            return Ok(OpcodeAction::Continue);
                        }
                        log::warn!("NewCall on VmNativeFunction #{}: returning empty object", id);
                        if id == BUILTIN_CTOR_ARRAYBUFFER {
                            self.new_target_stack.push(Value::VmNativeFunction(id));
                            let result = self.call_builtin(ctx, id, &args);
                            self.new_target_stack.pop();
                            self.stack.push(result);
                        } else {
                            self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, IndexMap::new())));
                        }
                    }
                }
                if let Some(thrown) = self.pending_throw.take() {
                    self.handle_throw(ctx, &thrown)?;
                    return Ok(OpcodeAction::Continue);
                }
            }
            _ => {
                // Check for VmObject with __proxy_target__, __native_id__ etc.
                if let Value::VmObject(ref map) = callee {
                    let function_id = get_function_id(*map);
                    let borrow = map.borrow();
                    if borrow.contains_key("__proxy_target__") || borrow.contains_key("__bound_target__") {
                        drop(borrow);
                        let args: Vec<Value<'gc>> = (0..arg_count)
                            .map(|_| self.stack.pop().expect("VM Stack underflow"))
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        self.stack.pop(); // pop constructor
                        match self.construct_value(ctx, &callee, &args, Some(&callee)) {
                            Ok(result) => self.stack.push(result),
                            Err(err) => {
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(ctx, &thrown)?;
                                } else {
                                    let thrown = self.vm_value_from_error(ctx, &err);
                                    self.handle_throw(ctx, &thrown)?;
                                }
                                return Ok(OpcodeAction::Continue);
                            }
                        }
                    } else if let Some(id) = function_id {
                        let is_async_ctor = matches!(borrow.get("__async_function_constructor__"), Some(Value::Boolean(true)));
                        let is_async_gen_ctor =
                            matches!(borrow.get("__async_generator_function_constructor__"), Some(Value::Boolean(true)));
                        let ctor_origin_global = borrow.get("__origin_global").cloned();
                        drop(borrow);
                        let ctor_prototype = {
                            let p = self.read_named_property(ctx, &callee, "prototype");
                            if matches!(p, Value::Undefined) { None } else { Some(p) }
                        };
                        let args: Vec<Value<'gc>> = (0..arg_count)
                            .map(|_| self.stack.pop().expect("VM Stack underflow"))
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        self.stack.pop(); // pop constructor

                        if (is_async_ctor || is_async_gen_ctor) && args.len() > 1 {
                            let params_src = args[..args.len() - 1].iter().map(value_to_string).collect::<Vec<_>>().join(",");
                            let has_forbidden =
                                self.has_forbidden_dynamic_param_tokens(&params_src, is_async_ctor || is_async_gen_ctor, is_async_gen_ctor);
                            if has_forbidden {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::from("SyntaxError"));
                                err_map.insert("message".to_string(), Value::from("Invalid dynamic function parameter list"));
                                self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                                return Ok(OpcodeAction::Continue);
                            }
                        }

                        // Date is exposed as a constructor object (with __native_id__),
                        // so handle `new Date(...)` here as well.
                        if id == BUILTIN_CTOR_DATE {
                            let ms = match self.date_construct_ms_with_coercion(ctx, &args) {
                                Some(ms) => ms,
                                None => {
                                    // abrupt completion from ToNumber
                                    let mut m = IndexMap::new();
                                    m.insert("__type__".to_string(), Value::from("Date"));
                                    m.insert("__date_ms__".to_string(), Value::Number(f64::NAN));
                                    if let Some(proto) = ctor_prototype.clone() {
                                        m.insert("__proto__".to_string(), proto);
                                    }
                                    self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, m)));
                                    return Ok(OpcodeAction::Continue);
                                }
                            };

                            let mut m = IndexMap::new();
                            m.insert("__type__".to_string(), Value::from("Date"));
                            m.insert("__date_ms__".to_string(), Value::Number(ms));
                            if let Some(proto) = ctor_prototype.clone() {
                                m.insert("__proto__".to_string(), proto);
                            }
                            self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, m)));
                            return Ok(OpcodeAction::Continue);
                        }

                        if id == BUILTIN_CTOR_REGEXP {
                            let (pattern, flags) = match args.first() {
                                Some(Value::VmObject(pat_obj))
                                    if pat_obj.borrow().get("__type__").map(value_to_string).as_deref() == Some("RegExp") =>
                                {
                                    let p = pat_obj.borrow().get("__regex_pattern__").map(value_to_string).unwrap_or_default();
                                    let f = if matches!(args.get(1), None | Some(Value::Undefined)) {
                                        pat_obj.borrow().get("__regex_flags__").map(value_to_string).unwrap_or_default()
                                    } else {
                                        self.vm_to_string(ctx, args.get(1).unwrap())
                                    };
                                    (p, f)
                                }
                                _ => {
                                    let p = match args.first() {
                                        None | Some(Value::Undefined) => String::new(),
                                        Some(v) => self.vm_to_string(ctx, v),
                                    };
                                    if self.pending_throw.is_some() {
                                        if let Some(thrown) = self.pending_throw.take() {
                                            self.handle_throw(ctx, &thrown)?;
                                        }
                                        return Ok(OpcodeAction::Continue);
                                    }
                                    let f = match args.get(1) {
                                        None | Some(Value::Undefined) => String::new(),
                                        Some(v) => self.vm_to_string(ctx, v),
                                    };
                                    if self.pending_throw.is_some() {
                                        if let Some(thrown) = self.pending_throw.take() {
                                            self.handle_throw(ctx, &thrown)?;
                                        }
                                        return Ok(OpcodeAction::Continue);
                                    }
                                    (p, f)
                                }
                            };
                            if self.pending_throw.is_some() {
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(ctx, &thrown)?;
                                }
                                return Ok(OpcodeAction::Continue);
                            }
                            if let Some(err_msg) = Self::validate_regexp_flags(&flags) {
                                self.throw_syntax_error(ctx, &err_msg);
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(ctx, &thrown)?;
                                }
                                return Ok(OpcodeAction::Continue);
                            }
                            // Validate pattern by attempting compilation
                            let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
                            let regress_flags: String = flags.chars().filter(|c| "gimsuvy".contains(*c)).collect();
                            if let Err(e) = super::get_or_compile_regex(&pattern_u16, &regress_flags) {
                                self.throw_syntax_error(ctx, &format!("Invalid regular expression: /{}/: {}", pattern, e));
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(ctx, &thrown)?;
                                }
                                return Ok(OpcodeAction::Continue);
                            }
                            let mut m = IndexMap::new();
                            m.insert("__regex_pattern__".to_string(), Value::from(&pattern));
                            m.insert("__regex_flags__".to_string(), Value::from(&flags));
                            m.insert("__type__".to_string(), Value::from("RegExp"));
                            m.insert("__toStringTag__".to_string(), Value::from("RegExp"));
                            m.insert("lastIndex".to_string(), Value::Number(0.0));
                            if let Some(proto) = ctor_prototype.clone() {
                                m.insert("__proto__".to_string(), proto);
                            }
                            m.insert("__nonconfigurable_lastIndex__".to_string(), Value::Boolean(true));
                            m.insert("__nonenumerable_lastIndex__".to_string(), Value::Boolean(true));
                            self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, m)));
                            return Ok(OpcodeAction::Continue);
                        }

                        if matches!(
                            id,
                            BUILTIN_CTOR_ERROR
                                | BUILTIN_CTOR_TYPEERROR
                                | BUILTIN_CTOR_SYNTAXERROR
                                | BUILTIN_CTOR_RANGEERROR
                                | BUILTIN_CTOR_REFERENCEERROR
                        ) {
                            let type_name = self.error_type_name_from_constructor(ctx, &callee, id);
                            let new_target = self.new_target_stack.last().cloned();
                            let instance_proto = if let Some(new_target) = new_target {
                                let proto = self.read_named_property(ctx, &new_target, "prototype");
                                if matches!(proto, Value::Undefined) {
                                    ctor_prototype.clone().unwrap_or(Value::Undefined)
                                } else {
                                    proto
                                }
                            } else {
                                ctor_prototype.clone().unwrap_or(Value::Undefined)
                            };
                            let mut m = IndexMap::new();
                            m.insert("__type__".to_string(), Value::from(type_name.as_str()));
                            if let Some(message) = args.first().filter(|value| !matches!(value, Value::Undefined)) {
                                let msg = self.vm_to_string(ctx, message);
                                Self::insert_property_with_attributes(&mut m, "message", &Value::from(msg.as_str()), true, false, true);
                            }
                            if !matches!(instance_proto, Value::Undefined) {
                                m.insert("__proto__".to_string(), instance_proto);
                            }
                            self.stack.push(Value::VmObject(new_gc_cell_ptr(ctx, m)));
                            return Ok(OpcodeAction::Continue);
                        }

                        // new Symbol() should throw TypeError
                        if id == BUILTIN_SYMBOL {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::from("TypeError"));
                            err_map.insert("message".to_string(), Value::from("Symbol is not a constructor"));
                            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                            return Ok(OpcodeAction::Continue);
                        }

                        let bool_ctor_value = if id == BUILTIN_CTOR_BOOLEAN {
                            Some(args.first().map(|v| v.to_truthy()).unwrap_or(false))
                        } else {
                            None
                        };
                        self.new_target_stack.push(callee.clone());
                        let result = self.call_builtin(ctx, id, &args);
                        self.new_target_stack.pop();
                        // For constructors like Number/String/Boolean,
                        // wrap the primitive result in an object
                        let wrapped = match id {
                            BUILTIN_CTOR_NUMBER => {
                                let mut m = IndexMap::new();
                                m.insert("__type__".to_string(), Value::from("Number"));
                                m.insert("__value__".to_string(), result);
                                if let Some(proto) = ctor_prototype.clone() {
                                    m.insert("__proto__".to_string(), proto);
                                }
                                Value::VmObject(new_gc_cell_ptr(ctx, m))
                            }
                            BUILTIN_CTOR_STRING => {
                                let mut m = IndexMap::new();
                                m.insert("__type__".to_string(), Value::from("String"));
                                m.insert("__value__".to_string(), result);
                                if let Some(proto) = ctor_prototype.clone() {
                                    m.insert("__proto__".to_string(), proto);
                                }
                                Value::VmObject(new_gc_cell_ptr(ctx, m))
                            }
                            BUILTIN_CTOR_BOOLEAN => {
                                let mut m = IndexMap::new();
                                m.insert("__type__".to_string(), Value::from("Boolean"));
                                let bool_value = bool_ctor_value.unwrap_or(false);
                                m.insert("__value__".to_string(), Value::Boolean(bool_value));
                                if let Some(proto) = ctor_prototype.clone() {
                                    m.insert("__proto__".to_string(), proto);
                                }
                                Value::VmObject(new_gc_cell_ptr(ctx, m))
                            }
                            _ => result,
                        };
                        if id == BUILTIN_CTOR_FUNCTION
                            && let Some(origin_global) = ctor_origin_global.clone()
                        {
                            self.mark_value_origin_global(ctx, &wrapped, &origin_global);
                        }
                        if is_async_ctor || is_async_gen_ctor {
                            self.finalize_dynamic_async_constructor_result(
                                ctx,
                                &wrapped,
                                ctor_prototype.as_ref(),
                                is_async_ctor,
                                is_async_gen_ctor,
                                ctor_prototype.clone().unwrap_or(Value::Undefined),
                                ctor_origin_global.clone(),
                            );
                        }
                        if let Value::VmObject(obj) = &wrapped {
                            let has_proto = obj.borrow().contains_key("__proto__");
                            if !has_proto && let Some(proto) = ctor_prototype {
                                obj.borrow_mut(ctx).insert("__proto__".to_string(), proto);
                            }
                        }
                        self.stack.push(wrapped);
                        if let Some(thrown) = self.pending_throw.take() {
                            self.handle_throw(ctx, &thrown)?;
                            return Ok(OpcodeAction::Continue);
                        }
                    } else if borrow.get("__host_fn__").is_some() {
                        if matches!(borrow.get("__non_constructor__"), Some(Value::Boolean(true))) {
                            drop(borrow);
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::from("TypeError"));
                            err_map.insert("message".to_string(), Value::from("is not a constructor"));
                            self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                            return Ok(OpcodeAction::Continue);
                        }
                        drop(borrow);
                        let args: Vec<Value<'gc>> = (0..arg_count)
                            .map(|_| self.stack.pop().expect("VM Stack underflow"))
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        self.stack.pop(); // pop constructor
                        match self.construct_value(ctx, &callee, &args, None) {
                            Ok(result) => self.stack.push(result),
                            Err(err) => {
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(ctx, &thrown)?;
                                } else {
                                    let thrown = self.vm_value_from_error(ctx, &err);
                                    self.handle_throw(ctx, &thrown)?;
                                }
                                return Ok(OpcodeAction::Continue);
                            }
                        }
                    } else if borrow.contains_key("__fn_body__") {
                        // Dynamic function created via `new Function(...)` — constructible.
                        let proto = borrow.get("prototype").cloned();
                        drop(borrow);
                        let args: Vec<Value<'gc>> = (0..arg_count)
                            .map(|_| self.stack.pop().expect("VM Stack underflow"))
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        self.stack.pop(); // pop constructor
                        let new_obj = new_gc_cell_ptr(ctx, IndexMap::new());
                        if let Some(p) = &proto
                            && matches!(
                                p,
                                Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..)
                            )
                        {
                            new_obj.borrow_mut(ctx).insert("__proto__".to_string(), p.clone());
                        }
                        let this_val = Value::VmObject(new_obj);
                        match self.vm_call_function_value(ctx, &callee, &this_val, &args) {
                            Ok(result) => match result {
                                Value::VmObject(_) | Value::VmArray(_) => self.stack.push(result),
                                _ => self.stack.push(this_val),
                            },
                            Err(err) => {
                                let thrown = self.vm_value_from_error(ctx, &err);
                                self.handle_throw(ctx, &thrown)?;
                                return Ok(OpcodeAction::Continue);
                            }
                        }
                    } else {
                        drop(borrow);
                        log::warn!("NewCall on non-constructor VmObject");
                        let callee_name = self.resolve_callee_name(self.stack.len().saturating_sub(arg_count + 1));
                        for _i in 0..arg_count {
                            self.stack.pop();
                        }
                        self.stack.pop();
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::from("TypeError"));
                        err_map.insert("message".to_string(), Value::from(&format!("{} is not a constructor", callee_name)));
                        self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                        return Ok(OpcodeAction::Continue);
                    }
                } else {
                    log::warn!("NewCall on non-VmFunction: treating as regular call");
                    let callee_name = self.resolve_callee_name(self.stack.len().saturating_sub(arg_count + 1));
                    for _i in 0..arg_count {
                        self.stack.pop();
                    }
                    self.stack.pop(); // pop constructor
                    let mut err_map = IndexMap::new();
                    err_map.insert("__type__".to_string(), Value::from("TypeError"));
                    err_map.insert("message".to_string(), Value::from(&format!("{} is not a constructor", callee_name)));
                    self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                    return Ok(OpcodeAction::Continue);
                }
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::DeleteIndex
    fn run_opcode_delete_index(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        // Stack: [..., obj, index]
        let idx_val = self.stack.pop().expect("VM Stack underflow on DeleteIndex (idx)");
        let obj = self.stack.pop().expect("VM Stack underflow on DeleteIndex (obj)");
        // Proxy handling
        {
            let key = match self.as_property_key_string(ctx, &idx_val) {
                Ok(k) => k,
                Err(_) => value_to_string(&idx_val),
            };
            match self.try_proxy_delete(ctx, &obj, &key) {
                Ok(Some(result)) => {
                    if !result && self.current_execution_is_strict() {
                        let mut err_map = IndexMap::new();
                        err_map.insert(
                            "message".to_string(),
                            Value::from(&format!("Cannot delete property '{}' of #<Object>", key)),
                        );
                        err_map.insert("__type__".to_string(), Value::from("TypeError"));
                        err_map.insert("name".to_string(), Value::from("TypeError"));
                        self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                    }
                    self.stack.push(Value::Boolean(result));
                    return Ok(OpcodeAction::Continue);
                }
                Ok(None) => {}
                Err(err) => {
                    self.set_pending_throw_from_error(&err);
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(ctx, &thrown)?;
                        return Ok(OpcodeAction::Continue);
                    }
                    return Err(err);
                }
            }
        }
        match &obj {
            Value::VmArray(arr) => {
                let key = match self.as_property_key_string(ctx, &idx_val) {
                    Ok(k) => k,
                    Err(_) => value_to_string(&idx_val),
                };
                let nc_key = format!("__nonconfigurable_{}__", key);
                if arr.borrow().props.contains_key(&nc_key) {
                    if self.current_execution_is_strict() {
                        let mut err_map = IndexMap::new();
                        err_map.insert(
                            "message".to_string(),
                            Value::from(&format!("Cannot delete property '{}' of #<Object>", key)),
                        );
                        err_map.insert("__type__".to_string(), Value::from("TypeError"));
                        self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                    }
                    self.stack.push(Value::Boolean(false));
                } else {
                    let mut borrow = arr.borrow_mut(ctx);
                    if let Ok(idx) = key.parse::<usize>()
                        && idx < borrow.elements.len()
                    {
                        borrow.elements[idx] = Value::Undefined;
                        borrow.props.insert(format!("__deleted_{}", idx), Value::Boolean(true));
                    }
                    borrow.props.shift_remove(&key);
                    borrow.props.shift_remove(&format!("__get_{}", key));
                    borrow.props.shift_remove(&format!("__set_{}", key));
                    borrow.props.shift_remove(&nc_key);
                    borrow.props.shift_remove(&format!("__nonenumerable_{}__", key));
                    borrow.props.shift_remove(&format!("__readonly_{}__", key));
                    self.stack.push(Value::Boolean(true));
                }
            }
            Value::VmObject(map) => {
                let key = match self.as_property_key_string(ctx, &idx_val) {
                    Ok(k) => k,
                    Err(_) => value_to_string(&idx_val),
                };
                // Module namespace exotic object [[Delete]]
                if map.borrow().contains_key("__module_namespace__") {
                    let is_export = if !key.starts_with("@@sym:") {
                        if let Some(Value::VmObject(bindings)) = map.borrow().get("__ns_bindings__") {
                            bindings.borrow().contains_key(&key)
                        } else {
                            // Loaded module namespace: check key directly
                            !key.starts_with("__") && map.borrow().contains_key(&key)
                        }
                    } else {
                        false
                    };
                    if is_export {
                        let err = self.make_type_error_object(ctx, &format!("Cannot delete property '{}' of [object Module]", key));
                        self.handle_throw(ctx, &err)?;
                        self.stack.push(Value::Boolean(false));
                    } else if key.starts_with("@@sym:") {
                        let nc_key = format!("__nonconfigurable_{}__", key);
                        if map.borrow().contains_key(&nc_key) {
                            let err =
                                self.make_type_error_object(ctx, "Cannot delete property 'Symbol(Symbol.toStringTag)' of [object Module]");
                            self.handle_throw(ctx, &err)?;
                            self.stack.push(Value::Boolean(false));
                        } else {
                            self.stack.push(Value::Boolean(true));
                        }
                    } else {
                        self.stack.push(Value::Boolean(true));
                    }
                    return Ok(OpcodeAction::Continue);
                }
                let nc_key = format!("__nonconfigurable_{}__", key);
                if map.borrow().contains_key(&nc_key) {
                    if self.current_execution_is_strict() {
                        let mut err_map = IndexMap::new();
                        err_map.insert(
                            "message".to_string(),
                            Value::from(&format!("Cannot delete property '{}' of #<Object>", key)),
                        );
                        err_map.insert("__type__".to_string(), Value::from("TypeError"));
                        self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                        self.stack.push(Value::Boolean(false));
                    } else {
                        self.stack.push(Value::Boolean(false));
                    }
                } else {
                    let getter_key = format!("__get_{}", key);
                    let setter_key = format!("__set_{}", key);
                    let ne_key = format!("__nonenumerable_{}__", key);
                    let ro_key = format!("__readonly_{}__", key);
                    let mut b = map.borrow_mut(ctx);
                    b.shift_remove(&key);
                    if key == "@@sym:4" || key == "Symbol(Symbol.toStringTag)" {
                        b.shift_remove("__toStringTag__");
                    }
                    b.shift_remove(&getter_key);
                    b.shift_remove(&setter_key);
                    b.shift_remove(&nc_key);
                    b.shift_remove(&ne_key);
                    b.shift_remove(&ro_key);
                    self.stack.push(Value::Boolean(true));
                }
            }
            Value::VmFunction(..) | Value::VmClosure(..) => {
                let key = match self.as_property_key_string(ctx, &idx_val) {
                    Ok(k) => k,
                    Err(_) => value_to_string(&idx_val),
                };
                let props = self.get_fn_props_for_value(ctx, &obj).unwrap();
                let nc_key = format!("__nonconfigurable_{}__", key);
                if props.borrow().contains_key(&nc_key) {
                    if self.current_execution_is_strict() {
                        let mut err_map = IndexMap::new();
                        err_map.insert(
                            "message".to_string(),
                            Value::from(&format!("Cannot delete property '{}' of #<Object>", key)),
                        );
                        err_map.insert("__type__".to_string(), Value::from("TypeError"));
                        self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                        self.stack.push(Value::Boolean(false));
                    } else {
                        self.stack.push(Value::Boolean(false));
                    }
                } else {
                    let getter_key = format!("__get_{}", key);
                    let setter_key = format!("__set_{}", key);
                    let ne_key = format!("__nonenumerable_{}__", key);
                    let ro_key = format!("__readonly_{}__", key);
                    let mut b = props.borrow_mut(ctx);
                    b.shift_remove(&key);
                    b.shift_remove(&getter_key);
                    b.shift_remove(&setter_key);
                    b.shift_remove(&nc_key);
                    b.shift_remove(&ne_key);
                    b.shift_remove(&ro_key);
                    self.stack.push(Value::Boolean(true));
                }
            }
            Value::VmNativeFunction(id) => {
                let key = match self.as_property_key_string(ctx, &idx_val) {
                    Ok(k) => k,
                    Err(_) => value_to_string(&idx_val),
                };
                let props = self.get_native_fn_props(ctx, *id);
                let nc_key = format!("__nonconfigurable_{}__", key);
                if props.borrow().contains_key(&nc_key) {
                    if self.current_execution_is_strict() {
                        let mut err_map = IndexMap::new();
                        err_map.insert(
                            "message".to_string(),
                            Value::from(&format!("Cannot delete property '{}' of #<Object>", key)),
                        );
                        err_map.insert("__type__".to_string(), Value::from("TypeError"));
                        self.handle_throw(ctx, &Value::VmObject(new_gc_cell_ptr(ctx, err_map)))?;
                        self.stack.push(Value::Boolean(false));
                    } else {
                        self.stack.push(Value::Boolean(false));
                    }
                } else {
                    let getter_key = format!("__get_{}", key);
                    let setter_key = format!("__set_{}", key);
                    let ne_key = format!("__nonenumerable_{}__", key);
                    let ro_key = format!("__readonly_{}__", key);
                    let mut b = props.borrow_mut(ctx);
                    b.shift_remove(&key);
                    b.shift_remove(&getter_key);
                    b.shift_remove(&setter_key);
                    b.shift_remove(&nc_key);
                    b.shift_remove(&ne_key);
                    b.shift_remove(&ro_key);
                    self.stack.push(Value::Boolean(true));
                }
            }
            Value::Null | Value::Undefined => {
                // Per spec §12.5.3.2 step 5b: ToObject on null/undefined throws TypeError
                let type_name = if matches!(obj, Value::Null) { "null" } else { "undefined" };
                let err = self.make_type_error_object(ctx, &format!("Cannot convert {} to object", type_name));
                self.handle_throw(ctx, &err)?;
                return Ok(OpcodeAction::Continue);
            }
            Value::String(s) => {
                // String: only own integer indices within range are non-deletable
                let key = match self.as_property_key_string(ctx, &idx_val) {
                    Ok(k) => k,
                    Err(_) => value_to_string(&idx_val),
                };
                if let Ok(idx) = key.parse::<usize>() {
                    if idx < s.len() {
                        // Character at index — non-configurable, strict mode throws
                        if self.current_execution_is_strict() {
                            let err = self.make_type_error_object(ctx, &format!("Cannot delete property '{}' of [object String]", key));
                            self.handle_throw(ctx, &err)?;
                            return Ok(OpcodeAction::Continue);
                        }
                        self.stack.push(Value::Boolean(false));
                    } else {
                        self.stack.push(Value::Boolean(true));
                    }
                } else if key == "length" {
                    if self.current_execution_is_strict() {
                        let err = self.make_type_error_object(ctx, "Cannot delete property 'length' of [object String]");
                        self.handle_throw(ctx, &err)?;
                        return Ok(OpcodeAction::Continue);
                    }
                    self.stack.push(Value::Boolean(false));
                } else {
                    self.stack.push(Value::Boolean(true));
                }
            }
            _ => self.stack.push(Value::Boolean(true)),
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::EnterFieldInit
    fn run_opcode_enter_field_init(&mut self, _ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        self.in_field_init = true;
        Ok(OpcodeAction::Continue)
    }

    // Opcode::LeaveFieldInit
    fn run_opcode_leave_field_init(&mut self, _ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        self.in_field_init = false;
        Ok(OpcodeAction::Continue)
    }

    // Opcode::AllocBrand
    fn run_opcode_alloc_brand(&mut self, _ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        self.runtime_brand_counter += 1;
        self.stack.push(Value::Number(self.runtime_brand_counter as f64));
        Ok(OpcodeAction::Continue)
    }

    // Opcode::AssertIterResult
    fn run_opcode_assert_iter_result(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        if let Some(top) = self.stack.last() {
            let is_object = matches!(top, Value::VmObject(_) | Value::VmArray(_) | Value::VmMap(_) | Value::VmSet(_));
            if !is_object {
                let err = self.make_type_error_object(ctx, "Iterator result is not an object");
                self.handle_throw(ctx, &err)?;
            }
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::BoxLocal
    fn run_opcode_box_local(&mut self, ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let index = self.read_byte() as usize;
        let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
        let val = if bp + index < self.stack.len() {
            self.stack[bp + index].clone()
        } else {
            Value::Undefined
        };
        let cell = new_gc_cell_ptr(ctx, val);
        if let Some(frame) = self.frames.last_mut() {
            frame.local_cells.insert(index, cell);
        } else {
            self.top_level_cells.insert(index, cell);
        }
        Ok(OpcodeAction::Continue)
    }

    // Opcode::InitNamedFnSelf — push the callee for a named function expression
    fn run_opcode_init_named_fn_self(&mut self, _ctx: &GcContext<'gc>) -> Result<OpcodeAction<'gc>, JSError> {
        let callee = self.named_fn_callee_stack.pop().unwrap_or(Value::Undefined);
        // Insert at frame base so the named fn self occupies local slot 0,
        // shifting existing args (already on the stack) up by one position.
        let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
        self.stack.insert(bp, callee);
        Ok(OpcodeAction::Continue)
    }
}

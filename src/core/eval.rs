use crate::core::{Gc, GcCell, MutationContext, SwitchCase, create_descriptor_object, new_gc_cell_ptr};
use crate::js_array::{create_array, handle_array_static_method, is_array, set_array_length};
use crate::js_async::handle_async_closure_call;
use crate::js_async_generator::handle_async_generator_function_call;
use crate::js_bigint::{bigint_constructor, compare_bigint_and_number, string_to_bigint_for_eq};
use crate::js_class::{create_class_object, prepare_call_env_with_this};
use crate::js_date::{handle_date_method, handle_date_static_method, is_date_object};
use crate::js_function::{handle_function_prototype_method, handle_global_function};
use crate::js_generator::handle_generator_function_call;
use crate::js_json::handle_json_method;
use crate::js_number::{handle_number_prototype_method, handle_number_static_method, number_constructor};
use crate::js_set::handle_set_instance_method;
use crate::js_string::{handle_string_method, string_from_char_code, string_from_code_point, string_raw};
use crate::js_typedarray::{ensure_typedarray_in_bounds, get_array_like_element, is_typedarray};
use crate::{
    JSError, JSErrorKind, PropertyKey, Value,
    core::{
        BinaryOp, ClosureData, DestructuringElement, EvalError, ExportSpecifier, Expr, ImportSpecifier, JSObjectDataPtr,
        ObjectDestructuringElement, Statement, StatementKind, create_error, env_get, env_get_own, env_get_strictness, env_set,
        env_set_recursive, env_set_strictness, get_own_property, is_error, new_js_object_data, object_get_key_value, object_get_length,
        object_set_key_value, object_set_length, to_primitive, value_to_string,
    },
    js_math::handle_math_call,
    raise_eval_error, raise_reference_error,
    unicode::{utf8_to_utf16, utf16_to_utf8},
};
use crate::{Token, parse_statements, raise_range_error, raise_syntax_error, raise_type_error, tokenize};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive, Zero};

thread_local! {
    static OPT_CHAIN_RECURSION_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

const OPT_CHAIN_RECURSION_LIMIT: u32 = 2000;

#[derive(Clone, Debug)]
pub enum ControlFlow<'gc> {
    Normal(Value<'gc>),
    Return(Value<'gc>),
    Throw(Value<'gc>, Option<usize>, Option<usize>), // value, line, column
    Break(Option<String>),
    Continue(Option<String>),
}

pub(crate) fn to_number<'gc>(val: &Value<'gc>) -> Result<f64, EvalError<'gc>> {
    match val {
        Value::Number(n) => Ok(*n),
        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Null => Ok(0.0),
        Value::Undefined | Value::Uninitialized => Ok(f64::NAN),
        Value::BigInt(_) => Err(raise_type_error!("Cannot convert a BigInt value to a number").into()),
        Value::String(s) => {
            let str_val = utf16_to_utf8(s);
            let trimmed = str_val.trim();
            if trimmed.is_empty() {
                return Ok(0.0);
            }
            if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
                return i64::from_str_radix(hex, 16)
                    .map(|n| n as f64)
                    .map_err(|_| raise_range_error!("Invalid number").into());
            }
            if let Some(bin) = trimmed.strip_prefix("0b").or_else(|| trimmed.strip_prefix("0B")) {
                return i64::from_str_radix(bin, 2)
                    .map(|n| n as f64)
                    .map_err(|_| raise_range_error!("Invalid number").into());
            }
            if let Some(oct) = trimmed.strip_prefix("0o").or_else(|| trimmed.strip_prefix("0O")) {
                return i64::from_str_radix(oct, 8)
                    .map(|n| n as f64)
                    .map_err(|_| raise_range_error!("Invalid number").into());
            }
            Ok(trimmed.parse::<f64>().unwrap_or(f64::NAN))
        }
        Value::Symbol(_) => Err(raise_type_error!("Cannot convert a Symbol value to a number").into()),
        Value::Object(_) => Err(raise_type_error!("Cannot convert object to number without context").into()),
        _ => Ok(f64::NAN),
    }
}

fn loose_equal<'gc>(mc: &MutationContext<'gc>, l: &Value<'gc>, r: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<bool, EvalError<'gc>> {
    match (l, r) {
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => Ok(true),
        (Value::Undefined, Value::Null) | (Value::Null, Value::Undefined) => Ok(true),
        (Value::Boolean(l), Value::Boolean(r)) => Ok(l == r),
        (Value::Number(l), Value::Number(r)) => Ok(l == r),
        (Value::String(l), Value::String(r)) => Ok(l == r),
        (Value::BigInt(l), Value::BigInt(r)) => Ok(l == r),
        (Value::Number(l), Value::String(_r)) => {
            let rn = to_number(r)?;
            Ok(*l == rn)
        }
        (Value::String(_l), Value::Number(r)) => {
            let ln = to_number(l)?;
            Ok(ln == *r)
        }
        (Value::Boolean(b), other) => {
            let n = if *b { 1.0 } else { 0.0 };
            loose_equal(mc, &Value::Number(n), other, env)
        }
        (other, Value::Boolean(b)) => {
            let n = if *b { 1.0 } else { 0.0 };
            loose_equal(mc, other, &Value::Number(n), env)
        }
        (Value::BigInt(l), Value::Number(r)) => {
            if !r.is_finite() || r.is_nan() || r.fract() != 0.0 {
                Ok(false)
            } else {
                Ok(BigInt::from_f64(*r).map(|rb| **l == rb).unwrap_or(false))
            }
        }
        (Value::Number(l), Value::BigInt(r)) => {
            if !l.is_finite() || l.is_nan() || l.fract() != 0.0 {
                Ok(false)
            } else {
                Ok(BigInt::from_f64(*l).map(|lb| lb == **r).unwrap_or(false))
            }
        }
        (Value::String(l), Value::BigInt(r)) => {
            let ls = utf16_to_utf8(l);
            Ok(string_to_bigint_for_eq(&ls).map(|lb| lb == **r).unwrap_or(false))
        }
        (Value::BigInt(l), Value::String(r)) => {
            let rs = utf16_to_utf8(r);
            Ok(string_to_bigint_for_eq(&rs).map(|rb| **l == rb).unwrap_or(false))
        }
        (Value::Symbol(l), Value::Symbol(r)) => Ok(Gc::as_ptr(*l) == Gc::as_ptr(*r)),
        (Value::Object(l), Value::Object(r)) => Ok(Gc::as_ptr(*l) == Gc::as_ptr(*r)),
        (Value::Object(l), r @ (Value::String(_) | Value::Number(_) | Value::BigInt(_) | Value::Symbol(_))) => {
            let l_prim = to_primitive(mc, &Value::Object(*l), "default", env)?;
            loose_equal(mc, &l_prim, r, env)
        }
        (l @ (Value::String(_) | Value::Number(_) | Value::BigInt(_) | Value::Symbol(_)), Value::Object(r)) => {
            let r_prim = to_primitive(mc, &Value::Object(*r), "default", env)?;
            loose_equal(mc, l, &r_prim, env)
        }
        _ => Ok(false),
    }
}

fn to_number_with_env<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<f64, EvalError<'gc>> {
    match val {
        Value::Object(_) => {
            let prim = to_primitive(mc, val, "number", env)?;
            to_number_with_env(mc, env, &prim)
        }
        _ => to_number(val),
    }
}

fn to_numeric_with_env<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    let prim = match val {
        Value::Object(_) => to_primitive(mc, val, "number", env)?,
        _ => val.clone(),
    };

    match prim {
        Value::BigInt(_) => Ok(prim),
        _ => Ok(Value::Number(to_number(&prim)?)),
    }
}

fn to_int32_value_with_env<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<i32, EvalError<'gc>> {
    let n = to_number_with_env(mc, env, val)?;
    if n.is_nan() || n == 0.0 || !n.is_finite() {
        return Ok(0);
    }
    let two32 = 4294967296.0_f64;
    let two31 = 2147483648.0_f64;
    let mut int = n.trunc() % two32;
    if int < 0.0 {
        int += two32;
    }
    if int >= two31 {
        int -= two32;
    }
    Ok(int as i32)
}

fn to_uint32_value_with_env<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<u32, EvalError<'gc>> {
    let n = to_number_with_env(mc, env, val)?;
    if n.is_nan() || n == 0.0 || !n.is_finite() {
        return Ok(0);
    }
    let two32 = 4294967296.0_f64;
    let mut int = n.trunc() % two32;
    if int < 0.0 {
        int += two32;
    }
    Ok(int as u32)
}

fn to_string_for_concat<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<String, EvalError<'gc>> {
    let prim = match val {
        Value::Object(_) => crate::core::to_primitive(mc, val, "string", env)?,
        _ => val.clone(),
    };
    if matches!(prim, Value::Symbol(_)) {
        return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
    }
    Ok(value_to_string(&prim))
}

fn maybe_set_function_name_for_default<'gc>(
    mc: &MutationContext<'gc>,
    name: &str,
    default_expr: &Expr,
    assigned_val: &Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    let is_arrow = matches!(default_expr, Expr::ArrowFunction(..) | Expr::AsyncArrowFunction(..));
    let is_anon_fn = matches!(
        default_expr,
        Expr::Function(None, ..)
            | Expr::GeneratorFunction(None, ..)
            | Expr::AsyncFunction(None, ..)
            | Expr::AsyncGeneratorFunction(None, ..)
    );
    let is_anon_class = matches!(default_expr, Expr::Class(class_def) if class_def.name.is_empty());

    if (is_arrow || is_anon_fn || is_anon_class)
        && let Value::Object(obj) = assigned_val
    {
        let mut should_set = false;
        if is_arrow {
            should_set = true;
        } else if let Some(name_rc) = object_get_key_value(obj, "name") {
            let existing_val = match &*name_rc.borrow() {
                Value::Property { value: Some(v), .. } => v.borrow().clone(),
                other => other.clone(),
            };
            let name_str = value_to_string(&existing_val);
            if name_str.is_empty() {
                should_set = true;
            }
        } else {
            should_set = true;
        }

        if should_set {
            let desc = create_descriptor_object(mc, &Value::String(utf8_to_utf16(name)), false, false, true)?;
            crate::js_object::define_property_internal(mc, obj, "name", &desc)?;
        }
    }
    Ok(())
}

fn bigint_shift_count<'gc>(count: &BigInt) -> Result<usize, EvalError<'gc>> {
    if count.sign() == num_bigint::Sign::Minus {
        return Err(raise_eval_error!("invalid bigint shift").into());
    }
    count.to_usize().ok_or_else(|| raise_eval_error!("invalid bigint shift").into())
}

fn collect_names_from_destructuring(pattern: &[DestructuringElement], names: &mut Vec<String>) {
    for element in pattern {
        collect_names_from_destructuring_element(element, names);
    }
}

fn collect_names_from_destructuring_element(element: &DestructuringElement, names: &mut Vec<String>) {
    match element {
        DestructuringElement::Variable(name, _) => names.push(name.clone()),
        DestructuringElement::Property(_, inner) => collect_names_from_destructuring_element(inner, names),
        DestructuringElement::ComputedProperty(_, inner) => collect_names_from_destructuring_element(inner, names),
        DestructuringElement::Rest(name) => names.push(name.clone()),
        DestructuringElement::RestPattern(inner) => collect_names_from_destructuring_element(inner, names),
        DestructuringElement::NestedArray(inner, _) => collect_names_from_destructuring(inner, names),
        DestructuringElement::NestedObject(inner, _) => collect_names_from_destructuring(inner, names),
        DestructuringElement::Empty => {}
    }
}

fn collect_names_from_object_destructuring(pattern: &[ObjectDestructuringElement], names: &mut Vec<String>) {
    for element in pattern {
        match element {
            ObjectDestructuringElement::Property { key: _, value } => collect_names_from_destructuring_element(value, names),
            ObjectDestructuringElement::ComputedProperty { key: _, value } => collect_names_from_destructuring_element(value, names),
            ObjectDestructuringElement::Rest(name) => names.push(name.clone()),
        }
    }
}

// Helper: bind inner object pattern (DestructuringElement::NestedObject) for let/const (block-scoped) bindings
fn bind_object_inner_for_letconst<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    pattern: &[DestructuringElement],
    obj: &JSObjectDataPtr<'gc>,
    is_const: bool,
) -> Result<(), EvalError<'gc>> {
    let mut excluded_keys: Vec<PropertyKey> = Vec::new();
    for inner in pattern.iter() {
        match inner {
            DestructuringElement::Property(key, boxed) => match &**boxed {
                DestructuringElement::Variable(name, default_expr) => {
                    let mut prop_val = get_property_with_accessors(mc, env, obj, key)?;
                    let mut used_default = false;
                    if matches!(prop_val, Value::Undefined)
                        && let Some(def) = default_expr
                    {
                        prop_val = evaluate_expr(mc, env, def)?;
                        used_default = true;
                    }
                    env_set(mc, env, name, &prop_val)?;
                    if is_const {
                        env.borrow_mut(mc).set_const(name.clone());
                    }
                    if used_default {
                        maybe_set_function_name_for_default(mc, name, default_expr.as_deref().unwrap(), &prop_val)?;
                    }
                    excluded_keys.push(PropertyKey::String(key.clone()));
                }
                DestructuringElement::NestedObject(nested, nested_default) => {
                    let mut prop_val = get_property_with_accessors(mc, env, obj, key)?;
                    // If property is undefined and a nested default exists, evaluate it
                    if matches!(prop_val, Value::Undefined)
                        && let Some(def) = nested_default
                    {
                        prop_val = evaluate_expr(mc, env, def)?;
                    }
                    if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                        let prop_name = nested
                            .iter()
                            .find_map(|p| {
                                if let DestructuringElement::Property(k, _) = p {
                                    Some(k.clone())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| "property".to_string());
                        let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                        return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
                    }
                    if let Value::Object(o3) = &prop_val {
                        bind_object_inner_for_letconst(mc, env, nested, o3, is_const)?;
                    } else {
                        return Err(raise_eval_error!("Expected object for nested destructuring").into());
                    }
                    excluded_keys.push(PropertyKey::String(key.clone()));
                }
                DestructuringElement::NestedArray(nested_arr, nested_default) => {
                    let mut prop_val = get_property_with_accessors(mc, env, obj, key)?;
                    if matches!(prop_val, Value::Undefined)
                        && let Some(def) = nested_default
                    {
                        prop_val = evaluate_expr(mc, env, def)?;
                    }
                    if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                        let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                        return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", key, val)).into());
                    }
                    if let Value::Object(oarr) = &prop_val {
                        bind_array_inner_for_letconst(mc, env, nested_arr, oarr, is_const, None, None)?;
                    } else {
                        return Err(raise_eval_error!("Expected array for nested array destructuring").into());
                    }
                    excluded_keys.push(PropertyKey::String(key.clone()));
                }
                _ => {
                    return Err(raise_eval_error!("Nested object destructuring not implemented").into());
                }
            },
            DestructuringElement::ComputedProperty(key_expr, boxed) => {
                // Diagnostic: record which GC env pointer is used to evaluate the computed key
                log::trace!("DBG pre-eval super computed key: env_gc_ptr={:p}", Gc::as_ptr(*env));
                let key_val = evaluate_expr(mc, env, key_expr)?;
                let prop_key = match key_val {
                    Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                    Value::Number(n) => PropertyKey::String(n.to_string()),
                    Value::Symbol(s) => PropertyKey::Symbol(s),
                    _ => PropertyKey::from(value_to_string(&key_val)),
                };
                let prop_name = match &prop_key {
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Symbol(_) => "<symbol>".to_string(),
                    PropertyKey::Private(..) => unreachable!("Computed property cannot be private"),
                };
                match &**boxed {
                    DestructuringElement::Variable(name, default_expr) => {
                        let mut prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                        let mut used_default = false;
                        if matches!(prop_val, Value::Undefined)
                            && let Some(def) = default_expr
                        {
                            prop_val = evaluate_expr(mc, env, def)?;
                            used_default = true;
                        }
                        env_set(mc, env, name, &prop_val)?;
                        if is_const {
                            env.borrow_mut(mc).set_const(name.clone());
                        }
                        if used_default {
                            maybe_set_function_name_for_default(mc, name, default_expr.as_deref().unwrap(), &prop_val)?;
                        }
                    }
                    DestructuringElement::NestedObject(nested, nested_default) => {
                        let mut prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                        if matches!(prop_val, Value::Undefined)
                            && let Some(def) = nested_default
                        {
                            prop_val = evaluate_expr(mc, env, def)?;
                        }
                        if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                            let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                            return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
                        }
                        if let Value::Object(o3) = &prop_val {
                            bind_object_inner_for_letconst(mc, env, nested, o3, is_const)?;
                        } else {
                            return Err(raise_eval_error!("Expected object for nested destructuring").into());
                        }
                    }
                    DestructuringElement::NestedArray(nested_arr, nested_default) => {
                        let mut prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                        if matches!(prop_val, Value::Undefined)
                            && let Some(def) = nested_default
                        {
                            prop_val = evaluate_expr(mc, env, def)?;
                        }
                        if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                            let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                            return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
                        }
                        if let Value::Object(oarr) = &prop_val {
                            bind_array_inner_for_letconst(mc, env, nested_arr, oarr, is_const, None, None)?;
                        } else {
                            return Err(raise_eval_error!("Expected array for nested array destructuring").into());
                        }
                    }
                    _ => {
                        return Err(raise_eval_error!("Nested object destructuring not implemented").into());
                    }
                }
                excluded_keys.push(prop_key);
            }
            DestructuringElement::Rest(name) => {
                let rest_obj = new_js_object_data(mc);
                let ordered = crate::core::ordinary_own_property_keys_mc(mc, obj)?;
                for k in ordered {
                    if excluded_keys.iter().any(|ex| ex == &k) {
                        continue;
                    }
                    // If this object is a proxy wrapper, delegate descriptor/get to proxy traps
                    if let Some(proxy_cell) = obj.borrow().properties.get(&PropertyKey::String("__proxy__".to_string()))
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        println!(
                            "TRACE: bind_object_inner_for_letconst Rest: obj_ptr={:p} proxy_ptr={:p} key={:?}",
                            obj.as_ptr(),
                            Gc::as_ptr(*proxy),
                            k
                        );
                        // Ask proxy for own property descriptor and check [[Enumerable]]
                        let desc_enum_opt = crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &k)?;
                        if desc_enum_opt.is_none() {
                            continue;
                        }
                        if !desc_enum_opt.unwrap() {
                            continue;
                        }
                        // Get property value via proxy get trap
                        let val_opt = crate::js_proxy::proxy_get_property(mc, proxy, &k)?;
                        let v = val_opt.unwrap_or(Value::Undefined);
                        object_set_key_value(mc, &rest_obj, k.clone(), &v)?;
                        continue;
                    }
                    if !obj.borrow().is_enumerable(&k) {
                        continue;
                    }
                    let v = get_property_with_accessors(mc, env, obj, &k)?;
                    object_set_key_value(mc, &rest_obj, k.clone(), &v)?;
                }
                env_set(mc, env, name, &Value::Object(rest_obj))?;
                if is_const {
                    env.borrow_mut(mc).set_const(name.clone());
                }
            }
            _ => {
                return Err(raise_eval_error!("Nested object destructuring not implemented").into());
            }
        }
    }
    Ok(())
}

// Helper: bind inner object pattern for var (function-scoped) bindings
fn bind_object_inner_for_var<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    pattern: &[DestructuringElement],
    obj: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    // Determine function-scope env
    let mut target_env = *env;
    while !target_env.borrow().is_function_scope {
        if let Some(proto) = target_env.borrow().prototype {
            target_env = proto;
        } else {
            break;
        }
    }

    let mut excluded_keys: Vec<PropertyKey> = Vec::new();

    for inner in pattern.iter() {
        match inner {
            DestructuringElement::Property(key, boxed) => match &**boxed {
                DestructuringElement::Variable(name, default_expr) => {
                    let mut prop_val = get_property_with_accessors(mc, env, obj, key)?;
                    let mut used_default = false;
                    if matches!(prop_val, Value::Undefined)
                        && let Some(def) = default_expr
                    {
                        prop_val = evaluate_expr(mc, env, def)?;
                        used_default = true;
                    }
                    env_set_recursive(mc, &target_env, name, &prop_val)?;
                    if used_default {
                        maybe_set_function_name_for_default(mc, name, default_expr.as_deref().unwrap(), &prop_val)?;
                    }
                    excluded_keys.push(PropertyKey::String(key.clone()));
                }
                DestructuringElement::NestedObject(nested, nested_default) => {
                    let mut prop_val = get_property_with_accessors(mc, env, obj, key)?;
                    if matches!(prop_val, Value::Undefined)
                        && let Some(def) = nested_default
                    {
                        prop_val = evaluate_expr(mc, env, def)?;
                    }
                    if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                        let prop_name = nested
                            .iter()
                            .find_map(|p| {
                                if let DestructuringElement::Property(k, _) = p {
                                    Some(k.clone())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| "property".to_string());
                        let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                        return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
                    }
                    if let Value::Object(o3) = &prop_val {
                        bind_object_inner_for_var(mc, env, nested, o3)?;
                    } else {
                        return Err(raise_eval_error!("Expected object for nested destructuring").into());
                    }
                    excluded_keys.push(PropertyKey::String(key.clone()));
                }
                DestructuringElement::NestedArray(nested_arr, nested_default) => {
                    let mut prop_val = get_property_with_accessors(mc, env, obj, key)?;
                    if matches!(prop_val, Value::Undefined)
                        && let Some(def) = nested_default
                    {
                        prop_val = evaluate_expr(mc, env, def)?;
                    }
                    if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                        let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                        return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", key, val)).into());
                    }
                    if let Value::Object(oarr) = &prop_val {
                        bind_array_inner_for_var(mc, env, nested_arr, oarr, None, None)?;
                    } else {
                        return Err(raise_eval_error!("Expected array for nested array destructuring").into());
                    }
                    excluded_keys.push(PropertyKey::String(key.clone()));
                }
                _ => {
                    return Err(raise_eval_error!("Nested object destructuring not implemented").into());
                }
            },
            DestructuringElement::ComputedProperty(key_expr, boxed) => {
                let key_val = evaluate_expr(mc, env, key_expr)?;
                let prop_key = match key_val {
                    Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                    Value::Number(n) => PropertyKey::String(n.to_string()),
                    Value::Symbol(s) => PropertyKey::Symbol(s),
                    _ => PropertyKey::from(value_to_string(&key_val)),
                };
                let prop_name = match &prop_key {
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Symbol(_) => "<symbol>".to_string(),
                    PropertyKey::Private(..) => unreachable!("Computed property cannot be private"),
                };
                match &**boxed {
                    DestructuringElement::Variable(name, default_expr) => {
                        let mut prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                        let mut used_default = false;
                        if matches!(prop_val, Value::Undefined)
                            && let Some(def) = default_expr
                        {
                            prop_val = evaluate_expr(mc, env, def)?;
                            used_default = true;
                        }
                        env_set_recursive(mc, &target_env, name, &prop_val)?;
                        if used_default {
                            maybe_set_function_name_for_default(mc, name, default_expr.as_deref().unwrap(), &prop_val)?;
                        }
                    }
                    DestructuringElement::NestedObject(nested, nested_default) => {
                        let mut prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                        if matches!(prop_val, Value::Undefined)
                            && let Some(def) = nested_default
                        {
                            prop_val = evaluate_expr(mc, env, def)?;
                        }
                        if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                            let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                            return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
                        }
                        if let Value::Object(o3) = &prop_val {
                            bind_object_inner_for_var(mc, env, nested, o3)?;
                        } else {
                            return Err(raise_eval_error!("Expected object for nested destructuring").into());
                        }
                    }
                    DestructuringElement::NestedArray(nested_arr, nested_default) => {
                        let mut prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                        if matches!(prop_val, Value::Undefined)
                            && let Some(def) = nested_default
                        {
                            prop_val = evaluate_expr(mc, env, def)?;
                        }
                        if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                            let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                            return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
                        }
                        if let Value::Object(oarr) = &prop_val {
                            bind_array_inner_for_var(mc, env, nested_arr, oarr, None, None)?;
                        } else {
                            return Err(raise_eval_error!("Expected array for nested array destructuring").into());
                        }
                    }
                    _ => {
                        return Err(raise_eval_error!("Nested object destructuring not implemented").into());
                    }
                }
                excluded_keys.push(prop_key);
            }
            DestructuringElement::Rest(name) => {
                let rest_obj = new_js_object_data(mc);
                // Diagnostic: log at start of Rest binding to verify proxy detection
                let obj_ptr = obj.as_ptr();
                let has_proxy = obj.borrow().properties.get(&PropertyKey::String("__proxy__".to_string())).is_some();
                println!(
                    "TRACE: bind_object_inner_for_var Rest: obj_ptr={:p} has_proxy={}",
                    obj_ptr, has_proxy
                );
                let ordered = crate::core::ordinary_own_property_keys_mc(mc, obj)?;
                for k in ordered {
                    if excluded_keys.iter().any(|ex| ex == &k) {
                        continue;
                    }
                    // If this object is a proxy wrapper, delegate descriptor/get to proxy traps
                    if let Some(proxy_cell) = obj.borrow().properties.get(&PropertyKey::String("__proxy__".to_string()))
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        println!(
                            "TRACE: bind_object_inner_for_var Rest: proxy detected obj_ptr={:p} proxy_ptr={:p} key={:?}",
                            obj.as_ptr(),
                            Gc::as_ptr(*proxy),
                            k
                        );
                        // Ask proxy for own property descriptor and check [[Enumerable]]
                        let desc_enum_opt = crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &k)?;
                        if desc_enum_opt.is_none() {
                            continue;
                        }
                        if !desc_enum_opt.unwrap() {
                            continue;
                        }
                        // Get property value via proxy get trap
                        let val_opt = crate::js_proxy::proxy_get_property(mc, proxy, &k)?;
                        let v = val_opt.unwrap_or(Value::Undefined);
                        object_set_key_value(mc, &rest_obj, k.clone(), &v)?;
                        continue;
                    }
                    if !obj.borrow().is_enumerable(&k) {
                        continue;
                    }
                    let v = get_property_with_accessors(mc, env, obj, &k)?;
                    object_set_key_value(mc, &rest_obj, k.clone(), &v)?;
                }
                env_set_recursive(mc, &target_env, name, &Value::Object(rest_obj))?;
            }
            _ => {
                return Err(raise_eval_error!("Nested object destructuring not implemented").into());
            }
        }
    }
    Ok(())
}

// Helper: bind inner array pattern for let/const bindings
fn bind_array_inner_for_letconst<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    pattern: &[DestructuringElement],
    arr_obj: &JSObjectDataPtr<'gc>,
    is_const: bool,
    stmt_line: Option<usize>,
    stmt_column: Option<usize>,
) -> Result<(), EvalError<'gc>> {
    let _current_len = object_get_length(arr_obj).unwrap_or(0);
    // For fixed-length TypedArrays, ensure the entire view fits in the buffer
    // before trying to access elements for nested destructuring.
    if is_typedarray(arr_obj) {
        ensure_typedarray_in_bounds(mc, env, stmt_line, stmt_column, arr_obj)?;
    }
    for (j, inner_elem) in pattern.iter().enumerate() {
        match inner_elem {
            DestructuringElement::Variable(name, default_expr) => {
                let mut elem_val = get_array_like_element(mc, env, arr_obj, j)?;
                if matches!(elem_val, Value::Undefined)
                    && let Some(def) = default_expr
                {
                    elem_val = evaluate_expr(mc, env, def)?;
                }
                env_set(mc, env, name, &elem_val)?;
                if is_const {
                    env.borrow_mut(mc).set_const(name.clone());
                }
            }
            DestructuringElement::NestedObject(inner_pattern, inner_default) => {
                // get element at index j
                let mut elem_val = get_array_like_element(mc, env, arr_obj, j)?;
                if matches!(elem_val, Value::Undefined)
                    && let Some(def) = inner_default
                {
                    elem_val = evaluate_expr(mc, env, def)?;
                }
                if matches!(elem_val, Value::Undefined) || matches!(elem_val, Value::Null) {
                    let prop_name = inner_pattern
                        .iter()
                        .find_map(|p| {
                            if let DestructuringElement::Property(k, _) = p {
                                Some(k.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "property".to_string());
                    let val = if matches!(elem_val, Value::Null) { "null" } else { "undefined" };
                    return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
                }
                if let Value::Object(obj2) = &elem_val {
                    bind_object_inner_for_letconst(mc, env, inner_pattern, obj2, is_const)?;
                } else {
                    return Err(raise_eval_error!("Expected object for nested destructuring").into());
                }
            }
            DestructuringElement::NestedArray(inner_array, inner_default) => {
                let mut elem_val = Value::Undefined;
                if let Some(cell) = object_get_key_value(arr_obj, j.to_string()) {
                    elem_val = cell.borrow().clone();
                }
                if matches!(elem_val, Value::Undefined)
                    && let Some(def) = inner_default
                {
                    elem_val = evaluate_expr(mc, env, def)?;
                }
                if matches!(elem_val, Value::Undefined) || matches!(elem_val, Value::Null) {
                    return Err(raise_type_error!("Cannot destructure array from undefined/null").into());
                }
                if let Value::Object(oarr) = &elem_val {
                    bind_array_inner_for_letconst(mc, env, inner_array, oarr, is_const, None, None)?;
                } else {
                    return Err(raise_eval_error!("Expected array for nested array destructuring").into());
                }
            }
            DestructuringElement::Rest(name) => {
                // Collect remaining elements into an array
                let arr_obj2 = crate::js_array::create_array(mc, env)?;
                // get length
                let len = if let Some(len_cell) = object_get_key_value(arr_obj, "length") {
                    if let Value::Number(n) = len_cell.borrow().clone() {
                        n as usize
                    } else {
                        0
                    }
                } else {
                    0
                };
                let mut idx2 = 0_usize;
                for jj in j..len {
                    let val = get_array_like_element(mc, env, arr_obj, jj)?;
                    if !matches!(val, Value::Undefined) {
                        object_set_key_value(mc, &arr_obj2, idx2, &val)?;
                        idx2 += 1;
                    }
                }
                object_set_key_value(mc, &arr_obj2, "length", &Value::Number(idx2 as f64))?;
                env_set(mc, env, name, &Value::Object(arr_obj2))?;
                if is_const {
                    env.borrow_mut(mc).set_const(name.clone());
                }
                break;
            }
            DestructuringElement::RestPattern(inner) => {
                // Collect remaining elements into an array
                let arr_obj2 = crate::js_array::create_array(mc, env)?;
                let len = if let Some(len_cell) = object_get_key_value(arr_obj, "length") {
                    if let Value::Number(n) = len_cell.borrow().clone() {
                        n as usize
                    } else {
                        0
                    }
                } else {
                    0
                };
                let mut idx2 = 0_usize;
                for jj in j..len {
                    let val = get_array_like_element(mc, env, arr_obj, jj)?;
                    if !matches!(val, Value::Undefined) {
                        object_set_key_value(mc, &arr_obj2, idx2, &val)?;
                        idx2 += 1;
                    }
                }
                object_set_key_value(mc, &arr_obj2, "length", &Value::Number(idx2 as f64))?;

                match &**inner {
                    DestructuringElement::Variable(name, _) => {
                        env_set(mc, env, name, &Value::Object(arr_obj2))?;
                        if is_const {
                            env.borrow_mut(mc).set_const(name.clone());
                        }
                    }
                    DestructuringElement::NestedArray(inner_array, _) => {
                        bind_array_inner_for_letconst(mc, env, inner_array, &arr_obj2, is_const, None, None)?;
                    }
                    DestructuringElement::NestedObject(inner_obj, _) => {
                        bind_object_inner_for_letconst(mc, env, inner_obj, &arr_obj2, is_const)?;
                    }
                    _ => return Err(raise_syntax_error!("Invalid rest binding pattern").into()),
                }
                break;
            }
            DestructuringElement::Empty => {}
            _ => {
                return Err(raise_syntax_error!("Nested array destructuring not implemented").into());
            }
        }
    }
    Ok(())
}

// Helper: bind inner array pattern (DestructuringElement::NestedArray) for let/const (block-scoped) bindings
fn bind_array_inner_for_var<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    pattern: &[DestructuringElement],
    arr_obj: &JSObjectDataPtr<'gc>,
    stmt_line: Option<usize>,
    stmt_column: Option<usize>,
) -> Result<(), EvalError<'gc>> {
    // Determine function-scope env
    let mut target_env = *env;
    while !target_env.borrow().is_function_scope {
        if let Some(proto) = target_env.borrow().prototype {
            target_env = proto;
        } else {
            break;
        }
    }

    let _current_len = object_get_length(arr_obj).unwrap_or(0);
    // For fixed-length TypedArrays, ensure the entire view fits in the buffer
    // before trying to access elements for nested destructuring.
    if is_typedarray(arr_obj) {
        ensure_typedarray_in_bounds(mc, env, stmt_line, stmt_column, arr_obj)?;
    }
    for (j, inner_elem) in pattern.iter().enumerate() {
        match inner_elem {
            DestructuringElement::Variable(name, default_expr) => {
                let mut elem_val = get_array_like_element(mc, env, arr_obj, j)?;
                if matches!(elem_val, Value::Undefined)
                    && let Some(def) = default_expr
                {
                    elem_val = evaluate_expr(mc, env, def)?;
                }
                env_set_recursive(mc, &target_env, name, &elem_val)?;
            }
            DestructuringElement::NestedObject(inner_pattern, inner_default) => {
                // get element at index j
                let mut elem_val = get_array_like_element(mc, env, arr_obj, j)?;
                if matches!(elem_val, Value::Undefined)
                    && let Some(def) = inner_default
                {
                    elem_val = evaluate_expr(mc, env, def)?;
                }
                if matches!(elem_val, Value::Undefined) || matches!(elem_val, Value::Null) {
                    let prop_name = inner_pattern
                        .iter()
                        .find_map(|p| {
                            if let DestructuringElement::Property(k, _) = p {
                                Some(k.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "property".to_string());
                    let val = if matches!(elem_val, Value::Null) { "null" } else { "undefined" };
                    return Err(raise_type_error!(format!("Cannot destructure property '{prop_name}' of {val}")).into());
                }
                if let Value::Object(obj2) = &elem_val {
                    // bind inner properties as var (function scope)
                    for inner in inner_pattern.iter() {
                        match inner {
                            DestructuringElement::Property(key, boxed) => {
                                match &**boxed {
                                    DestructuringElement::Variable(name, default_expr) => {
                                        let mut prop_val = Value::Undefined;
                                        if let Some(cell) = object_get_key_value(obj2, key) {
                                            prop_val = cell.borrow().clone();
                                        }
                                        if matches!(prop_val, Value::Undefined)
                                            && let Some(def) = default_expr
                                        {
                                            prop_val = evaluate_expr(mc, env, def)?;
                                        }
                                        env_set_recursive(mc, &target_env, name, &prop_val)?;
                                    }
                                    DestructuringElement::NestedObject(nested, nested_default) => {
                                        let mut prop_val = Value::Undefined;
                                        if let Some(cell) = object_get_key_value(obj2, key) {
                                            prop_val = cell.borrow().clone();
                                        }
                                        if matches!(prop_val, Value::Undefined)
                                            && let Some(def) = nested_default
                                        {
                                            prop_val = evaluate_expr(mc, env, def)?;
                                        }
                                        if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                            let prop = nested
                                                .iter()
                                                .find_map(|p| {
                                                    if let DestructuringElement::Property(k, _) = p {
                                                        Some(k.clone())
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or_else(|| "property".to_string());
                                            let v = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                                            return Err(raise_type_error!(format!("Cannot destructure property '{prop}' of {v}")).into());
                                        }
                                        if let Value::Object(o3) = &prop_val {
                                            // recursively bind as var
                                            bind_object_inner_for_var(mc, env, nested, o3)?;
                                        } else {
                                            return Err(raise_eval_error!("Expected object for nested destructuring").into());
                                        }
                                    }
                                    DestructuringElement::NestedArray(nested_arr, nested_default) => {
                                        let mut prop_val = Value::Undefined;
                                        if let Some(cell) = object_get_key_value(obj2, key) {
                                            prop_val = cell.borrow().clone();
                                        }
                                        if matches!(prop_val, Value::Undefined)
                                            && let Some(def) = nested_default
                                        {
                                            prop_val = evaluate_expr(mc, env, def)?;
                                        }
                                        if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                            let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                                            return Err(raise_type_error!(format!("Cannot destructure property '{key}' of {val}",)).into());
                                        }
                                        if let Value::Object(oarr) = &prop_val {
                                            bind_array_inner_for_var(mc, env, nested_arr, oarr, None, None)?;
                                        } else {
                                            return Err(raise_eval_error!("Expected array for nested array destructuring").into());
                                        }
                                    }
                                    _ => {
                                        return Err(crate::raise_syntax_error!("Nested object destructuring not implemented").into());
                                    }
                                }
                            }
                            _ => {
                                return Err(raise_eval_error!("Nested object destructuring not implemented").into());
                            }
                        }
                    }
                } else {
                    return Err(raise_eval_error!("Expected object for nested destructuring").into());
                }
            }
            DestructuringElement::NestedArray(inner_array, inner_default) => {
                let mut elem_val = Value::Undefined;
                if let Some(cell) = object_get_key_value(arr_obj, j.to_string()) {
                    elem_val = cell.borrow().clone();
                }
                if matches!(elem_val, Value::Undefined)
                    && let Some(def) = inner_default
                {
                    elem_val = evaluate_expr(mc, env, def)?;
                }
                if matches!(elem_val, Value::Undefined) || matches!(elem_val, Value::Null) {
                    return Err(raise_type_error!("Cannot destructure array from undefined/null").into());
                }
                if let Value::Object(oarr) = &elem_val {
                    bind_array_inner_for_var(mc, env, inner_array, oarr, None, None)?;
                } else {
                    return Err(raise_eval_error!("Expected array for nested array destructuring").into());
                }
            }
            DestructuringElement::Rest(name) => {
                // Collect remaining elements into an array
                let arr_obj2 = crate::js_array::create_array(mc, env)?;
                // get length
                let len = if let Some(len_cell) = object_get_key_value(arr_obj, "length") {
                    if let Value::Number(n) = len_cell.borrow().clone() {
                        n as usize
                    } else {
                        0
                    }
                } else {
                    0
                };
                let mut idx2 = 0_usize;
                for jj in j..len {
                    let val = get_array_like_element(mc, env, arr_obj, jj)?;
                    if !matches!(val, Value::Undefined) {
                        object_set_key_value(mc, &arr_obj2, idx2, &val)?;
                        idx2 += 1;
                    }
                }
                object_set_key_value(mc, &arr_obj2, "length", &Value::Number(idx2 as f64))?;
                // bind var in function scope
                env_set_recursive(mc, &target_env, name, &Value::Object(arr_obj2))?;
            }
            DestructuringElement::RestPattern(inner) => {
                // Collect remaining elements into an array
                let arr_obj2 = crate::js_array::create_array(mc, env)?;
                let len = if let Some(len_cell) = object_get_key_value(arr_obj, "length") {
                    if let Value::Number(n) = len_cell.borrow().clone() {
                        n as usize
                    } else {
                        0
                    }
                } else {
                    0
                };
                let mut idx2 = 0_usize;
                for jj in j..len {
                    let val = get_array_like_element(mc, env, arr_obj, jj)?;
                    if !matches!(val, Value::Undefined) {
                        object_set_key_value(mc, &arr_obj2, idx2, &val)?;
                        idx2 += 1;
                    }
                }
                object_set_key_value(mc, &arr_obj2, "length", &Value::Number(idx2 as f64))?;

                match &**inner {
                    DestructuringElement::Variable(name, _) => {
                        env_set_recursive(mc, &target_env, name, &Value::Object(arr_obj2))?;
                    }
                    DestructuringElement::NestedArray(inner_array, _) => {
                        bind_array_inner_for_var(mc, env, inner_array, &arr_obj2, None, None)?;
                    }
                    DestructuringElement::NestedObject(inner_obj, _) => {
                        bind_object_inner_for_var(mc, env, inner_obj, &arr_obj2)?;
                    }
                    _ => return Err(raise_syntax_error!("Invalid rest binding pattern").into()),
                }
            }
            DestructuringElement::Empty => {}
            _ => {
                return Err(raise_syntax_error!("Nested array destructuring not implemented").into());
            }
        }
    }
    Ok(())
}

fn hoist_name<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    name: &str,
    is_indirect_eval: bool,
) -> Result<(), EvalError<'gc>> {
    let mut target_env = *env;
    log::trace!("hoist_name: called for '{}' with env {:p}", name, env);
    while !target_env.borrow().is_function_scope {
        if let Some(proto) = target_env.borrow().prototype {
            target_env = proto;
        } else {
            break;
        }
    }
    if env_get_own(&target_env, name).is_none() {
        // If target is global object, use CreateGlobalVarBinding semantics
        // For indirect eval, CreateGlobalVarBinding(..., true) should be used which creates
        // a configurable=true property. Detect indirect eval marker on the global env.
        if target_env.borrow().prototype.is_none() {
            // If creating a global var binding during an indirect eval, the spec
            // requires we detect conflicts with existing global lexical
            // declarations and throw a SyntaxError. Check for that here before
            // creating the property on the global env.
            log::trace!(
                "hoist_name: creating global var '{}' - checking for existing global lexical/TDZ on env {:p}",
                name,
                &target_env
            );
            log::trace!(
                "hoist_name: creating global var - target_env ptr={:p} extensible={}",
                &target_env,
                target_env.borrow().is_extensible()
            );
            if target_env.borrow().has_lexical(name) {
                log::trace!("hoist_name: conflict - global env {:p} already has lexical '{}'", &target_env, name);
                return Err(raise_syntax_error!("Variable declaration would conflict with existing lexical declaration").into());
            }
            if let Some(existing_val_ptr) = env_get_own(&target_env, name)
                && let Value::Uninitialized = &*existing_val_ptr.borrow()
            {
                log::trace!(
                    "hoist_name: conflict - global env {:p} already has Uninitialized binding '{}'",
                    &target_env,
                    name
                );
                return Err(raise_syntax_error!("Variable declaration would conflict with existing lexical declaration").into());
            }

            // If the global object is non-extensible, CanDeclareGlobalVar should be false
            // and a TypeError must be thrown per spec.
            if !target_env.borrow().is_extensible() {
                log::trace!(
                    "hoist_name: global_env {:p} is non-extensible -> Cannot declare global var '{}'",
                    &target_env,
                    name
                );
                return Err(raise_type_error!("Cannot add property to non-extensible object").into());
            }

            // Use the provided is_indirect_eval flag to determine deletability
            let deletable = is_indirect_eval;
            let desc_obj = create_descriptor_object(mc, &Value::Undefined, true, true, deletable)?;
            crate::js_object::define_property_internal(mc, &target_env, name, &desc_obj)?;
        } else {
            env_set(mc, &target_env, name, &Value::Undefined)?;
        }
    } else {
        // If the binding already exists in a non-global function scope, we do
        // not need to perform global lexical conflict checks. Var declarations
        // inside functions may share names with global lexicals without error.
        if target_env.borrow().prototype.is_some() {
            return Ok(());
        }
        // If there's already an own binding, then detect the special case where an
        // existing lexical declaration (TDZ / Uninitialized) blocks creation of a
        // global var binding for indirect evals in non-strict mode. The ECMAScript
        // semantics require a SyntaxError in that situation.
        // Compute the topmost global environment (prototype == None).
        let mut top_env = Some(*env);
        let mut global_env = None;
        while let Some(e) = top_env {
            if e.borrow().prototype.is_none() {
                global_env = Some(e);
                break;
            }
            top_env = e.borrow().prototype;
        }

        if let Some(g_env) = global_env {
            // Use the provided is_indirect_eval flag to decide whether to apply
            // the indirect-eval lexical conflict checks. When true, an indirect
            // non-strict eval attempting to create a var binding must throw a
            // SyntaxError if an existing global lexical or TDZ binding exists.
            if is_indirect_eval {
                log::trace!(
                    "hoist_name: checking lexical conflict for '{}' - has_lexical={} own_exists={} is_indirect_eval={}",
                    name,
                    g_env.borrow().has_lexical(name),
                    env_get_own(&g_env, name).is_some(),
                    is_indirect_eval
                );
                if g_env.borrow().has_lexical(name) {
                    log::trace!("hoist_name: existing own lexical declaration '{}' on global env", name);
                    return Err(raise_syntax_error!("Variable declaration would conflict with existing lexical declaration").into());
                }

                if let Some(existing_val_ptr) = env_get_own(&g_env, name) {
                    if let Value::Uninitialized = &*existing_val_ptr.borrow() {
                        log::trace!("hoist_name: existing own binding '{}' is Uninitialized on global env", name);
                        return Err(raise_syntax_error!("Variable declaration would conflict with existing lexical declaration").into());
                    } else {
                        log::trace!("hoist_name: existing own binding '{}' exists but is not Uninitialized", name);
                    }
                }
            } else {
                if g_env.borrow().has_lexical(name) {
                    log::trace!("hoist_name: existing own lexical declaration '{}' on global env", name);
                    return Err(raise_syntax_error!("Variable declaration would conflict with existing lexical declaration").into());
                }
                if let Some(existing_val_ptr) = env_get_own(&g_env, name)
                    && let Value::Uninitialized = &*existing_val_ptr.borrow()
                {
                    log::trace!(
                        "hoist_name: conflict - global env {:p} already has Uninitialized binding '{}'",
                        &g_env,
                        name
                    );
                    return Err(raise_syntax_error!("Variable declaration would conflict with existing lexical declaration").into());
                }
            }
        }
    }
    Ok(())
}

fn hoist_var_declarations<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
    is_indirect_eval: bool,
) -> Result<(), EvalError<'gc>> {
    log::trace!(
        "hoist_var_declarations: called with env {:p} (is_function_scope={})",
        env,
        env.borrow().is_function_scope
    );
    for stmt in statements {
        match &*stmt.kind {
            StatementKind::Var(decls) => {
                for (name, _) in decls {
                    hoist_name(mc, env, name, is_indirect_eval)?;
                }
            }
            StatementKind::VarDestructuringArray(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_destructuring(pattern, &mut names);
                for name in names {
                    hoist_name(mc, env, &name, is_indirect_eval)?;
                }
            }
            StatementKind::VarDestructuringObject(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_object_destructuring(pattern, &mut names);
                for name in names {
                    hoist_name(mc, env, &name, is_indirect_eval)?;
                }
            }
            StatementKind::Block(stmts) => hoist_var_declarations(mc, env, stmts, is_indirect_eval)?,
            StatementKind::If(if_stmt) => {
                let if_stmt = if_stmt.as_ref();
                hoist_var_declarations(mc, env, &if_stmt.then_body, is_indirect_eval)?;
                if let Some(else_stmts) = &if_stmt.else_body {
                    hoist_var_declarations(mc, env, else_stmts, is_indirect_eval)?;
                }
            }
            StatementKind::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    hoist_var_declarations(mc, env, std::slice::from_ref(init), is_indirect_eval)?;
                }
                hoist_var_declarations(mc, env, &for_stmt.body, is_indirect_eval)?;
            }
            StatementKind::ForIn(decl, name, _, body) => {
                if let Some(crate::core::VarDeclKind::Var) = decl {
                    hoist_name(mc, env, name, is_indirect_eval)?;
                }
                hoist_var_declarations(mc, env, body, is_indirect_eval)?;
            }
            StatementKind::ForOf(decl, name, _, body) => {
                if let Some(crate::core::VarDeclKind::Var) = decl {
                    hoist_name(mc, env, name, is_indirect_eval)?;
                }
                hoist_var_declarations(mc, env, body, is_indirect_eval)?;
            }
            StatementKind::ForAwaitOf(decl, name, _, body) => {
                if let Some(crate::core::VarDeclKind::Var) = decl {
                    hoist_name(mc, env, name, is_indirect_eval)?;
                }
                hoist_var_declarations(mc, env, body, is_indirect_eval)?;
            }
            StatementKind::ForOfDestructuringObject(decl, pattern, _, body) => {
                if let Some(crate::core::VarDeclKind::Var) = decl {
                    let mut names = Vec::new();
                    collect_names_from_object_destructuring(pattern, &mut names);
                    // Use passed is_indirect_eval
                    for name in names {
                        hoist_name(mc, env, &name, is_indirect_eval)?;
                    }
                }
                hoist_var_declarations(mc, env, body, is_indirect_eval)?;
            }
            StatementKind::ForOfDestructuringArray(decl, pattern, _, body) => {
                if let Some(crate::core::VarDeclKind::Var) = decl {
                    let mut names = Vec::new();
                    collect_names_from_destructuring(pattern, &mut names);
                    // Use passed is_indirect_eval
                    for name in names {
                        hoist_name(mc, env, &name, is_indirect_eval)?;
                    }
                }
                hoist_var_declarations(mc, env, body, is_indirect_eval)?;
            }
            StatementKind::ForAwaitOfDestructuringObject(decl, pattern, _, body) => {
                if let Some(crate::core::VarDeclKind::Var) = decl {
                    let mut names = Vec::new();
                    collect_names_from_object_destructuring(pattern, &mut names);
                    for name in names {
                        hoist_name(mc, env, &name, is_indirect_eval)?;
                    }
                }
                hoist_var_declarations(mc, env, body, is_indirect_eval)?;
            }
            StatementKind::ForAwaitOfDestructuringArray(decl, pattern, _, body) => {
                if let Some(crate::core::VarDeclKind::Var) = decl {
                    let mut names = Vec::new();
                    collect_names_from_destructuring(pattern, &mut names);
                    for name in names {
                        hoist_name(mc, env, &name, is_indirect_eval)?;
                    }
                }
                hoist_var_declarations(mc, env, body, is_indirect_eval)?;
            }
            StatementKind::ForInDestructuringObject(decl, pattern, _, body) => {
                if let Some(crate::core::VarDeclKind::Var) = decl {
                    let mut names = Vec::new();
                    collect_names_from_object_destructuring(pattern, &mut names);
                    // Use passed is_indirect_eval
                    for name in names {
                        hoist_name(mc, env, &name, is_indirect_eval)?;
                    }
                }
                hoist_var_declarations(mc, env, body, is_indirect_eval)?;
            }
            StatementKind::ForInDestructuringArray(decl, pattern, _, body) => {
                if let Some(crate::core::VarDeclKind::Var) = decl {
                    let mut names = Vec::new();
                    collect_names_from_destructuring(pattern, &mut names);
                    // Use passed is_indirect_eval
                    for name in names {
                        hoist_name(mc, env, &name, is_indirect_eval)?;
                    }
                }
                hoist_var_declarations(mc, env, body, is_indirect_eval)?;
            }
            StatementKind::While(_, body) => hoist_var_declarations(mc, env, body, is_indirect_eval)?,
            StatementKind::DoWhile(body, _) => hoist_var_declarations(mc, env, body, is_indirect_eval)?,
            StatementKind::TryCatch(tc_stmt) => {
                let tc_stmt = tc_stmt.as_ref();
                hoist_var_declarations(mc, env, &tc_stmt.try_body, is_indirect_eval)?;
                if let Some(catch_stmts) = &tc_stmt.catch_body {
                    hoist_var_declarations(mc, env, catch_stmts, is_indirect_eval)?;
                }
                if let Some(finally_stmts) = &tc_stmt.finally_body {
                    hoist_var_declarations(mc, env, finally_stmts, is_indirect_eval)?;
                }
            }
            StatementKind::Switch(sw_stmt) => {
                for case in &sw_stmt.cases {
                    match case {
                        crate::core::SwitchCase::Case(_, stmts) => hoist_var_declarations(mc, env, stmts, is_indirect_eval)?,
                        crate::core::SwitchCase::Default(stmts) => hoist_var_declarations(mc, env, stmts, is_indirect_eval)?,
                    }
                }
            }
            StatementKind::Label(_, stmt) => {
                // Label contains a single statement, but it might be a block or loop
                // We need to wrap it in a slice to recurse
                hoist_var_declarations(mc, env, std::slice::from_ref(stmt), is_indirect_eval)?;
            }
            StatementKind::Export(_, Some(decl), _) => {
                hoist_var_declarations(mc, env, std::slice::from_ref(decl), is_indirect_eval)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn hoist_declarations<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
    skip_lexicals: bool,
    is_indirect_eval: bool,
) -> Result<(), EvalError<'gc>> {
    // 1. Hoist FunctionDeclarations (only top-level in this list of statements)
    for stmt in statements {
        if let StatementKind::FunctionDeclaration(name, params, body, is_generator, is_async) = &*stmt.kind {
            let mut body_clone = body.clone();
            log::trace!(
                "hoist_declarations: found function declaration '{}'; exec env proto is_none={}",
                name,
                env.borrow().prototype.is_none()
            );
            if *is_generator {
                // Create a generator (or async generator) function object (hoisted)
                let func_obj = crate::core::new_js_object_data(mc);
                // Set __proto__ to Function.prototype
                if let Some(func_ctor_val) = env_get(env, "Function")
                    && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                    && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                    && let Value::Object(proto) = &*proto_val.borrow()
                {
                    func_obj.borrow_mut(mc).prototype = Some(*proto);
                }

                let is_strict = body.first()
                    .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
                    .unwrap_or(false);
                let closure_data = ClosureData {
                    params: params.clone(),
                    body: body.clone(),
                    env: Some(*env),
                    is_strict,
                    enforce_strictness_inheritance: true,
                    ..ClosureData::default()
                };
                // Use AsyncGeneratorFunction for async generator declarations
                let closure_val = if *is_async {
                    Value::AsyncGeneratorFunction(Some(name.clone()), Gc::new(mc, closure_data))
                } else {
                    Value::GeneratorFunction(Some(name.clone()), Gc::new(mc, closure_data))
                };
                func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
                object_set_key_value(mc, &func_obj, "name", &Value::String(utf8_to_utf16(name)))?;

                // Set 'length' property for generator function
                let mut fn_length = 0_usize;
                for p in params.iter() {
                    match p {
                        crate::core::DestructuringElement::Variable(_, default_opt) => {
                            if default_opt.is_some() {
                                break;
                            }
                            fn_length += 1;
                        }
                        crate::core::DestructuringElement::Rest(_) => break,
                        crate::core::DestructuringElement::NestedArray(..) | crate::core::DestructuringElement::NestedObject(..) => {
                            fn_length += 1;
                        }
                        crate::core::DestructuringElement::Empty => {}
                        _ => {}
                    }
                }
                let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(fn_length as f64), false, false, true)?;
                crate::js_object::define_property_internal(mc, &func_obj, "length", &desc_len)?;

                // Create prototype object
                let proto_obj = crate::core::new_js_object_data(mc);
                // For generator (and async generator) functions, the created
                // `prototype` object's internal [[Prototype]] should point to the
                // realm's Generator.prototype (or AsyncGenerator.prototype).
                // Fall back to Object.prototype when the intrinsic is not available.
                if *is_async {
                    if let Some(async_gen_val) = env_get(env, "AsyncGenerator")
                        && let Value::Object(async_gen_ctor) = &*async_gen_val.borrow()
                        && let Some(async_proto_val) = object_get_key_value(async_gen_ctor, "prototype")
                    {
                        let proto_value = match &*async_proto_val.borrow() {
                            Value::Property { value: Some(v), .. } => v.borrow().clone(),
                            other => other.clone(),
                        };
                        if let Value::Object(async_proto) = proto_value {
                            proto_obj.borrow_mut(mc).prototype = Some(async_proto);
                        }
                    } else if let Some(obj_val) = env_get(env, "Object")
                        && let Value::Object(obj_ctor) = &*obj_val.borrow()
                        && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
                        && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
                    {
                        proto_obj.borrow_mut(mc).prototype = Some(*obj_proto);
                    }
                } else if let Some(gen_val) = env_get(env, "Generator")
                    && let Value::Object(gen_ctor) = &*gen_val.borrow()
                    && let Some(gen_proto_val) = object_get_key_value(gen_ctor, "prototype")
                {
                    let proto_value = match &*gen_proto_val.borrow() {
                        Value::Property { value: Some(v), .. } => v.borrow().clone(),
                        other => other.clone(),
                    };
                    if let Value::Object(gen_proto) = proto_value {
                        proto_obj.borrow_mut(mc).prototype = Some(gen_proto);
                    }
                } else if let Some(obj_val) = env_get(env, "Object")
                    && let Value::Object(obj_ctor) = &*obj_val.borrow()
                    && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
                    && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
                {
                    proto_obj.borrow_mut(mc).prototype = Some(*obj_proto);
                }

                // Define the 'prototype' property on the function with the standard
                // attributes: writable:true, enumerable:false, configurable:false
                let desc_proto = crate::core::create_descriptor_object(mc, &Value::Object(proto_obj), true, false, false)?;
                crate::js_object::define_property_internal(mc, &func_obj, "prototype", &desc_proto)?;
                env_set(mc, env, name, &Value::Object(func_obj))?;
            } else if *is_async {
                let func_obj = crate::core::new_js_object_data(mc);

                if let Some(func_ctor_val) = env_get(env, "Function")
                    && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                    && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                    && let Value::Object(proto) = &*proto_val.borrow()
                {
                    func_obj.borrow_mut(mc).prototype = Some(*proto);
                }

                let is_strict = body.first()
                    .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
                    .unwrap_or(false);
                let closure_data = ClosureData {
                    params: params.clone(),
                    body: body.clone(),
                    env: Some(*env),
                    is_strict,
                    enforce_strictness_inheritance: true,
                    ..ClosureData::default()
                };
                let closure_val = Value::AsyncClosure(Gc::new(mc, closure_data));

                func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
                object_set_key_value(mc, &func_obj, "name", &Value::String(utf8_to_utf16(name)))?;
                // Set 'length' property for async function declaration
                let mut fn_length = 0_usize;
                for p in params.iter() {
                    match p {
                        crate::core::DestructuringElement::Variable(_, default_opt) => {
                            if default_opt.is_some() {
                                break;
                            }
                            fn_length += 1;
                        }
                        crate::core::DestructuringElement::Rest(_) => break,
                        crate::core::DestructuringElement::NestedArray(..) | crate::core::DestructuringElement::NestedObject(..) => {
                            fn_length += 1;
                        }
                        crate::core::DestructuringElement::Empty => {}
                        _ => {}
                    }
                }
                let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(fn_length as f64), false, false, true)?;
                crate::js_object::define_property_internal(mc, &func_obj, "length", &desc_len)?;
                env_set(mc, env, name, &Value::Object(func_obj))?;
            } else {
                let func = evaluate_function_expression(mc, env, None, params, &mut body_clone)?;
                if let Value::Object(func_obj) = &func {
                    object_set_key_value(mc, func_obj, "name", &Value::String(utf8_to_utf16(name)))?;
                    // CreateGlobalFunctionBinding semantics when executing in the global environment
                    let key = crate::core::PropertyKey::String(name.clone());
                    if env.borrow().prototype.is_none() {
                        let existing = get_own_property(env, &key);
                        log::trace!(
                            "hoist_declarations: creating global function binding for '{}' existing={:?} is_configurable={}",
                            name,
                            existing,
                            env.borrow().is_configurable(&key)
                        );
                        if existing.is_none() || env.borrow().is_configurable(&key) {
                            let desc_obj = crate::core::create_descriptor_object(mc, &Value::Object(*func_obj), true, true, true)?;
                            crate::js_object::define_property_internal(mc, env, &key, &desc_obj)?;
                        } else {
                            let desc_obj = crate::core::new_js_object_data(mc);
                            object_set_key_value(mc, &desc_obj, "value", &Value::Object(*func_obj))?;
                            crate::js_object::define_property_internal(mc, env, &key, &desc_obj)?;
                        }
                        log::trace!(
                            "hoist_declarations: after create binding is_configurable={}",
                            env.borrow().is_configurable(&key)
                        );
                    } else {
                        env_set(mc, env, name, &Value::Object(*func_obj))?;
                    }
                } else {
                    env_set(mc, env, name, &func)?;
                }
            }
        }
    }

    // 2. Hoist Var declarations (recursively)
    hoist_var_declarations(mc, env, statements, is_indirect_eval)?;

    // 3. Hoist Lexical declarations (let, const, class) - top-level only, initialize to Uninitialized (TDZ)
    if !skip_lexicals {
        for stmt in statements {
            match &*stmt.kind {
                StatementKind::Let(decls) => {
                    for (name, _) in decls {
                        env_set(mc, env, name, &Value::Uninitialized)?;
                        env.borrow_mut(mc).set_lexical(name.clone());
                        log::trace!("hoist_declarations: hoisted lexical '{}' into env {:p}", name, env);
                    }
                }
                StatementKind::Const(decls) => {
                    for (name, _) in decls {
                        env_set(mc, env, name, &Value::Uninitialized)?;
                        env.borrow_mut(mc).set_lexical(name.clone());
                        log::trace!("hoist_declarations: hoisted const lexical '{}' into env {:p}", name, env);
                    }
                }
                StatementKind::Class(class_def) => {
                    env_set(mc, env, &class_def.name, &Value::Uninitialized)?;
                    env.borrow_mut(mc).set_lexical(class_def.name.clone());
                    log::trace!("hoist_declarations: hoisted class lexical '{}' into env {:p}", class_def.name, env);
                }
                StatementKind::Import(specifiers, _) => {
                    for spec in specifiers {
                        match spec {
                            ImportSpecifier::Default(name) => {
                                env_set(mc, env, name, &Value::Uninitialized)?;
                            }
                            ImportSpecifier::Named(name, alias) => {
                                let binding_name = alias.as_ref().unwrap_or(name);
                                env_set(mc, env, binding_name, &Value::Uninitialized)?;
                                env.borrow_mut(mc).set_lexical(binding_name.clone());
                            }
                            ImportSpecifier::Namespace(name) => {
                                env_set(mc, env, name, &Value::Uninitialized)?;
                            }
                        }
                    }
                }
                StatementKind::LetDestructuringArray(pattern, _) | StatementKind::ConstDestructuringArray(pattern, _) => {
                    let mut names = Vec::new();
                    collect_names_from_destructuring(pattern, &mut names);
                    for name in names {
                        env_set(mc, env, &name, &Value::Uninitialized)?;
                    }
                }
                StatementKind::LetDestructuringObject(pattern, _) | StatementKind::ConstDestructuringObject(pattern, _) => {
                    let mut names = Vec::new();
                    collect_names_from_object_destructuring(pattern, &mut names);
                    for name in names {
                        env_set(mc, env, &name, &Value::Uninitialized)?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

pub fn evaluate_statements<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
) -> Result<Value<'gc>, EvalError<'gc>> {
    log::trace!(
        "evaluate_statements: entering env {:p} lexicals={:?}",
        env,
        env.borrow().lexical_declarations
    );
    match evaluate_statements_with_labels(mc, env, statements, &[], &[])? {
        ControlFlow::Normal(val) => Ok(val),
        ControlFlow::Return(val) => Ok(val),
        ControlFlow::Throw(val, line, column) => Err(EvalError::Throw(val, line, column)),
        ControlFlow::Break(_) => Err(raise_syntax_error!("break statement not in loop or switch").into()),
        ControlFlow::Continue(_) => Err(raise_syntax_error!("continue statement not in loop").into()),
    }
}

/// belongs to src/js_object.rs module
pub(crate) fn handle_object_prototype_to_string<'gc>(
    mc: &MutationContext<'gc>,
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Value<'gc> {
    let tag = match val {
        Value::Undefined => "Undefined".to_string(),
        Value::Null => "Null".to_string(),
        Value::String(_) => "String".to_string(),
        Value::Number(_) => "Number".to_string(),
        Value::Boolean(_) => "Boolean".to_string(),
        Value::BigInt(_) => "BigInt".to_string(),
        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..) => "Function".to_string(),
        Value::Object(obj) => {
            if is_array(mc, obj) {
                "Array".to_string()
            } else if is_date_object(obj) {
                "Date".to_string()
            } else {
                let mut t = "Object".to_string();
                if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                    && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                    && let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
                    && let Value::Symbol(s) = &*tag_sym.borrow()
                    && let Some(val) = object_get_key_value(obj, s)
                    && let Value::String(s_val) = &*val.borrow()
                {
                    t = crate::unicode::utf16_to_utf8(s_val);
                }
                t
            }
        }
        _ => "Object".to_string(),
    };
    Value::String(utf8_to_utf16(&format!("[object {}]", tag)))
}

pub fn evaluate_statements_with_context<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
    labels: &[String],
) -> Result<ControlFlow<'gc>, EvalError<'gc>> {
    evaluate_statements_with_labels(mc, env, statements, labels, &[])
}

pub fn evaluate_statements_with_context_and_last_value<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
    labels: &[String],
) -> Result<(ControlFlow<'gc>, Value<'gc>), EvalError<'gc>> {
    evaluate_statements_with_labels_and_last(mc, env, statements, labels, &[])
}

fn check_expr_for_forbidden_assignment(e: &Expr) -> bool {
    match e {
        Expr::Assign(lhs, _rhs) => {
            if let Expr::Var(name, ..) = &**lhs {
                return name == "arguments" || name == "eval";
            }
            false
        }
        Expr::Property(obj, _)
        | Expr::Call(obj, _)
        | Expr::New(obj, _)
        | Expr::Index(obj, _)
        | Expr::OptionalProperty(obj, _)
        | Expr::OptionalPrivateMember(obj, _)
        | Expr::OptionalCall(obj, _)
        | Expr::OptionalIndex(obj, _) => check_expr_for_forbidden_assignment(obj),
        Expr::Binary(l, _, r) | Expr::Comma(l, r) | Expr::Conditional(l, r, _) => {
            check_expr_for_forbidden_assignment(l) || check_expr_for_forbidden_assignment(r)
        }
        Expr::LogicalAnd(l, r) | Expr::LogicalOr(l, r) | Expr::NullishCoalescing(l, r) => {
            check_expr_for_forbidden_assignment(l) || check_expr_for_forbidden_assignment(r)
        }
        Expr::AddAssign(l, r)
        | Expr::SubAssign(l, r)
        | Expr::MulAssign(l, r)
        | Expr::DivAssign(l, r)
        | Expr::ModAssign(l, r)
        | Expr::BitAndAssign(l, r)
        | Expr::BitOrAssign(l, r)
        | Expr::BitXorAssign(l, r)
        | Expr::LeftShiftAssign(l, r)
        | Expr::RightShiftAssign(l, r)
        | Expr::UnsignedRightShiftAssign(l, r)
        | Expr::LogicalAndAssign(l, r)
        | Expr::LogicalOrAssign(l, r)
        | Expr::NullishAssign(l, r) => {
            if let Expr::Var(name, ..) = &**l
                && (name == "arguments" || name == "eval")
            {
                return true;
            }
            check_expr_for_forbidden_assignment(l) || check_expr_for_forbidden_assignment(r)
        }
        Expr::UnaryNeg(inner)
        | Expr::UnaryPlus(inner)
        | Expr::LogicalNot(inner)
        | Expr::TypeOf(inner)
        | Expr::Delete(inner)
        | Expr::Void(inner)
        | Expr::Await(inner)
        | Expr::Yield(Some(inner))
        | Expr::YieldStar(inner)
        | Expr::PostIncrement(inner)
        | Expr::PostDecrement(inner)
        | Expr::Increment(inner)
        | Expr::Decrement(inner) => check_expr_for_forbidden_assignment(inner),
        Expr::Function(_, _, body)
        | Expr::GeneratorFunction(_, _, body)
        | Expr::AsyncFunction(_, _, body)
        | Expr::AsyncArrowFunction(_, body)
        | Expr::ArrowFunction(_, body) => body.iter().any(check_stmt_for_forbidden_assignment),
        _ => false,
    }
}

fn check_expr_for_var_forbidden_names(e: &Expr) -> bool {
    match e {
        Expr::Function(name, _, body) | Expr::GeneratorFunction(name, _, body) | Expr::AsyncFunction(name, _, body) => {
            if let Some(n) = name
                && (n == "eval" || n == "arguments")
            {
                return true;
            }
            body.iter().any(check_stmt_for_var_forbidden_names)
        }
        Expr::ArrowFunction(_, body) | Expr::AsyncArrowFunction(_, body) => body.iter().any(check_stmt_for_var_forbidden_names),
        Expr::Binary(l, _, r) => check_expr_for_var_forbidden_names(l) || check_expr_for_var_forbidden_names(r),
        Expr::Assign(l, r) => check_expr_for_var_forbidden_names(l) || check_expr_for_var_forbidden_names(r),
        Expr::Call(callee, args) => check_expr_for_var_forbidden_names(callee) || args.iter().any(check_expr_for_var_forbidden_names),
        Expr::UnaryNeg(val)
        | Expr::UnaryPlus(val)
        | Expr::BitNot(val)
        | Expr::LogicalNot(val)
        | Expr::Increment(val)
        | Expr::Decrement(val)
        | Expr::TypeOf(val)
        | Expr::Void(val)
        | Expr::Delete(val)
        | Expr::Await(val)
        | Expr::Spread(val) => check_expr_for_var_forbidden_names(val),
        // Simplification: We need to traverse structure to find function expressions.
        _ => false,
    }
}

fn check_stmt_for_var_forbidden_names(stmt: &Statement) -> bool {
    match &*stmt.kind {
        StatementKind::Var(decls) | StatementKind::Let(decls) => {
            for (name, _) in decls {
                if name == "eval" || name == "arguments" {
                    return true;
                }
            }
            false
        }
        StatementKind::Const(decls) => {
            for (name, _) in decls {
                if name == "eval" || name == "arguments" {
                    return true;
                }
            }
            false
        }
        StatementKind::FunctionDeclaration(name, _, body, _, _) => {
            if name == "eval" || name == "arguments" {
                return true;
            }
            body.iter().any(check_stmt_for_var_forbidden_names)
        }
        StatementKind::Class(def) => def.name == "eval" || def.name == "arguments",
        StatementKind::Block(stmts) => stmts.iter().any(check_stmt_for_var_forbidden_names),
        StatementKind::If(if_stmt) => {
            if_stmt.then_body.iter().any(check_stmt_for_var_forbidden_names)
                || if_stmt
                    .else_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(check_stmt_for_var_forbidden_names))
        }
        StatementKind::For(for_stmt) => {
            if let Some(init) = &for_stmt.init
                && check_stmt_for_var_forbidden_names(init)
            {
                return true;
            }
            for_stmt.body.iter().any(check_stmt_for_var_forbidden_names)
        }
        StatementKind::ForOf(_, name, _, body) | StatementKind::ForAwaitOf(_, name, _, body) | StatementKind::ForIn(_, name, _, body) => {
            if name == "eval" || name == "arguments" {
                return true;
            }
            body.iter().any(check_stmt_for_var_forbidden_names)
        }
        StatementKind::ForOfDestructuringObject(_, _, _, body)
        | StatementKind::ForOfDestructuringArray(_, _, _, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, _, _, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, _, _, body)
        | StatementKind::ForInDestructuringObject(_, _, _, body)
        | StatementKind::ForInDestructuringArray(_, _, _, body)
        | StatementKind::ForOfExpr(_, _, body)
        | StatementKind::ForAwaitOfExpr(_, _, body)
        | StatementKind::ForInExpr(_, _, body)
        | StatementKind::While(_, body) => body.iter().any(check_stmt_for_var_forbidden_names),
        StatementKind::DoWhile(body, _) => body.iter().any(check_stmt_for_var_forbidden_names),
        StatementKind::Switch(switch_stmt) => switch_stmt.cases.iter().any(|c| match c {
            crate::core::SwitchCase::Case(_, stmts) => stmts.iter().any(check_stmt_for_var_forbidden_names),
            crate::core::SwitchCase::Default(stmts) => stmts.iter().any(check_stmt_for_var_forbidden_names),
        }),
        StatementKind::With(_, body) => body.iter().any(check_stmt_for_var_forbidden_names),
        StatementKind::TryCatch(try_stmt) => {
            if let Some(param) = &try_stmt.catch_param
                && (param == "eval" || param == "arguments")
            {
                return true;
            }
            try_stmt.try_body.iter().any(check_stmt_for_var_forbidden_names)
                || try_stmt
                    .catch_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(check_stmt_for_var_forbidden_names))
                || try_stmt
                    .finally_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(check_stmt_for_var_forbidden_names))
        }
        StatementKind::Label(_, stmt) => check_stmt_for_var_forbidden_names(stmt),
        StatementKind::Expr(e) => check_expr_for_var_forbidden_names(e),
        StatementKind::Return(Some(e)) => check_expr_for_var_forbidden_names(e),
        StatementKind::Throw(e) => check_expr_for_var_forbidden_names(e),
        _ => false,
    }
}

fn check_stmt_for_forbidden_assignment(stmt: &Statement) -> bool {
    match &*stmt.kind {
        StatementKind::Expr(e) => check_expr_for_forbidden_assignment(e),
        StatementKind::If(if_stmt) => {
            check_expr_for_forbidden_assignment(&if_stmt.condition)
                || if_stmt.then_body.iter().any(check_stmt_for_forbidden_assignment)
                || if_stmt
                    .else_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(check_stmt_for_forbidden_assignment))
        }
        StatementKind::Block(stmts) => stmts.iter().any(check_stmt_for_forbidden_assignment),
        StatementKind::TryCatch(try_stmt) => {
            try_stmt.try_body.iter().any(check_stmt_for_forbidden_assignment)
                || try_stmt
                    .catch_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(check_stmt_for_forbidden_assignment))
                || try_stmt
                    .finally_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(check_stmt_for_forbidden_assignment))
        }
        StatementKind::FunctionDeclaration(_, _, body, _, _) => body.iter().any(check_stmt_for_forbidden_assignment),
        _ => false,
    }
}

fn check_strict_mode_violations<'gc>(stmts: &[Statement]) -> Result<(), EvalError<'gc>> {
    // Minimal enforcement: detect 'with' statements and reserved identifier 'static' in variable declarations when executed in strict mode.
    for stmt in stmts {
        match &*stmt.kind {
            StatementKind::With(_, _) => {
                return Err(raise_syntax_error!("Strict mode code may not include 'with' statements").into());
            }
            StatementKind::Var(decls) => {
                for (name, _) in decls {
                    if name == "static" {
                        return Err(raise_syntax_error!("Unexpected reserved word in strict mode").into());
                    }
                }
            }
            StatementKind::Let(decls) => {
                for (name, _) in decls {
                    if name == "static" {
                        return Err(raise_syntax_error!("Unexpected reserved word in strict mode").into());
                    }
                }
            }
            StatementKind::Const(decls) => {
                for (name, _) in decls {
                    if name == "static" {
                        return Err(raise_syntax_error!("Unexpected reserved word in strict mode").into());
                    }
                }
            }
            StatementKind::FunctionDeclaration(_, _, body, _, _) => check_strict_mode_violations(body)?,
            StatementKind::Block(stmts) => check_strict_mode_violations(stmts)?,
            StatementKind::If(if_stmt) => {
                check_strict_mode_violations(&if_stmt.then_body)?;
                if let Some(else_body) = &if_stmt.else_body {
                    check_strict_mode_violations(else_body)?;
                }
            }
            StatementKind::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    match &*init.kind {
                        StatementKind::Var(decls) => {
                            for (name, _) in decls {
                                if name == "static" {
                                    return Err(raise_syntax_error!("Unexpected reserved word in strict mode").into());
                                }
                            }
                        }
                        StatementKind::Let(decls) => {
                            for (name, _) in decls {
                                if name == "static" {
                                    return Err(raise_syntax_error!("Unexpected reserved word in strict mode").into());
                                }
                            }
                        }
                        StatementKind::Const(decls) => {
                            for (name, _) in decls {
                                if name == "static" {
                                    return Err(raise_syntax_error!("Unexpected reserved word in strict mode").into());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                check_strict_mode_violations(&for_stmt.body)?;
            }
            StatementKind::ForOf(_, _, _, body)
            | StatementKind::ForAwaitOf(_, _, _, body)
            | StatementKind::ForIn(_, _, _, body)
            | StatementKind::ForOfDestructuringObject(_, _, _, body)
            | StatementKind::ForOfDestructuringArray(_, _, _, body)
            | StatementKind::ForAwaitOfDestructuringObject(_, _, _, body)
            | StatementKind::ForAwaitOfDestructuringArray(_, _, _, body)
            | StatementKind::ForInDestructuringObject(_, _, _, body)
            | StatementKind::ForInDestructuringArray(_, _, _, body)
            | StatementKind::ForAwaitOfExpr(_, _, body)
            | StatementKind::While(_, body)
            | StatementKind::DoWhile(body, _) => check_strict_mode_violations(body)?,
            StatementKind::Switch(sw) => {
                for case in &sw.cases {
                    match case {
                        SwitchCase::Case(_, body) => check_strict_mode_violations(body)?,
                        SwitchCase::Default(body) => check_strict_mode_violations(body)?,
                    }
                }
            }
            StatementKind::TryCatch(tc) => {
                check_strict_mode_violations(&tc.try_body)?;
                if let Some(c) = &tc.catch_body {
                    check_strict_mode_violations(c)?;
                }
                if let Some(f) = &tc.finally_body {
                    check_strict_mode_violations(f)?;
                }
            }
            StatementKind::Label(_, s) => check_strict_mode_violations(std::slice::from_ref(s))?,
            StatementKind::Expr(e) => {
                // For expressions, check nested function bodies
                match e {
                    Expr::Function(_, _, body) | Expr::GeneratorFunction(_, _, body) | Expr::AsyncFunction(_, _, body) => {
                        check_strict_mode_violations(body)?;
                    }
                    Expr::ArrowFunction(_, body) | Expr::AsyncArrowFunction(_, body) => {
                        check_strict_mode_violations(body)?;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn evaluate_statements_with_labels<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
    labels: &[String],
    own_labels: &[String],
) -> Result<ControlFlow<'gc>, EvalError<'gc>> {
    // If this statement sequence begins with a "use strict" directive, we
    // need to mark the evaluation environment as strict. For indirect evals
    // executed in the global environment, strict-mode eval code must not
    // instantiate top-level FunctionDeclarations into the global variable
    // environment. In that case we create a fresh declarative environment with
    // its prototype set to the global env and perform hoisting/evaluation there
    // so declarations don't leak into the global scope.
    let mut exec_env = *env;

    // Detect indirect eval marker on the provided environment (global) so we
    // can apply the correct hoisting semantics for indirect evals.
    let is_indirect_eval = if let Some(flag_rc) = object_get_key_value(env, "__is_indirect_eval") {
        matches!(*flag_rc.borrow(), Value::Boolean(true))
    } else {
        false
    };

    // Check for 'use strict' directive
    let starts_with_use_strict = if let Some(stmt0) = statements.first()
        && let StatementKind::Expr(expr) = &*stmt0.kind
        && let Expr::StringLit(s) = expr
        && utf16_to_utf8(s).as_str() == "use strict"
    {
        true
    } else {
        false
    };

    if is_indirect_eval {
        if starts_with_use_strict {
            log::trace!("evaluate_statements: indirect strict eval - creating declarative env");
            let new_env = crate::core::new_js_object_data(mc);
            new_env.borrow_mut(mc).prototype = Some(*env);
            // Prevent hoisting into the global env: treat this declarative env as a function scope
            new_env.borrow_mut(mc).is_function_scope = true;
            exec_env = new_env;
            env_set_strictness(mc, &exec_env, true)?;

            // In the strict indirect case, hoisting should be performed on the
            // new environment so function declarations do not leak into the global
            // environment.
            hoist_declarations(mc, &exec_env, statements, false, true)?;
        } else {
            // Non-strict indirect eval: lexical declarations (let/const/class)
            // must be created in a fresh declarative environment whose prototype
            // is the global env, but var/function/var-hoisted declarations still
            // go into the global variable environment.
            log::trace!("evaluate_statements: non-strict indirect eval - create lex env for lexical bindings");
            // Find topmost global environment (prototype == None)
            let mut top_env = Some(*env);
            let mut global_env = *env;
            while let Some(e) = top_env {
                if e.borrow().prototype.is_none() {
                    global_env = e;
                    break;
                }
                top_env = e.borrow().prototype;
            }

            log::trace!(
                "evaluate_statements: computed global_env ptr={:p} extensible={}",
                &global_env,
                global_env.borrow().is_extensible()
            );
            let lex_env = crate::core::new_js_object_data(mc);
            lex_env.borrow_mut(mc).prototype = Some(global_env);

            // Hoist functions and var declarations into the global env,
            // but skip lexical hoisting there.
            log::trace!(
                "evaluate_statements: hoisting functions/vars on global env {:p} (skip_lexicals=true)",
                &global_env
            );
            hoist_declarations(mc, &global_env, statements, true, true)?;
            log::trace!(
                "evaluate_statements: finished hoisting functions/vars on global env {:p}",
                &global_env
            );

            // Now perform lexical hoisting (TDZ) into the lex env
            for stmt in statements {
                match &*stmt.kind {
                    StatementKind::Let(decls) => {
                        for (name, _) in decls {
                            env_set(mc, &lex_env, name, &Value::Uninitialized)?;
                            lex_env.borrow_mut(mc).set_lexical(name.clone());
                            log::trace!(
                                "evaluate_statements: non-strict indirect - hoisted lexical '{}' into lex_env {:p}",
                                name,
                                &lex_env
                            );
                        }
                    }
                    StatementKind::Const(decls) => {
                        for (name, _) in decls {
                            env_set(mc, &lex_env, name, &Value::Uninitialized)?;
                            lex_env.borrow_mut(mc).set_lexical(name.clone());
                            log::trace!(
                                "evaluate_statements: non-strict indirect - hoisted const lexical '{}' into lex_env {:p}",
                                name,
                                &lex_env
                            );
                        }
                    }
                    StatementKind::Class(class_def) => {
                        env_set(mc, &lex_env, &class_def.name, &Value::Uninitialized)?;
                        lex_env.borrow_mut(mc).set_lexical(class_def.name.clone());
                        log::trace!(
                            "evaluate_statements: non-strict indirect - hoisted class lexical '{}' into lex_env {:p}",
                            class_def.name,
                            &lex_env
                        );
                    }
                    StatementKind::Import(specifiers, _) => {
                        for spec in specifiers {
                            match spec {
                                ImportSpecifier::Default(name) => {
                                    env_set(mc, &lex_env, name, &Value::Uninitialized)?;
                                }
                                ImportSpecifier::Named(name, alias) => {
                                    let binding_name = alias.as_ref().unwrap_or(name);
                                    env_set(mc, &lex_env, binding_name, &Value::Uninitialized)?;
                                }
                                ImportSpecifier::Namespace(name) => {
                                    env_set(mc, &lex_env, name, &Value::Uninitialized)?;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Execute in the lex env so lexical references resolve correctly
            exec_env = lex_env;
        }
    } else {
        // Not an indirect eval: just honor 'use strict' directive if present
        if starts_with_use_strict {
            log::trace!("evaluate_statements: detected 'use strict' directive; marking env as strict");
            env_set_strictness(mc, &exec_env, true)?;
        }

        hoist_declarations(mc, &exec_env, statements, false, false)?;
    }

    // If the execution environment is marked strict, scan for certain forbidden
    // patterns such as assignment to the Identifier 'arguments' in function
    // bodies which should be a SyntaxError under strict mode (matching Test262
    // expectations for eval'd code in strict contexts).
    if env_get_strictness(&exec_env) {
        for stmt in statements {
            let mut err_msg = None;
            if check_stmt_for_forbidden_assignment(stmt) {
                err_msg = Some("Strict mode violation: assignment to 'arguments' or 'eval'");
            } else if check_stmt_for_var_forbidden_names(stmt) {
                err_msg = Some("Strict mode violation: invalid variable name 'eval' or 'arguments'");
            }

            if let Some(msg_str) = err_msg {
                log::debug!("evaluate_statements: strict mode violation: {}", msg_str);
                // Construct a SyntaxError object and throw it so it behaves like a JS exception
                if let Some(syn_ctor_val) = object_get_key_value(&exec_env, "SyntaxError")
                    && let Value::Object(syn_ctor) = &*syn_ctor_val.borrow()
                    && let Some(proto_val_rc) = object_get_key_value(syn_ctor, "prototype")
                    && let Value::Object(proto_ptr) = &*proto_val_rc.borrow()
                {
                    let msg = Value::String(utf8_to_utf16(msg_str));
                    let err_obj = crate::core::create_error(mc, Some(*proto_ptr), msg)?;

                    // DEBUG: Check constructor name
                    // In real code we wouldn't do this, but for debugging why assert.throws fails:
                    // log::trace!("Throwing Strict Violation Error: {:?}", err_obj);

                    return Err(EvalError::Throw(err_obj, None, None));
                }
                // If we couldn't construct a SyntaxError instance for some reason, fall back
                return Err(raise_syntax_error!(msg_str).into());
            }
        }
    }

    let mut last_value = Value::Undefined;
    for stmt in statements {
        if let Some(cf) = eval_res(mc, stmt, &mut last_value, &exec_env, labels, own_labels)? {
            return Ok(cf);
        }
    }
    Ok(ControlFlow::Normal(last_value))
}

pub fn evaluate_statements_with_labels_and_last<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
    labels: &[String],
    own_labels: &[String],
) -> Result<(ControlFlow<'gc>, Value<'gc>), EvalError<'gc>> {
    // This mirrors evaluate_statements_with_labels but returns the last_value alongside the ControlFlow

    let mut exec_env = *env;

    // Detect indirect eval marker on the provided environment (global) so we
    // can apply the correct hoisting semantics for indirect evals.
    let is_indirect_eval = if let Some(flag_rc) = object_get_key_value(env, "__is_indirect_eval") {
        matches!(*flag_rc.borrow(), Value::Boolean(true))
    } else {
        false
    };

    // Check for 'use strict' directive
    let starts_with_use_strict = if let Some(stmt0) = statements.first()
        && let StatementKind::Expr(expr) = &*stmt0.kind
        && let Expr::StringLit(s) = expr
        && utf16_to_utf8(s).as_str() == "use strict"
    {
        true
    } else {
        false
    };

    if is_indirect_eval {
        if starts_with_use_strict {
            let new_env = crate::core::new_js_object_data(mc);
            new_env.borrow_mut(mc).prototype = Some(*env);
            new_env.borrow_mut(mc).is_function_scope = true;
            exec_env = new_env;
            env_set_strictness(mc, &exec_env, true)?;

            // In strict indirect case, hoist into the new env so declarations don't
            // mutate the global bindings.
            hoist_declarations(mc, &exec_env, statements, false, true)?;
        } else {
            // Non-strict indirect: lexical declarations should use a fresh
            // decl env while var/function remain in global env.
            let lex_env = crate::core::new_js_object_data(mc);
            lex_env.borrow_mut(mc).prototype = Some(*env);

            hoist_declarations(mc, env, statements, true, true)?;

            // Hoist lexical decls into lex env
            for stmt in statements {
                match &*stmt.kind {
                    StatementKind::Let(decls) => {
                        for (name, _) in decls {
                            env_set(mc, &lex_env, name, &Value::Uninitialized)?;
                        }
                    }
                    StatementKind::Const(decls) => {
                        for (name, _) in decls {
                            env_set(mc, &lex_env, name, &Value::Uninitialized)?;
                        }
                    }
                    StatementKind::Class(class_def) => {
                        env_set(mc, &lex_env, &class_def.name, &Value::Uninitialized)?;
                    }
                    StatementKind::Import(specifiers, _) => {
                        for spec in specifiers {
                            match spec {
                                ImportSpecifier::Default(name) => {
                                    env_set(mc, &lex_env, name, &Value::Uninitialized)?;
                                }
                                ImportSpecifier::Named(name, alias) => {
                                    let binding_name = alias.as_ref().unwrap_or(name);
                                    env_set(mc, &lex_env, binding_name, &Value::Uninitialized)?;
                                }
                                ImportSpecifier::Namespace(name) => {
                                    env_set(mc, &lex_env, name, &Value::Uninitialized)?;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            exec_env = lex_env;
        }
    } else {
        if starts_with_use_strict {
            env_set_strictness(mc, &exec_env, true)?;
        }

        hoist_declarations(mc, &exec_env, statements, false, false)?;
    }

    // Debug: show any const bindings hoisted into the execution environment
    log::trace!(
        "evaluate_statements: post-hoist exec_env constants={:?}",
        exec_env.borrow().constants
    );

    if env_get_strictness(&exec_env) {
        for stmt in statements {
            let mut err_msg = None;
            if check_stmt_for_forbidden_assignment(stmt) {
                err_msg = Some("Strict mode violation: assignment to 'arguments' or 'eval'");
            } else if check_stmt_for_var_forbidden_names(stmt) {
                err_msg = Some("Strict mode violation: invalid variable name 'eval' or 'arguments'");
            }

            if let Some(msg_str) = err_msg {
                if let Some(syn_ctor_val) = object_get_key_value(&exec_env, "SyntaxError")
                    && let Value::Object(syn_ctor) = &*syn_ctor_val.borrow()
                    && let Some(proto_val_rc) = object_get_key_value(syn_ctor, "prototype")
                    && let Value::Object(proto_ptr) = &*proto_val_rc.borrow()
                {
                    let msg = Value::String(utf8_to_utf16(msg_str));
                    let err_obj = crate::core::create_error(mc, Some(*proto_ptr), msg)?;
                    return Err(EvalError::Throw(err_obj, Some(stmt.line), Some(stmt.column)));
                }
                return Err(raise_syntax_error!(msg_str).into());
            }
        }
    }

    let mut last_value = Value::Undefined;
    for stmt in statements {
        if let Some(cf) = eval_res(mc, stmt, &mut last_value, &exec_env, labels, own_labels)? {
            return Ok((cf, last_value));
        }
    }
    Ok((ControlFlow::Normal(last_value.clone()), last_value))
}

fn set_name_if_anonymous<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>, expr: &Expr, name: &str) -> Result<(), EvalError<'gc>> {
    let should_set = match expr {
        Expr::Function(None, ..)
        | Expr::GeneratorFunction(None, ..)
        | Expr::AsyncFunction(None, ..)
        | Expr::ArrowFunction(..)
        | Expr::AsyncArrowFunction(..) => true,
        // Class expressions do not receive inferred names from surrounding
        // variable/lexical bindings per the spec; name inference for classes is
        // handled in object literals and other specific contexts.
        _ => false,
    };

    if should_set && let Value::Object(obj) = val {
        let desc = create_descriptor_object(mc, &Value::String(utf8_to_utf16(name)), false, false, true)?;
        crate::js_object::define_property_internal(mc, obj, "name", &desc)?;
    }
    Ok(())
}

fn evaluate_destructuring_array_assignment<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    pattern: &[DestructuringElement],
    val: &Value<'gc>,
    is_const: bool,
    stmt_line: Option<usize>,
    stmt_column: Option<usize>,
) -> Result<(), EvalError<'gc>> {
    if matches!(val, Value::Undefined | Value::Null) {
        return Err(raise_type_error!("Cannot destructure undefined or null").into());
    }

    let mut iterator: Option<crate::core::JSObjectDataPtr<'gc>> = None;
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
        && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
    {
        let method = if let Value::Object(obj) = val {
            log::debug!(
                "destructuring: looking up Symbol.iterator on obj ptr={:p} sym_ptr={:?} sym={:?}",
                Gc::as_ptr(*obj),
                Gc::as_ptr(*iter_sym_data),
                iter_sym_data
            );
            let is_gen = crate::core::object_get_key_value(obj, "__generator__").is_some();
            let proto_ptr = obj.borrow().prototype.map(Gc::as_ptr);
            log::debug!("destructuring: obj is_generator = {} proto = {:?}", is_gen, proto_ptr);
            let m = get_property_with_accessors(mc, env, obj, iter_sym_data)?;
            log::debug!("destructuring: method lookup result = {:?}", m);
            m
        } else {
            let m = get_primitive_prototype_property(mc, env, val, iter_sym_data)?;
            log::debug!("destructuring: primitive method lookup result = {:?}", m);
            m
        };
        if !matches!(method, Value::Undefined | Value::Null) {
            let res = evaluate_call_dispatch(mc, env, &method, Some(val), &[])?;
            if let Value::Object(iter_obj) = res {
                iterator = Some(iter_obj);
            }
        }
    }

    let iter_obj = if let Some(i) = iterator {
        i
    } else {
        return Err(raise_type_error!("Object is not iterable (or Symbol.iterator not found)").into());
    };

    let mut iterator_done = false;

    for elem in pattern {
        if matches!(elem, DestructuringElement::Empty) {
            if !iterator_done {
                let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                if matches!(next_method, Value::Undefined | Value::Null) {
                    return Err(raise_type_error!("Iterator has no next method").into());
                }
                let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                if let Value::Object(next_res) = next_res_val {
                    let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                    if matches!(done_val, Value::Boolean(true)) {
                        iterator_done = true;
                    }
                } else {
                    return Err(raise_type_error!("Iterator result is not an object").into());
                }
            }
            continue;
        }

        match elem {
            DestructuringElement::Rest(name) => {
                let rest_arr = create_array(mc, env)?;
                let mut idx = 0;
                if !iterator_done {
                    let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                    if matches!(next_method, Value::Undefined | Value::Null) {
                        return Err(raise_type_error!("Iterator has no next method").into());
                    }
                    loop {
                        let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                        if let Value::Object(next_res) = next_res_val {
                            let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                            if matches!(done_val, Value::Boolean(true)) {
                                iterator_done = true;
                                break;
                            }
                            let value = get_property_with_accessors(mc, env, &next_res, "value")?;
                            object_set_key_value(mc, &rest_arr, idx, &value)?;
                            idx += 1;
                        } else {
                            return Err(raise_type_error!("Iterator result is not an object").into());
                        }
                    }
                }
                env_set(mc, env, name, &Value::Object(rest_arr))?;
                if is_const {
                    env.borrow_mut(mc).set_const(name.clone());
                }
            }
            DestructuringElement::RestPattern(inner_elem) => {
                let rest_arr = create_array(mc, env)?;
                let mut idx = 0;
                if !iterator_done {
                    let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                    if matches!(next_method, Value::Undefined | Value::Null) {
                        return Err(raise_type_error!("Iterator has no next method").into());
                    }
                    loop {
                        let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                        if let Value::Object(next_res) = next_res_val {
                            let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                            if matches!(done_val, Value::Boolean(true)) {
                                iterator_done = true;
                                break;
                            }
                            let value = get_property_with_accessors(mc, env, &next_res, "value")?;
                            object_set_key_value(mc, &rest_arr, idx, &value)?;
                            idx += 1;
                        } else {
                            return Err(raise_type_error!("Iterator result is not an object").into());
                        }
                    }
                }
                evaluate_destructuring_element_rec(mc, env, inner_elem, &Value::Object(rest_arr), is_const, stmt_line, stmt_column)?;
            }
            other => {
                let mut value = Value::Undefined;
                if !iterator_done {
                    let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                    if matches!(next_method, Value::Undefined | Value::Null) {
                        return Err(raise_type_error!("Iterator has no next method").into());
                    }
                    let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                    if let Value::Object(next_res) = next_res_val {
                        let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                        if matches!(done_val, Value::Boolean(true)) {
                            iterator_done = true;
                        } else {
                            value = get_property_with_accessors(mc, env, &next_res, "value")?;
                        }
                    } else {
                        return Err(raise_type_error!("Iterator result is not an object").into());
                    }
                }
                evaluate_destructuring_element_rec(mc, env, other, &value, is_const, stmt_line, stmt_column)?;
            }
        }
    }

    if !iterator_done {
        iterator_close(mc, env, &iter_obj)?;
    }
    Ok(())
}

fn evaluate_destructuring_element_rec<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    elem: &DestructuringElement,
    val: &Value<'gc>,
    is_const: bool,
    stmt_line: Option<usize>,
    stmt_column: Option<usize>,
) -> Result<(), EvalError<'gc>> {
    match elem {
        DestructuringElement::Variable(name, default_expr) => {
            let mut v = val.clone();
            if matches!(v, Value::Undefined)
                && let Some(def_expr) = default_expr
            {
                v = evaluate_expr(mc, env, def_expr)?;
                maybe_set_function_name_for_default(mc, name, def_expr, &v)?;
            }
            env_set(mc, env, name, &v)?;
            if is_const {
                env.borrow_mut(mc).set_const(name.clone());
            }
        }
        DestructuringElement::NestedArray(inner_pattern, default_expr) => {
            let mut v = val.clone();
            if matches!(v, Value::Undefined)
                && let Some(def_expr) = default_expr
            {
                v = evaluate_expr(mc, env, def_expr)?;
            }
            evaluate_destructuring_array_assignment(mc, env, inner_pattern, &v, is_const, stmt_line, stmt_column)?;
        }
        DestructuringElement::NestedObject(inner_pattern, default_expr) => {
            let mut v = val.clone();
            if matches!(v, Value::Undefined)
                && let Some(def_expr) = default_expr
            {
                v = evaluate_expr(mc, env, def_expr)?;
            }
            if matches!(v, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot destructure undefined or null").into());
            }
            let obj_val = if let Value::Object(obj) = v {
                obj
            } else {
                return Err(raise_type_error!("Expected object for nested destructuring").into());
            };
            bind_object_inner_for_letconst(mc, env, inner_pattern, &obj_val, is_const)?;
        }
        _ => {}
    }
    Ok(())
}

fn eval_res<'gc>(
    mc: &MutationContext<'gc>,
    stmt: &Statement,
    last_value: &mut Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
    labels: &[String],
    own_labels: &[String],
) -> Result<Option<ControlFlow<'gc>>, EvalError<'gc>> {
    match &*stmt.kind {
        StatementKind::Expr(expr) => {
            log::trace!(
                "DEBUG: executing statement Expr: {:?} (line={}, col={})",
                expr,
                stmt.line,
                stmt.column
            );
            match evaluate_expr(mc, env, expr) {
                Ok(val) => {
                    let suppress_dynamic_import = env_get(env, "__suppress_dynamic_import_result")
                        .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
                        .unwrap_or(false);
                    let allow_dynamic_import = env_get(env, "__allow_dynamic_import_result")
                        .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
                        .unwrap_or(false);

                    // Do not treat top-level dynamic import() as the script result unless eval opted in.
                    if matches!(expr, Expr::DynamicImport(_)) && suppress_dynamic_import && !allow_dynamic_import {
                        *last_value = Value::Undefined;
                    } else {
                        *last_value = val;
                    }
                    Ok(None)
                }
                Err(e) => Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
            }
        }
        StatementKind::Let(decls) => {
            let mut _last_init = Value::Undefined;
            for (name, expr_opt) in decls {
                let val = if let Some(expr) = expr_opt {
                    if let Expr::Class(class_def) = expr
                        && class_def.name.is_empty()
                    {
                        // For variable/lexical declaration initializers that are anonymous
                        // class expressions, create the class object with the binding name
                        // so that static initializers see the inferred class-name during
                        // evaluation (per Test262 semantics).
                        crate::js_class::create_class_object(mc, name, &class_def.extends, &class_def.members, env, false)
                            .map_err(|e| refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e))?
                    } else {
                        let v = match evaluate_expr(mc, env, expr) {
                            Ok(v) => v,
                            Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                        };
                        set_name_if_anonymous(mc, &v, expr, name)?;
                        v
                    }
                } else {
                    Value::Undefined
                };
                _last_init = val.clone();
                env_set(mc, env, name, &val)?;
            }
            // *last_value = _last_init;
            Ok(None)
        }
        StatementKind::Var(decls) => {
            for (name, expr_opt) in decls {
                if let Some(expr) = expr_opt {
                    let val = if let Expr::Class(class_def) = expr
                        && class_def.name.is_empty()
                    {
                        // For variable/lexical declaration initializers that are anonymous
                        // class expressions, create the class object with the binding name
                        // so that static initializers see the inferred class-name during
                        // evaluation (per Test262 semantics).
                        crate::js_class::create_class_object(mc, name, &class_def.extends, &class_def.members, env, false)
                            .map_err(|e| refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e))?
                    } else {
                        let v = match evaluate_expr(mc, env, expr) {
                            Ok(v) => v,
                            Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                        };
                        set_name_if_anonymous(mc, &v, expr, name)?;
                        v
                    };

                    let mut target_env = *env;
                    while !target_env.borrow().is_function_scope {
                        if let Some(proto) = target_env.borrow().prototype {
                            target_env = proto;
                        } else {
                            break;
                        }
                    }
                    env_set(mc, &target_env, name, &val)?;
                }
            }
            Ok(None)
        }
        StatementKind::Const(decls) => {
            let mut _last_init = Value::Undefined;
            for (name, expr) in decls {
                let val = if let Expr::Class(class_def) = expr
                    && class_def.name.is_empty()
                {
                    // For variable/lexical declaration initializers that are anonymous
                    // class expressions, create the class object with the binding name
                    // so that static initializers see the inferred class-name during
                    // evaluation (per Test262 semantics).
                    crate::js_class::create_class_object(mc, name, &class_def.extends, &class_def.members, env, false)
                        .map_err(|e| refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e))?
                } else {
                    let v = match evaluate_expr(mc, env, expr) {
                        Ok(v) => v,
                        Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                    };
                    set_name_if_anonymous(mc, &v, expr, name)?;
                    v
                };
                _last_init = val.clone();
                // Bind value and mark the binding as const so subsequent assignments fail
                env_set(mc, env, name, &val)?;
                env.borrow_mut(mc).set_const(name.clone());
            }
            // *last_value = _last_init;
            Ok(None)
        }
        StatementKind::Class(class_def) => {
            // Evaluate class definition and bind to environment
            // This initializes the class binding which was hoisted as Uninitialized
            if let Err(e) = crate::js_class::create_class_object(mc, &class_def.name, &class_def.extends, &class_def.members, env, true) {
                return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e));
            }
            // *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Import(specifiers, source) => {
            // Try to deduce base path from env or use current dir
            let base_path = if let Some(cell) = env_get(env, "__filepath") {
                if let Value::String(s) = cell.borrow().clone() {
                    Some(crate::unicode::utf16_to_utf8(&s))
                } else {
                    None
                }
            } else {
                None
            };

            let exports = crate::js_module::load_module(mc, source, base_path.as_deref(), Some(*env))
                .map_err(|e| refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e))?;

            if let Value::Object(exports_obj) = exports {
                for spec in specifiers {
                    match spec {
                        ImportSpecifier::Named(name, alias) => {
                            let binding_name = alias.as_ref().unwrap_or(name);

                            let val_ptr_res = object_get_key_value(&exports_obj, name);
                            let val = if let Some(cell) = val_ptr_res {
                                cell.borrow().clone()
                            } else {
                                Value::Undefined
                            };
                            env_set(mc, env, binding_name, &val)?;
                        }
                        ImportSpecifier::Default(name) => {
                            let val_ptr_res = object_get_key_value(&exports_obj, "default");
                            let val = if let Some(cell) = val_ptr_res {
                                cell.borrow().clone()
                            } else {
                                Value::Undefined
                            };
                            env_set(mc, env, name, &val)?;
                        }
                        ImportSpecifier::Namespace(name) => {
                            env_set(mc, env, name, &Value::Object(exports_obj))?;
                        }
                    }
                }
            }
            Ok(None)
        }
        StatementKind::Export(specifiers, inner_stmt, source) => {
            if let Some(source) = source {
                let base_path = if let Some(cell) = env_get(env, "__filepath")
                    && let Value::String(s) = cell.borrow().clone()
                {
                    Some(utf16_to_utf8(&s))
                } else {
                    None
                };

                let mut resolved_self = false;
                let mut self_exports = None;
                if let Some(base) = base_path.as_deref() {
                    let current_path = std::path::Path::new(base).canonicalize().ok();
                    let source_path = crate::js_module::resolve_module_path(source, base_path.as_deref())
                        .ok()
                        .and_then(|p| std::path::Path::new(&p).canonicalize().ok());
                    if let (Some(current), Some(target)) = (current_path, source_path)
                        && current == target
                        && let Some(cell) = env_get(env, "exports")
                        && let Value::Object(exports_obj) = cell.borrow().clone()
                    {
                        resolved_self = true;
                        self_exports = Some(exports_obj);
                    }
                }

                let exports = if resolved_self {
                    let exports_obj = match self_exports {
                        Some(obj) => obj,
                        None => return Err(raise_type_error!("Module is not an object").into()),
                    };
                    Value::Object(exports_obj)
                } else {
                    crate::js_module::load_module(mc, source, base_path.as_deref(), Some(*env))
                        .map_err(|e| refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e))?
                };

                if let Value::Object(exports_obj) = exports {
                    for spec in specifiers {
                        match spec {
                            ExportSpecifier::Named(name, alias) => {
                                let export_name = alias.as_ref().unwrap_or(name);
                                let val_ptr_res = object_get_key_value(&exports_obj, name);
                                let val = if let Some(cell) = val_ptr_res {
                                    cell.borrow().clone()
                                } else {
                                    Value::Undefined
                                };
                                export_value(mc, env, export_name, &val)?;
                            }
                            ExportSpecifier::Namespace(name) => {
                                export_value(mc, env, name, &Value::Object(exports_obj))?;
                            }
                            ExportSpecifier::Star => {
                                for key in exports_obj.borrow().properties.keys() {
                                    if let PropertyKey::String(name) = key
                                        && name != "default"
                                        && let Some(cell) = object_get_key_value(&exports_obj, name)
                                    {
                                        export_value(mc, env, name, &cell.borrow())?;
                                    }
                                }
                            }
                            ExportSpecifier::Default(_) => {
                                return Err(raise_syntax_error!("Unexpected default export in re-export clause").into());
                            }
                        }
                    }
                } else {
                    return Err(raise_type_error!("Module is not an object").into());
                }
                return Ok(None);
            }

            // 1. Evaluate inner statement if present, to bind variables in current env
            if let Some(stmt) = inner_stmt {
                // Recursively evaluate inner statement
                // Note: inner_stmt is a Box<Statement>. We need to call eval_res or evaluate_statements on it.
                // Since evaluate_statements expects a slice, we can wrap it.
                let stmts = vec![*stmt.clone()];
                match evaluate_statements(mc, env, &stmts) {
                    Ok(_) => {} // Declarations are hoisted or executed, binding should be in env
                    Err(e) => return Err(e),
                }

                // If inner stmt was a declaration, we need to export the declared names.
                // For now, we handle named exports via specifiers only for `export { ... }`.
                // For `export var x = 1`, the parser should have produced specifiers?
                // My parser implementation for export var/function didn't produce specifiers, just inner_stmt.
                // So we need to look at inner_stmt kind to determine what to export.

                match &*stmt.kind {
                    StatementKind::Var(decls) => {
                        for (name, _) in decls {
                            if env_get(env, name).is_some() {
                                export_binding(mc, env, name, name)?;
                            }
                        }
                    }
                    StatementKind::Let(decls) => {
                        for (name, _) in decls {
                            if env_get(env, name).is_some() {
                                export_binding(mc, env, name, name)?;
                            }
                        }
                    }
                    StatementKind::Const(decls) => {
                        for (name, _) in decls {
                            if env_get(env, name).is_some() {
                                export_binding(mc, env, name, name)?;
                            }
                        }
                    }
                    StatementKind::FunctionDeclaration(name, ..) => {
                        if env_get(env, name).is_some() {
                            export_binding(mc, env, name, name)?;
                        }
                    }
                    _ => {}
                }
            }

            // 2. Handle explicit specifiers
            for spec in specifiers {
                match spec {
                    ExportSpecifier::Named(name, alias) => {
                        // export { name as alias }
                        // Use a live binding to the local environment.
                        if env_get(env, name).is_some() {
                            let export_name = alias.as_ref().unwrap_or(name);
                            export_binding(mc, env, export_name, name)?;
                        } else {
                            return Err(raise_reference_error!(format!("{} is not defined", name)).into());
                        }
                    }
                    ExportSpecifier::Default(expr) => {
                        // export default expr
                        // If it is an anonymous class expression, use "default" as class name
                        let val = if let Expr::Class(class_def) = expr
                            && class_def.name.is_empty()
                        {
                            create_class_object(mc, "default", &class_def.extends, &class_def.members, env, false)
                                .map_err(|e| refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e))?
                        } else {
                            evaluate_expr(mc, env, expr)?
                        };
                        export_value(mc, env, "default", &val)?;
                    }
                    ExportSpecifier::Namespace(_) => {
                        return Err(raise_syntax_error!("Namespace export requires a module source").into());
                    }
                    ExportSpecifier::Star => {
                        return Err(raise_syntax_error!("Star export requires a module source").into());
                    }
                }
            }

            Ok(None)
        }
        StatementKind::Return(expr_opt) => {
            let val = if let Some(expr) = expr_opt {
                match evaluate_expr(mc, env, expr) {
                    Ok(v) => v,
                    Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                }
            } else {
                Value::Undefined
            };
            Ok(Some(ControlFlow::Return(val)))
        }
        StatementKind::FunctionDeclaration(..) => {
            // Function declarations are hoisted, so they are already defined.
            Ok(None)
        }
        // Array destructuring: let/var/const [a, b] = expr
        StatementKind::LetDestructuringArray(pattern, expr) | StatementKind::ConstDestructuringArray(pattern, expr) => {
            let val = match evaluate_expr(mc, env, expr) {
                Ok(v) => v,
                Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
            };
            let is_const = matches!(stmt.kind.as_ref(), StatementKind::ConstDestructuringArray(..));
            evaluate_destructuring_array_assignment(mc, env, pattern, &val, is_const, Some(stmt.line), Some(stmt.column))?;
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::VarDestructuringArray(pattern, expr) => {
            let val = match evaluate_expr(mc, env, expr) {
                Ok(v) => v,
                Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
            };

            // Find VariableEnvironment (function scope or global)
            let mut var_env = *env;
            while !var_env.borrow().is_function_scope {
                if let Some(proto) = var_env.borrow().prototype {
                    var_env = proto;
                } else {
                    break;
                }
            }

            evaluate_destructuring_array_assignment(mc, &var_env, pattern, &val, false, Some(stmt.line), Some(stmt.column))?;
            *last_value = Value::Undefined;
            Ok(None)
        }
        // Object destructuring: let/var/const {a, b} = expr
        StatementKind::LetDestructuringObject(pattern, expr) | StatementKind::ConstDestructuringObject(pattern, expr) => {
            let val = match evaluate_expr(mc, env, expr) {
                Ok(v) => v,
                Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
            };
            log::trace!("TRACE: LetDestructuringObject: val={:?}", val);

            // If RHS is undefined/null, throw a helpful error referencing first property name
            if matches!(val, Value::Undefined) || matches!(val, Value::Null) {
                let prop_name = pattern
                    .iter()
                    .find_map(|p| {
                        if let ObjectDestructuringElement::Property { key, .. } = p {
                            Some(key.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "property".to_string());
                let val = if matches!(val, Value::Null) { "null" } else { "undefined" };
                return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
            }

            // Collect excluded names (and evaluate computed keys) so rest can skip them
            let mut excluded_names: Vec<crate::core::PropertyKey<'gc>> = Vec::new();
            let mut computed_keys: Vec<Option<crate::core::PropertyKey<'gc>>> = vec![None; pattern.len()];

            // DEBUG: print pattern element types and evaluate computed keys
            log::trace!("TRACE: LetDestructuringObject: pattern.len={}", pattern.len());
            for (i, p) in pattern.iter().enumerate() {
                match p {
                    ObjectDestructuringElement::Property { key, .. } => {
                        log::trace!("TRACE: pattern[{}] Property key={}", i, key);
                        excluded_names.push(crate::core::PropertyKey::String(key.clone()));
                    }
                    ObjectDestructuringElement::ComputedProperty { key: key_expr, .. } => {
                        log::trace!("TRACE: pattern[{}] ComputedProperty", i);
                        let key_val = evaluate_expr(mc, env, key_expr)?;
                        let prop_key = match key_val {
                            Value::Symbol(sd) => crate::core::PropertyKey::Symbol(sd),
                            Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                            Value::Number(n) => crate::core::PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                            Value::Object(_) => {
                                let prim = crate::core::to_primitive(mc, &key_val, "string", env)?;
                                match prim {
                                    Value::Symbol(s) => crate::core::PropertyKey::Symbol(s),
                                    Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                                    Value::Number(n) => crate::core::PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                                    other => crate::core::PropertyKey::String(crate::core::value_to_string(&other)),
                                }
                            }
                            other => crate::core::PropertyKey::String(crate::core::value_to_string(&other)),
                        };
                        log::trace!("TRACE: LetDestructuringObject: computed_key[{}] = {:?}", i, prop_key);
                        excluded_names.push(prop_key.clone());
                        computed_keys[i] = Some(prop_key);
                    }
                    ObjectDestructuringElement::Rest(name) => log::trace!("TRACE: pattern[{}] Rest name={}", i, name),
                }
            }
            for (i, prop) in pattern.iter().enumerate() {
                match prop {
                    ObjectDestructuringElement::Property { key, value } => {
                        match value {
                            DestructuringElement::Variable(name, default_expr) => {
                                // lookup property on object
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined)
                                    && let Some(def) = default_expr
                                {
                                    prop_val = evaluate_expr(mc, env, def)?;
                                }
                                env_set(mc, env, name, &prop_val)?;
                                if matches!(*stmt.kind, StatementKind::ConstDestructuringObject(_, _)) {
                                    env.borrow_mut(mc).set_const(name.clone());
                                }
                            }
                            DestructuringElement::NestedObject(inner_pattern, inner_default) => {
                                // fetch property
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined)
                                    && let Some(def) = inner_default
                                {
                                    prop_val = evaluate_expr(mc, env, def)?;
                                }
                                if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                    let prop_name = inner_pattern
                                        .iter()
                                        .find_map(|p| {
                                            if let DestructuringElement::Property(k, _) = p {
                                                Some(k.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_else(|| "property".to_string());
                                    let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                                    return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
                                }
                                if let Value::Object(obj2) = &prop_val {
                                    let is_const = matches!(*stmt.kind, StatementKind::ConstDestructuringObject(_, _));
                                    bind_object_inner_for_letconst(mc, env, inner_pattern, obj2, is_const)?;
                                } else {
                                    return Err(raise_eval_error!("Expected object for nested destructuring").into());
                                }
                            }
                            DestructuringElement::NestedArray(inner_array, inner_default) => {
                                // fetch property
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined)
                                    && let Some(def) = inner_default
                                {
                                    prop_val = evaluate_expr(mc, env, def)?;
                                }
                                if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                    let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                                    return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", key, val)).into());
                                }
                                if let Value::Object(oarr) = &prop_val {
                                    let is_const = matches!(*stmt.kind, StatementKind::ConstDestructuringObject(_, _));
                                    bind_array_inner_for_letconst(mc, env, inner_array, oarr, is_const, None, None)?;
                                } else {
                                    return Err(raise_eval_error!("Expected array for nested array destructuring").into());
                                }
                            }
                            _ => {
                                return Err(raise_eval_error!("Nested object destructuring not implemented").into());
                            }
                        }
                    }
                    ObjectDestructuringElement::ComputedProperty { key: _key_expr, value } => {
                        // Use precomputed key to avoid re-evaluating computed property expr
                        let prop_key = computed_keys[i].as_ref().expect("computed key evaluated").clone();
                        match value {
                            DestructuringElement::Variable(name, default_expr) => {
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, &prop_key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined)
                                    && let Some(def) = default_expr
                                {
                                    prop_val = evaluate_expr(mc, env, def)?;
                                }
                                env_set(mc, env, name, &prop_val)?;
                                if matches!(*stmt.kind, StatementKind::ConstDestructuringObject(_, _)) {
                                    env.borrow_mut(mc).set_const(name.clone());
                                }
                            }
                            _ => {
                                return Err(raise_eval_error!("Computed property value patterns not implemented").into());
                            }
                        }
                    }
                    ObjectDestructuringElement::Rest(name) => {
                        // Create a new object with remaining properties
                        let obj = new_js_object_data(mc);
                        if let Value::Object(orig) = &val {
                            println!(
                                "TRACE: LetRest: orig_ptr={:p} has_proxy? {}",
                                orig.as_ptr(),
                                orig.borrow()
                                    .properties
                                    .get(&PropertyKey::String("__proxy__".to_string()))
                                    .is_some()
                            );
                            // copy all own properties except those in pattern keys
                            let ordered = crate::core::ordinary_own_property_keys_mc(mc, orig)?;
                            for k in ordered {
                                // Skip if listed in the excluded names (precomputed earlier)
                                if excluded_names.iter().any(|ek| ek == &k) {
                                    continue;
                                }

                                // If this object is a proxy wrapper, delegate descriptor/get to proxy traps
                                if let Some(proxy_cell) = orig.borrow().properties.get(&PropertyKey::String("__proxy__".to_string()))
                                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                                {
                                    // Ask proxy for own property descriptor and check [[Enumerable]]
                                    let desc_enum_opt = crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &k)?;
                                    println!("TRACE: LetRest: key={:?} desc_enum_opt={:?}", k, desc_enum_opt);
                                    if desc_enum_opt.is_none() {
                                        continue;
                                    }
                                    if !desc_enum_opt.unwrap() {
                                        continue;
                                    }
                                    // Get property value via proxy get trap
                                    println!("TRACE: LetRest: calling proxy_get_property for key={:?}", k);
                                    let val_opt = crate::js_proxy::proxy_get_property(mc, proxy, &k)?;
                                    let v = val_opt.unwrap_or(Value::Undefined);
                                    object_set_key_value(mc, &obj, k.clone(), &v)?;
                                    continue;
                                }
                                if !orig.borrow().is_enumerable(&k) {
                                    continue;
                                }
                                let v = get_property_with_accessors(mc, env, orig, &k)?;
                                object_set_key_value(mc, &obj, k.clone(), &v)?;
                            }
                        }
                        env_set(mc, env, name, &Value::Object(obj))?;
                    }
                }
            }

            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::VarDestructuringObject(pattern, expr) => {
            let val = match evaluate_expr(mc, env, expr) {
                Ok(v) => v,
                Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
            };

            // If RHS is undefined/null, throw a helpful error referencing first property name
            if matches!(val, Value::Undefined) || matches!(val, Value::Null) {
                let prop_name = pattern
                    .iter()
                    .find_map(|p| {
                        if let ObjectDestructuringElement::Property { key, .. } = p {
                            Some(key.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "property".to_string());
                let val = if matches!(val, Value::Null) { "null" } else { "undefined" };
                return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
            }

            // Collect excluded names (and evaluate computed keys) so rest can skip them
            let mut excluded_names: Vec<crate::core::PropertyKey<'gc>> = Vec::new();
            let mut computed_keys: Vec<Option<crate::core::PropertyKey<'gc>>> = vec![None; pattern.len()];

            // DEBUG: print pattern element types and evaluate computed keys
            println!("TRACE: VarDestructuringObject: pattern.len={}", pattern.len());
            for (i, p) in pattern.iter().enumerate() {
                match p {
                    ObjectDestructuringElement::Property { key, .. } => {
                        println!("TRACE: pattern[{}] Property key={}", i, key);
                        excluded_names.push(crate::core::PropertyKey::String(key.clone()));
                    }
                    ObjectDestructuringElement::ComputedProperty { key: key_expr, .. } => {
                        println!("TRACE: pattern[{}] ComputedProperty", i);
                        let key_val = evaluate_expr(mc, env, key_expr)?;
                        let prop_key = match key_val {
                            Value::Symbol(sd) => crate::core::PropertyKey::Symbol(sd),
                            Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                            Value::Number(n) => crate::core::PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                            Value::Object(_) => {
                                let prim = crate::core::to_primitive(mc, &key_val, "string", env)?;
                                match prim {
                                    Value::Symbol(s) => crate::core::PropertyKey::Symbol(s),
                                    Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                                    Value::Number(n) => crate::core::PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                                    other => crate::core::PropertyKey::String(crate::core::value_to_string(&other)),
                                }
                            }
                            other => crate::core::PropertyKey::String(crate::core::value_to_string(&other)),
                        };
                        println!("TRACE: VarDestructuringObject: computed_key[{}] = {:?}", i, prop_key);
                        excluded_names.push(prop_key.clone());
                        computed_keys[i] = Some(prop_key);
                    }
                    ObjectDestructuringElement::Rest(name) => println!("TRACE: pattern[{}] Rest name={}", i, name),
                }
            }

            for (i, prop) in pattern.iter().enumerate() {
                match prop {
                    ObjectDestructuringElement::Property { key, value } => {
                        match value {
                            DestructuringElement::Variable(name, default_expr) => {
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined)
                                    && let Some(def) = default_expr
                                {
                                    prop_val = evaluate_expr(mc, env, def)?;
                                }
                                // Bind var in function scope
                                let mut target_env = *env;
                                while !target_env.borrow().is_function_scope {
                                    if let Some(proto) = target_env.borrow().prototype {
                                        target_env = proto;
                                    } else {
                                        break;
                                    }
                                }
                                env_set_recursive(mc, &target_env, name, &prop_val)?;
                            }
                            DestructuringElement::NestedObject(inner_pattern, inner_default) => {
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined)
                                    && let Some(def) = inner_default
                                {
                                    prop_val = evaluate_expr(mc, env, def)?;
                                }
                                if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                    let prop_name = inner_pattern
                                        .iter()
                                        .find_map(|p| {
                                            if let DestructuringElement::Property(k, _) = p {
                                                Some(k.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_else(|| "property".to_string());
                                    let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                                    return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", prop_name, val)).into());
                                }
                                if let Value::Object(obj2) = &prop_val {
                                    bind_object_inner_for_var(mc, env, inner_pattern, obj2)?;
                                } else {
                                    return Err(raise_eval_error!("Expected object for nested destructuring").into());
                                }
                            }
                            DestructuringElement::NestedArray(inner_array, inner_default) => {
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined)
                                    && let Some(def) = inner_default
                                {
                                    prop_val = evaluate_expr(mc, env, def)?;
                                }
                                if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                    let val = if matches!(prop_val, Value::Null) { "null" } else { "undefined" };
                                    return Err(raise_type_error!(format!("Cannot destructure property '{}' of {}", key, val)).into());
                                }
                                if let Value::Object(oarr) = &prop_val {
                                    bind_array_inner_for_var(mc, env, inner_array, oarr, None, None)?;
                                } else {
                                    return Err(raise_eval_error!("Expected array for nested array destructuring").into());
                                }
                            }
                            _ => {
                                return Err(raise_syntax_error!("Nested object destructuring not implemented").into());
                            }
                        }
                    }
                    ObjectDestructuringElement::ComputedProperty { key: _key_expr, value } => {
                        // Use precomputed key to avoid re-evaluating computed property expr
                        let prop_key = computed_keys[i].as_ref().expect("computed key evaluated").clone();
                        match value {
                            DestructuringElement::Variable(name, default_expr) => {
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, &prop_key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined)
                                    && let Some(def) = default_expr
                                {
                                    prop_val = evaluate_expr(mc, env, def)?;
                                }
                                // Bind var in function scope
                                let mut target_env = *env;
                                while !target_env.borrow().is_function_scope {
                                    if let Some(proto) = target_env.borrow().prototype {
                                        target_env = proto;
                                    } else {
                                        break;
                                    }
                                }
                                env_set_recursive(mc, &target_env, name, &prop_val)?;
                            }
                            _ => {
                                return Err(raise_eval_error!("Computed property value patterns not implemented").into());
                            }
                        }
                    }

                    ObjectDestructuringElement::Rest(name) => {
                        let obj = new_js_object_data(mc);
                        if let Value::Object(orig) = &val {
                            // Use ordinary own property keys to ensure proxies are observed
                            let ordered = crate::core::ordinary_own_property_keys_mc(mc, orig)?;
                            for k in ordered {
                                // Skip if listed in the excluded names (precomputed earlier)
                                if excluded_names.iter().any(|ek| ek == &k) {
                                    continue;
                                }

                                // If this object is a proxy wrapper, delegate descriptor/get to proxy traps
                                if let Some(proxy_cell) = orig.borrow().properties.get(&PropertyKey::String("__proxy__".to_string()))
                                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                                {
                                    // Ask proxy for own property descriptor and check [[Enumerable]]
                                    let desc_enum_opt = crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &k)?;
                                    println!("TRACE: VarRest: key={:?} desc_enum_opt={:?}", k, desc_enum_opt);
                                    if desc_enum_opt.is_none() {
                                        continue;
                                    }
                                    if !desc_enum_opt.unwrap() {
                                        continue;
                                    }
                                    // Get property value via proxy get trap
                                    println!("TRACE: VarRest: calling proxy_get_property for key={:?}", k);
                                    let val_opt = crate::js_proxy::proxy_get_property(mc, proxy, &k)?;
                                    let v = val_opt.unwrap_or(Value::Undefined);
                                    object_set_key_value(mc, &obj, k.clone(), &v)?;
                                    continue;
                                }

                                if !orig.borrow().is_enumerable(&k) {
                                    continue;
                                }
                                let v = get_property_with_accessors(mc, env, orig, &k)?;
                                object_set_key_value(mc, &obj, k.clone(), &v)?;
                            }
                        }
                        // Bind var in function scope
                        let mut target_env = *env;
                        while !target_env.borrow().is_function_scope {
                            if let Some(proto) = target_env.borrow().prototype {
                                target_env = proto;
                            } else {
                                break;
                            }
                        }
                        env_set_recursive(mc, &target_env, name, &Value::Object(obj))?;
                    }
                }
            }

            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Throw(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            if let Value::Object(obj) = val {
                let mut filename = String::new();
                if let Some(val_ptr) = object_get_key_value(env, "__filepath")
                    && let Value::String(s) = &*val_ptr.borrow()
                {
                    filename = utf16_to_utf8(s);
                }
                let mut frame_name = "<anonymous>".to_string();
                if let Some(frame_val) = object_get_key_value(env, "__frame")
                    && let Value::String(s) = &*frame_val.borrow()
                {
                    frame_name = utf16_to_utf8(s);
                }
                let frame = format!("at {} ({}:{}:{})", frame_name, filename, stmt.line, stmt.column);
                let current_stack = obj.borrow().get_property("stack").unwrap_or_default();
                let new_stack = if current_stack.is_empty() {
                    frame.clone()
                } else {
                    format!("{}\n    {}", current_stack, frame)
                };
                obj.borrow_mut(mc)
                    .set_property(mc, "stack", Value::String(utf8_to_utf16(&new_stack)));

                obj.borrow_mut(mc).set_line(stmt.line, mc)?;
                obj.borrow_mut(mc).set_column(stmt.column, mc)?;
            }
            Ok(Some(ControlFlow::Throw(val, Some(stmt.line), Some(stmt.column))))
        }
        StatementKind::Block(stmts) => {
            let stmts_clone = stmts.clone();
            let block_env = new_js_object_data(mc);
            block_env.borrow_mut(mc).prototype = Some(*env);

            // Propagate strictness into block env as an OWN property if any ancestor
            // environment is strict. This ensures closures created inside blocks
            // will correctly inherit strictness even if prototypes are transiently
            // modified (e.g., during indirect eval clearing).
            let mut p = Some(*env);
            while let Some(cur) = p {
                if env_get_strictness(&cur) {
                    env_set_strictness(mc, &block_env, true)?;
                    break;
                }
                p = cur.borrow().prototype;
            }

            let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &block_env, &stmts_clone, labels)?;
            match res {
                ControlFlow::Normal(_) => {
                    *last_value = vbody;
                    Ok(None)
                }
                other => {
                    if matches!(other, ControlFlow::Break(_) | ControlFlow::Continue(_)) && !matches!(vbody, Value::Undefined) {
                        *last_value = vbody;
                    }
                    Ok(Some(other))
                }
            }
        }
        StatementKind::If(if_stmt) => {
            let if_stmt = if_stmt.as_ref();
            let cond_val = evaluate_expr(mc, env, &if_stmt.condition)?;
            let is_true = cond_val.to_truthy();

            if is_true {
                let stmts = if_stmt.then_body.clone();
                let block_env = new_js_object_data(mc);
                block_env.borrow_mut(mc).prototype = Some(*env);
                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &block_env, &stmts, labels)?;
                match res {
                    ControlFlow::Normal(_) => {
                        *last_value = vbody;
                        Ok(None)
                    }
                    other => {
                        if matches!(other, ControlFlow::Break(_) | ControlFlow::Continue(_)) && !matches!(vbody, Value::Undefined) {
                            *last_value = vbody;
                        }
                        Ok(Some(other))
                    }
                }
            } else if let Some(else_stmts) = &if_stmt.else_body {
                let stmts = else_stmts.clone();
                let block_env = new_js_object_data(mc);
                block_env.borrow_mut(mc).prototype = Some(*env);
                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &block_env, &stmts, labels)?;
                match res {
                    ControlFlow::Normal(_) => {
                        *last_value = vbody;
                        Ok(None)
                    }
                    other => {
                        if matches!(other, ControlFlow::Break(_) | ControlFlow::Continue(_)) && !matches!(vbody, Value::Undefined) {
                            *last_value = vbody;
                        }
                        Ok(Some(other))
                    }
                }
            } else {
                Ok(None)
            }
        }
        StatementKind::TryCatch(tc_stmt) => {
            let tc_stmt = tc_stmt.as_ref();
            let try_stmts = tc_stmt.try_body.clone();
            // In generators/async functions, reuse the current env for try bodies so
            // block-scoped bindings survive across yields.
            let in_generator = object_get_key_value(env, "__in_generator").is_some_and(|v| matches!(*v.borrow(), Value::Boolean(true)));
            let try_env = if in_generator {
                *env
            } else {
                let te = crate::core::new_js_object_data(mc);
                te.borrow_mut(mc).prototype = Some(*env);
                te
            };
            let try_res = evaluate_statements_with_context_and_last_value(mc, &try_env, &try_stmts, labels);

            let mut result = match try_res {
                Ok((cf, val)) => {
                    if !matches!(val, Value::Undefined) {
                        *last_value = val;
                    }
                    cf
                }
                Err(e) => match e {
                    EvalError::Js(js_err) => {
                        let val = js_error_to_value(mc, env, &js_err);
                        ControlFlow::Throw(val, js_err.inner.js_line, js_err.inner.js_column)
                    }
                    EvalError::Throw(val, line, column) => ControlFlow::Throw(val, line, column),
                },
            };

            if let ControlFlow::Throw(val, ..) = &result
                && let Some(catch_stmts) = &tc_stmt.catch_body
            {
                // DEBUG: log caught value for tracing failing assert.throws cases
                log::debug!("TryCatch: caught value = {:?}", val);
                if let Value::Object(obj) = val {
                    // Attempt to extract constructor.name for debugging (use prototype chain lookup)
                    let ctor_prop = get_property_with_accessors(mc, env, obj, "constructor").ok();
                    let ctor_name = if let Some(ctor_val) = ctor_prop {
                        match &ctor_val {
                            Value::Object(cobj) => {
                                if let Some(name_val) = object_get_key_value(cobj, "name") {
                                    if let Value::String(s) = &*name_val.borrow() {
                                        Some(crate::unicode::utf16_to_utf8(s))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            }
                            Value::Function(name) => Some(name.clone()),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    log::debug!("TryCatch: caught.constructor.name = {:?}", ctor_name);

                    // Also inspect the thrown object's [[Prototype]]'s constructor.name
                    if let Some(proto_ptr) = obj.borrow().prototype {
                        if let Some(ctor_val) = object_get_key_value(&proto_ptr, "constructor") {
                            match &*ctor_val.borrow() {
                                Value::Object(cobj) => {
                                    if let Some(name_val) = object_get_key_value(cobj, "name")
                                        && let Value::String(s) = &*name_val.borrow()
                                    {
                                        log::debug!("TryCatch: prototype.constructor.name = {}", crate::unicode::utf16_to_utf8(s));
                                    }
                                }
                                Value::Function(name) => {
                                    log::debug!("TryCatch: prototype.constructor.name = {}", name);
                                }
                                _ => {}
                            }
                        } else {
                            log::debug!("TryCatch: prototype has no constructor own-property");
                        }
                    } else {
                        log::debug!("TryCatch: thrown object has no [[Prototype]]");
                    }
                }

                // Create new scope for catch
                let catch_env = crate::core::new_js_object_data(mc);
                catch_env.borrow_mut(mc).prototype = Some(*env);

                if let Some(param_name) = &tc_stmt.catch_param {
                    env_set(mc, &catch_env, param_name, val)?;
                }

                let catch_stmts_clone = catch_stmts.clone();
                let catch_res = evaluate_statements_with_context_and_last_value(mc, &catch_env, &catch_stmts_clone, labels);
                match catch_res {
                    Ok((cf, val)) => {
                        // Catch executed.
                        if !matches!(val, Value::Undefined) {
                            *last_value = val;
                        }
                        result = cf
                    }
                    Err(e) => match e {
                        EvalError::Js(js_err) => {
                            let val = js_error_to_value(mc, env, &js_err);
                            result = ControlFlow::Throw(val, js_err.inner.js_line, js_err.inner.js_column);
                        }
                        EvalError::Throw(val, line, column) => result = ControlFlow::Throw(val, line, column),
                    },
                }
            }

            if let Some(finally_stmts) = &tc_stmt.finally_body {
                let finally_stmts_clone = finally_stmts.clone();
                // Evaluate finally body in its own block environment to ensure any
                // block-scoped declarations are properly localized.
                let finally_env = crate::core::new_js_object_data(mc);
                finally_env.borrow_mut(mc).prototype = Some(*env);
                let finally_res = evaluate_statements_with_context_and_last_value(mc, &finally_env, &finally_stmts_clone, labels);
                match finally_res {
                    Ok((other, val)) => {
                        match other {
                            ControlFlow::Normal(_) => {
                                // Normal completion of finally -> ignore value, keep result
                            }
                            _ => {
                                // Abrupt completion -> override result
                                // If break/continue, we might need value
                                if matches!(other, ControlFlow::Break(_) | ControlFlow::Continue(_)) && !matches!(val, Value::Undefined) {
                                    *last_value = val;
                                }
                                result = other;
                            }
                        }
                    }
                    Err(e) => match e {
                        EvalError::Js(js_err) => {
                            let val = js_error_to_value(mc, env, &js_err);
                            result = ControlFlow::Throw(val, js_err.inner.js_line, js_err.inner.js_column);
                        }
                        EvalError::Throw(val, line, column) => result = ControlFlow::Throw(val, line, column),
                    },
                }
            }

            match result {
                ControlFlow::Normal(val) => {
                    *last_value = val;
                    Ok(None)
                }
                other => Ok(Some(other)),
            }
        }
        StatementKind::Label(label, stmt) => {
            let stmts = vec![*stmt.clone()];
            // If inner is a loop or label, pass current label down
            let new_labels = match *stmt.kind {
                StatementKind::For(..)
                | StatementKind::ForIn(..)
                | StatementKind::ForOf(..)
                | StatementKind::ForAwaitOf(..)
                | StatementKind::While(..)
                | StatementKind::DoWhile(..)
                | StatementKind::Label(..) => {
                    let mut l = labels.to_vec();
                    l.push(label.clone());
                    l
                }
                _ => Vec::new(),
            };
            let effective_labels = if new_labels.is_empty() { labels } else { &new_labels };
            let mut new_own = own_labels.to_vec();
            new_own.push(label.clone());
            let (res, vbody) = evaluate_statements_with_labels_and_last(mc, env, &stmts, effective_labels, &new_own)?;
            match res {
                ControlFlow::Normal(_) => {
                    *last_value = vbody;
                    Ok(None)
                }
                ControlFlow::Break(Some(ref l)) if l == label => {
                    if !matches!(vbody, Value::Undefined) {
                        *last_value = vbody;
                    }
                    Ok(None)
                }
                other => {
                    if matches!(other, ControlFlow::Break(_) | ControlFlow::Continue(_)) && !matches!(vbody, Value::Undefined) {
                        *last_value = vbody;
                    }
                    Ok(Some(other))
                }
            }
        }
        StatementKind::Break(label) => Ok(Some(ControlFlow::Break(label.clone()))),
        StatementKind::Continue(label) => Ok(Some(ControlFlow::Continue(label.clone()))),
        StatementKind::Debugger => Ok(None),
        StatementKind::For(for_stmt) => {
            let for_stmt = for_stmt.as_ref();

            let mut use_lexical_env = if let Some(init_stmt) = &for_stmt.init {
                matches!(
                    &*init_stmt.kind,
                    StatementKind::Let(_)
                        | StatementKind::Const(_)
                        | StatementKind::LetDestructuringArray(..)
                        | StatementKind::ConstDestructuringArray(..)
                        | StatementKind::LetDestructuringObject(..)
                        | StatementKind::ConstDestructuringObject(..)
                )
            } else {
                false
            };

            if !use_lexical_env && body_has_lexical(&for_stmt.body) {
                use_lexical_env = true;
            }

            if let Some(flag) = crate::core::object_get_key_value(env, "__in_generator")
                && matches!(*flag.borrow(), Value::Boolean(true))
            {
                use_lexical_env = false;
            }

            let loop_env = if use_lexical_env {
                let le = new_js_object_data(mc);
                le.borrow_mut(mc).prototype = Some(*env);
                le
            } else {
                *env
            };

            if let Some(init_stmt) = &for_stmt.init {
                evaluate_statements_with_context(mc, &loop_env, std::slice::from_ref(init_stmt), labels)?;
            }
            // The `for` statement's completion value is `undefined` if the loop
            // is not entered. Reset `last_value` after the init so it doesn't
            // retain the init's result when the test is false and the loop
            // body is never evaluated.
            *last_value = Value::Undefined;
            loop {
                if let Some(test_expr) = &for_stmt.test {
                    let cond_val = evaluate_expr(mc, &loop_env, test_expr)?;
                    let is_true = cond_val.to_truthy();
                    if !is_true {
                        break;
                    }
                }
                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &loop_env, &for_stmt.body, labels)?;
                match res {
                    ControlFlow::Normal(_) => *last_value = vbody,
                    ControlFlow::Break(label) => {
                        if !matches!(&vbody, Value::Undefined) {
                            *last_value = vbody;
                        }
                        if label.is_none() {
                            break;
                        }
                        // If break has label, check if it matches us? No, breaks targets are handled by Label stmt or loop.
                        // But loops can be targets of breaks too if labeled.
                        // However, Label stmt handles breaks. Loops only handle unlabeled breaks (implicit break of current loop).
                        // If we have label, we pass it up to Label stmt.

                        // Wait, if I have `L: while` and `break L`, the Label stmt handles it.
                        // So loop just returns Break(L).
                        return Ok(Some(ControlFlow::Break(label)));
                    }
                    ControlFlow::Continue(label) => {
                        if !matches!(&vbody, Value::Undefined) {
                            *last_value = vbody;
                        }
                        if let Some(ref l) = label {
                            if own_labels.contains(l) {
                                // Match! Continue this loop.
                            } else {
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                        // Continue loop (either unlabeled or matched label)
                    }
                    ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                    ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                }
                if let Some(update_stmt) = &for_stmt.update {
                    evaluate_statements_with_context(mc, &loop_env, std::slice::from_ref(update_stmt), labels)?;
                }
            }
            Ok(None)
        }
        StatementKind::While(cond, body) => {
            let loop_env = new_js_object_data(mc);
            loop_env.borrow_mut(mc).prototype = Some(*env);
            // Per ECMAScript completion semantics, the loop's completion value is
            // `undefined` if the loop is not entered or if it completes abruptly
            // with no value (e.g., `break;`). Ensure we start with `undefined` so
            // previous statement results don't leak through.
            *last_value = Value::Undefined;
            loop {
                let cond_val = evaluate_expr(mc, &loop_env, cond)?;
                let is_true = cond_val.to_truthy();
                if !is_true {
                    break;
                }
                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &loop_env, body, labels)?;
                match res {
                    ControlFlow::Normal(_) => *last_value = vbody,
                    ControlFlow::Break(label) => {
                        if label.is_none() {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            break;
                        }
                        if !matches!(&vbody, Value::Undefined) {
                            *last_value = vbody;
                        }
                        return Ok(Some(ControlFlow::Break(label)));
                    }
                    ControlFlow::Continue(label) => {
                        if !matches!(&vbody, Value::Undefined) {
                            *last_value = vbody;
                        }
                        if let Some(ref l) = label {
                            if own_labels.contains(l) {
                                // Match! Continue this loop.
                            } else {
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                    }
                    ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                    ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                }
            }
            Ok(None)
        }
        StatementKind::DoWhile(body, cond) => {
            let loop_env = new_js_object_data(mc);
            loop_env.borrow_mut(mc).prototype = Some(*env);
            // As with `while`, initialize the completion value to `undefined` to
            // avoid leaking prior values when the body completes abruptly with
            // no value (e.g., `break;`).
            *last_value = Value::Undefined;
            loop {
                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &loop_env, body, labels)?;
                match res {
                    ControlFlow::Normal(_) => *last_value = vbody,
                    ControlFlow::Break(label) => {
                        if label.is_none() {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            break;
                        }
                        if !matches!(&vbody, Value::Undefined) {
                            *last_value = vbody;
                        }
                        return Ok(Some(ControlFlow::Break(label)));
                    }
                    ControlFlow::Continue(label) => {
                        if !matches!(&vbody, Value::Undefined) {
                            *last_value = vbody;
                        }
                        if let Some(ref l) = label {
                            if own_labels.contains(l) {
                                // Match! Continue this loop.
                            } else {
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                    }
                    ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                    ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                }
                let cond_val = evaluate_expr(mc, &loop_env, cond)?;
                let is_true = cond_val.to_truthy();
                if !is_true {
                    break;
                }
            }
            Ok(None)
        }
        StatementKind::With(obj_expr, body) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let with_env = new_js_object_data(mc);
            with_env.borrow_mut(mc).prototype = Some(*env);

            // Copy own properties of the object into the with-environment so they
            // shadow outer bindings during the evaluation of the body.
            if let Value::Object(o) = obj_val {
                for (k, v) in o.borrow().properties.iter() {
                    if let crate::core::PropertyKey::String(s) = k {
                        object_set_key_value(mc, &with_env, s, &v.borrow())?;
                    }
                }
            }

            let res = evaluate_statements_with_context(mc, &with_env, body, labels)?;
            match res {
                ControlFlow::Normal(v) => *last_value = v,
                ControlFlow::Break(label) => return Ok(Some(ControlFlow::Break(label))),
                ControlFlow::Continue(label) => return Ok(Some(ControlFlow::Continue(label))),
                ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
            }
            Ok(None)
        }
        StatementKind::ForOf(decl_kind_opt, var_name, iterable, body) => {
            // Hoist var declaration if necessary
            if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                let is_indirect_eval = crate::core::object_get_key_value(env, "__is_indirect_eval").is_some();
                hoist_name(mc, env, var_name, is_indirect_eval)?;
            }

            // If this is a lexical (let/const) declaration, create a head lexical environment
            // which contains an uninitialized binding (TDZ) for the loop variable. The
            // iterable expression is evaluated with this head env to ensure TDZ accesses
            // throw ReferenceError as per spec.
            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                env_set(mc, &he, var_name, &Value::Uninitialized)?;
                head_env = Some(he);
            }
            let iter_eval_env = head_env.as_ref().unwrap_or(env);
            let iter_val = evaluate_expr(mc, iter_eval_env, iterable)?;
            let mut iterator = None;

            // Try to use Symbol.iterator
            if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
                && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
            {
                let method = if let Value::Object(obj) = &iter_val {
                    if let Some(c) = object_get_key_value(obj, iter_sym_data) {
                        c.borrow().clone()
                    } else {
                        Value::Undefined
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &iter_val, iter_sym_data)?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, &method, Some(&iter_val), &[])?;

                    if let Value::Object(iter_obj) = res {
                        iterator = Some(iter_obj);
                    }
                }
            }

            if let Some(iter_obj) = iterator {
                // V is the last normal completion value per spec ForIn/OfBodyEvaluation
                let mut v = Value::Undefined;
                loop {
                    let next_method = object_get_key_value(&iter_obj, "next")
                        .ok_or(EvalError::Js(raise_type_error!("Iterator has no next method")))?
                        .borrow()
                        .clone();

                    let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;

                    if let Value::Object(next_res) = next_res_val {
                        let done = if let Some(done_val) = object_get_key_value(&next_res, "done") {
                            match &*done_val.borrow() {
                                Value::Boolean(b) => *b,
                                _ => false,
                            }
                        } else {
                            false
                        };

                        if done {
                            break;
                        }

                        let value = if let Some(val) = object_get_key_value(&next_res, "value") {
                            val.borrow().clone()
                        } else {
                            Value::Undefined
                        };

                        match decl_kind_opt {
                            Some(crate::core::VarDeclKind::Var) | None => {
                                // var or assignment form: update existing binding
                                env_set_recursive(mc, env, var_name, &value)?;
                                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, env, body, labels)?;
                                match res {
                                    ControlFlow::Normal(_) => v = vbody.clone(),
                                    ControlFlow::Break(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            v = vbody.clone();
                                        }
                                        *last_value = v.clone();
                                        if label.is_none() {
                                            break;
                                        }
                                        return Ok(Some(ControlFlow::Break(label)));
                                    }
                                    ControlFlow::Continue(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            v = vbody.clone();
                                        }
                                        if let Some(ref l) = label
                                            && !own_labels.contains(l)
                                        {
                                            *last_value = v.clone();
                                            return Ok(Some(ControlFlow::Continue(label)));
                                        }
                                    }
                                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                    ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                                }
                            }
                            Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                // create a fresh lexical env for each iteration
                                let iter_env = new_js_object_data(mc);
                                iter_env.borrow_mut(mc).prototype = Some(*env);
                                env_set(mc, &iter_env, var_name, &value)?;
                                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?;
                                match res {
                                    ControlFlow::Normal(_) => v = vbody.clone(),
                                    ControlFlow::Break(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            v = vbody.clone();
                                        }
                                        *last_value = v.clone();
                                        if label.is_none() {
                                            break;
                                        }
                                        return Ok(Some(ControlFlow::Break(label)));
                                    }
                                    ControlFlow::Continue(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            v = vbody.clone();
                                        }
                                        if let Some(ref l) = label
                                            && !own_labels.contains(l)
                                        {
                                            *last_value = v.clone();
                                            return Ok(Some(ControlFlow::Continue(label)));
                                        }
                                    }
                                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                    ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                                }
                            }
                        }
                    } else {
                        return Err(raise_type_error!("Iterator result is not an object").into());
                    }
                }
                *last_value = v.clone();
                return Ok(None);
            }

            if let Value::Object(obj) = iter_val
                && is_array(mc, &obj)
            {
                let len_val = object_get_key_value(&obj, "length").unwrap().borrow().clone();
                let len = match len_val {
                    Value::Number(n) => n as usize,
                    _ => 0,
                };
                // Track last normal completion value (V)
                let mut v = Value::Undefined;
                for i in 0..len {
                    let val = get_property_with_accessors(mc, env, &obj, i)?;
                    match decl_kind_opt {
                        Some(crate::core::VarDeclKind::Var) | None => {
                            crate::core::env_set_recursive(mc, env, var_name, &val)?;
                            let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, env, body, labels)?;
                            match res {
                                ControlFlow::Normal(_) => v = vbody.clone(),
                                ControlFlow::Break(label) => {
                                    if !matches!(&vbody, Value::Undefined) {
                                        v = vbody.clone();
                                    }
                                    *last_value = v.clone();
                                    if label.is_none() {
                                        break;
                                    }
                                    return Ok(Some(ControlFlow::Break(label)));
                                }
                                ControlFlow::Continue(label) => {
                                    if !matches!(&vbody, Value::Undefined) {
                                        v = vbody.clone();
                                    }
                                    if let Some(ref l) = label
                                        && !own_labels.contains(l)
                                    {
                                        *last_value = v.clone();
                                        return Ok(Some(ControlFlow::Continue(label)));
                                    }
                                }
                                ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                            }
                        }
                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                            let iter_env = new_js_object_data(mc);
                            iter_env.borrow_mut(mc).prototype = Some(*env);
                            env_set(mc, &iter_env, var_name, &val)?;
                            let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?;
                            match res {
                                ControlFlow::Normal(_) => v = vbody.clone(),
                                ControlFlow::Break(label) => {
                                    if !matches!(&vbody, Value::Undefined) {
                                        v = vbody.clone();
                                    }
                                    *last_value = v.clone();
                                    if label.is_none() {
                                        break;
                                    }
                                    return Ok(Some(ControlFlow::Break(label)));
                                }
                                ControlFlow::Continue(label) => {
                                    if !matches!(&vbody, Value::Undefined) {
                                        v = vbody.clone();
                                    }
                                    if let Some(ref l) = label
                                        && !own_labels.contains(l)
                                    {
                                        *last_value = v.clone();
                                        return Ok(Some(ControlFlow::Continue(label)));
                                    }
                                }
                                ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                            }
                        }
                    }
                }
                *last_value = v.clone();
                return Ok(None);
            }

            Err(raise_type_error!("Value is not iterable").into())
        }
        StatementKind::ForAwaitOf(decl_kind_opt, var_name, iterable, body) => {
            if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                let is_indirect_eval = crate::core::object_get_key_value(env, "__is_indirect_eval").is_some();
                hoist_name(mc, env, var_name, is_indirect_eval)?;
            }

            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                env_set(mc, &he, var_name, &Value::Uninitialized)?;
                head_env = Some(he);
            }
            let iter_eval_env = head_env.as_ref().unwrap_or(env);
            let iter_val = evaluate_expr(mc, iter_eval_env, iterable)?;

            let mut iterator: Option<JSObjectDataPtr<'gc>> = None;
            let mut is_async_iter = false;

            if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(async_iter_sym) = object_get_key_value(sym_obj, "asyncIterator")
                && let Value::Symbol(async_iter_sym_data) = &*async_iter_sym.borrow()
            {
                let method = if let Value::Object(obj) = &iter_val {
                    if let Some(c) = object_get_key_value(obj, async_iter_sym_data) {
                        c.borrow().clone()
                    } else {
                        Value::Undefined
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &iter_val, async_iter_sym_data)?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, &method, Some(&iter_val), &[])?;
                    let res = await_promise_value(mc, env, &res)?;
                    if let Value::Object(iter_obj) = res {
                        iterator = Some(iter_obj);
                        is_async_iter = true;
                    }
                }
            }

            if iterator.is_none()
                && let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
                && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
            {
                let method = if let Value::Object(obj) = &iter_val {
                    if let Some(c) = object_get_key_value(obj, iter_sym_data) {
                        c.borrow().clone()
                    } else {
                        Value::Undefined
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &iter_val, iter_sym_data)?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, &method, Some(&iter_val), &[])?;
                    if let Value::Object(iter_obj) = res {
                        iterator = Some(iter_obj);
                        is_async_iter = false;
                    }
                }
            }

            if let Some(iter_obj) = iterator {
                let mut v = Value::Undefined;
                loop {
                    let next_method = object_get_key_value(&iter_obj, "next")
                        .ok_or(EvalError::Js(raise_type_error!("Iterator has no next method")))?
                        .borrow()
                        .clone();

                    let mut next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                    if is_async_iter {
                        next_res_val = await_promise_value(mc, env, &next_res_val)?;
                    }

                    if let Value::Object(next_res) = next_res_val {
                        let done = if let Some(done_val) = object_get_key_value(&next_res, "done") {
                            match &*done_val.borrow() {
                                Value::Boolean(b) => *b,
                                _ => false,
                            }
                        } else {
                            false
                        };

                        if done {
                            break;
                        }

                        let mut value = if let Some(val) = object_get_key_value(&next_res, "value") {
                            val.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        value = await_promise_value(mc, env, &value)?;

                        match decl_kind_opt {
                            Some(crate::core::VarDeclKind::Var) | None => {
                                crate::core::env_set_recursive(mc, env, var_name, &value)?;
                                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, env, body, labels)?;
                                match res {
                                    ControlFlow::Normal(_) => v = vbody.clone(),
                                    ControlFlow::Break(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            v = vbody.clone();
                                        }
                                        *last_value = v.clone();
                                        if label.is_none() {
                                            break;
                                        }
                                        return Ok(Some(ControlFlow::Break(label)));
                                    }
                                    ControlFlow::Continue(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            v = vbody.clone();
                                        }
                                        if let Some(ref l) = label
                                            && !own_labels.contains(l)
                                        {
                                            *last_value = v.clone();
                                            return Ok(Some(ControlFlow::Continue(label)));
                                        }
                                    }
                                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                    ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                                }
                            }
                            Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                let iter_env = new_js_object_data(mc);
                                iter_env.borrow_mut(mc).prototype = Some(*env);
                                env_set(mc, &iter_env, var_name, &value)?;
                                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?;
                                match res {
                                    ControlFlow::Normal(_) => v = vbody.clone(),
                                    ControlFlow::Break(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            v = vbody.clone();
                                        }
                                        *last_value = v.clone();
                                        if label.is_none() {
                                            break;
                                        }
                                        return Ok(Some(ControlFlow::Break(label)));
                                    }
                                    ControlFlow::Continue(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            v = vbody.clone();
                                        }
                                        if let Some(ref l) = label
                                            && !own_labels.contains(l)
                                        {
                                            *last_value = v.clone();
                                            return Ok(Some(ControlFlow::Continue(label)));
                                        }
                                    }
                                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                    ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                                }
                            }
                        }
                    } else {
                        return Err(raise_type_error!("Iterator result is not an object").into());
                    }
                }
                *last_value = v.clone();
                return Ok(None);
            }

            Err(raise_type_error!("Value is not iterable").into())
        }
        StatementKind::ForAwaitOfExpr(lhs, iterable, body) => {
            let iter_val = evaluate_expr(mc, env, iterable)?;
            let mut iterator: Option<JSObjectDataPtr<'gc>> = None;
            let mut is_async_iter = false;

            if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(async_iter_sym) = object_get_key_value(sym_obj, "asyncIterator")
                && let Value::Symbol(async_iter_sym_data) = &*async_iter_sym.borrow()
            {
                let method = if let Value::Object(obj) = &iter_val {
                    if let Some(c) = object_get_key_value(obj, async_iter_sym_data) {
                        c.borrow().clone()
                    } else {
                        Value::Undefined
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &iter_val, async_iter_sym_data)?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, &method, Some(&iter_val), &[])?;
                    let res = await_promise_value(mc, env, &res)?;
                    if let Value::Object(iter_obj) = res {
                        iterator = Some(iter_obj);
                        is_async_iter = true;
                    }
                }
            }

            if iterator.is_none()
                && let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
                && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
            {
                let method = if let Value::Object(obj) = &iter_val {
                    if let Some(c) = object_get_key_value(obj, iter_sym_data) {
                        c.borrow().clone()
                    } else {
                        Value::Undefined
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &iter_val, iter_sym_data)?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, &method, Some(&iter_val), &[])?;
                    if let Value::Object(iter_obj) = res {
                        iterator = Some(iter_obj);
                        is_async_iter = false;
                    }
                }
            }

            if let Some(iter_obj) = iterator {
                let mut v = Value::Undefined;
                loop {
                    let next_method = object_get_key_value(&iter_obj, "next")
                        .ok_or(EvalError::Js(raise_type_error!("Iterator has no next method")))?
                        .borrow()
                        .clone();

                    let mut next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                    if is_async_iter {
                        next_res_val = await_promise_value(mc, env, &next_res_val)?;
                    }

                    if let Value::Object(next_res) = next_res_val {
                        let done = if let Some(done_val) = object_get_key_value(&next_res, "done") {
                            match &*done_val.borrow() {
                                Value::Boolean(b) => *b,
                                _ => false,
                            }
                        } else {
                            false
                        };

                        if done {
                            break;
                        }

                        let mut value = if let Some(val) = object_get_key_value(&next_res, "value") {
                            val.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        value = await_promise_value(mc, env, &value)?;

                        evaluate_assign_target_with_value(mc, env, lhs, &value)?;
                        let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, env, body, labels)?;
                        match res {
                            ControlFlow::Normal(_) => v = vbody.clone(),
                            ControlFlow::Break(label) => {
                                if !matches!(&vbody, Value::Undefined) {
                                    v = vbody.clone();
                                }
                                *last_value = v.clone();
                                if label.is_none() {
                                    break;
                                }
                                return Ok(Some(ControlFlow::Break(label)));
                            }
                            ControlFlow::Continue(label) => {
                                if !matches!(&vbody, Value::Undefined) {
                                    v = vbody.clone();
                                }
                                if let Some(ref l) = label
                                    && !own_labels.contains(l)
                                {
                                    *last_value = v.clone();
                                    return Ok(Some(ControlFlow::Continue(label)));
                                }
                            }
                            ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                            ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                        }
                    } else {
                        return Err(raise_type_error!("Iterator result is not an object").into());
                    }
                }
                *last_value = v.clone();
                return Ok(None);
            }

            Err(raise_type_error!("Value is not iterable").into())
        }
        StatementKind::ForOfExpr(lhs, iterable, body) => {
            let iter_val = evaluate_expr(mc, env, iterable)?;
            let mut iterator = None;

            // Try to use Symbol.iterator
            if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
                && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
            {
                let method = if let Value::Object(obj) = &iter_val {
                    if let Some(c) = object_get_key_value(obj, iter_sym_data) {
                        c.borrow().clone()
                    } else {
                        Value::Undefined
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &iter_val, iter_sym_data)?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, &method, Some(&iter_val), &[])?;

                    if let Value::Object(iter_obj) = res {
                        iterator = Some(iter_obj);
                    }
                }
            }

            if let Some(iter_obj) = iterator {
                // V is the last normal completion value per spec ForIn/OfBodyEvaluation
                let mut v = Value::Undefined;
                loop {
                    let next_method = object_get_key_value(&iter_obj, "next")
                        .ok_or(EvalError::Js(raise_type_error!("Iterator has no next method")))?
                        .borrow()
                        .clone();

                    let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;

                    if let Value::Object(next_res) = next_res_val {
                        let done = if let Some(done_val) = object_get_key_value(&next_res, "done") {
                            match &*done_val.borrow() {
                                Value::Boolean(b) => *b,
                                _ => false,
                            }
                        } else {
                            false
                        };

                        if done {
                            break;
                        }

                        let value = if let Some(val) = object_get_key_value(&next_res, "value") {
                            val.borrow().clone()
                        } else {
                            Value::Undefined
                        };

                        // Assignment form: assign to lhs expression
                        evaluate_assign_target_with_value(mc, env, lhs, &value)?;
                        let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, env, body, labels)?;
                        match res {
                            ControlFlow::Normal(_) => v = vbody.clone(),
                            ControlFlow::Break(label) => {
                                if !matches!(&vbody, Value::Undefined) {
                                    v = vbody.clone();
                                }
                                *last_value = v.clone();
                                if label.is_none() {
                                    break;
                                }
                                return Ok(Some(ControlFlow::Break(label)));
                            }
                            ControlFlow::Continue(label) => {
                                if !matches!(&vbody, Value::Undefined) {
                                    v = vbody.clone();
                                }
                                if let Some(ref l) = label
                                    && !own_labels.contains(l)
                                {
                                    *last_value = v.clone();
                                    return Ok(Some(ControlFlow::Continue(label)));
                                }
                            }
                            ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                            ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                        }
                    } else {
                        return Err(raise_type_error!("Iterator result is not an object").into());
                    }
                }
                *last_value = v.clone();
                return Ok(None);
            }

            if let Value::Object(obj) = iter_val
                && is_array(mc, &obj)
            {
                let len_val = object_get_key_value(&obj, "length").unwrap().borrow().clone();
                let len = match len_val {
                    Value::Number(n) => n as usize,
                    _ => 0,
                };
                let mut v = Value::Undefined;
                for i in 0..len {
                    let val = get_property_with_accessors(mc, env, &obj, i)?;
                    // Assignment form: assign to lhs expression
                    evaluate_assign_target_with_value(mc, env, lhs, &val)?;
                    let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, env, body, labels)?;
                    match res {
                        ControlFlow::Normal(_) => v = vbody.clone(),
                        ControlFlow::Break(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                v = vbody.clone();
                            }
                            *last_value = v.clone();
                            if label.is_none() {
                                break;
                            }
                            return Ok(Some(ControlFlow::Break(label)));
                        }
                        ControlFlow::Continue(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                v = vbody.clone();
                            }
                            if let Some(ref l) = label
                                && !own_labels.contains(l)
                            {
                                *last_value = v.clone();
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                        ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                        ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                    }
                }
                *last_value = v.clone();
                return Ok(None);
            }

            Err(raise_type_error!("Value is not iterable").into())
        }
        StatementKind::ForIn(decl_kind, var_name, iterable, body) => {
            // If this is a lexical (let/const) declaration, create a head env with
            // TDZ binding for the loop variable so that iterable evaluation will see the
            // binding as uninitialized and throw ReferenceError on access.
            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                env_set(mc, &he, var_name, &Value::Uninitialized)?;
                head_env = Some(he);
            }
            let iter_eval_env = head_env.as_ref().unwrap_or(env);

            let iter_val = evaluate_expr(mc, iter_eval_env, iterable)?;

            // If the evaluated expression is null or undefined, the iteration is skipped per spec
            // and the completion value of the ForIn statement is empty (represented as `undefined`).
            if matches!(iter_val, Value::Undefined | Value::Null) {
                *last_value = Value::Undefined;
                return Ok(None);
            }

            // If the for-in uses `var`, ensure the variable is hoisted to function scope
            if let Some(crate::core::VarDeclKind::Var) = decl_kind {
                let is_indirect_eval = crate::core::object_get_key_value(env, "__is_indirect_eval").is_some();
                hoist_name(mc, env, var_name, is_indirect_eval)?;
            }

            if let Value::Object(obj) = iter_val {
                // Collect enumerable string keys across prototype chain using "ordinary own property keys"
                // semantics so that array index keys (numeric) are ordered first, then other
                // string keys in insertion order.
                let mut keys: Vec<String> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut current = Some(obj);
                while let Some(o) = current {
                    // Obtain the object's own property keys in ordinary own property keys order
                    // (array index keys sorted numerically, followed by other string keys,
                    // then symbol keys). This ensures per-object ordering matches the spec.
                    let own_keys = crate::core::ordinary_own_property_keys_mc(mc, &o)?;
                    for key in own_keys.iter() {
                        if let PropertyKey::String(s) = key {
                            if s == "length" {
                                continue;
                            }
                            // Record that this name exists on the chain so that it
                            // shadows properties further down the prototype chain,
                            // even if the current property is non-enumerable.
                            if !seen.contains(s) {
                                seen.insert(s.clone());
                                if o.borrow().is_enumerable(key) {
                                    keys.push(s.clone());
                                }
                            }
                        }
                    }
                    current = o.borrow().prototype;
                }

                log::trace!("for-in keys: {keys:?}");

                // Per spec, the iteration completion value V starts as undefined
                *last_value = Value::Undefined;

                match decl_kind {
                    // `var` declaration or assignment form: single binding in the surrounding
                    // scope (function/global). Update that binding each iteration and run body
                    // in the same environment (`env`).
                    Some(crate::core::VarDeclKind::Var) | None => {
                        for k in &keys {
                            // Skip keys that were deleted (or otherwise no longer exist).
                            // `object_get_key_value` only looks at the property map, but TypedArray
                            // indexed elements are conceptual own properties (0..length-1)
                            // and may not be materialized. Handle that specially here so
                            // for-in will iterate over typed array indices even when they
                            // aren't present in the properties map.
                            let mut key_present = false;
                            if let Some(val_rc) = object_get_key_value(&obj, k) {
                                log::trace!("for-in property {k} -> {}", value_to_string(&val_rc.borrow()));
                                key_present = true;
                            } else {
                                log::trace!("for-in missing property {k}, checking typedarray indices...");
                                // Check for a TypedArray element for numeric index keys.
                                if let Ok(idx) = k.parse::<usize>() {
                                    log::trace!("for-in numeric key parsed: {idx}");
                                    if let Some(ta_cell) = obj.borrow().properties.get(&PropertyKey::String("__typedarray".to_string())) {
                                        log::trace!("for-in object has __typedarray marker");
                                        if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                                            let cur_len = if ta.length_tracking {
                                                let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                                                if buf_len <= ta.byte_offset {
                                                    0
                                                } else {
                                                    (buf_len - ta.byte_offset) / ta.element_size()
                                                }
                                            } else {
                                                ta.length
                                            };
                                            if idx < cur_len {
                                                match ta.get(idx) {
                                                    Ok(num) => log::trace!("for-in property {k} -> {}", num),
                                                    Err(_) => log::trace!("for-in property {k} -> <typedarray element error>"),
                                                }
                                                key_present = true;
                                            }
                                        }
                                    }
                                }
                            }
                            if !key_present {
                                continue;
                            }

                            env_set_recursive(mc, env, var_name, &Value::String(utf8_to_utf16(k)))?;
                            let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, env, body, labels)?;
                            match res {
                                ControlFlow::Normal(_) => *last_value = vbody,
                                ControlFlow::Break(label) => {
                                    if !matches!(&vbody, Value::Undefined) {
                                        *last_value = vbody;
                                    }
                                    if label.is_none() {
                                        break;
                                    }
                                    return Ok(Some(ControlFlow::Break(label)));
                                }
                                ControlFlow::Continue(label) => {
                                    if !matches!(&vbody, Value::Undefined) {
                                        *last_value = vbody;
                                    }
                                    if let Some(ref l) = label
                                        && !own_labels.contains(l)
                                    {
                                        return Ok(Some(ControlFlow::Continue(label)));
                                    }
                                }
                                ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                                ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                            }
                        }
                        return Ok(None);
                    }
                    // `let` / `const` declaration: create a fresh lexical environment for
                    // each iteration so closures capture a distinct binding per iteration.
                    Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                        for k in &keys {
                            // Skip keys that were deleted (or otherwise no longer exist)
                            let mut key_present = false;
                            if let Some(val_rc) = object_get_key_value(&obj, k) {
                                log::trace!("for-in property {k} -> {}", value_to_string(&val_rc.borrow()));
                                key_present = true;
                            } else {
                                // Check for a TypedArray element for numeric index keys.
                                if let Ok(idx) = k.parse::<usize>()
                                    && let Some(ta_cell) = obj.borrow().properties.get(&PropertyKey::String("__typedarray".to_string()))
                                    && let Value::TypedArray(ta) = &*ta_cell.borrow()
                                {
                                    // Compute current length for length-tracking views
                                    let cur_len = if ta.length_tracking {
                                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                                        if buf_len <= ta.byte_offset {
                                            0
                                        } else {
                                            (buf_len - ta.byte_offset) / ta.element_size()
                                        }
                                    } else {
                                        ta.length
                                    };
                                    if idx < cur_len {
                                        match ta.get(idx) {
                                            Ok(num) => log::trace!("for-in property {k} -> {num}"),
                                            Err(_) => log::trace!("for-in property {k} -> <typedarray element error>"),
                                        }
                                        key_present = true;
                                    }
                                }
                            }
                            if !key_present {
                                continue;
                            }

                            let iter_env = new_js_object_data(mc);
                            iter_env.borrow_mut(mc).prototype = Some(*env);
                            env_set(mc, &iter_env, var_name, &Value::String(utf8_to_utf16(k)))?;
                            let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?;
                            match res {
                                ControlFlow::Normal(_) => *last_value = vbody,
                                ControlFlow::Break(label) => {
                                    if !matches!(&vbody, Value::Undefined) {
                                        *last_value = vbody;
                                    }
                                    if label.is_none() {
                                        break;
                                    }
                                    return Ok(Some(ControlFlow::Break(label)));
                                }
                                ControlFlow::Continue(label) => {
                                    if !matches!(&vbody, Value::Undefined) {
                                        *last_value = vbody;
                                    }
                                    if let Some(ref l) = label
                                        && !own_labels.contains(l)
                                    {
                                        return Ok(Some(ControlFlow::Continue(label)));
                                    }
                                }
                                ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                                ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                            }
                        }
                        return Ok(None);
                    }
                }
            }
            Ok(None)
        }
        StatementKind::ForInExpr(lhs, iterable, body) => {
            let iter_val = evaluate_expr(mc, env, iterable)?;

            // If the evaluated expression is null or undefined, the iteration is skipped per spec
            // and the completion value of the ForIn statement is empty (represented as `undefined`).
            if matches!(iter_val, Value::Undefined | Value::Null) {
                *last_value = Value::Undefined;
                return Ok(None);
            }

            if let Value::Object(obj) = iter_val {
                // Collect enumerable string keys across prototype chain in insertion order
                let mut keys: Vec<String> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut current = Some(obj);
                while let Some(o) = current {
                    for (key, _val) in o.borrow().properties.iter() {
                        if let PropertyKey::String(s) = key {
                            if s == "length" {
                                continue;
                            }
                            if !seen.contains(s) {
                                seen.insert(s.clone());
                                if o.borrow().is_enumerable(key) {
                                    keys.push(s.clone());
                                }
                            }
                        }
                    }
                    current = o.borrow().prototype;
                }

                log::trace!("for-in expr keys: {keys:?}");

                // Per spec, the iteration completion value V starts as undefined
                *last_value = Value::Undefined;

                let mut v = Value::Undefined;
                for k in &keys {
                    // Skip keys that were deleted (or otherwise no longer exist)
                    let mut key_present = false;
                    if let Some(val_rc) = object_get_key_value(&obj, k) {
                        log::trace!("for-in property {k} -> {}", value_to_string(&val_rc.borrow()));
                        key_present = true;
                    } else {
                        // Check for a TypedArray element for numeric index keys.
                        if let Ok(idx) = k.parse::<usize>()
                            && let Some(ta_cell) = obj.borrow().properties.get(&PropertyKey::String("__typedarray".to_string()))
                            && let Value::TypedArray(ta) = &*ta_cell.borrow()
                        {
                            let cur_len = if ta.length_tracking {
                                let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                                if buf_len <= ta.byte_offset {
                                    0
                                } else {
                                    (buf_len - ta.byte_offset) / ta.element_size()
                                }
                            } else {
                                ta.length
                            };
                            if idx < cur_len {
                                match ta.get(idx) {
                                    Ok(num) => log::trace!("for-in property {k} -> {}", num),
                                    Err(_) => log::trace!("for-in property {k} -> <typedarray element error>"),
                                }
                                key_present = true;
                            }
                        }
                    }
                    if !key_present {
                        continue;
                    }

                    evaluate_assign_target_with_value(mc, env, lhs, &Value::String(utf8_to_utf16(k)))?;
                    let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, env, body, labels)?;
                    match res {
                        ControlFlow::Normal(_) => v = vbody.clone(),
                        ControlFlow::Break(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                v = vbody.clone();
                            }
                            *last_value = v.clone();
                            if label.is_none() {
                                break;
                            }
                            return Ok(Some(ControlFlow::Break(label)));
                        }
                        ControlFlow::Continue(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                v = vbody.clone();
                            }
                            if let Some(ref l) = label
                                && !own_labels.contains(l)
                            {
                                *last_value = v.clone();
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                        ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                        ControlFlow::Throw(val, l, c) => return Ok(Some(ControlFlow::Throw(val, l, c))),
                    }
                }
                *last_value = v.clone();
                return Ok(None);
            }

            Ok(None)
        }
        StatementKind::ForInDestructuringObject(decl_kind_opt, pattern, iterable, body) => {
            // Hoist var declarations from destructuring pattern (var case)
            let mut names = Vec::new();
            collect_names_from_object_destructuring(pattern, &mut names);
            for name in names.iter() {
                if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                    let is_indirect_eval = crate::core::object_get_key_value(env, "__is_indirect_eval").is_some();
                    hoist_name(mc, env, name, is_indirect_eval)?;
                }
            }

            // If this is a lexical (let/const) declaration, create a head env with
            // TDZ bindings for the names so that iterable evaluation will see the
            // bindings as uninitialized and trigger ReferenceError on access.
            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                for name in names.iter() {
                    env_set(mc, &he, name, &Value::Uninitialized)?;
                }
                head_env = Some(he);
            }
            let iter_eval_env = head_env.as_ref().unwrap_or(env);

            // Evaluate the RHS expression in the head env
            let iter_val = evaluate_expr(mc, iter_eval_env, iterable)?;

            // If the evaluated expression is null or undefined, the iteration is skipped
            if matches!(iter_val, Value::Undefined | Value::Null) {
                *last_value = Value::Undefined;
                return Ok(None);
            }

            // Only support objects for now (for-in enumerates property keys)
            if let Value::Object(obj) = iter_val {
                // Collect enumerable string keys across prototype chain in insertion order
                let mut keys: Vec<String> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut current = Some(obj);
                while let Some(o) = current {
                    for (key, _val) in o.borrow().properties.iter() {
                        if let PropertyKey::String(s) = key {
                            if s == "length" {
                                continue;
                            }
                            if !seen.contains(s) {
                                seen.insert(s.clone());
                                if o.borrow().is_enumerable(key) {
                                    keys.push(s.clone());
                                }
                            }
                        }
                    }
                    current = o.borrow().prototype;
                }

                log::trace!("for-in-destructuring keys: {keys:?}");

                // Per spec, the iteration completion value V starts as undefined
                *last_value = Value::Undefined;

                for k in &keys {
                    // Skip keys that were deleted (or otherwise no longer exist)
                    if object_get_key_value(&obj, k).is_none() {
                        continue;
                    }

                    // Prepare per-iteration environment depending on decl kind
                    let iter_env = if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                        let e = new_js_object_data(mc);
                        e.borrow_mut(mc).prototype = Some(*env);
                        e
                    } else {
                        // var or assignment form: use parent env (but create a delegating object for API compatibility)
                        let e = new_js_object_data(mc);
                        e.borrow_mut(mc).prototype = Some(*env);
                        e
                    };

                    // The value for destructuring is the property key string
                    let key_str = k.clone();

                    // Box the string into an object that has index properties and length
                    let boxed = new_js_object_data(mc);
                    boxed.borrow_mut(mc).prototype = Some(*env);
                    let mut char_indices = Vec::new();
                    for (i, ch) in key_str.chars().enumerate() {
                        object_set_key_value(mc, &boxed, i, &Value::String(utf8_to_utf16(&ch.to_string())))?;
                        char_indices.push(ch);
                    }
                    object_set_key_value(mc, &boxed, "length", &Value::Number(char_indices.len() as f64))?;

                    // Perform object destructuring: pattern is Vec<ObjectDestructuringElement>
                    for elem in pattern {
                        match elem {
                            ObjectDestructuringElement::Property { key, value } => {
                                // Property lookup on boxed string
                                let mut prop_val = Value::Undefined;
                                if let Some(cell) = object_get_key_value(&boxed, key) {
                                    prop_val = cell.borrow().clone();
                                }

                                if let DestructuringElement::Variable(name, _) = value {
                                    match decl_kind_opt {
                                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                            env_set(mc, &iter_env, name, &prop_val)?;
                                        }
                                        _ => {
                                            crate::core::env_set_recursive(mc, env, name, &prop_val)?;
                                        }
                                    }
                                }
                            }
                            ObjectDestructuringElement::ComputedProperty { key: key_expr, value } => {
                                // Evaluate computed key, then lookup property on boxed string
                                let key_val = evaluate_expr(mc, &iter_env, key_expr)?;
                                let key_str = match key_val {
                                    Value::String(s) => crate::unicode::utf16_to_utf8(&s),
                                    other => crate::core::value_to_string(&other),
                                };
                                let mut prop_val = Value::Undefined;
                                if let Some(cell) = object_get_key_value(&boxed, &key_str) {
                                    prop_val = cell.borrow().clone();
                                }
                                if let DestructuringElement::Variable(name, _) = value {
                                    match decl_kind_opt {
                                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                            env_set(mc, &iter_env, name, &prop_val)?;
                                        }
                                        _ => {
                                            crate::core::env_set_recursive(mc, env, name, &prop_val)?;
                                        }
                                    }
                                }
                            }
                            ObjectDestructuringElement::Rest(_name) => {
                                // Simplified rest handling
                            }
                        }
                    }

                    // Execute body in appropriate env
                    let (res, vbody) = if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                        evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?
                    } else {
                        evaluate_statements_with_context_and_last_value(mc, env, body, labels)?
                    };

                    match res {
                        ControlFlow::Normal(_) => *last_value = vbody,
                        ControlFlow::Break(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if label.is_none() {
                                break;
                            }
                            return Ok(Some(ControlFlow::Break(label)));
                        }
                        ControlFlow::Continue(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if let Some(ref l) = label
                                && !own_labels.contains(l)
                            {
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                        ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                        ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                    }
                }
                return Ok(None);
            }
            Err(raise_type_error!("ForInDestructuringObject only supports Objects currently").into())
        }
        StatementKind::ForInDestructuringArray(decl_kind_opt, pattern, iterable, body) => {
            // Hoist var declarations from destructuring pattern (var case)
            let mut names = Vec::new();
            collect_names_from_destructuring(pattern, &mut names);
            for name in names.iter() {
                if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                    let is_indirect_eval = crate::core::object_get_key_value(env, "__is_indirect_eval").is_some();
                    hoist_name(mc, env, name, is_indirect_eval)?;
                }
            }

            // If this is a lexical (let/const) declaration, create a head env with
            // TDZ bindings for the names so that iterable evaluation will see the
            // bindings as uninitialized and trigger ReferenceError on access.
            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                for name in names.iter() {
                    env_set(mc, &he, name, &Value::Uninitialized)?;
                }
                head_env = Some(he);
            }
            let iter_eval_env = head_env.as_ref().unwrap_or(env);

            // Evaluate the RHS expression in the head env
            let iter_val = evaluate_expr(mc, iter_eval_env, iterable)?;

            // If the evaluated expression is null or undefined, the iteration is skipped
            if matches!(iter_val, Value::Undefined | Value::Null) {
                *last_value = Value::Undefined;
                return Ok(None);
            }

            // Only support objects for now (for-in enumerates property keys)
            if let Value::Object(obj) = iter_val {
                // Collect enumerable string keys across prototype chain in insertion order
                let mut keys: Vec<String> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut current = Some(obj);
                while let Some(o) = current {
                    for (key, _val) in o.borrow().properties.iter() {
                        if !o.borrow().is_enumerable(key) {
                            continue;
                        }
                        if let PropertyKey::String(s) = key {
                            if s == "length" {
                                continue;
                            }
                            if !seen.contains(s) {
                                keys.push(s.clone());
                                seen.insert(s.clone());
                            }
                        }
                    }
                    current = o.borrow().prototype;
                }

                log::trace!("for-in-destructuring-array keys: {keys:?}");

                for k in &keys {
                    // Prepare per-iteration environment depending on decl kind
                    let iter_env = if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                        let e = new_js_object_data(mc);
                        e.borrow_mut(mc).prototype = Some(*env);
                        e
                    } else {
                        // var or assignment form: use parent env (but create a delegating object for API compatibility)
                        let e = new_js_object_data(mc);
                        e.borrow_mut(mc).prototype = Some(*env);
                        e
                    };

                    // The value for destructuring is the property key string
                    let key_str = k.clone();

                    // Box the string into an object that has index properties and length
                    let boxed = new_js_object_data(mc);
                    boxed.borrow_mut(mc).prototype = Some(*env);
                    let mut char_indices = Vec::new();
                    for (i, ch) in key_str.chars().enumerate() {
                        object_set_key_value(mc, &boxed, i, &Value::String(utf8_to_utf16(&ch.to_string())))?;
                        char_indices.push(ch);
                    }
                    object_set_key_value(mc, &boxed, "length", &Value::Number(char_indices.len() as f64))?;
                    // Mark boxed as array-like so array-destructuring helpers can access numeric indices
                    object_set_key_value(mc, &boxed, "__is_array", &Value::Boolean(true))?;
                    boxed.borrow_mut(mc).non_enumerable.insert("__is_array".into());
                    // Mark `length` as non-enumerable like real arrays
                    boxed.borrow_mut(mc).set_non_enumerable("length");

                    // Perform array destructuring: bind elements from boxed
                    match decl_kind_opt {
                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                            bind_array_inner_for_letconst(
                                mc,
                                &iter_env,
                                pattern,
                                &boxed,
                                matches!(decl_kind_opt, Some(crate::core::VarDeclKind::Const)),
                                Some(stmt.line),
                                Some(stmt.column),
                            )?;
                        }
                        _ => {
                            bind_array_inner_for_var(mc, env, pattern, &boxed, Some(stmt.line), Some(stmt.column))?;
                        }
                    }

                    // Execute body in appropriate env
                    let (res, vbody) = if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                        evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?
                    } else {
                        evaluate_statements_with_context_and_last_value(mc, env, body, labels)?
                    };

                    match res {
                        ControlFlow::Normal(_) => *last_value = vbody,
                        ControlFlow::Break(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if label.is_none() {
                                break;
                            }
                            return Ok(Some(ControlFlow::Break(label)));
                        }
                        ControlFlow::Continue(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if let Some(ref l) = label
                                && !own_labels.contains(l)
                            {
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                        ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                        ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                    }
                }
                return Ok(None);
            }
            Err(raise_type_error!("ForInDestructuringArray only supports Objects currently").into())
        }
        StatementKind::ForAwaitOfDestructuringObject(decl_kind_opt, pattern, iterable, body) => {
            let mut names = Vec::new();
            collect_names_from_object_destructuring(pattern, &mut names);
            for name in names.iter() {
                if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                    let is_indirect_eval = crate::core::object_get_key_value(env, "__is_indirect_eval").is_some();
                    hoist_name(mc, env, name, is_indirect_eval)?;
                }
            }

            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                for name in names.iter() {
                    env_set(mc, &he, name, &Value::Uninitialized)?;
                }
                head_env = Some(he);
            }
            let iter_eval_env = head_env.as_ref().unwrap_or(env);

            let iter_val = evaluate_expr(mc, iter_eval_env, iterable)?;
            if let Value::Object(obj) = iter_val
                && is_array(mc, &obj)
            {
                let len = object_get_length(&obj).unwrap_or(0);
                for i in 0..len {
                    let mut val = get_property_with_accessors(mc, env, &obj, i)?;
                    val = await_promise_value(mc, env, &val)?;

                    let iter_env = if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                        let e = new_js_object_data(mc);
                        e.borrow_mut(mc).prototype = Some(*env);
                        e
                    } else {
                        new_js_object_data(mc)
                    };

                    for elem in pattern {
                        match elem {
                            ObjectDestructuringElement::Property { key, value } => {
                                let prop_val = if let Value::Object(o) = &val {
                                    get_property_with_accessors(mc, env, o, key)?
                                } else {
                                    Value::Undefined
                                };
                                if let DestructuringElement::Variable(name, _) = value {
                                    match decl_kind_opt {
                                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                            env_set(mc, &iter_env, name, &prop_val)?;
                                        }
                                        _ => {
                                            crate::core::env_set_recursive(mc, env, name, &prop_val)?;
                                        }
                                    }
                                }
                            }
                            ObjectDestructuringElement::ComputedProperty { key: key_expr, value } => {
                                // Evaluate computed key and convert to string key for property lookup on object
                                let key_val = evaluate_expr(mc, &iter_env, key_expr)?;
                                let key_str = match key_val {
                                    Value::String(s) => crate::unicode::utf16_to_utf8(&s),
                                    other => crate::core::value_to_string(&other),
                                };
                                let prop_val = if let Value::Object(o) = &val {
                                    get_property_with_accessors(mc, env, o, &key_str)?
                                } else {
                                    Value::Undefined
                                };
                                if let DestructuringElement::Variable(name, _) = value {
                                    match decl_kind_opt {
                                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                            env_set(mc, &iter_env, name, &prop_val)?;
                                        }
                                        _ => {
                                            crate::core::env_set_recursive(mc, env, name, &prop_val)?;
                                        }
                                    }
                                }
                            }
                            ObjectDestructuringElement::Rest(_name) => {}
                        }
                    }

                    let (res, vbody) = if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                        evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?
                    } else {
                        evaluate_statements_with_context_and_last_value(mc, env, body, labels)?
                    };
                    match res {
                        ControlFlow::Normal(_) => *last_value = vbody,
                        ControlFlow::Break(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if label.is_none() {
                                break;
                            }
                            return Ok(Some(ControlFlow::Break(label)));
                        }
                        ControlFlow::Continue(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if let Some(ref l) = label
                                && !own_labels.contains(l)
                            {
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                        ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                        ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                    }
                }
                return Ok(None);
            }
            Err(raise_type_error!("ForAwaitOfDestructuringObject only supports Arrays currently").into())
        }
        StatementKind::ForOfDestructuringObject(decl_kind_opt, pattern, iterable, body) => {
            // Hoist var declarations from destructuring pattern (var case)
            let mut names = Vec::new();
            collect_names_from_object_destructuring(pattern, &mut names);
            for name in names.iter() {
                if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                    let is_indirect_eval = crate::core::object_get_key_value(env, "__is_indirect_eval").is_some();
                    hoist_name(mc, env, name, is_indirect_eval)?;
                }
            }

            // If this is a lexical (let/const) declaration, create a head env with
            // TDZ bindings for the names so that iterable evaluation will see the
            // bindings as uninitialized and trigger ReferenceError on access.
            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                for name in names.iter() {
                    env_set(mc, &he, name, &Value::Uninitialized)?;
                }
                head_env = Some(he);
            }
            let iter_eval_env = head_env.as_ref().unwrap_or(env);

            // Simplified: assume array for now
            let iter_val = evaluate_expr(mc, iter_eval_env, iterable)?;
            if let Value::Object(obj) = iter_val
                && is_array(mc, &obj)
            {
                let len = object_get_length(&obj).unwrap_or(0);
                for i in 0..len {
                    let val = get_property_with_accessors(mc, env, &obj, i)?;
                    // Determine per-iteration env depending on decl kind
                    let iter_env = if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                        let e = new_js_object_data(mc);
                        e.borrow_mut(mc).prototype = Some(*env);
                        e
                    } else {
                        // For var or assignment form, reuse parent env (no fresh binding)
                        new_js_object_data(mc) // use a temporary env that delegates to parent but we'll set into parent when needed
                    };

                    // Perform object destructuring
                    for elem in pattern {
                        match elem {
                            ObjectDestructuringElement::Property { key, value } => {
                                let prop_val = if let Value::Object(o) = &val {
                                    get_property_with_accessors(mc, env, o, key)?
                                } else {
                                    Value::Undefined
                                };
                                if let DestructuringElement::Variable(name, _) = value {
                                    match decl_kind_opt {
                                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                            env_set(mc, &iter_env, name, &prop_val)?;
                                        }
                                        _ => {
                                            crate::core::env_set_recursive(mc, env, name, &prop_val)?;
                                        }
                                    }
                                }
                            }
                            ObjectDestructuringElement::ComputedProperty { key: key_expr, value } => {
                                // Evaluate computed key expression and perform lookup
                                let key_val = evaluate_expr(mc, &iter_env, key_expr)?;
                                let key_str = match key_val {
                                    Value::String(s) => crate::unicode::utf16_to_utf8(&s),
                                    other => crate::core::value_to_string(&other),
                                };
                                let prop_val = if let Value::Object(o) = &val {
                                    get_property_with_accessors(mc, env, o, &key_str)?
                                } else {
                                    Value::Undefined
                                };
                                if let DestructuringElement::Variable(name, _) = value {
                                    match decl_kind_opt {
                                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                            env_set(mc, &iter_env, name, &prop_val)?;
                                        }
                                        _ => {
                                            crate::core::env_set_recursive(mc, env, name, &prop_val)?;
                                        }
                                    }
                                }
                            }
                            ObjectDestructuringElement::Rest(_name) => {
                                // Simplified rest
                            }
                        }
                    }

                    let (res, vbody) = if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                        evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?
                    } else {
                        // var or assignment form: evaluate in parent env
                        evaluate_statements_with_context_and_last_value(mc, env, body, labels)?
                    };

                    match res {
                        ControlFlow::Normal(_) => *last_value = vbody,
                        ControlFlow::Break(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if label.is_none() {
                                break;
                            }
                            return Ok(Some(ControlFlow::Break(label)));
                        }
                        ControlFlow::Continue(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if let Some(ref l) = label
                                && !own_labels.contains(l)
                            {
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                        ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                        ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                    }
                }
                return Ok(None);
            }
            Err(raise_type_error!("ForOfDestructuringObject only supports Arrays currently").into())
        }
        StatementKind::ForOfDestructuringArray(decl_kind_opt, pattern, iterable, body) => {
            // Hoist var declarations from destructuring pattern (var case)
            let mut names = Vec::new();
            collect_names_from_destructuring(pattern, &mut names);
            for name in names.iter() {
                if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                    let is_indirect_eval = crate::core::object_get_key_value(env, "__is_indirect_eval").is_some();
                    hoist_name(mc, env, name, is_indirect_eval)?;
                }
            }

            // Try iterator first (support for Map, Set, etc.)
            // If this is a lexical (let/const) declaration, create a head env with
            // TDZ bindings for each name in the pattern so that iterable evaluation
            // sees the inner bindings in TDZ and will throw on access.
            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                for name in names.iter() {
                    env_set(mc, &he, name, &Value::Uninitialized)?;
                }
                head_env = Some(he);
            }
            let iter_eval_env = head_env.as_ref().unwrap_or(env);
            let iter_val = evaluate_expr(mc, iter_eval_env, iterable)?;
            let mut iterator = None;

            // Try to use Symbol.iterator
            if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
                && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
            {
                let method = if let Value::Object(obj) = &iter_val {
                    if let Some(c) = object_get_key_value(obj, iter_sym_data) {
                        c.borrow().clone()
                    } else {
                        Value::Undefined
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &iter_val, iter_sym_data)?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, &method, Some(&iter_val), &[])?;

                    if let Value::Object(iter_obj) = res {
                        iterator = Some(iter_obj);
                    }
                }
            }

            if let Some(iter_obj) = iterator {
                loop {
                    let next_method = object_get_key_value(&iter_obj, "next")
                        .ok_or(EvalError::Js(raise_type_error!("Iterator has no next method")))?
                        .borrow()
                        .clone();

                    let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;

                    if let Value::Object(next_res) = next_res_val {
                        let done = if let Some(done_val) = object_get_key_value(&next_res, "done") {
                            match &*done_val.borrow() {
                                Value::Boolean(b) => *b,
                                _ => false,
                            }
                        } else {
                            false
                        };

                        if done {
                            break;
                        }

                        let value = if let Some(val) = object_get_key_value(&next_res, "value") {
                            val.borrow().clone()
                        } else {
                            Value::Undefined
                        };

                        match decl_kind_opt {
                            Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                // fresh lexical environment per iteration
                                let iter_env = new_js_object_data(mc);
                                iter_env.borrow_mut(mc).prototype = Some(*env);

                                // perform destructuring into iter_env
                                for (j, elem) in pattern.iter().enumerate() {
                                    if let DestructuringElement::Variable(name, _) = elem {
                                        let elem_val = if let Value::Object(o) = &value {
                                            if is_array(mc, o) {
                                                if let Some(cell) = object_get_key_value(o, j) {
                                                    cell.borrow().clone()
                                                } else {
                                                    Value::Undefined
                                                }
                                            } else {
                                                Value::Undefined
                                            }
                                        } else {
                                            Value::Undefined
                                        };
                                        env_set(mc, &iter_env, name, &elem_val)?;
                                    }
                                }

                                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?;
                                match res {
                                    ControlFlow::Normal(_) => *last_value = vbody,
                                    ControlFlow::Break(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            *last_value = vbody;
                                        }
                                        if label.is_none() {
                                            break;
                                        }
                                        return Ok(Some(ControlFlow::Break(label)));
                                    }
                                    ControlFlow::Continue(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            *last_value = vbody;
                                        }
                                        if let Some(ref l) = label
                                            && !own_labels.contains(l)
                                        {
                                            return Ok(Some(ControlFlow::Continue(label)));
                                        }
                                    }
                                    ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                                    ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                                }
                            }
                            _ => {
                                // var or assignment form: bind into parent env
                                for (j, elem) in pattern.iter().enumerate() {
                                    if let DestructuringElement::Variable(name, _) = elem {
                                        let elem_val = if let Value::Object(o) = &value {
                                            if is_array(mc, o) {
                                                if let Some(cell) = object_get_key_value(o, j) {
                                                    cell.borrow().clone()
                                                } else {
                                                    Value::Undefined
                                                }
                                            } else {
                                                Value::Undefined
                                            }
                                        } else {
                                            Value::Undefined
                                        };
                                        crate::core::env_set_recursive(mc, env, name, &elem_val)?;
                                    }
                                }

                                let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, env, body, labels)?;
                                match res {
                                    ControlFlow::Normal(_) => *last_value = vbody,
                                    ControlFlow::Break(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            *last_value = vbody;
                                        }
                                        if label.is_none() {
                                            break;
                                        }
                                        return Ok(Some(ControlFlow::Break(label)));
                                    }
                                    ControlFlow::Continue(label) => {
                                        if !matches!(&vbody, Value::Undefined) {
                                            *last_value = vbody;
                                        }
                                        if let Some(ref l) = label
                                            && !own_labels.contains(l)
                                        {
                                            return Ok(Some(ControlFlow::Continue(label)));
                                        }
                                    }
                                    ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                                    ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                                }
                            }
                        }
                    } else {
                        return Err(raise_type_error!("Iterator result is not an object").into());
                    }
                }
                return Ok(None);
            }

            // Simplified: assume array for now (existing fallback)
            if let Value::Object(obj) = iter_val
                && is_array(mc, &obj)
            {
                let len = object_get_length(&obj).unwrap_or(0);
                let loop_env = new_js_object_data(mc);
                loop_env.borrow_mut(mc).prototype = Some(*env);
                for i in 0..len {
                    let val = get_property_with_accessors(mc, env, &obj, i)?;
                    // Perform array destructuring
                    for (j, elem) in pattern.iter().enumerate() {
                        if let DestructuringElement::Variable(name, _) = elem {
                            let elem_val = if let Value::Object(o) = &val {
                                if is_array(mc, o) {
                                    get_property_with_accessors(mc, env, o, j)?
                                } else {
                                    Value::Undefined
                                }
                            } else {
                                Value::Undefined
                            };
                            crate::core::env_set_recursive(mc, env, name, &elem_val)?;
                        }
                    }
                    let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &loop_env, body, labels)?;
                    match res {
                        ControlFlow::Normal(_) => *last_value = vbody,
                        ControlFlow::Break(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if label.is_none() {
                                break;
                            }
                            return Ok(Some(ControlFlow::Break(label)));
                        }
                        ControlFlow::Continue(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if let Some(ref l) = label
                                && !own_labels.contains(l)
                            {
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                        ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                        ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                    }
                }
                return Ok(None);
            }
            Err(raise_type_error!("ForOfDestructuringArray only supports Arrays currently").into())
        }
        StatementKind::ForAwaitOfDestructuringArray(decl_kind_opt, pattern, iterable, body) => {
            let iter_val = evaluate_expr(mc, env, iterable)?;
            if let Value::Object(obj) = iter_val
                && is_array(mc, &obj)
            {
                let len = object_get_length(&obj).unwrap_or(0);
                for i in 0..len {
                    let mut val = get_property_with_accessors(mc, env, &obj, i)?;
                    val = await_promise_value(mc, env, &val)?;

                    let iter_env = if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                        let e = new_js_object_data(mc);
                        e.borrow_mut(mc).prototype = Some(*env);
                        e
                    } else {
                        new_js_object_data(mc)
                    };

                    let pattern_expr = Expr::Array(convert_array_pattern_inner(pattern));
                    evaluate_binding_target_with_value(mc, &iter_env, &pattern_expr, &val)?;

                    let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &iter_env, body, labels)?;
                    match res {
                        ControlFlow::Normal(_) => *last_value = vbody,
                        ControlFlow::Break(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if label.is_none() {
                                break;
                            }
                            return Ok(Some(ControlFlow::Break(label)));
                        }
                        ControlFlow::Continue(label) => {
                            if !matches!(&vbody, Value::Undefined) {
                                *last_value = vbody;
                            }
                            if let Some(ref l) = label
                                && !own_labels.contains(l)
                            {
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                        }
                        ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                        ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                    }
                }
                return Ok(None);
            }
            Err(raise_type_error!("ForAwaitOfDestructuringArray only supports Arrays currently").into())
        }
        StatementKind::Switch(sw_stmt) => {
            let sw_stmt = sw_stmt.as_ref();
            let disc = match evaluate_expr(mc, env, &sw_stmt.expr) {
                Ok(v) => v,
                Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
            };

            // Find start index: first matching Case, otherwise first Default, otherwise nothing executes
            let mut start_idx: Option<usize> = None;
            let mut default_idx: Option<usize> = None;
            for (i, case) in sw_stmt.cases.iter().enumerate() {
                match case {
                    crate::core::SwitchCase::Case(test_expr, _stmts) => {
                        // Evaluate test expression and compare
                        let test_val = match evaluate_expr(mc, env, test_expr) {
                            Ok(v) => v,
                            Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                        };
                        if crate::core::values_equal(mc, &disc, &test_val) {
                            start_idx = Some(i);
                            break;
                        }
                    }
                    crate::core::SwitchCase::Default(_stmts) => {
                        if default_idx.is_none() {
                            default_idx = Some(i);
                        }
                    }
                }
            }

            let start = if let Some(i) = start_idx {
                i
            } else if let Some(d) = default_idx {
                d
            } else {
                return Ok(None);
            };

            let switch_env = new_js_object_data(mc);
            switch_env.borrow_mut(mc).prototype = Some(*env);

            for i in start..sw_stmt.cases.len() {
                match &sw_stmt.cases[i] {
                    crate::core::SwitchCase::Case(_test, stmts) => {
                        let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &switch_env, stmts, labels)?;
                        match res {
                            ControlFlow::Normal(_) => *last_value = vbody,
                            ControlFlow::Break(label) => {
                                if !matches!(vbody, Value::Undefined) {
                                    *last_value = vbody;
                                }
                                if label.is_none() {
                                    return Ok(None);
                                }
                                return Ok(Some(ControlFlow::Break(label)));
                            }
                            ControlFlow::Continue(label) => {
                                if !matches!(vbody, Value::Undefined) {
                                    *last_value = vbody;
                                }
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                            ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                            ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                        }
                    }
                    crate::core::SwitchCase::Default(stmts) => {
                        let (res, vbody) = evaluate_statements_with_context_and_last_value(mc, &switch_env, stmts, labels)?;
                        match res {
                            ControlFlow::Normal(_) => *last_value = vbody,
                            ControlFlow::Break(label) => {
                                if !matches!(vbody, Value::Undefined) {
                                    *last_value = vbody;
                                }
                                if label.is_none() {
                                    return Ok(None);
                                }
                                return Ok(Some(ControlFlow::Break(label)));
                            }
                            ControlFlow::Continue(label) => {
                                if !matches!(vbody, Value::Undefined) {
                                    *last_value = vbody;
                                }
                                return Ok(Some(ControlFlow::Continue(label)));
                            }
                            ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                            ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                        }
                    }
                }
            }
            Ok(None)
        }
        _ => todo!("Statement kind not implemented yet"),
    }
}

pub fn export_value<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str, v: &Value<'gc>) -> Result<(), EvalError<'gc>> {
    if let Some(exports_cell) = env_get(env, "exports") {
        let exports = exports_cell.borrow().clone();
        if let Value::Object(exports_obj) = exports {
            object_set_key_value(mc, &exports_obj, name, v)?;
            return Ok(());
        }
    }

    if let Some(module_cell) = env_get(env, "module") {
        let module = module_cell.borrow().clone();
        if let Value::Object(module_obj) = module
            && let Some(exports_val) = object_get_key_value(&module_obj, "exports")
            && let Value::Object(exports_obj) = &*exports_val.borrow()
        {
            object_set_key_value(mc, exports_obj, name, v)?;
        }
    }
    Ok(())
}

fn export_binding<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    export_name: &str,
    binding_name: &str,
) -> Result<(), EvalError<'gc>> {
    let getter_body = vec![Statement {
        kind: Box::new(StatementKind::Return(Some(Expr::Var(binding_name.to_string(), None, None)))),
        line: 0,
        column: 0,
    }];
    let getter_val = Value::Getter(getter_body, *env, None);
    let prop = Value::Property {
        value: None,
        getter: Some(Box::new(getter_val)),
        setter: None,
    };

    if let Some(exports_cell) = env_get(env, "exports") {
        let exports = exports_cell.borrow().clone();
        if let Value::Object(exports_obj) = exports {
            object_set_key_value(mc, &exports_obj, export_name, &prop)?;
            return Ok(());
        }
    }

    if let Some(module_cell) = env_get(env, "module") {
        let module = module_cell.borrow().clone();
        if let Value::Object(module_obj) = module
            && let Some(exports_val) = object_get_key_value(&module_obj, "exports")
            && let Value::Object(exports_obj) = &*exports_val.borrow()
        {
            object_set_key_value(mc, exports_obj, export_name, &prop)?;
        }
    }
    Ok(())
}

fn refresh_error_by_additional_stack_frame<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    line: usize,
    column: usize,
    mut e: EvalError<'gc>,
) -> EvalError<'gc> {
    let mut filename = String::new();
    if let Some(val_ptr) = object_get_key_value(env, "__filepath")
        && let Value::String(s) = &*val_ptr.borrow()
    {
        filename = utf16_to_utf8(s);
    }
    // Prefer an explicit frame name if the call environment has one (set in call_closure)
    let mut frame_name = "<anonymous>".to_string();
    if let Some(frame_val) = object_get_key_value(env, "__frame")
        && let Value::String(s) = &*frame_val.borrow()
    {
        frame_name = utf16_to_utf8(s);
    }
    let frame = format!("at {} ({}:{}:{})", frame_name, filename, line, column);
    if let EvalError::Js(js_err) = &mut e {
        js_err.inner.stack.push(frame.clone());
    }

    if let EvalError::Throw(val, l, c) = &mut e {
        if !is_error(val) {
            *l = Some(line);
            *c = Some(column);
        }
        if let Value::Object(obj) = val {
            // For user-defined/non-native thrown objects (e.g., Test262Error),
            // prefer reporting the caller site as the top-level JS location so
            // test harnesses can point at the assertion call site. For native
            // Error instances created via `new Error`, preserve the original
            // throw-site as the top-level location.
            let current_stack = obj.borrow().get_property("stack").unwrap_or_default();
            let new_stack = if current_stack.is_empty() {
                frame.clone()
            } else {
                format!("{}\n    {}", current_stack, frame)
            };
            obj.borrow_mut(mc).set_property(mc, "stack", new_stack.into());
        }
    }
    e
}

fn get_primitive_prototype_property<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj_val: &Value<'gc>,
    key: impl Into<PropertyKey<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let key = &key.into();
    if let PropertyKey::String(s) = key
        && s == "name"
    {
        let name = match obj_val {
            Value::Function(name) => Some(name.as_str()),
            Value::GeneratorFunction(name, ..) => name.as_deref(),
            Value::AsyncGeneratorFunction(name, ..) => name.as_deref(),
            _ => None,
        };
        if let Some(name) = name {
            return Ok(Value::String(utf8_to_utf16(name)));
        }
    }

    let proto_name = match obj_val {
        Value::BigInt(_) => "BigInt",
        Value::Number(_) => "Number",
        Value::String(_) => "String",
        Value::Boolean(_) => "Boolean",
        Value::Symbol(_) => "Symbol",
        Value::Closure(_)
        | Value::Function(_)
        | Value::AsyncClosure(_)
        | Value::GeneratorFunction(..)
        | Value::AsyncGeneratorFunction(..) => "Function",
        _ => return Ok(Value::Undefined),
    };

    if let Ok(ctor) = evaluate_var(mc, env, proto_name)
        && let Value::Object(ctor_obj) = ctor
        && let Some(proto_ref) = object_get_key_value(&ctor_obj, "prototype")
        && let Value::Object(proto) = &*proto_ref.borrow()
    {
        // Special-case Symbol.prototype.description: return primitive description without invoking getter
        if proto_name == "Symbol"
            && let PropertyKey::String(s) = key
            && s == "description"
            && let Value::Symbol(sd) = obj_val
        {
            if let Some(desc) = sd.description() {
                return Ok(Value::String(crate::unicode::utf8_to_utf16(desc)));
            }
            return Ok(Value::Undefined);
        }

        if let Some(val) = object_get_key_value(proto, key) {
            return Ok(val.borrow().clone());
        }
    }

    // Special-case string indexing: numeric property on a string primitive returns the character
    if let Value::String(s) = obj_val
        && let PropertyKey::String(skey) = key
        && let Ok(idx) = skey.parse::<usize>()
    {
        let us = crate::unicode::utf16_to_utf8(s);
        if let Some(ch) = us.chars().nth(idx) {
            return Ok(Value::String(crate::unicode::utf8_to_utf16(&ch.to_string())));
        }
    }

    Ok(Value::Undefined)
}

// Helper: perform IteratorClose semantics used by destructuring when an abrupt
// completion occurs while an iterator is in use. Returns the completion to
// be propagated (either the original completion or an inner completion).
fn iterator_close_on_error<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_obj: &crate::core::JSObjectDataPtr<'gc>,
    orig_err: EvalError<'gc>,
) -> EvalError<'gc> {
    // Attempt to get the 'return' property (using accessors so getters execute)
    let return_val_res = match get_property_with_accessors(mc, env, iter_obj, "return") {
        Ok(v) => Ok(v),
        Err(e) => {
            // If original is a throw completion, prefer the original
            match &orig_err {
                EvalError::Throw(_, _, _) => return orig_err,
                _ => return e,
            }
        }
    };

    let return_val = match return_val_res {
        Ok(v) => v,
        Err(e) => return e,
    };

    // If return is undefined or null, just return original completion
    if matches!(return_val, Value::Undefined) || matches!(return_val, Value::Null) {
        return orig_err;
    }

    // If not callable, create a TypeError; but if orig_err is throw, prefer orig_err
    let is_callable = matches!(return_val, Value::Function(_) | Value::Closure(_) | Value::Object(_));
    if !is_callable {
        match &orig_err {
            EvalError::Throw(_, _, _) => return orig_err,
            _ => return raise_type_error!("Iterator return property is not callable").into(),
        }
    }

    // Call the return method with iterator as this
    let call_res = evaluate_call_dispatch(mc, env, &return_val, Some(&Value::Object(*iter_obj)), &[]);
    match call_res {
        Ok(val) => {
            // If result is not an object, produce TypeError unless orig_err is throw
            if !matches!(val, Value::Object(_)) {
                match &orig_err {
                    EvalError::Throw(_, _, _) => return orig_err,
                    _ => return raise_type_error!("Iterator return did not return an object").into(),
                }
            }
            orig_err
        }
        Err(e) => match &orig_err {
            EvalError::Throw(_, _, _) => orig_err,
            _ => e,
        },
    }
}

// Helper: perform IteratorClose on a normal (non-abrupt) completion. Returns
// Result<(), EvalError> - any error during closing will be propagated.
fn iterator_close<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_obj: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    // Get the 'return' property
    let return_val = get_property_with_accessors(mc, env, iter_obj, "return")?;

    // If return is undefined or null, just return Ok(())
    if matches!(return_val, Value::Undefined) || matches!(return_val, Value::Null) {
        return Ok(());
    }

    // If not callable, TypeError
    let is_callable = matches!(return_val, Value::Function(_) | Value::Closure(_) | Value::Object(_));
    if !is_callable {
        return Err(raise_type_error!("Iterator return property is not callable").into());
    }

    // Call the return method with iterator as this
    let call_res = evaluate_call_dispatch(mc, env, &return_val, Some(&Value::Object(*iter_obj)), &[]);
    match call_res {
        Ok(val) => {
            if !matches!(val, Value::Object(_)) {
                return Err(raise_type_error!("Iterator return did not return an object").into());
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn maybe_set_default_name<'gc>(
    mc: &MutationContext<'gc>,
    lhs: &Expr,
    default_expr: &Expr,
    assigned_val: &Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    if let Expr::Var(name, _, _) = lhs {
        let is_arrow = matches!(default_expr, Expr::ArrowFunction(..) | Expr::AsyncArrowFunction(..));
        let is_anon_fn = matches!(
            default_expr,
            Expr::Function(None, ..)
                | Expr::GeneratorFunction(None, ..)
                | Expr::AsyncFunction(None, ..)
                | Expr::AsyncGeneratorFunction(None, ..)
        );
        let is_anon_class = matches!(default_expr, Expr::Class(class_def) if class_def.name.is_empty());

        if (is_arrow || is_anon_fn || is_anon_class)
            && let Value::Object(obj) = assigned_val
        {
            let mut should_set = false;
            if is_arrow {
                should_set = true;
            } else if let Some(name_rc) = object_get_key_value(obj, "name") {
                let existing_val = match &*name_rc.borrow() {
                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                    other => other.clone(),
                };
                let name_str = value_to_string(&existing_val);
                if name_str.is_empty() {
                    should_set = true;
                }
            } else {
                should_set = true;
            }

            if should_set {
                let desc = create_descriptor_object(mc, &Value::String(utf8_to_utf16(name)), false, false, true)?;
                crate::js_object::define_property_internal(mc, obj, "name", &desc)?;
            }
        }
    }
    Ok(())
}

enum TargetTemp<'gc> {
    Var(String),
    PropBase(JSObjectDataPtr<'gc>, String),
    PrivatePropBase(JSObjectDataPtr<'gc>, String, u32),
    IndexBase(JSObjectDataPtr<'gc>, Box<Value<'gc>>),
}

fn put_temp<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    temp: TargetTemp<'gc>,
    value: &Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    match temp {
        TargetTemp::Var(name) => {
            if name == "await" {
                println!(
                    "DEBUG: put_temp assign to 'await' in env ptr={:p} is_function_scope={} has_prop={}",
                    env,
                    env.borrow().is_function_scope,
                    env.borrow().properties.contains_key(&PropertyKey::String("await".to_string()))
                );
            }
            env_set_recursive(mc, env, &name, value)?;
        }
        TargetTemp::PropBase(obj, static_key) => {
            set_property_with_accessors(mc, env, &obj, &static_key, value)?;
        }
        TargetTemp::PrivatePropBase(obj, name, id) => {
            let key = PropertyKey::Private(name, id);
            set_property_with_accessors(mc, env, &obj, &key, value)?;
        }
        TargetTemp::IndexBase(obj, raw_key_val) => {
            let key = match *raw_key_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &raw_key_val, "string", env)?;
                    match prim {
                        Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                        Value::Symbol(s) => PropertyKey::Symbol(s),
                        other => PropertyKey::String(value_to_string(&other)),
                    }
                }
                _ => PropertyKey::String(value_to_string(&raw_key_val)),
            };
            set_property_with_accessors(mc, env, &obj, &key, value)?;
        }
    }
    Ok(())
}

fn precompute_target<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target_expr: &Expr,
) -> Result<TargetTemp<'gc>, EvalError<'gc>> {
    match target_expr {
        Expr::Var(name, _, _) => Ok(TargetTemp::Var(name.clone())),
        Expr::Property(obj_expr, key_str) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                Ok(TargetTemp::PropBase(obj, key_str.clone()))
            } else {
                Err(raise_eval_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let raw_key = evaluate_expr(mc, env, key_expr)?;
            if let Value::Object(obj) = obj_val {
                Ok(TargetTemp::IndexBase(obj, Box::new(raw_key)))
            } else {
                Err(raise_eval_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let pv = evaluate_var(mc, env, name)?;
                if let Value::PrivateName(n, id) = pv {
                    Ok(TargetTemp::PrivatePropBase(obj, n, id))
                } else {
                    Err(raise_syntax_error!(format!("Private field '{}' must be declared in an enclosing class", name)).into())
                }
            } else {
                Err(raise_type_error!("Cannot access private member of non-object").into())
            }
        }
        _ => Err(raise_eval_error!("Assignment target not supported").into()),
    }
}

// Helper to produce the candidate name for NamedEvaluation. If the LHS
// is a parenthesized identifier (CoverParenthesizedExpression) we should
// use the empty string. We can heuristically detect parentheses by looking
// at the source file stored in env.__filepath and examining the character
// before the identifier token position.
fn candidate_name_from_target<'gc>(env: &JSObjectDataPtr<'gc>, target: &Expr) -> String {
    if let Expr::Var(n, maybe_line, maybe_col) = target {
        if let Some(val_ptr) = env_get(env, "__filepath")
            && let Value::String(s) = &*val_ptr.borrow()
        {
            let path = utf16_to_utf8(s);
            if let Ok(txt) = std::fs::read_to_string(path) {
                let mut matched_paren = false;
                if let Some(line) = maybe_line
                    && let Some(l) = txt.lines().nth(line.saturating_sub(1))
                {
                    // Fast path: if the line starts with a parenthesized identifier like `(name)`
                    // treat this as CoverParenthesizedExpression and return empty.
                    let trimmed = l.trim_start();
                    if let Some(rest) = trimmed.strip_prefix('(') {
                        let rest = rest.trim_start();
                        let mut rest_chars = rest.chars();
                        let mut ok = true;
                        for nc in n.chars() {
                            match rest_chars.next() {
                                Some(rc) if rc == nc => {}
                                _ => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok {
                            let rest_after: String = rest_chars.collect();
                            let after_trim = rest_after.trim_start();
                            if after_trim.starts_with(')') {
                                matched_paren = true;
                            }
                        }
                    }

                    if !matched_paren {
                        let chars: Vec<char> = l.chars().collect();
                        let mut col_idx_opt = maybe_col.map(|c| c.saturating_sub(1));

                        if col_idx_opt.is_none()
                            && let Some(byte_idx) = l.find(n)
                        {
                            let char_idx = l[..byte_idx].chars().count();
                            col_idx_opt = Some(char_idx);
                        }

                        if let Some(col_idx) = col_idx_opt {
                            let mut i = col_idx;
                            while i > 0 {
                                i -= 1;
                                let ch = chars[i];
                                if ch.is_whitespace() {
                                    continue;
                                }
                                if ch == '(' {
                                    matched_paren = true;
                                }
                                break;
                            }
                        }
                    }
                }

                if !matched_paren {
                    for l in txt.lines() {
                        let trimmed = l.trim_start();
                        if let Some(rest) = trimmed.strip_prefix('(') {
                            let rest = rest.trim_start();
                            let mut rest_chars = rest.chars();
                            let mut ok = true;
                            for nc in n.chars() {
                                match rest_chars.next() {
                                    Some(rc) if rc == nc => {}
                                    _ => {
                                        ok = false;
                                        break;
                                    }
                                }
                            }
                            if ok {
                                let rest_after: String = rest_chars.collect();
                                let after_trim = rest_after.trim_start();
                                if let Some(stripped) = after_trim.strip_prefix(')') {
                                    let after_paren = stripped.trim_start();
                                    if after_paren.starts_with('=') {
                                        matched_paren = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                if matched_paren {
                    return String::new();
                }
            }
        }
        return n.clone();
    }
    String::new()
}

// Pre-evaluate target components (base object and property key) so that any
// side-effects or throws from LHS occur before RHS evaluation.
// Keep raw values for base/keys so we do NOT perform ToPropertyKey or ToObject
// conversions until after the RHS is evaluated (per spec requirements).
enum Precomputed<'gc> {
    Var(String),
    Property(Box<Value<'gc>>, Box<PropertyKey<'gc>>), // base value (may be null/primitive), static key
    Index(Box<Value<'gc>>, Box<Value<'gc>>),          // base value, raw key value (ToPropertyKey deferred)
    SuperProperty(Box<PropertyKey<'gc>>),             // super.prop (defer super base resolution)
    SuperIndex(Box<Value<'gc>>),                      // super[raw_key]
    PrivateMember(Box<Value<'gc>>, String, u32),
}

fn evaluate_expr_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Per ECMAScript semantics, evaluate the left-hand side (target) first and
    // then evaluate the right-hand side. This ensures side-effects and throws
    // on the left occur before the RHS is evaluated.

    // Candidate name for NamedEvaluation (set if RHS is an anonymous function)
    let mut maybe_name_to_set: Option<String> = None;

    match value_expr {
        crate::core::Expr::Function(name_opt, ..) if name_opt.is_none() => {
            maybe_name_to_set = Some(candidate_name_from_target(env, target));
        }
        crate::core::Expr::ArrowFunction(..) | crate::core::Expr::AsyncArrowFunction(..) => {
            maybe_name_to_set = Some(candidate_name_from_target(env, target));
        }
        crate::core::Expr::GeneratorFunction(name_opt, ..) if name_opt.is_none() => {
            maybe_name_to_set = Some(candidate_name_from_target(env, target));
        }
        crate::core::Expr::AsyncFunction(name_opt, ..) if name_opt.is_none() => {
            maybe_name_to_set = Some(candidate_name_from_target(env, target));
        }
        crate::core::Expr::Class(class_def) if class_def.name.is_empty() => {
            maybe_name_to_set = Some(candidate_name_from_target(env, target));
        }
        _ => {}
    }

    // Support destructuring assignment targets (array/object patterns).
    // Per spec, evaluate the RHS value first, then evaluate property names and
    // evaluate assignment targets in the required order while honoring
    // IteratorClose semantics on abrupt completions.
    if let Expr::Array(elements) = target {
        // Evaluate RHS first
        let rhs = evaluate_expr(mc, env, value_expr)?;

        if matches!(rhs, Value::Undefined | Value::Null) {
            return Err(raise_type_error!("Cannot destructure undefined or null").into());
        }

        // Obtain iterator if available
        let mut iterator: Option<crate::core::JSObjectDataPtr<'gc>> = None;
        if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
            && let Value::Object(sym_obj) = &*sym_ctor.borrow()
            && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
            && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
        {
            let method = if let Value::Object(obj) = &rhs {
                get_property_with_accessors(mc, env, obj, iter_sym_data)?
            } else {
                get_primitive_prototype_property(mc, env, &rhs, iter_sym_data)?
            };

            log::trace!("DEBUG: method for iterator: {:?}", method);
            if matches!(method, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Object is not iterable").into());
            }
            let res = evaluate_call_dispatch(mc, env, &method, Some(&rhs), &[])?;
            if let Value::Object(iter_obj) = res {
                iterator = Some(iter_obj);
            }
        }

        if let Some(iter_obj) = iterator {
            object_set_key_value(mc, env, "__pending_iterator", &Value::Object(iter_obj))?;
            object_set_key_value(mc, env, "__pending_iterator_done", &Value::Boolean(false))?;

            let clear_pending_iterator = || -> Result<(), EvalError<'gc>> {
                object_set_key_value(mc, env, "__pending_iterator_done", &Value::Boolean(true))?;
                object_set_key_value(mc, env, "__pending_iterator", &Value::Undefined)?;
                Ok(())
            };

            // Iterate and assign
            let mut iterator_done = false;

            for elem_opt in elements.iter() {
                let mut precomputed_temp: Option<TargetTemp<'gc>> = None;
                if let Some(elem_expr) = elem_opt {
                    match elem_expr {
                        Expr::Assign(lhs, _) => {
                            if !matches!(&**lhs, Expr::Array(_) | Expr::Object(_)) {
                                precomputed_temp = match precompute_target(mc, env, lhs) {
                                    Ok(t) => Some(t),
                                    Err(e) => {
                                        let closed = iterator_close_on_error(mc, env, &iter_obj, e);
                                        clear_pending_iterator()?;
                                        return Err(closed);
                                    }
                                };
                            }
                        }
                        Expr::Var(..) | Expr::Property(..) | Expr::Index(..) => {
                            precomputed_temp = match precompute_target(mc, env, elem_expr) {
                                Ok(t) => Some(t),
                                Err(e) => {
                                    let closed = iterator_close_on_error(mc, env, &iter_obj, e);
                                    clear_pending_iterator()?;
                                    return Err(closed);
                                }
                            };
                        }
                        _ => {}
                    }
                }

                if let Some(elem_expr) = elem_opt
                    && let Expr::Spread(spread_expr) = elem_expr
                {
                    let mut rest_temp: Option<TargetTemp<'gc>> = None;
                    if !matches!(&**spread_expr, Expr::Array(_) | Expr::Object(_)) {
                        rest_temp = match precompute_target(mc, env, spread_expr) {
                            Ok(t) => Some(t),
                            Err(e) => {
                                let closed = iterator_close_on_error(mc, env, &iter_obj, e);
                                clear_pending_iterator()?;
                                return Err(closed);
                            }
                        };
                    }

                    let rest_obj = crate::js_array::create_array(mc, env)?;
                    let mut idx2 = 0_usize;
                    if !iterator_done {
                        loop {
                            let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                            if matches!(next_method, Value::Undefined | Value::Null) {
                                return Err(raise_type_error!("Iterator has no next method").into());
                            }
                            let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                            if let Value::Object(next_res) = next_res_val {
                                let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                                let done = matches!(done_val, Value::Boolean(true));
                                if done {
                                    iterator_done = true;
                                    break;
                                }
                                let value = get_property_with_accessors(mc, env, &next_res, "value")?;
                                object_set_key_value(mc, &rest_obj, idx2, &value)?;
                                idx2 += 1;
                            } else {
                                return Err(raise_type_error!("Iterator result is not an object").into());
                            }
                        }
                    }
                    object_set_key_value(mc, &rest_obj, "length", &Value::Number(idx2 as f64))?;
                    if let Some(temp) = rest_temp {
                        put_temp(mc, env, temp, &Value::Object(rest_obj))?;
                    } else {
                        match &**spread_expr {
                            Expr::Var(name, _, _) => {
                                env_set_recursive(mc, env, name, &Value::Object(rest_obj))?;
                            }
                            other => {
                                evaluate_assign_target_with_value(mc, env, other, &Value::Object(rest_obj))?;
                            }
                        }
                    }
                    break;
                }

                let value = if iterator_done {
                    Value::Undefined
                } else {
                    let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                    if matches!(next_method, Value::Undefined | Value::Null) {
                        return Err(raise_type_error!("Iterator has no next method").into());
                    }
                    let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                    if let Value::Object(next_res) = next_res_val {
                        let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                        let done = matches!(done_val, Value::Boolean(true));
                        if done {
                            iterator_done = true;
                            Value::Undefined
                        } else {
                            get_property_with_accessors(mc, env, &next_res, "value")?
                        }
                    } else {
                        return Err(raise_type_error!("Iterator result is not an object").into());
                    }
                };

                if let Some(elem_expr) = elem_opt {
                    match elem_expr {
                        Expr::Assign(boxed_lhs, boxed_default) => {
                            if matches!(value, Value::Undefined) {
                                match evaluate_expr(mc, env, boxed_default) {
                                    Ok(dv) => {
                                        maybe_set_default_name(mc, boxed_lhs, boxed_default, &dv)?;
                                        let assign_res = match precomputed_temp.take() {
                                            Some(TargetTemp::Var(_)) => {
                                                evaluate_assign_target_with_value(mc, env, boxed_lhs, &dv).map(|_| ())
                                            }
                                            Some(temp) => put_temp(mc, env, temp, &dv),
                                            None => evaluate_assign_target_with_value(mc, env, boxed_lhs, &dv).map(|_| ()),
                                        };
                                        if let Err(e) = assign_res {
                                            let closed = iterator_close_on_error(mc, env, &iter_obj, e);
                                            clear_pending_iterator()?;
                                            return Err(closed);
                                        }
                                    }
                                    Err(e) => {
                                        let closed = iterator_close_on_error(mc, env, &iter_obj, e);
                                        clear_pending_iterator()?;
                                        return Err(closed);
                                    }
                                }
                            } else {
                                let assign_res = if let Some(temp) = precomputed_temp.take() {
                                    put_temp(mc, env, temp, &value)
                                } else {
                                    evaluate_assign_target_with_value(mc, env, boxed_lhs, &value).map(|_| ())
                                };
                                if let Err(e) = assign_res {
                                    let closed = iterator_close_on_error(mc, env, &iter_obj, e);
                                    clear_pending_iterator()?;
                                    return Err(closed);
                                }
                            }
                        }
                        Expr::Var(name, _, _) => {
                            if let Some(temp) = precomputed_temp.take() {
                                put_temp(mc, env, temp, &value)?;
                            } else {
                                env_set_recursive(mc, env, name, &value)?;
                            }
                        }
                        other => {
                            if let Some(temp) = precomputed_temp.take() {
                                if let Err(e) = put_temp(mc, env, temp, &value) {
                                    let closed = iterator_close_on_error(mc, env, &iter_obj, e);
                                    clear_pending_iterator()?;
                                    return Err(closed);
                                }
                            } else if let Err(e) = evaluate_assign_target_with_value(mc, env, other, &value) {
                                let closed = iterator_close_on_error(mc, env, &iter_obj, e);
                                clear_pending_iterator()?;
                                return Err(closed);
                            }
                        }
                    }
                }
            }

            if !iterator_done {
                iterator_close(mc, env, &iter_obj)?;
                clear_pending_iterator()?;
            } else {
                clear_pending_iterator()?;
            }

            return Ok(rhs);
        }

        // If not iterator: assume array-like object
        if let Value::Object(obj) = rhs {
            log::debug!("Array destructuring fallback for object");
            // simple index-based extraction
            for (i, elem_opt) in elements.iter().enumerate() {
                let val_at = if is_typedarray(&obj) {
                    log::debug!("Array destructuring fallback: TypedArray index {}", i);
                    // For TypedArrays, use property access which handles indexed elements
                    match get_property_with_accessors(mc, env, &obj, PropertyKey::String(i.to_string())) {
                        Ok(v) => {
                            log::debug!("Array destructuring fallback: got value {:?}", v);
                            v
                        }
                        Err(e) => return Err(e),
                    }
                } else if let Some(cell) = object_get_key_value(&obj, i.to_string()) {
                    cell.borrow().clone()
                } else {
                    Value::Undefined
                };

                if let Some(elem_expr) = elem_opt {
                    match elem_expr {
                        Expr::Assign(boxed_lhs, boxed_default) => {
                            let mut final_val = val_at.clone();
                            if matches!(final_val, Value::Undefined) {
                                final_val = evaluate_expr(mc, env, boxed_default)?;
                            }
                            evaluate_assign_target_with_value(mc, env, boxed_lhs, &final_val)?;
                        }
                        Expr::Var(name, _, _) => {
                            env_set_recursive(mc, env, name, &val_at)?;
                        }
                        Expr::Spread(spread_expr) => {
                            let rest_obj = crate::js_array::create_array(mc, env)?;
                            let len = if let Some(len_cell) = object_get_key_value(&obj, "length") {
                                if let Value::Number(n) = len_cell.borrow().clone() {
                                    n as usize
                                } else {
                                    0
                                }
                            } else {
                                0
                            };
                            let mut idx2 = 0_usize;
                            for j in i..len {
                                let v = if is_typedarray(&obj) {
                                    get_property_with_accessors(mc, env, &obj, j)?
                                } else if let Some(cell) = object_get_key_value(&obj, j.to_string()) {
                                    cell.borrow().clone()
                                } else {
                                    Value::Undefined
                                };
                                object_set_key_value(mc, &rest_obj, idx2, &v)?;
                                idx2 += 1;
                            }
                            object_set_key_value(mc, &rest_obj, "length", &Value::Number(idx2 as f64))?;
                            match &**spread_expr {
                                Expr::Var(name, _, _) => {
                                    env_set_recursive(mc, env, name, &Value::Object(rest_obj))?;
                                }
                                other => {
                                    evaluate_assign_target_with_value(mc, env, other, &Value::Object(rest_obj))?;
                                }
                            }
                            break;
                        }
                        other => {
                            evaluate_assign_target_with_value(mc, env, other, &val_at)?;
                        }
                    }
                }
            }
            return Ok(Value::Object(obj));
        }
    }

    if let Expr::Object(properties) = target {
        let rhs = evaluate_expr(mc, env, value_expr)?;

        if matches!(rhs, Value::Undefined | Value::Null) {
            return Err(raise_type_error!("Cannot destructure undefined or null").into());
        }

        let mut excluded_keys: Vec<PropertyKey> = Vec::new();
        for (key_expr, target_expr, is_spread, _is_plain) in properties.iter() {
            if *is_spread {
                let rest_obj = new_js_object_data(mc);
                if let Some(obj_val) = env_get(env, "Object")
                    && let Value::Object(obj_ctor) = &*obj_val.borrow()
                    && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
                    && let Value::Object(proto) = &*proto_val.borrow()
                {
                    rest_obj.borrow_mut(mc).prototype = Some(*proto);
                }
                let ordered = if let Value::Object(obj) = &rhs {
                    crate::core::ordinary_own_property_keys_mc(mc, obj)?
                } else {
                    Vec::new()
                };
                if let Value::Object(obj) = &rhs {
                    // If this object is a proxy wrapper, delegate descriptor/get to proxy traps
                    // so that Proxy traps are observed (ownKeys was already delegated above).
                    if let Some(proxy_cell) = obj.borrow().properties.get(&PropertyKey::String("__proxy__".to_string()))
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        for k in ordered {
                            if excluded_keys.iter().any(|ex| ex == &k) {
                                continue;
                            }
                            // Ask proxy for own property descriptor and check [[Enumerable]]
                            let desc_enum_opt = crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &k)?;
                            if desc_enum_opt.is_none() {
                                continue;
                            }
                            if !desc_enum_opt.unwrap() {
                                continue;
                            }
                            // Get property value via proxy get trap
                            let val_opt = crate::js_proxy::proxy_get_property(mc, proxy, &k)?;
                            let v = val_opt.unwrap_or(Value::Undefined);
                            object_set_key_value(mc, &rest_obj, k.clone(), &v)?;
                        }
                    } else {
                        for k in ordered {
                            if !obj.borrow().is_enumerable(&k) {
                                continue;
                            }
                            if excluded_keys.iter().any(|ex| ex == &k) {
                                continue;
                            }
                            let v = get_property_with_accessors(mc, env, obj, &k)?;
                            object_set_key_value(mc, &rest_obj, k.clone(), &v)?;
                        }
                    }
                } else if let Value::String(s) = &rhs {
                    let len = s.len();
                    for i in 0..len {
                        let key = PropertyKey::from(i.to_string());
                        if excluded_keys.iter().any(|ex| ex == &key) {
                            continue;
                        }
                        let ch = s.get(i..=i).unwrap_or(&[]).to_vec();
                        object_set_key_value(mc, &rest_obj, key.clone(), &Value::String(ch))?;
                    }
                }
                match target_expr {
                    Expr::Var(name, _, _) => {
                        env_set_recursive(mc, env, name, &Value::Object(rest_obj))?;
                    }
                    other => {
                        evaluate_assign_target_with_value(mc, env, other, &Value::Object(rest_obj))?;
                    }
                }
                continue;
            }

            let name_val = evaluate_expr(mc, env, key_expr)?;
            let source_key = match name_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &name_val, "string", env)?;
                    match prim {
                        Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                        Value::Symbol(s) => PropertyKey::Symbol(s),
                        other => PropertyKey::String(value_to_string(&other)),
                    }
                }
                _ => PropertyKey::String(value_to_string(&name_val)),
            };
            excluded_keys.push(source_key.clone());

            let mut precomputed_temp: Option<TargetTemp<'gc>> = None;
            match target_expr {
                Expr::Assign(lhs, _) => {
                    if !matches!(&**lhs, Expr::Array(_) | Expr::Object(_)) {
                        precomputed_temp = Some(precompute_target(mc, env, lhs)?);
                    }
                }
                Expr::Var(..) | Expr::Property(..) | Expr::Index(..) => {
                    precomputed_temp = Some(precompute_target(mc, env, target_expr)?);
                }
                _ => {}
            }

            let rhs_value = match &rhs {
                Value::Object(obj) => get_property_with_accessors(mc, env, obj, &source_key)?,
                _ => get_primitive_prototype_property(mc, env, &rhs, &source_key)?,
            };

            match target_expr {
                Expr::Assign(boxed_lhs, boxed_default) => {
                    if matches!(rhs_value, Value::Undefined) {
                        let dv = evaluate_expr(mc, env, boxed_default)?;
                        maybe_set_default_name(mc, boxed_lhs, boxed_default, &dv)?;
                        match precomputed_temp.take() {
                            Some(TargetTemp::Var(_)) => {
                                evaluate_assign_target_with_value(mc, env, boxed_lhs, &dv)?;
                            }
                            Some(temp) => {
                                put_temp(mc, env, temp, &dv)?;
                            }
                            None => {
                                evaluate_assign_target_with_value(mc, env, boxed_lhs, &dv)?;
                            }
                        }
                    } else if let Some(temp) = precomputed_temp.take() {
                        put_temp(mc, env, temp, &rhs_value)?;
                    } else {
                        evaluate_assign_target_with_value(mc, env, boxed_lhs, &rhs_value)?;
                    }
                }
                Expr::Var(_, _, _) => {
                    if let Some(temp) = precomputed_temp.take() {
                        put_temp(mc, env, temp, &rhs_value)?;
                    } else if let Expr::Var(name, _, _) = target_expr {
                        env_set_recursive(mc, env, name, &rhs_value)?;
                    }
                }
                other => {
                    if let Some(temp) = precomputed_temp.take() {
                        put_temp(mc, env, temp, &rhs_value)?;
                    } else {
                        evaluate_assign_target_with_value(mc, env, other, &rhs_value)?;
                    }
                }
            }
        }

        return Ok(rhs);
    }

    let pre = match target {
        Expr::Var(name, _, _) => Precomputed::Var(name.clone()),
        Expr::Property(obj_expr, key) => {
            // If this is a `super.prop` form, do not resolve `super` yet; defer
            // until PutValue. Otherwise evaluate base now.
            if let Expr::Super = &**obj_expr {
                Precomputed::SuperProperty(Box::new(key.into()))
            } else {
                let obj_val = evaluate_expr(mc, env, obj_expr)?;
                // Do not error yet on null/undefined; defer ToObject until PutValue
                Precomputed::Property(Box::new(obj_val), Box::new(key.into()))
            }
        }
        Expr::SuperProperty(key) => Precomputed::SuperProperty(Box::new(key.into())),
        Expr::PrivateMember(obj_expr, name) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let pv = evaluate_var(mc, env, name)?;
            if let Value::PrivateName(n, id) = pv {
                Precomputed::PrivateMember(Box::new(obj_val), n, id)
            } else {
                return Err(raise_syntax_error!(format!("Private field '{name}' must be declared in an enclosing class")).into());
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            // If this is `super[... ]`, evaluate the raw key expression but defer
            // resolving `super` until PutValue.
            if let Expr::Super = &**obj_expr {
                let raw_key = evaluate_expr(mc, env, key_expr)?; // raw value; ToPropertyKey deferred
                Precomputed::SuperIndex(Box::new(raw_key))
            } else {
                let obj_val = evaluate_expr(mc, env, obj_expr)?;
                let key_val_res = evaluate_expr(mc, env, key_expr)?;
                // Defer ToPropertyKey conversion until after RHS evaluation
                Precomputed::Index(Box::new(obj_val), Box::new(key_val_res))
            }
        }
        _ => {
            // Unsupported assignment target
            let variant = match target {
                Expr::Var(_, _, _) => "Var",
                Expr::Property(_, _) => "Property",
                Expr::Index(_, _) => "Index",
                Expr::Array(_) => "Array",
                Expr::Object(_) => "Object",
                Expr::Spread(_) => "Spread",
                Expr::Call(_, _) => "Call",
                Expr::New(_, _) => "New",
                Expr::Function(_, _, _) => "Function",
                Expr::ArrowFunction(_, _) => "ArrowFunction",
                Expr::Assign(_, _) => "Assign",
                Expr::Comma(_, _) => "Comma",
                Expr::OptionalProperty(_, _) => "OptionalProperty",
                Expr::OptionalPrivateMember(_, _) => "OptionalPrivateMember",
                Expr::OptionalIndex(_, _) => "OptionalIndex",
                Expr::OptionalCall(_, _) => "OptionalCall",
                Expr::TaggedTemplate(_, _, _) => "TaggedTemplate",
                Expr::TemplateString(_) => "TemplateString",
                Expr::GeneratorFunction(_, _, _) => "GeneratorFunction",
                Expr::AsyncFunction(_, _, _) => "AsyncFunction",
                Expr::AsyncArrowFunction(_, _) => "AsyncArrowFunction",
                _ => "Other",
            };
            log::trace!("Unsupported assignment target reached in evaluate_expr_assign: {variant}");
            return Err(raise_eval_error!("Assignment target not supported").into());
        }
    };

    // Now evaluate RHS
    let val = if let Expr::Class(class_def) = value_expr
        && class_def.name.is_empty()
        && maybe_name_to_set.is_some()
    {
        // Specialized NamedEvaluation for class expressions to ensure static initializers
        // and other class-internal steps see the inferred name when the RHS is an
        // anonymous class expression being assigned to a target identifier.
        let inferred_name = maybe_name_to_set.clone().unwrap();
        create_class_object(mc, &inferred_name, &class_def.extends, &class_def.members, env, false)?
    } else {
        evaluate_expr(mc, env, value_expr)?
    };

    // NamedEvaluation: after RHS evaluated, set function 'name' if appropriate
    if let Some(nm) = maybe_name_to_set.clone() {
        log::debug!(
            "NamedEvaluation: maybe_name_to_set={:?} value_expr={:?} val={:?}",
            maybe_name_to_set,
            value_expr,
            val
        );
        log::debug!("NamedEvaluation: candidate name='{}' val={:?}", nm, val);
        if let Value::Object(obj) = &val {
            // If the function is an arrow function, prefer to set the name (arrow functions are anonymous and
            // should receive the assignment target name). For other anonymous functions, only set if the
            // existing name is absent or empty.
            let mut should_set = false;
            let force_set_for_arrow = matches!(
                value_expr,
                crate::core::Expr::ArrowFunction(..) | crate::core::Expr::AsyncArrowFunction(..)
            ) || format!("{:?}", value_expr).contains("ArrowFunction");
            log::trace!("NamedEvaluation: force_set_for_arrow = {force_set_for_arrow} value_expr={value_expr:?}",);
            if force_set_for_arrow {
                should_set = true;
            } else if let Some(name_rc) = object_get_key_value(obj, "name") {
                // Normalize the existing name property's stored value to a Value and convert to string
                log::debug!("NamedEvaluation: existing name prop = {:?}", name_rc.borrow().clone());
                let existing_val = match &*name_rc.borrow() {
                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                    other => other.clone(),
                };
                // convert to a JS string representation to check emptiness robustly
                let name_str = crate::core::value_to_string(&existing_val);
                log::debug!("NamedEvaluation: existing name resolved to string = '{}'", name_str);
                if name_str.is_empty() {
                    should_set = true;
                }
            } else {
                should_set = true;
            }
            log::debug!("NamedEvaluation: should_set = {}", should_set);

            if should_set {
                let desc = create_descriptor_object(mc, &Value::String(crate::unicode::utf8_to_utf16(&nm)), false, false, true)?;
                crate::js_object::define_property_internal(mc, obj, "name", &desc)?;
                log::debug!("NamedEvaluation: set name='{}' on object", nm);
            } else {
                log::debug!("NamedEvaluation: did not set name for object");
            }
        }
    }

    // Perform the actual assignment using precomputed LHS components
    match pre {
        Precomputed::Var(name) => {
            // Disallow assignment to the special 'arguments' binding in strict-function scope
            if name == "arguments" {
                let caller_is_strict = env_get_strictness(env);
                log::trace!(
                    "DEBUG: assignment to 'arguments' detected: env ptr={:p} is_function_scope={} caller_is_strict={}",
                    env,
                    env.borrow().is_function_scope,
                    caller_is_strict
                );
                if env.borrow().is_function_scope && caller_is_strict {
                    return Err(raise_syntax_error!("Assignment to 'arguments' is not allowed in strict mode").into());
                }
            }
            env_set_recursive(mc, env, &name, &val)?;
            Ok(val)
        }
        Precomputed::Property(base_val, key) => {
            // ToObject semantics: throw on null/undefined, box primitives, or use object directly.
            let obj = match *base_val {
                Value::Object(o) => o,
                Value::Undefined | Value::Null => return Err(raise_type_error!("Cannot assign to property of non-object").into()),
                Value::Number(n) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", &Value::Function("Number_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", &Value::Function("Number_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "__value__", &Value::Number(n))?;
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Number");
                    obj
                }
                Value::Boolean(b) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", &Value::Function("Boolean_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", &Value::Function("Boolean_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "__value__", &Value::Boolean(b))?;
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Boolean");
                    obj
                }
                Value::String(s) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", &Value::Function("String_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", &Value::Function("String_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "length", &Value::Number(s.len() as f64))?;
                    object_set_key_value(mc, &obj, "__value__", &Value::String(s.clone()))?;
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "String");
                    obj
                }
                Value::BigInt(h) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "__value__", &Value::BigInt(h.clone()))?;
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "BigInt");
                    obj
                }
                Value::Symbol(sym_rc) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "__value__", &Value::Symbol(sym_rc))?;
                    obj
                }
                _ => return Err(raise_eval_error!("Cannot assign to property of non-object").into()),
            };
            set_property_with_accessors(mc, env, &obj, &*key, &val)?;
            Ok(val)
        }
        Precomputed::Index(base_val, raw_key) => {
            // Convert raw_key to PropertyKey now (ToPropertyKey may throw)
            let key = match *raw_key {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &raw_key, "string", env)?;
                    match prim {
                        Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                        Value::Symbol(s) => PropertyKey::Symbol(s),
                        other => PropertyKey::String(value_to_string(&other)),
                    }
                }
                _ => PropertyKey::String(value_to_string(&raw_key)),
            };

            let obj = match *base_val {
                Value::Object(o) => o,
                Value::Undefined | Value::Null => return Err(raise_type_error!("Cannot assign to property of non-object").into()),
                Value::Number(n) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", &Value::Function("Number_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", &Value::Function("Number_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "__value__", &Value::Number(n))?;
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Number");
                    obj
                }
                Value::Boolean(b) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", &Value::Function("Boolean_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", &Value::Function("Boolean_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "__value__", &Value::Boolean(b))?;
                    crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Boolean")?;
                    obj
                }
                Value::String(s) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", &Value::Function("String_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", &Value::Function("String_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "length", &Value::Number(s.len() as f64))?;
                    object_set_key_value(mc, &obj, "__value__", &Value::String(s.clone()))?;
                    crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "String")?;
                    obj
                }
                Value::BigInt(h) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "__value__", &Value::BigInt(h.clone()))?;
                    crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "BigInt")?;
                    obj
                }
                Value::Symbol(sym_rc) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "__value__", &Value::Symbol(sym_rc))?;
                    obj
                }
                _ => return Err(raise_eval_error!("Cannot assign to property of non-object").into()),
            };

            set_property_with_accessors(mc, env, &obj, &key, &val)?;
            Ok(val)
        }
        Precomputed::PrivateMember(base_val, name, id) => {
            if let Value::Object(obj) = *base_val {
                set_property_with_accessors(mc, env, &obj, PropertyKey::Private(name, id), &val)?;
                Ok(val)
            } else {
                Err(raise_type_error!("Cannot write private member to non-object").into())
            }
        }
        Precomputed::SuperProperty(key) => {
            let (receiver, super_base) = resolve_super_assignment_base(env)?;
            set_super_property_with_accessors(mc, env, &receiver, super_base, &key, &val)?;
            Ok(val)
        }
        Precomputed::SuperIndex(raw_key) => {
            // Similar to SuperProperty: resolve property on prototype chain but call
            // setter with the original receiver (`this`). Convert raw key to PropertyKey
            let key = match *raw_key {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(n.to_string()),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                _ => PropertyKey::from(value_to_string(&raw_key)),
            };
            let (receiver, super_base) = resolve_super_assignment_base(env)?;
            set_super_property_with_accessors(mc, env, &receiver, super_base, &key, &val)?;
            Ok(val)
        }
    }
}

fn resolve_super_assignment_base<'gc>(
    env: &JSObjectDataPtr<'gc>,
) -> Result<(JSObjectDataPtr<'gc>, Option<JSObjectDataPtr<'gc>>), EvalError<'gc>> {
    let home_obj = env
        .borrow()
        .get_home_object()
        .ok_or_else(|| raise_reference_error!("super is not available"))?;

    let receiver = match crate::core::env_get(env, "this") {
        Some(this_val_ptr) => match &*this_val_ptr.borrow() {
            Value::Object(o) => *o,
            _ => return Err(raise_type_error!("Invalid receiver for super assignment").into()),
        },
        None => return Err(raise_type_error!("Invalid receiver for super assignment").into()),
    };

    let super_base = {
        let home_ptr = *home_obj.borrow();
        home_ptr.borrow().prototype
    };

    // If the home object's prototype is null, super property assignments should throw
    if super_base.is_none() {
        return Err(raise_type_error!("Cannot access 'super' of a class with null prototype").into());
    }

    Ok((receiver, super_base))
}

fn set_super_property_with_accessors<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    mut super_base: Option<JSObjectDataPtr<'gc>>,
    key: &PropertyKey<'gc>,
    val: &Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    let strict = crate::core::env_get_strictness(env);

    while let Some(proto_obj) = super_base {
        if let Some(prop_ptr) = get_own_property(&proto_obj, key) {
            let prop = prop_ptr.borrow().clone();
            match prop {
                Value::Property { setter, getter, .. } => {
                    if let Some(s) = setter {
                        let s_clone = (*s).clone();
                        return call_setter(mc, receiver, &s_clone, val);
                    }
                    if getter.is_some() {
                        return Err(raise_type_error!("Cannot set property which has only a getter").into());
                    }
                    let writable = { proto_obj.borrow().is_writable(key) };
                    if !writable {
                        if strict {
                            return Err(raise_type_error!("Cannot assign to read-only property").into());
                        }
                        return Ok(());
                    }
                    object_set_key_value(mc, receiver, key, val)?;
                    return Ok(());
                }
                Value::Setter(params, body, captured_env, home_opt) => {
                    return call_setter_raw(mc, receiver, &params, &body, &captured_env, home_opt.clone(), val);
                }
                Value::Getter(..) => {
                    return Err(raise_type_error!("Cannot set property which has only a getter").into());
                }
                _ => {
                    let writable = { proto_obj.borrow().is_writable(key) };
                    if !writable {
                        if strict {
                            return Err(raise_type_error!("Cannot assign to read-only property").into());
                        }
                        return Ok(());
                    }
                    object_set_key_value(mc, receiver, key, val)?;
                    return Ok(());
                }
            }
        }
        super_base = proto_obj.borrow().prototype;
    }

    object_set_key_value(mc, receiver, key, val)?;
    Ok(())
}

// Helper: assign a precomputed runtime value to an assignment target expression
pub(crate) fn evaluate_assign_target_with_value<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    val: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Assign(lhs, default_expr) => {
            let mut assigned_val = val.clone();
            let mut used_default = false;
            if matches!(assigned_val, Value::Undefined) {
                assigned_val = evaluate_expr(mc, env, default_expr)?;
                used_default = true;
            }

            if used_default && let Expr::Var(name, _, _) = &**lhs {
                let is_arrow = matches!(&**default_expr, Expr::ArrowFunction(..) | Expr::AsyncArrowFunction(..));
                let is_anon_fn = matches!(
                    &**default_expr,
                    Expr::Function(None, ..)
                        | Expr::GeneratorFunction(None, ..)
                        | Expr::AsyncFunction(None, ..)
                        | Expr::AsyncGeneratorFunction(None, ..)
                );
                let is_anon_class = matches!(
                    &**default_expr,
                    Expr::Class(class_def) if class_def.name.is_empty()
                );

                if (is_arrow || is_anon_fn || is_anon_class)
                    && let Value::Object(obj) = &assigned_val
                {
                    let mut should_set = false;
                    if is_arrow {
                        should_set = true;
                    } else if let Some(name_rc) = object_get_key_value(obj, "name") {
                        let existing_val = match &*name_rc.borrow() {
                            Value::Property { value: Some(v), .. } => v.borrow().clone(),
                            other => other.clone(),
                        };
                        let name_str = value_to_string(&existing_val);
                        if name_str.is_empty() {
                            should_set = true;
                        }
                    } else {
                        should_set = true;
                    }

                    if should_set {
                        let desc = create_descriptor_object(mc, &Value::String(utf8_to_utf16(name)), false, false, true)?;
                        crate::js_object::define_property_internal(mc, obj, "name", &desc)?;
                    }
                }
            }

            evaluate_assign_target_with_value(mc, env, lhs, &assigned_val)
        }
        Expr::Var(name, _, _) => {
            // Disallow assignment to the special 'arguments' binding in strict-function scope
            if name == "arguments" {
                let caller_is_strict = env_get_strictness(env);
                log::trace!(
                    "DEBUG: assignment-to-arguments (assign_target_with_value): env ptr={:p} is_function_scope={} caller_is_strict={}",
                    env,
                    env.borrow().is_function_scope,
                    caller_is_strict
                );
                if env.borrow().is_function_scope && caller_is_strict {
                    return Err(crate::raise_syntax_error!("Assignment to 'arguments' is not allowed in strict mode").into());
                }
            }

            // DEBUG: log assignments to 'await' to diagnose nested async scoping issues
            if name == "await" {
                println!(
                    "DEBUG: assign to 'await' in env ptr={:p} is_function_scope={} has_prop={}",
                    env,
                    env.borrow().is_function_scope,
                    env.borrow().properties.contains_key(&PropertyKey::String("await".to_string()))
                );
            }

            env_set_recursive(mc, env, name, val)?;
            Ok(val.clone())
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = if expr_contains_optional_chain(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };
            if let Value::Object(obj) = obj_val {
                set_property_with_accessors(mc, env, &obj, key, val)?;
                Ok(val.clone())
            } else {
                Err(raise_eval_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let pv = evaluate_var(mc, env, name)?;
                if let Value::PrivateName(n, id) = pv {
                    set_property_with_accessors(mc, env, &obj, PropertyKey::Private(n, id), val)?;
                    Ok(val.clone())
                } else {
                    Err(raise_syntax_error!(format!("Private field '{}' must be declared in an enclosing class", name)).into())
                }
            } else {
                Err(raise_type_error!("Cannot access private member of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val_res = evaluate_expr(mc, env, key_expr)?;

            let key = match key_val_res {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(n.to_string()),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                _ => PropertyKey::from(value_to_string(&key_val_res)),
            };

            if let Value::Object(obj) = obj_val {
                set_property_with_accessors(mc, env, &obj, &key, val)?;
                Ok(val.clone())
            } else {
                Err(raise_eval_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Array(elements) => {
            // Array destructuring
            // Evaluate RHS first
            let rhs = val.clone();
            if matches!(rhs, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot destructure undefined or null").into());
            }
            // Obtain iterator if available
            let mut iterator: Option<crate::core::JSObjectDataPtr<'gc>> = None;
            println!("DEBUG: before iterator lookup in assign_target");
            if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
                && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
            {
                let method = if let Value::Object(obj) = &rhs {
                    get_property_with_accessors(mc, env, obj, iter_sym_data)?
                } else {
                    get_primitive_prototype_property(mc, env, &rhs, iter_sym_data)?
                };
                println!("DEBUG: method for iterator: {:?}", method);
                if matches!(method, Value::Undefined | Value::Null) {
                    return Err(raise_type_error!("Object is not iterable").into());
                }
                let res = evaluate_call_dispatch(mc, env, &method, Some(&rhs), &[])?;
                if let Value::Object(iter_obj) = res {
                    iterator = Some(iter_obj);
                }
            }
            if let Some(iter_obj) = iterator {
                // Iterate and assign
                let mut iterator_done = false;
                for elem_opt in elements.iter() {
                    if let Some(elem_expr) = elem_opt
                        && let Expr::Spread(spread_expr) = elem_expr
                    {
                        let rest_obj = crate::js_array::create_array(mc, env)?;
                        let mut idx2 = 0_usize;
                        if !iterator_done {
                            loop {
                                let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                                if matches!(next_method, Value::Undefined | Value::Null) {
                                    return Err(raise_type_error!("Iterator has no next method").into());
                                }
                                let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                                if let Value::Object(next_res) = next_res_val {
                                    let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                                    let done = matches!(done_val, Value::Boolean(true));
                                    if done {
                                        iterator_done = true;
                                        break;
                                    }
                                    let value = get_property_with_accessors(mc, env, &next_res, "value")?;
                                    object_set_key_value(mc, &rest_obj, idx2, &value)?;
                                    idx2 += 1;
                                } else {
                                    return Err(raise_type_error!("Iterator result is not an object").into());
                                }
                            }
                        }
                        object_set_key_value(mc, &rest_obj, "length", &Value::Number(idx2 as f64))?;
                        match &**spread_expr {
                            Expr::Var(name, _, _) => {
                                env_set_recursive(mc, env, name, &Value::Object(rest_obj))?;
                            }
                            other => {
                                evaluate_assign_target_with_value(mc, env, other, &Value::Object(rest_obj))?;
                            }
                        }
                        break;
                    }

                    let value = if iterator_done {
                        Value::Undefined
                    } else {
                        let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                        if matches!(next_method, Value::Undefined | Value::Null) {
                            return Err(raise_type_error!("Iterator has no next method").into());
                        }
                        let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                        if let Value::Object(next_res) = next_res_val {
                            let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                            let done = matches!(done_val, Value::Boolean(true));
                            if done {
                                iterator_done = true;
                                Value::Undefined
                            } else {
                                get_property_with_accessors(mc, env, &next_res, "value")?
                            }
                        } else {
                            return Err(raise_type_error!("Iterator result is not an object").into());
                        }
                    };

                    if let Some(elem_expr) = elem_opt {
                        match elem_expr {
                            Expr::Var(name, _, _) => {
                                env_set_recursive(mc, env, name, &value)?;
                            }
                            Expr::Assign(_, _) => {
                                evaluate_assign_target_with_value(mc, env, elem_expr, &value)?;
                            }
                            other => {
                                evaluate_assign_target_with_value(mc, env, other, &value)?;
                            }
                        }
                    }
                }
                if !iterator_done {
                    iterator_close(mc, env, &iter_obj)?;
                }
                Ok(val.clone())
            } else {
                // If not iterator: assume array-like object
                if let Value::Object(obj) = rhs {
                    log::debug!("Array destructuring fallback for object");
                    // simple index-based extraction
                    for (i, elem_opt) in elements.iter().enumerate() {
                        let val_at = if is_typedarray(&obj) {
                            log::debug!("Array destructuring fallback: TypedArray index {}", i);
                            // For TypedArrays, use property access which handles indexed elements
                            match get_property_with_accessors(mc, env, &obj, i) {
                                Ok(v) => {
                                    log::debug!("Array destructuring fallback: got value {:?}", v);
                                    v
                                }
                                Err(e) => return Err(e),
                            }
                        } else if let Some(cell) = object_get_key_value(&obj, i.to_string()) {
                            cell.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        if let Some(elem_expr) = elem_opt {
                            match elem_expr {
                                Expr::Var(name, _, _) => {
                                    env_set_recursive(mc, env, name, &val_at)?;
                                }
                                Expr::Assign(_, _) => {
                                    evaluate_assign_target_with_value(mc, env, elem_expr, &val_at)?;
                                }
                                Expr::Spread(spread_expr) => {
                                    let rest_obj = crate::js_array::create_array(mc, env)?;
                                    let len = if let Some(len_cell) = object_get_key_value(&obj, "length") {
                                        if let Value::Number(n) = len_cell.borrow().clone() {
                                            n as usize
                                        } else {
                                            0
                                        }
                                    } else {
                                        0
                                    };
                                    let mut idx2 = 0_usize;
                                    for j in i..len {
                                        let v = if is_typedarray(&obj) {
                                            get_property_with_accessors(mc, env, &obj, j)?
                                        } else if let Some(cell) = object_get_key_value(&obj, j.to_string()) {
                                            cell.borrow().clone()
                                        } else {
                                            Value::Undefined
                                        };
                                        object_set_key_value(mc, &rest_obj, idx2, &v)?;
                                        idx2 += 1;
                                    }
                                    object_set_key_value(mc, &rest_obj, "length", &Value::Number(idx2 as f64))?;
                                    match &**spread_expr {
                                        Expr::Var(name, _, _) => {
                                            env_set_recursive(mc, env, name, &Value::Object(rest_obj))?;
                                        }
                                        other => {
                                            evaluate_assign_target_with_value(mc, env, other, &Value::Object(rest_obj))?;
                                        }
                                    }
                                    break;
                                }
                                other => {
                                    evaluate_assign_target_with_value(mc, env, other, &val_at)?;
                                }
                            }
                        }
                    }
                    Ok(val.clone())
                } else {
                    Err(raise_type_error!("Cannot destructure non-object").into())
                }
            }
        }
        Expr::Object(properties) => {
            let rhs = val.clone();
            if matches!(rhs, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot destructure undefined or null").into());
            }

            let mut excluded_keys: Vec<PropertyKey> = Vec::new();
            for (key_expr, target_expr, is_spread, _is_plain) in properties.iter() {
                if *is_spread {
                    let rest_obj = new_js_object_data(mc);
                    if let Some(obj_val) = env_get(env, "Object")
                        && let Value::Object(obj_ctor) = &*obj_val.borrow()
                        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
                        && let Value::Object(proto) = &*proto_val.borrow()
                    {
                        rest_obj.borrow_mut(mc).prototype = Some(*proto);
                    }
                    let ordered = if let Value::Object(obj) = &rhs {
                        crate::core::ordinary_own_property_keys_mc(mc, obj)?
                    } else {
                        Vec::new()
                    };
                    if let Value::Object(obj) = &rhs {
                        // If this object is a proxy wrapper, delegate descriptor/get to proxy traps
                        // so that Proxy traps are observed (ownKeys was already delegated above).
                        if let Some(proxy_cell) = obj.borrow().properties.get(&PropertyKey::String("__proxy__".to_string()))
                            && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                        {
                            for k in ordered {
                                if excluded_keys.iter().any(|ex| ex == &k) {
                                    continue;
                                }
                                println!(
                                    "TRACE: object-literal spread proxy: obj_ptr={:p} proxy_ptr={:p} key={:?}",
                                    obj.as_ptr(),
                                    Gc::as_ptr(*proxy),
                                    k
                                );
                                // Ask proxy for own property descriptor and check [[Enumerable]]
                                let desc_enum_opt = crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &k)?;
                                println!(
                                    "TRACE: object-literal spread proxy: desc_enum_opt={:?} for key={:?}",
                                    desc_enum_opt, k
                                );
                                if desc_enum_opt.is_none() {
                                    continue;
                                }
                                if !desc_enum_opt.unwrap() {
                                    continue;
                                }
                                // Get property value via proxy get trap
                                let val_opt = crate::js_proxy::proxy_get_property(mc, proxy, &k)?;
                                println!("TRACE: object-literal spread proxy: got val_opt={:?} for key={:?}", val_opt, k);
                                let v = val_opt.unwrap_or(Value::Undefined);
                                object_set_key_value(mc, &rest_obj, k.clone(), &v)?;
                            }
                        } else {
                            for k in ordered {
                                if !obj.borrow().is_enumerable(&k) {
                                    continue;
                                }
                                if excluded_keys.iter().any(|ex| ex == &k) {
                                    continue;
                                }
                                let v = get_property_with_accessors(mc, env, obj, &k)?;
                                object_set_key_value(mc, &rest_obj, k.clone(), &v)?;
                            }
                        }
                    } else if let Value::String(s) = &rhs {
                        let len = s.len();
                        for i in 0..len {
                            let key = PropertyKey::from(i.to_string());
                            if excluded_keys.iter().any(|ex| ex == &key) {
                                continue;
                            }
                            let ch = s.get(i..=i).unwrap_or(&[]).to_vec();
                            object_set_key_value(mc, &rest_obj, key.clone(), &Value::String(ch))?;
                        }
                    }
                    match target_expr {
                        Expr::Var(name, _, _) => {
                            env_set_recursive(mc, env, name, &Value::Object(rest_obj))?;
                        }
                        other => {
                            evaluate_assign_target_with_value(mc, env, other, &Value::Object(rest_obj))?;
                        }
                    }
                    continue;
                }

                let name_val = evaluate_expr(mc, env, key_expr)?;
                let source_key = match name_val {
                    Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                    Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                    Value::Symbol(s) => PropertyKey::Symbol(s),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &name_val, "string", env)?;
                        match prim {
                            Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                            Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                            Value::Symbol(s) => PropertyKey::Symbol(s),
                            other => PropertyKey::String(value_to_string(&other)),
                        }
                    }
                    _ => PropertyKey::String(value_to_string(&name_val)),
                };
                excluded_keys.push(source_key.clone());

                let rhs_value = match &rhs {
                    Value::Object(obj) => get_property_with_accessors(mc, env, obj, &source_key)?,
                    _ => get_primitive_prototype_property(mc, env, &rhs, &source_key)?,
                };

                match target_expr {
                    Expr::Assign(_, _) => {
                        evaluate_assign_target_with_value(mc, env, target_expr, &rhs_value)?;
                    }
                    Expr::Var(name, _, _) => {
                        env_set_recursive(mc, env, name, &rhs_value)?;
                    }
                    other => {
                        evaluate_assign_target_with_value(mc, env, other, &rhs_value)?;
                    }
                }
            }
            Ok(val.clone())
        }
        _ => Err(raise_eval_error!("Assignment target not supported").into()),
    }
}

// Helper: binding initialization for parameters and lexical bindings.
// Similar to evaluate_assign_target_with_value but assigns to bindings in the
// current environment without walking the prototype chain or triggering TDZ
// errors on Uninitialized bindings. This mirrors InitializeReferencedBinding.
pub(crate) fn evaluate_binding_target_with_value<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    val: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Assign(lhs, default_expr) => {
            let mut assigned_val = val.clone();
            let mut used_default = false;
            if matches!(assigned_val, Value::Undefined) {
                assigned_val = evaluate_expr(mc, env, default_expr)?;
                used_default = true;
            }
            if used_default && let Expr::Var(name, _, _) = &**lhs {
                maybe_set_function_name_for_default(mc, name, default_expr, &assigned_val)?;
            }
            evaluate_binding_target_with_value(mc, env, lhs, &assigned_val)
        }
        Expr::Var(name, _, _) => {
            env_set(mc, env, name, val)?;
            Ok(val.clone())
        }
        Expr::Array(elements) => {
            let rhs = val.clone();
            if matches!(rhs, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot destructure undefined or null").into());
            }

            let mut iterator: Option<crate::core::JSObjectDataPtr<'gc>> = None;
            if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
                && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
            {
                let method = if let Value::Object(obj) = &rhs {
                    get_property_with_accessors(mc, env, obj, iter_sym_data)?
                } else {
                    get_primitive_prototype_property(mc, env, &rhs, iter_sym_data)?
                };
                if matches!(method, Value::Undefined | Value::Null) {
                    return Err(raise_type_error!("Object is not iterable").into());
                }
                let res = evaluate_call_dispatch(mc, env, &method, Some(&rhs), &[])?;
                if let Value::Object(iter_obj) = res {
                    iterator = Some(iter_obj);
                }
            }

            if let Some(iter_obj) = iterator {
                let mut iterator_done = false;
                for elem_opt in elements.iter() {
                    if let Some(elem_expr) = elem_opt
                        && let Expr::Spread(spread_expr) = elem_expr
                    {
                        let rest_obj = crate::js_array::create_array(mc, env)?;
                        let mut idx2 = 0_usize;
                        if !iterator_done {
                            loop {
                                let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                                if matches!(next_method, Value::Undefined | Value::Null) {
                                    return Err(raise_type_error!("Iterator has no next method").into());
                                }
                                let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                                if let Value::Object(next_res) = next_res_val {
                                    let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                                    let done = matches!(done_val, Value::Boolean(true));
                                    if done {
                                        iterator_done = true;
                                        break;
                                    }
                                    let value = get_property_with_accessors(mc, env, &next_res, "value")?;
                                    object_set_key_value(mc, &rest_obj, idx2, &value)?;
                                    idx2 += 1;
                                } else {
                                    return Err(raise_type_error!("Iterator result is not an object").into());
                                }
                            }
                        }
                        object_set_key_value(mc, &rest_obj, "length", &Value::Number(idx2 as f64))?;
                        match &**spread_expr {
                            Expr::Var(name, _, _) => {
                                env_set(mc, env, name, &Value::Object(rest_obj))?;
                            }
                            other => {
                                evaluate_binding_target_with_value(mc, env, other, &Value::Object(rest_obj))?;
                            }
                        }
                        break;
                    }

                    let value = if iterator_done {
                        Value::Undefined
                    } else {
                        let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                        if matches!(next_method, Value::Undefined | Value::Null) {
                            return Err(raise_type_error!("Iterator has no next method").into());
                        }
                        let next_res_val = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                        if let Value::Object(next_res) = next_res_val {
                            let done_val = get_property_with_accessors(mc, env, &next_res, "done")?;
                            let done = matches!(done_val, Value::Boolean(true));
                            if done {
                                iterator_done = true;
                                Value::Undefined
                            } else {
                                get_property_with_accessors(mc, env, &next_res, "value")?
                            }
                        } else {
                            return Err(raise_type_error!("Iterator result is not an object").into());
                        }
                    };

                    if let Some(elem_expr) = elem_opt {
                        match elem_expr {
                            Expr::Var(name, _, _) => {
                                env_set(mc, env, name, &value)?;
                            }
                            Expr::Assign(_, _) => {
                                evaluate_binding_target_with_value(mc, env, elem_expr, &value)?;
                            }
                            other => {
                                evaluate_binding_target_with_value(mc, env, other, &value)?;
                            }
                        }
                    }
                }
                if !iterator_done {
                    iterator_close(mc, env, &iter_obj)?;
                }
                Ok(val.clone())
            } else {
                Err(raise_type_error!("Object is not iterable (or Symbol.iterator not found)").into())
            }
        }
        Expr::Object(properties) => {
            let rhs = val.clone();
            if matches!(rhs, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot destructure undefined or null").into());
            }

            let mut excluded_keys: Vec<PropertyKey> = Vec::new();
            for (key_expr, target_expr, is_spread, _is_plain) in properties.iter() {
                if *is_spread {
                    let rest_obj = new_js_object_data(mc);
                    if let Some(obj_val) = env_get(env, "Object")
                        && let Value::Object(obj_ctor) = &*obj_val.borrow()
                        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
                        && let Value::Object(proto) = &*proto_val.borrow()
                    {
                        rest_obj.borrow_mut(mc).prototype = Some(*proto);
                    }
                    let ordered = if let Value::Object(obj) = &rhs {
                        crate::core::ordinary_own_property_keys_mc(mc, obj)?
                    } else {
                        Vec::new()
                    };
                    if let Value::Object(obj) = &rhs {
                        for k in ordered {
                            if !obj.borrow().is_enumerable(&k) {
                                continue;
                            }
                            if excluded_keys.iter().any(|ex| ex == &k) {
                                continue;
                            }
                            let v = get_property_with_accessors(mc, env, obj, &k)?;
                            object_set_key_value(mc, &rest_obj, k.clone(), &v)?;
                        }
                    } else if let Value::String(s) = &rhs {
                        let len = s.len();
                        for i in 0..len {
                            let key = PropertyKey::from(i.to_string());
                            if excluded_keys.iter().any(|ex| ex == &key) {
                                continue;
                            }
                            let ch = s.get(i..=i).unwrap_or(&[]).to_vec();
                            object_set_key_value(mc, &rest_obj, key.clone(), &Value::String(ch))?;
                        }
                    }
                    match target_expr {
                        Expr::Var(name, _, _) => {
                            env_set(mc, env, name, &Value::Object(rest_obj))?;
                        }
                        other => {
                            evaluate_binding_target_with_value(mc, env, other, &Value::Object(rest_obj))?;
                        }
                    }
                    continue;
                }

                let name_val = evaluate_expr(mc, env, key_expr)?;
                let source_key = match name_val {
                    Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                    Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                    Value::Symbol(s) => PropertyKey::Symbol(s),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &name_val, "string", env)?;
                        match prim {
                            Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                            Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                            Value::Symbol(s) => PropertyKey::Symbol(s),
                            other => PropertyKey::String(value_to_string(&other)),
                        }
                    }
                    _ => PropertyKey::String(value_to_string(&name_val)),
                };
                excluded_keys.push(source_key.clone());

                let rhs_value = match &rhs {
                    Value::Object(obj) => get_property_with_accessors(mc, env, obj, &source_key)?,
                    _ => get_primitive_prototype_property(mc, env, &rhs, &source_key)?,
                };

                match target_expr {
                    Expr::Assign(_, _) => {
                        evaluate_binding_target_with_value(mc, env, target_expr, &rhs_value)?;
                    }
                    Expr::Var(name, _, _) => {
                        env_set(mc, env, name, &rhs_value)?;
                    }
                    other => {
                        evaluate_binding_target_with_value(mc, env, other, &rhs_value)?;
                    }
                }
            }
            Ok(val.clone())
        }
        _ => Err(raise_eval_error!("Assignment target not supported").into()),
    }
}

fn compute_add<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    l: &Value<'gc>,
    r: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let l_prim = crate::core::to_primitive(mc, l, "default", env)?;
    let r_prim = crate::core::to_primitive(mc, r, "default", env)?;

    if matches!(l_prim, Value::String(_)) || matches!(r_prim, Value::String(_)) {
        let mut res = match &l_prim {
            Value::String(ls) => ls.clone(),
            _ => utf8_to_utf16(&to_string_for_concat(mc, env, &l_prim)?),
        };
        match &r_prim {
            Value::String(rs) => res.extend(rs.clone()),
            _ => res.extend(utf8_to_utf16(&to_string_for_concat(mc, env, &r_prim)?)),
        }
        return Ok(Value::String(res));
    }

    match (l_prim, r_prim) {
        (Value::BigInt(ln), Value::BigInt(rn)) => Ok(Value::BigInt(Box::new(*ln + *rn))),
        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
        (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln + rn)),
        (lprim, rprim) => Ok(Value::Number(to_number(&lprim)? + to_number(&rprim)?)),
    }
}

fn to_property_key_for_assignment<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    key_val: &Value<'gc>,
) -> Result<PropertyKey<'gc>, EvalError<'gc>> {
    Ok(match key_val {
        Value::String(s) => PropertyKey::String(utf16_to_utf8(s)),
        Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(*n))),
        Value::Symbol(s) => PropertyKey::Symbol(*s),
        Value::Object(_) => {
            let prim = crate::core::to_primitive(mc, key_val, "string", env)?;
            match prim {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                other => PropertyKey::String(value_to_string(&other)),
            }
        }
        _ => PropertyKey::String(value_to_string(key_val)),
    })
}

fn eval_private_member_ref<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj_expr: &Expr,
    name: &str,
) -> Result<(JSObjectDataPtr<'gc>, PropertyKey<'gc>), EvalError<'gc>> {
    let obj_val = evaluate_expr(mc, env, obj_expr)?;
    let obj = match obj_val {
        Value::Object(obj) => obj,
        Value::Undefined | Value::Null => {
            return Err(raise_type_error!("Cannot read properties of null or undefined").into());
        }
        _ => return Err(raise_type_error!("Cannot access private field on non-object").into()),
    };
    let pv = evaluate_var(mc, env, name)?;
    if let Value::PrivateName(n, id) = pv {
        Ok((obj, PropertyKey::Private(n, id)))
    } else {
        Err(raise_syntax_error!(format!("Private field '{name}' must be declared in an enclosing class")).into())
    }
}

fn evaluate_expr_add_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = compute_add(mc, env, &current, &val)?;
            env_set_recursive(mc, env, name, &new_val)?;
            Ok(new_val)
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = if expr_contains_optional_chain(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = compute_add(mc, env, &current, &val)?;
                set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = if expr_contains_optional_chain(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = compute_add(mc, env, &current, &val)?;
                set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = compute_add(mc, env, &current, &val)?;
            set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
            Ok(new_val)
        }
        _ => Err(raise_eval_error!("AddAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_bitand_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let current_num = to_numeric_with_env(mc, env, &current)?;
            let val_num = to_numeric_with_env(mc, env, &val)?;
            match (current_num, val_num) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(Box::new(*ln & *rn));
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(l), Value::Number(r)) => {
                    let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                    let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                    let new_val = Value::Number((l & r) as f64);
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise AND assignment").into()),
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = if expr_contains_optional_chain(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let current_num = to_numeric_with_env(mc, env, &current)?;
                let val_num = to_numeric_with_env(mc, env, &val)?;
                match (current_num, val_num) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(Box::new(*ln & *rn));
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (Value::Number(l), Value::Number(r)) => {
                        let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                        let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                        let new_val = Value::Number((l & r) as f64);
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                    _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise AND assignment").into()),
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let current_num = to_numeric_with_env(mc, env, &current)?;
                let val_num = to_numeric_with_env(mc, env, &val)?;
                match (current_num, val_num) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(Box::new(*ln & *rn));
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (Value::Number(l), Value::Number(r)) => {
                        let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                        let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                        let new_val = Value::Number((l & r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                    _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise AND assignment").into()),
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let current_num = to_numeric_with_env(mc, env, &current)?;
            let val_num = to_numeric_with_env(mc, env, &val)?;
            match (current_num, val_num) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(Box::new(*ln & *rn));
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(l), Value::Number(r)) => {
                    let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                    let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                    let new_val = Value::Number((l & r) as f64);
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise AND assignment").into()),
            }
        }
        _ => Err(raise_eval_error!("BitAndAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_bitor_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let current_num = to_numeric_with_env(mc, env, &current)?;
            let val_num = to_numeric_with_env(mc, env, &val)?;
            match (current_num, val_num) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(Box::new(*ln | *rn));
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(l), Value::Number(r)) => {
                    let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                    let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                    let new_val = Value::Number((l | r) as f64);
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise OR assignment").into()),
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = if expr_contains_optional_chain(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let current_num = to_numeric_with_env(mc, env, &current)?;
                let val_num = to_numeric_with_env(mc, env, &val)?;
                match (current_num, val_num) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(Box::new(*ln | *rn));
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (Value::Number(l), Value::Number(r)) => {
                        let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                        let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                        let new_val = Value::Number((l | r) as f64);
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                    _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise OR assignment").into()),
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let current_num = to_numeric_with_env(mc, env, &current)?;
                let val_num = to_numeric_with_env(mc, env, &val)?;
                match (current_num, val_num) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(Box::new(*ln | *rn));
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (Value::Number(l), Value::Number(r)) => {
                        let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                        let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                        let new_val = Value::Number((l | r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                    _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise OR assignment").into()),
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let current_num = to_numeric_with_env(mc, env, &current)?;
            let val_num = to_numeric_with_env(mc, env, &val)?;
            match (current_num, val_num) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(Box::new(*ln | *rn));
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(l), Value::Number(r)) => {
                    let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                    let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                    let new_val = Value::Number((l | r) as f64);
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise OR assignment").into()),
            }
        }
        _ => Err(raise_eval_error!("BitOrAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_bitxor_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let current_num = to_numeric_with_env(mc, env, &current)?;
            let val_num = to_numeric_with_env(mc, env, &val)?;
            match (current_num, val_num) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(Box::new(*ln ^ *rn));
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(l), Value::Number(r)) => {
                    let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                    let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                    let new_val = Value::Number((l ^ r) as f64);
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise XOR assignment").into()),
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let current_num = to_numeric_with_env(mc, env, &current)?;
                let val_num = to_numeric_with_env(mc, env, &val)?;
                match (current_num, val_num) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(Box::new(*ln ^ *rn));
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (Value::Number(l), Value::Number(r)) => {
                        let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                        let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                        let new_val = Value::Number((l ^ r) as f64);
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                    _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise XOR assignment").into()),
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let current_num = to_numeric_with_env(mc, env, &current)?;
                let val_num = to_numeric_with_env(mc, env, &val)?;
                match (current_num, val_num) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(Box::new(*ln ^ *rn));
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (Value::Number(l), Value::Number(r)) => {
                        let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                        let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                        let new_val = Value::Number((l ^ r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                    _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise XOR assignment").into()),
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let current_num = to_numeric_with_env(mc, env, &current)?;
            let val_num = to_numeric_with_env(mc, env, &val)?;
            match (current_num, val_num) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(Box::new(*ln ^ *rn));
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(l), Value::Number(r)) => {
                    let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                    let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                    let new_val = Value::Number((l ^ r) as f64);
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise XOR assignment").into()),
            }
        }
        _ => Err(raise_eval_error!("BitXorAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_leftshift_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let shift = bigint_shift_count(&rn)?;
                    let new_val = Value::BigInt(Box::new(*ln << shift));
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (l, r) => {
                    let l = to_int32_value_with_env(mc, env, &l)?;
                    let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                    let new_val = Value::Number(((l << r) as i32) as f64);
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let shift = bigint_shift_count(&rn)?;
                        let new_val = Value::BigInt(Box::new(*ln << shift));
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (l, r) => {
                        let l = to_int32_value_with_env(mc, env, &l)?;
                        let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                        let new_val = Value::Number(((l << r) as i32) as f64);
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let shift = bigint_shift_count(&rn)?;
                        let new_val = Value::BigInt(Box::new(*ln << shift));
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (l, r) => {
                        let l = to_int32_value_with_env(mc, env, &l)?;
                        let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                        let new_val = Value::Number(((l << r) as i32) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let shift = bigint_shift_count(&rn)?;
                    let new_val = Value::BigInt(Box::new(*ln << shift));
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (l, r) => {
                    let l = to_int32_value_with_env(mc, env, &l)?;
                    let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                    let new_val = Value::Number(((l << r) as i32) as f64);
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
            }
        }
        _ => Err(raise_eval_error!("LeftShiftAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_rightshift_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let shift = bigint_shift_count(&rn)?;
                    let new_val = Value::BigInt(Box::new(*ln >> shift));
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (l, r) => {
                    let l = to_int32_value_with_env(mc, env, &l)?;
                    let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                    let new_val = Value::Number((l >> r) as f64);
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let shift = bigint_shift_count(&rn)?;
                        let new_val = Value::BigInt(Box::new(*ln >> shift));
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (l, r) => {
                        let l = to_int32_value_with_env(mc, env, &l)?;
                        let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                        let new_val = Value::Number((l >> r) as f64);
                        set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let shift = bigint_shift_count(&rn)?;
                        let new_val = Value::BigInt(Box::new(*ln >> shift));
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                    (l, r) => {
                        let l = to_int32_value_with_env(mc, env, &l)?;
                        let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                        let new_val = Value::Number((l >> r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let shift = bigint_shift_count(&rn)?;
                    let new_val = Value::BigInt(Box::new(*ln >> shift));
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (l, r) => {
                    let l = to_int32_value_with_env(mc, env, &l)?;
                    let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                    let new_val = Value::Number((l >> r) as f64);
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
            }
        }
        _ => Err(raise_eval_error!("RightShiftAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_urightshift_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            match (current, val) {
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Unsigned right shift").into()),
                (l, r) => {
                    let l = to_uint32_value_with_env(mc, env, &l)?;
                    let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                    let new_val = Value::Number((l >> r) as f64);
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                if let Value::BigInt(_) = current {
                    return Err(raise_type_error!("Unsigned right shift").into());
                }
                let (l, r) = (current, val);
                {
                    let l = to_uint32_value_with_env(mc, env, &l)?;
                    let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                    let new_val = Value::Number((l >> r) as f64);
                    set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                    Ok(new_val)
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                if let Value::BigInt(_) = current {
                    return Err(raise_type_error!("Unsigned right shift").into());
                }
                let (l, r) = (current, val);
                {
                    let l = to_uint32_value_with_env(mc, env, &l)?;
                    let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                    let new_val = Value::Number((l >> r) as f64);
                    set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                    Ok(new_val)
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            if let Value::BigInt(_) = current {
                return Err(raise_type_error!("Unsigned right shift").into());
            }
            let (l, r) = (current, val);
            let l = to_uint32_value_with_env(mc, env, &l)?;
            let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
            let new_val = Value::Number((l >> r) as f64);
            set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
            Ok(new_val)
        }
        _ => Err(raise_eval_error!("UnsignedRightShiftAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_sub_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(Box::new(*ln - *rn));
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (l, r) => {
                    // Coerce to numbers (perform ToPrimitive when needed) and validate not NaN
                    let ln = to_number_with_env(mc, env, &l)?;
                    let rn = to_number_with_env(mc, env, &r)?;
                    let new_val = Value::Number(ln - rn);
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(Box::new(*ln - *rn)),
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => {
                        let ln = to_number_with_env(mc, env, &l)?;
                        let rn = to_number_with_env(mc, env, &r)?;
                        Value::Number(ln - rn)
                    }
                };
                set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(Box::new(*ln - *rn)),
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => {
                        let ln = to_number_with_env(mc, env, &l)?;
                        let rn = to_number_with_env(mc, env, &r)?;
                        Value::Number(ln - rn)
                    }
                };
                set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(Box::new(*ln - *rn)),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                }
                (l, r) => {
                    let ln = to_number_with_env(mc, env, &l)?;
                    let rn = to_number_with_env(mc, env, &r)?;
                    Value::Number(ln - rn)
                }
            };
            set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
            Ok(new_val)
        }
        _ => Err(raise_eval_error!("SubAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_mul_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(Box::new(*ln * *rn));
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (l, r) => {
                    let ln = to_number_with_env(mc, env, &l)?;
                    let rn = to_number_with_env(mc, env, &r)?;
                    let new_val = Value::Number(ln * rn);
                    env_set_recursive(mc, env, name, &new_val)?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(Box::new(*ln * *rn)),
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => {
                        let ln = to_number_with_env(mc, env, &l)?;
                        let rn = to_number_with_env(mc, env, &r)?;
                        Value::Number(ln * rn)
                    }
                };
                set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key_str = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key_str)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(Box::new(*ln * *rn)),
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => {
                        let ln = to_number_with_env(mc, env, &l)?;
                        let rn = to_number_with_env(mc, env, &r)?;
                        Value::Number(ln * rn)
                    }
                };
                set_property_with_accessors(mc, env, &obj, &key_str, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(Box::new(*ln * *rn)),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                }
                (l, r) => {
                    let ln = to_number_with_env(mc, env, &l)?;
                    let rn = to_number_with_env(mc, env, &r)?;
                    Value::Number(ln * rn)
                }
            };
            set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
            Ok(new_val)
        }
        _ => Err(raise_eval_error!("MulAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_div_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    if rn.is_zero() {
                        return Err(raise_eval_error!("Division by zero").into());
                    }
                    Value::BigInt(Box::new(*ln / *rn))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                }
                (l, r) => {
                    let denom = to_number_with_env(mc, env, &r)?;
                    let ln = to_number_with_env(mc, env, &l)?;
                    Value::Number(ln / denom)
                }
            };
            env_set_recursive(mc, env, name, &new_val)?;
            Ok(new_val)
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        if rn.is_zero() {
                            return Err(raise_eval_error!("Division by zero").into());
                        }
                        Value::BigInt(Box::new(*ln / *rn))
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => {
                        let denom = to_number_with_env(mc, env, &r)?;
                        let ln = to_number_with_env(mc, env, &l)?;
                        Value::Number(ln / denom)
                    }
                };
                set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        if rn.is_zero() {
                            return Err(raise_eval_error!("Division by zero").into());
                        }
                        Value::BigInt(Box::new(*ln / *rn))
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => {
                        let denom = to_number_with_env(mc, env, &r)?;
                        let ln = to_number_with_env(mc, env, &l)?;
                        Value::Number(ln / denom)
                    }
                };
                set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    if rn.is_zero() {
                        return Err(raise_eval_error!("Division by zero").into());
                    }
                    Value::BigInt(Box::new(*ln / *rn))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                }
                (l, r) => {
                    let denom = to_number_with_env(mc, env, &r)?;
                    let ln = to_number_with_env(mc, env, &l)?;
                    Value::Number(ln / denom)
                }
            };
            set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
            Ok(new_val)
        }
        _ => Err(raise_eval_error!("DivAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_mod_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    if rn.is_zero() {
                        return Err(raise_eval_error!("Division by zero").into());
                    }
                    Value::BigInt(Box::new(*ln % *rn))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                }
                (l, r) => {
                    let denom = to_number_with_env(mc, env, &r)?;
                    let ln = to_number_with_env(mc, env, &l)?;
                    Value::Number(ln % denom)
                }
            };
            env_set_recursive(mc, env, name, &new_val)?;
            Ok(new_val)
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        if rn.is_zero() {
                            return Err(raise_eval_error!("Division by zero").into());
                        }
                        Value::BigInt(Box::new(*ln % *rn))
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => {
                        let denom = to_number_with_env(mc, env, &r)?;
                        let ln = to_number_with_env(mc, env, &l)?;
                        Value::Number(ln % denom)
                    }
                };
                set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        if rn.is_zero() {
                            return Err(raise_eval_error!("Division by zero").into());
                        }
                        Value::BigInt(Box::new(*ln % *rn))
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => {
                        let denom = to_number_with_env(mc, env, &r)?;
                        let ln = to_number_with_env(mc, env, &l)?;
                        Value::Number(ln % denom)
                    }
                };
                set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    if rn.is_zero() {
                        return Err(raise_eval_error!("Division by zero").into());
                    }
                    Value::BigInt(Box::new(*ln % *rn))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                }
                (l, r) => {
                    let denom = to_number_with_env(mc, env, &r)?;
                    let ln = to_number_with_env(mc, env, &l)?;
                    Value::Number(ln % denom)
                }
            };
            set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
            Ok(new_val)
        }
        _ => Err(raise_eval_error!("ModAssign only for variables, properties or indexes").into()),
    }
}

fn evaluate_expr_pow_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = match (current, val) {
                (Value::BigInt(base), Value::BigInt(exp)) => {
                    if exp.sign() == num_bigint::Sign::Minus {
                        return Err(raise_range_error!("Exponent must be non-negative").into());
                    }
                    let e = exp
                        .to_u32()
                        .ok_or_else(|| EvalError::Js(raise_range_error!("Exponent too large")))?;
                    Value::BigInt(Box::new(base.pow(e)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                }
                (l, r) => Value::Number(to_number_with_env(mc, env, &l)?.powf(to_number_with_env(mc, env, &r)?)),
            };
            env_set_recursive(mc, env, name, &new_val)?;
            Ok(new_val)
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(base), Value::BigInt(exp)) => {
                        if exp.sign() == num_bigint::Sign::Minus {
                            return Err(raise_range_error!("Exponent must be non-negative").into());
                        }
                        let e = exp
                            .to_u32()
                            .ok_or_else(|| EvalError::Js(raise_range_error!("Exponent too large")))?;
                        Value::BigInt(Box::new(base.pow(e)))
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => Value::Number(to_number_with_env(mc, env, &l)?.powf(to_number_with_env(mc, env, &r)?)),
                };
                set_property_with_accessors(mc, env, &obj, key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot assign to property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &key_val)?;
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let val = evaluate_expr(mc, env, value_expr)?;
                let new_val = match (current, val) {
                    (Value::BigInt(base), Value::BigInt(exp)) => {
                        if exp.sign() == num_bigint::Sign::Minus {
                            return Err(raise_range_error!("Exponent must be non-negative").into());
                        }
                        let e = exp
                            .to_u32()
                            .ok_or_else(|| EvalError::Js(raise_range_error!("Exponent too large")))?;
                        Value::BigInt(Box::new(base.pow(e)))
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                    }
                    (l, r) => Value::Number(to_number_with_env(mc, env, &l)?.powf(to_number_with_env(mc, env, &r)?)),
                };
                set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
                Ok(new_val)
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let (obj, key) = eval_private_member_ref(mc, env, obj_expr, name)?;
            let current = get_property_with_accessors(mc, env, &obj, &key)?;
            let val = evaluate_expr(mc, env, value_expr)?;
            let new_val = match (current, val) {
                (Value::BigInt(base), Value::BigInt(exp)) => {
                    if exp.sign() == num_bigint::Sign::Minus {
                        return Err(raise_range_error!("Exponent must be non-negative").into());
                    }
                    let e = exp
                        .to_u32()
                        .ok_or_else(|| EvalError::Js(raise_range_error!("Exponent too large")))?;
                    Value::BigInt(Box::new(base.pow(e)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    return Err(raise_type_error!("Cannot mix BigInt and other types").into());
                }
                (l, r) => Value::Number(to_number_with_env(mc, env, &l)?.powf(to_number_with_env(mc, env, &r)?)),
            };
            set_property_with_accessors(mc, env, &obj, &key, &new_val)?;
            Ok(new_val)
        }
        _ => Err(raise_eval_error!("PowAssign only for variables, properties or indexes").into()),
    }
}

pub fn check_top_level_return<'gc>(stmts: &[Statement]) -> Result<(), EvalError<'gc>> {
    for stmt in stmts {
        match &*stmt.kind {
            StatementKind::Return(_) => {
                return Err(raise_syntax_error!("Illegal return statement").into());
            }
            StatementKind::Block(inner) => check_top_level_return(inner)?,
            StatementKind::If(if_stmt) => {
                check_top_level_return(&if_stmt.then_body)?;
                if let Some(else_body) = &if_stmt.else_body {
                    check_top_level_return(else_body)?;
                }
            }
            StatementKind::While(_, body) => check_top_level_return(body)?,
            StatementKind::DoWhile(body, _) => check_top_level_return(body)?,
            StatementKind::For(for_stmt) => check_top_level_return(&for_stmt.body)?,
            StatementKind::ForOf(_, _, _, body) => check_top_level_return(body)?,
            StatementKind::ForAwaitOf(_, _, _, body) => check_top_level_return(body)?,
            StatementKind::ForIn(_, _, _, body) => check_top_level_return(body)?,
            StatementKind::ForOfDestructuringObject(_, _, _, body) => check_top_level_return(body)?,
            StatementKind::ForOfDestructuringArray(_, _, _, body) => check_top_level_return(body)?,
            StatementKind::ForAwaitOfDestructuringObject(_, _, _, body) => check_top_level_return(body)?,
            StatementKind::ForAwaitOfDestructuringArray(_, _, _, body) => check_top_level_return(body)?,
            StatementKind::Switch(sw) => {
                for case in &sw.cases {
                    match case {
                        crate::core::SwitchCase::Case(_, body) => check_top_level_return(body)?,
                        crate::core::SwitchCase::Default(body) => check_top_level_return(body)?,
                    }
                }
            }
            StatementKind::TryCatch(tc) => {
                check_top_level_return(&tc.try_body)?;
                if let Some(c) = &tc.catch_body {
                    check_top_level_return(c)?;
                }
                if let Some(f) = &tc.finally_body {
                    check_top_level_return(f)?;
                }
            }
            StatementKind::Label(_, s) => check_top_level_return(std::slice::from_ref(s))?,
            StatementKind::Export(_, Some(s), _) => check_top_level_return(std::slice::from_ref(s))?,
            _ => {}
        }
    }
    Ok(())
}

fn check_global_declarations<'gc>(env: &JSObjectDataPtr<'gc>, statements: &[Statement]) -> Result<(), EvalError<'gc>> {
    let mut fn_names: Vec<String> = Vec::new();
    for stmt in statements {
        if let StatementKind::FunctionDeclaration(name, ..) = &*stmt.kind
            && !fn_names.contains(name)
        {
            fn_names.push(name.clone());
        }
    }
    // Check in reverse order as per spec semantics
    for name in fn_names.iter().rev() {
        let key = crate::core::PropertyKey::String(name.clone());
        if let Some(existing_rc) = get_own_property(env, &key) {
            // If it's configurable we can replace it freely
            if env.borrow().is_configurable(&key) {
                continue;
            }

            // If the existing property is an accessor, defining a data value would be incompatible
            let existing_is_accessor = match &*existing_rc.borrow() {
                crate::core::Value::Property { getter, setter, .. } => getter.is_some() || setter.is_some(),
                crate::core::Value::Getter(..) | crate::core::Value::Setter(..) => true,
                _ => false,
            };

            if existing_is_accessor {
                return Err(raise_type_error!(format!("Cannot declare global function '{name}'")).into());
            }

            // If it's a non-writable data property and non-configurable, we cannot change its value
            if !env.borrow().is_writable(&key) {
                return Err(raise_type_error!(format!("Cannot declare global function '{name}'")).into());
            }
        }
    }
    Ok(())
}

fn run_with_global_strictness_cleared<'gc, F>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    f: F,
) -> Result<Value<'gc>, EvalError<'gc>>
where
    F: FnOnce() -> Result<Value<'gc>, EvalError<'gc>>,
{
    let original_strict = env_get_strictness(env);

    // Remove the __is_strict own property temporarily
    env_set_strictness(mc, env, false)?;

    let res = f();

    env_set_strictness(mc, env, original_strict)?;

    res
}

fn walk_expr(e: &Expr, has_super_call: &mut bool, has_super_prop: &mut bool, has_new_target: &mut bool, has_arguments: &mut bool) {
    match e {
        Expr::Var(name, _, _) => {
            if name == "arguments" {
                *has_arguments = true;
            }
        }
        Expr::SuperCall(args) => {
            *has_super_call = true;
            for a in args {
                walk_expr(a, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        Expr::SuperProperty(_) => {
            *has_super_prop = true;
        }
        Expr::NewTarget => {
            *has_new_target = true;
        }
        Expr::Super => {
            // Bare `super` appearing as an object in an index expression
            // (e.g. `super["x"]`) should be treated as a SuperProperty
            log::trace!("walk_expr (core eval): found Expr::Super");
            *has_super_prop = true;
        }
        Expr::Call(callee, args) | Expr::New(callee, args) => {
            walk_expr(callee, has_super_call, has_super_prop, has_new_target, has_arguments);
            for a in args {
                walk_expr(a, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        Expr::Property(obj, _)
        | Expr::OptionalProperty(obj, _)
        | Expr::OptionalPrivateMember(obj, _)
        | Expr::TypeOf(obj)
        | Expr::UnaryNeg(obj)
        | Expr::UnaryPlus(obj)
        | Expr::BitNot(obj)
        | Expr::Delete(obj)
        | Expr::Void(obj)
        | Expr::Await(obj)
        | Expr::Yield(Some(obj))
        | Expr::YieldStar(obj)
        | Expr::LogicalNot(obj)
        | Expr::PostIncrement(obj)
        | Expr::PostDecrement(obj)
        | Expr::Spread(obj)
        | Expr::OptionalCall(obj, _)
        | Expr::TaggedTemplate(obj, _, _)
        | Expr::DynamicImport(obj)
        | Expr::BitAndAssign(obj, _) => {
            walk_expr(obj, has_super_call, has_super_prop, has_new_target, has_arguments);
        }
        Expr::Assign(l, r)
        | Expr::Binary(l, _, r)
        | Expr::Conditional(l, _, r)
        | Expr::Comma(l, r)
        | Expr::LogicalAnd(l, r)
        | Expr::LogicalOr(l, r)
        | Expr::Mod(l, r)
        | Expr::Pow(l, r) => {
            walk_expr(l, has_super_call, has_super_prop, has_new_target, has_arguments);
            walk_expr(r, has_super_call, has_super_prop, has_new_target, has_arguments);
        }
        Expr::Index(obj, idx) | Expr::OptionalIndex(obj, idx) => {
            walk_expr(obj, has_super_call, has_super_prop, has_new_target, has_arguments);
            walk_expr(idx, has_super_call, has_super_prop, has_new_target, has_arguments);
        }
        Expr::Object(kv) => {
            for (k, v, _flag, _) in kv {
                walk_expr(k, has_super_call, has_super_prop, has_new_target, has_arguments);
                walk_expr(v, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        Expr::Array(elems) => {
            for e in elems.iter().flatten() {
                walk_expr(e, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        Expr::Function(_, _, _)
        | Expr::AsyncFunction(_, _, _)
        | Expr::GeneratorFunction(_, _, _)
        | Expr::AsyncGeneratorFunction(_, _, _) => {
            // Do not descend into nested function bodies (they establish new super binding)
        }
        Expr::ArrowFunction(params, body) | Expr::AsyncArrowFunction(params, body) => {
            // Arrow functions inherit super/new.target/this, so we must descend
            for p in params {
                walk_destructuring(p, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
            for s in body {
                walk_stmt(s, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        _ => {}
    }
}

fn walk_stmt(s: &Statement, has_super_call: &mut bool, has_super_prop: &mut bool, has_new_target: &mut bool, has_arguments: &mut bool) {
    match &*s.kind {
        StatementKind::Expr(expr) => walk_expr(expr, has_super_call, has_super_prop, has_new_target, has_arguments),
        StatementKind::Return(Some(expr)) | StatementKind::Throw(expr) => {
            walk_expr(expr, has_super_call, has_super_prop, has_new_target, has_arguments)
        }
        StatementKind::Let(vars) | StatementKind::Var(vars) => {
            for (_, init) in vars {
                if let Some(e) = init {
                    walk_expr(e, has_super_call, has_super_prop, has_new_target, has_arguments);
                }
            }
        }
        StatementKind::Const(vars) => {
            for (_, init) in vars {
                walk_expr(init, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        StatementKind::If(ifstmt) => {
            walk_expr(&ifstmt.condition, has_super_call, has_super_prop, has_new_target, has_arguments);
            for st in &ifstmt.then_body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
            if let Some(else_body) = &ifstmt.else_body {
                for st in else_body {
                    walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
                }
            }
        }
        StatementKind::While(cond, body) => {
            walk_expr(cond, has_super_call, has_super_prop, has_new_target, has_arguments);
            for st in body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        StatementKind::DoWhile(body, cond) => {
            for st in body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
            walk_expr(cond, has_super_call, has_super_prop, has_new_target, has_arguments);
        }
        StatementKind::For(forstmt) => {
            if let Some(init) = &forstmt.init {
                walk_stmt(init, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
            if let Some(test) = &forstmt.test {
                walk_expr(test, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
            if let Some(update) = &forstmt.update {
                walk_stmt(update, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
            for st in &forstmt.body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        StatementKind::Block(vec) => {
            for st in vec {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        StatementKind::TryCatch(tc) => {
            for st in &tc.try_body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
            if let Some(cb) = &tc.catch_body {
                for st in cb {
                    walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
                }
            }
            if let Some(fb) = &tc.finally_body {
                for st in fb {
                    walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
                }
            }
        }
        StatementKind::Switch(sw) => {
            walk_expr(&sw.expr, has_super_call, has_super_prop, has_new_target, has_arguments);
            for case in &sw.cases {
                match case {
                    SwitchCase::Case(_, stmts) => {
                        for st in stmts {
                            walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
                        }
                    }
                    SwitchCase::Default(stmts) => {
                        for st in stmts {
                            walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_arguments);
                        }
                    }
                }
            }
        }

        StatementKind::FunctionDeclaration(_, _, _body, _, _) => {}
        _ => {}
    }
}

fn walk_destructuring(
    elem: &crate::core::DestructuringElement,
    has_super_call: &mut bool,
    has_super_prop: &mut bool,
    has_new_target: &mut bool,
    has_arguments: &mut bool,
) {
    match elem {
        DestructuringElement::Variable(_, Some(e)) => {
            walk_expr(e, has_super_call, has_super_prop, has_new_target, has_arguments);
        }
        DestructuringElement::Property(_, inner) | DestructuringElement::RestPattern(inner) => {
            walk_destructuring(inner, has_super_call, has_super_prop, has_new_target, has_arguments);
        }
        DestructuringElement::ComputedProperty(expr, inner) => {
            walk_expr(expr, has_super_call, has_super_prop, has_new_target, has_arguments);
            walk_destructuring(inner, has_super_call, has_super_prop, has_new_target, has_arguments);
        }
        DestructuringElement::NestedArray(list, maybe_expr) | DestructuringElement::NestedObject(list, maybe_expr) => {
            for el in list {
                walk_destructuring(el, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
            if let Some(e) = maybe_expr {
                walk_expr(e, has_super_call, has_super_prop, has_new_target, has_arguments);
            }
        }
        _ => {}
    }
}

fn handle_eval_function<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    eval_args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let first_arg = eval_args.first().cloned().unwrap_or(Value::Undefined);
    // Diagnostic: print the type and brief value of eval first_arg to diagnose comment tests
    log::trace!("HANDLE_EVAL FIRST_ARG: {:?}", first_arg);
    if let Value::String(script_str) = first_arg {
        let script = utf16_to_utf8(&script_str);
        let mut tokens = tokenize(&script)?;
        if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
            tokens.pop();
        }

        // Fast string-level quick-check for import.meta usage. Token windows may
        // miss some simple cases where the tokenization step behaves unexpectedly
        // or is skipped, so include a cheap substring check first to ensure we
        // reliably catch direct evals that reference `import.meta`.
        if script.contains("import.meta") {
            let is_indirect_eval = object_get_key_value(env, "__is_indirect_eval")
                .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
                .unwrap_or(false);
            let mut root_env = *env;
            while let Some(proto) = root_env.borrow().prototype {
                root_env = proto;
            }
            let is_in_module = object_get_key_value(&root_env, "__import_meta").is_some();
            log::trace!(
                "HANDLE_EVAL quick-check (string): script contains import.meta, is_in_module={} is_indirect_eval={}",
                is_in_module,
                is_indirect_eval
            );
            if is_in_module && !is_indirect_eval {
                let msg = "import.meta is not allowed in eval code";
                let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
                let constructor_val = if let Some(v) = env_get(env, "SyntaxError") {
                    v.borrow().clone()
                } else {
                    return Err(raise_syntax_error!(msg).into());
                };
                match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                    Ok(Value::Object(obj)) => return Err(EvalError::Throw(Value::Object(obj), None, None)),
                    Ok(other) => return Err(EvalError::Throw(other, None, None)),
                    Err(_) => return Err(raise_syntax_error!(msg).into()),
                }
            }
        }

        // Track whether the token stream contains `super` so we can perform
        // additional AST-based early error checks after parsing (we need the AST
        // to determine whether the use is a SuperCall or SuperProperty).
        let _contains_super_token = tokens.iter().any(|td| matches!(td.token, Token::Super));

        // Fast token-based quick-check: if the token stream contains the sequence
        // Identifier("import"), Dot, Identifier("meta"), and the caller is in a
        // module context performing a direct eval, this should throw a SyntaxError
        // per the spec (import.meta is only valid when the syntactic goal is Module).
        let has_import_meta_token = tokens.windows(3).any(|w| {
            matches!(w[0].token, Token::Identifier(ref id) if id == "import")
                && matches!(w[1].token, Token::Dot)
                && matches!(&w[2].token, Token::Identifier(id2) if id2 == "meta")
        });
        if has_import_meta_token {
            let is_indirect_eval = object_get_key_value(env, "__is_indirect_eval")
                .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
                .unwrap_or(false);
            let mut root_env = *env;
            while let Some(proto) = root_env.borrow().prototype {
                root_env = proto;
            }
            let is_in_module = object_get_key_value(&root_env, "__import_meta").is_some();
            log::trace!(
                "HANDLE_EVAL quick-check: has_import_meta_token={} is_in_module={} is_indirect_eval={}",
                has_import_meta_token,
                is_in_module,
                is_indirect_eval
            );
            if is_in_module && !is_indirect_eval {
                let msg = "import.meta is not allowed in eval code";
                let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
                let constructor_val = if let Some(v) = env_get(env, "SyntaxError") {
                    v.borrow().clone()
                } else {
                    return Err(raise_syntax_error!(msg).into());
                };
                match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                    Ok(Value::Object(obj)) => return Err(EvalError::Throw(Value::Object(obj), None, None)),
                    Ok(other) => return Err(EvalError::Throw(other, None, None)),
                    Err(_) => return Err(raise_syntax_error!(msg).into()),
                }
            }
        }

        let mut index = 0;
        // Debug: print eval context
        log::trace!(
            "HANDLE_EVAL: env_ptr={:p} has___is_indirect_eval={} has___function_local={}",
            env,
            crate::core::object_get_key_value(env, "__is_indirect_eval").is_some(),
            crate::core::object_get_key_value(env, "__function").is_some()
        );
        // Also print full env chain for diagnosis
        let mut trace_env = Some(*env);
        while let Some(e) = trace_env {
            let has_inst = crate::core::object_get_key_value(&e, "__instance").is_some();
            let has_fn = crate::core::object_get_key_value(&e, "__function").is_some();
            let is_arrow = crate::core::object_get_key_value(&e, "__is_arrow_function")
                .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
                .unwrap_or(false);
            log::trace!(
                "HANDLE_EVAL ENV: ptr={:p} is_function_scope={} has_inst={} has_fn={} is_arrow={}",
                e,
                e.borrow().is_function_scope,
                has_inst,
                has_fn,
                is_arrow
            );
            trace_env = e.borrow().prototype;
        }

        // DEBUG: show tokens for eval body when debugging new.target issues
        log::trace!(
            "HANDLE_EVAL TOKENS: {:?}",
            tokens.iter().map(|td| format!("{:?}", td.token)).collect::<Vec<_>>()
        );
        // Fast special-case: detect a token-only single `new.target` statement
        // and handle it without parsing to avoid parser-level rejection in some contexts.
        // Pattern: [New, Dot, Identifier("target"), opt Semicolon]
        let mut t_iter = tokens.iter().filter(|td| !matches!(td.token, Token::LineTerminator));
        let is_single_new_target = match (t_iter.next(), t_iter.next(), t_iter.next(), t_iter.next()) {
            (Some(a), Some(b), Some(c), Some(d)) => {
                matches!(a.token, Token::New)
                    && matches!(b.token, Token::Dot)
                    && matches!(&c.token, Token::Identifier(id) if id == "target")
                    && matches!(d.token, Token::Semicolon)
            }
            (Some(a), Some(b), Some(c), None) => {
                matches!(a.token, Token::New)
                    && matches!(b.token, Token::Dot)
                    && matches!(&c.token, Token::Identifier(id) if id == "target")
            }
            _ => false,
        };
        if is_single_new_target {
            // is_indirect_eval = true when this is an indirect eval
            let is_indirect_eval = object_get_key_value(env, "__is_indirect_eval")
                .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
                .unwrap_or(false);
            // Find nearest function scope and whether it is an arrow
            // NOTE: do not treat the global environment (prototype == None) as a function scope
            let mut cur = Some(*env);
            let mut in_function = false;
            let mut in_arrow = false;
            while let Some(e) = cur {
                if e.borrow().is_function_scope && e.borrow().prototype.is_some() {
                    in_function = true;
                    if let Some(flag_rc) = object_get_key_value(&e, "__is_arrow_function") {
                        in_arrow = matches!(*flag_rc.borrow(), Value::Boolean(true));
                    }
                    break;
                }
                cur = e.borrow().prototype;
            }

            if !(!is_indirect_eval && in_function && !in_arrow) {
                let msg = "Invalid use of 'new.target' in eval code";
                let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
                let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                    v.borrow().clone()
                } else {
                    return Err(EvalError::Js(raise_syntax_error!(msg)));
                };
                match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                    Ok(Value::Object(obj)) => return Err(EvalError::Throw(Value::Object(obj), None, None)),
                    Ok(other) => return Err(EvalError::Throw(other, None, None)),
                    Err(_) => return Err(EvalError::Js(raise_syntax_error!(msg))),
                }
            }
            // Allowed: single `new.target` statement evaluates to the current new.target
            // which is the function object when the function was invoked with `new`,
            // otherwise `undefined`.
            if in_function && let Some(inst_val_rc) = object_get_key_value(&cur.unwrap(), "__instance") {
                // If __instance is present (constructor call), return the function object stored in __function
                if !matches!(*inst_val_rc.borrow(), Value::Undefined)
                    && let Some(func_val_rc) = object_get_key_value(&cur.unwrap(), "__function")
                {
                    return Ok(func_val_rc.borrow().clone());
                }
            }
            return Ok(Value::Undefined);
        }
        let statements = parse_statements(&tokens, &mut index)?;
        log::trace!("HANDLE_EVAL PARSED: {:#?}", statements);

        if statements
            .iter()
            .any(|s| matches!(&*s.kind, StatementKind::Import(..) | StatementKind::Export(..)))
        {
            let msg = "Import/Export declarations may not appear in eval code";
            let msg_val = crate::core::Value::String(crate::unicode::utf8_to_utf16(msg));
            let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                v.borrow().clone()
            } else {
                return Err(EvalError::Js(raise_syntax_error!(msg)));
            };
            match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                Ok(crate::core::Value::Object(obj)) => return Err(EvalError::Throw(crate::core::Value::Object(obj), None, None)),
                Ok(other) => return Err(EvalError::Throw(other, None, None)),
                Err(_) => return Err(EvalError::Js(raise_syntax_error!(msg))),
            }
        }

        // If executing in the global environment, perform EvalDeclarationInstantiation
        // checks for FunctionDeclarations per spec: if any function cannot be declared
        // as a global (e.g., conflicts with non-configurable existing property such
        // as 'NaN'), throw a TypeError and do not create any global functions.
        if env.borrow().prototype.is_none() {
            check_global_declarations(env, &statements)?;
        }

        // Walk the parsed AST to locate SuperCall, SuperProperty and NewTarget occurrences and apply the ECMAScript early error rules.
        // Previously this was gated by a token-level fast-path (`contains_super_token`), but
        // to ensure we never miss cases where `super` or `new.target` are parsed in contexts that the tokenizer
        // didn't flag, always inspect the AST for SuperCall/SuperProperty/NewTarget occurrences.
        // Walk AST to find SuperCall, SuperProperty and NewTarget occurrences
        let mut has_super_call = false;
        let mut has_super_prop = false;
        let mut has_new_target = false;
        let mut has_arguments = false;

        for s in &statements {
            walk_stmt(s, &mut has_super_call, &mut has_super_prop, &mut has_new_target, &mut has_arguments);
        }

        // Now compute inMethod / inConstructor by finding an env with [[HomeObject]]
        let mut cur_env = Some(*env);
        let mut in_method = false;
        let mut in_constructor = false;
        let mut in_class_field_initializer = false;

        while let Some(e) = cur_env {
            if let Some(flag_rc) = crate::core::object_get_key_value(&e, "__class_field_initializer")
                && matches!(*flag_rc.borrow(), Value::Boolean(true))
            {
                in_class_field_initializer = true;
            }

            if e.borrow().get_home_object().is_some() {
                in_method = true;
                // check whether associated function object is a constructor
                if let Some(f_rc) = crate::core::object_get_key_value(&e, "__function") {
                    let f_val = f_rc.borrow().clone();
                    if let crate::core::Value::Object(obj) = f_val
                        && let Some(is_ctor_ptr) = crate::core::object_get_key_value(&obj, "__is_constructor")
                        && matches!(*is_ctor_ptr.borrow(), crate::core::Value::Boolean(true))
                    {
                        in_constructor = true;
                    }
                } else {
                    in_class_field_initializer = true;
                }
                break;
            }
            cur_env = e.borrow().prototype;
        }

        log::debug!(
            "eval-super-check: has_super_call={} has_super_prop={} has_new_target={} has_arguments={} in_method={} in_constructor={} in_class_field_initializer={}",
            has_super_call,
            has_super_prop,
            has_new_target,
            has_arguments,
            in_method,
            in_constructor,
            in_class_field_initializer
        );

        if has_super_call && !(in_method && in_constructor) {
            let msg = "Invalid use of 'super' in eval code";
            let msg_val = crate::core::Value::String(crate::unicode::utf8_to_utf16(msg));
            let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                v.borrow().clone()
            } else {
                return Err(EvalError::Js(raise_syntax_error!(msg)));
            };
            match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                Ok(crate::core::Value::Object(obj)) => return Err(EvalError::Throw(crate::core::Value::Object(obj), None, None)),
                Ok(other) => return Err(EvalError::Throw(other, None, None)),
                Err(_) => return Err(EvalError::Js(raise_syntax_error!(msg))),
            }
        }

        if has_super_prop && !in_method {
            let msg = "Invalid use of 'super' in eval code";
            let msg_val = crate::core::Value::String(crate::unicode::utf8_to_utf16(msg));
            let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                v.borrow().clone()
            } else {
                return Err(EvalError::Js(raise_syntax_error!(msg)));
            };
            match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                Ok(crate::core::Value::Object(obj)) => return Err(EvalError::Throw(crate::core::Value::Object(obj), None, None)),
                Ok(other) => return Err(EvalError::Throw(other, None, None)),
                Err(_) => return Err(EvalError::Js(raise_syntax_error!(msg))),
            }
        }

        if has_arguments && in_class_field_initializer {
            let msg = "Invalid use of 'arguments' in class field initializer";
            let msg_val = crate::core::Value::String(crate::unicode::utf8_to_utf16(msg));
            let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                v.borrow().clone()
            } else {
                return Err(EvalError::Js(raise_syntax_error!(msg)));
            };
            match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                Ok(crate::core::Value::Object(obj)) => return Err(EvalError::Throw(crate::core::Value::Object(obj), None, None)),
                Ok(other) => return Err(EvalError::Throw(other, None, None)),
                Err(_) => return Err(EvalError::Js(raise_syntax_error!(msg))),
            }
        }

        if has_new_target {
            // is_indirect_eval = true when this is an indirect eval
            let is_indirect_eval = crate::core::object_get_key_value(env, "__is_indirect_eval")
                .map(|c| matches!(*c.borrow(), crate::core::Value::Boolean(true)))
                .unwrap_or(false);

            // Walk env chain to find the nearest function scope and detect arrow-ness
            let mut cur = Some(*env);
            let mut in_function = false;
            let mut in_arrow = false;
            while let Some(e) = cur {
                if e.borrow().is_function_scope {
                    in_function = true;
                    if let Some(flag_rc) = crate::core::object_get_key_value(&e, "__is_arrow_function") {
                        in_arrow = matches!(*flag_rc.borrow(), crate::core::Value::Boolean(true));
                    } else {
                        in_arrow = false;
                    }
                    break;
                }
                cur = e.borrow().prototype;
            }

            // Allowed only when direct eval, inside a function, and that function is NOT an arrow
            if !(!is_indirect_eval && in_function && !in_arrow) {
                let msg = "Invalid use of 'new.target' in eval code";
                let msg_val = crate::core::Value::String(crate::unicode::utf8_to_utf16(msg));
                let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                    v.borrow().clone()
                } else {
                    return Err(EvalError::Js(raise_syntax_error!(msg)));
                };
                match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                    Ok(crate::core::Value::Object(obj)) => return Err(EvalError::Throw(crate::core::Value::Object(obj), None, None)),
                    Ok(other) => return Err(EvalError::Throw(other, None, None)),
                    Err(_) => return Err(EvalError::Js(raise_syntax_error!(msg))),
                }
            }
        }

        // If the evaluated script begins with a "use strict" directive,
        // create a fresh declarative environment whose prototype points
        // to the current env so strict direct evals and strict indirect
        // evals do not instantiate top-level bindings into the caller
        // or global variable environment.
        let starts_with_use_strict = statements.first()
            .map(|s| matches!(&*s.kind, StatementKind::Expr(e) if matches!(e, Expr::StringLit(ss) if utf16_to_utf8(ss).as_str() == "use strict")))
            .unwrap_or(false);

        // Determine effective strictness
        let caller_is_strict = env_get_strictness(env);

        let is_indirect_eval = object_get_key_value(env, "__is_indirect_eval")
            .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
            .unwrap_or(false);

        let is_strict_eval = starts_with_use_strict || (caller_is_strict && !is_indirect_eval);

        // Prepare execution environment
        // For strict evals (direct or indirect), create a fresh declarative environment
        // whose prototype points to the current env so top-level bindings do not affect the
        // caller or the global variable environment. For non-strict evals, execute in the
        // caller environment.

        // Enforce strict-mode parsing restrictions for strict evals (direct or indirect)
        if is_strict_eval {
            check_strict_mode_violations(&statements)?;
        }
        let exec_env = if is_strict_eval {
            let new_env = crate::core::new_js_object_data(mc);
            new_env.borrow_mut(mc).prototype = Some(*env);
            new_env.borrow_mut(mc).is_function_scope = true;
            env_set_strictness(mc, &new_env, true)?;
            new_env
        } else {
            *env
        };

        // Execution closure
        let run_stmts = || {
            object_set_key_value(mc, &exec_env, "__allow_dynamic_import_result", &Value::Boolean(true))?;
            let res = check_top_level_return(&statements).and_then(|_| evaluate_statements(mc, &exec_env, &statements));
            let _ = exec_env
                .borrow_mut(mc)
                .properties
                .shift_remove(&PropertyKey::String("__allow_dynamic_import_result".to_string()));
            res
        };

        // Run with temporary global strictness clearing if needed (Global Scope + Non-Strict Eval)
        let res = if env.borrow().prototype.is_none() && !is_strict_eval {
            run_with_global_strictness_cleared(mc, env, run_stmts)
        } else {
            run_stmts()
        };

        if let Ok(v) = &res {
            log::trace!("handle_eval_function result={}", crate::core::value_to_string(v));
        }
        res
    } else {
        Ok(first_arg)
    }
}

// Helper: dispatch calls for named functions, marking the global env for indirect `eval`.
fn call_named_eval_or_dispatch<'gc>(
    mc: &MutationContext<'gc>,
    env_for_call: &JSObjectDataPtr<'gc>,
    call_env: &JSObjectDataPtr<'gc>,
    name: &str,
    this_arg: Option<&Value<'gc>>,
    eval_args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    if name == "eval" {
        let key = PropertyKey::String("__is_indirect_eval".to_string());
        object_set_key_value(mc, env_for_call, &key, &Value::Boolean(true))?;
        let res = evaluate_call_dispatch(mc, call_env, &Value::Function(name.to_string()), this_arg, eval_args);
        let _ = env_for_call.borrow_mut(mc).properties.shift_remove(&key);
        res
    } else {
        evaluate_call_dispatch(mc, call_env, &Value::Function(name.to_string()), this_arg, eval_args)
    }
}

// Helper: when calling the topl-level dispatcher for a possibly-indirect eval
fn dispatch_with_indirect_eval_marker<'gc>(
    mc: &MutationContext<'gc>,
    env_for_call: &JSObjectDataPtr<'gc>,
    func_val: &Value<'gc>,
    this_val: Option<&Value<'gc>>,
    eval_args: &[Value<'gc>],
    is_indirect: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Only set the indirect-eval marker when this is actually an indirect eval
    // (i.e., caller called `eval` indirectly).  Direct eval should not set this flag.
    if is_indirect
        && let Value::Function(name) = &func_val
        && name == "eval"
    {
        let key = PropertyKey::String("__is_indirect_eval".to_string());
        object_set_key_value(mc, env_for_call, &key, &Value::Boolean(true))?;
        let res = evaluate_call_dispatch(mc, env_for_call, func_val, this_val, eval_args);
        let _ = env_for_call.borrow_mut(mc).properties.shift_remove(&key);
        return res;
    }
    evaluate_call_dispatch(mc, env_for_call, func_val, this_val, eval_args)
}

pub fn evaluate_call_dispatch<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    func_val: &Value<'gc>,
    this_val: Option<&Value<'gc>>,
    eval_args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Debug: show the concrete variant of func_val when calling
    log::trace!("CALL_DISPATCH: func_val variant = {:?}", func_val);
    match func_val {
        Value::Closure(cl) => call_closure(mc, cl, this_val, eval_args, env, None),
        Value::AsyncClosure(cl) => Ok(handle_async_closure_call(mc, cl, this_val, eval_args, env, None)?),
        Value::GeneratorFunction(_, cl) => Ok(handle_generator_function_call(mc, cl, eval_args, this_val, None, None)?),
        Value::AsyncGeneratorFunction(_, cl) => Ok(handle_async_generator_function_call(mc, cl, eval_args, None)?),
        Value::Function(name) => {
            if let Some(res) = call_native_function(mc, name, this_val, eval_args, env)? {
                return Ok(res);
            }
            if name == "eval" {
                log::trace!(
                    "CALL_DISPATCH-eval: env_ptr={:p} has___is_indirect_eval={} has___function_local={}",
                    env,
                    crate::core::object_get_key_value(env, "__is_indirect_eval").is_some(),
                    crate::core::object_get_key_value(env, "__function").is_some()
                );
                // Print env chain for diagnostic
                let mut cenv = Some(*env);
                while let Some(e) = cenv {
                    let has_inst = crate::core::object_get_key_value(&e, "__instance").is_some();
                    let has_fn = crate::core::object_get_key_value(&e, "__function").is_some();
                    let is_arrow = crate::core::object_get_key_value(&e, "__is_arrow_function")
                        .map(|c| matches!(*c.borrow(), crate::core::Value::Boolean(true)))
                        .unwrap_or(false);
                    log::trace!(
                        "CALL_DISPATCH-eval ENV: ptr={:p} is_function_scope={} has_inst={} has_fn={} is_arrow={}",
                        e,
                        e.borrow().is_function_scope,
                        has_inst,
                        has_fn,
                        is_arrow
                    );
                    cenv = e.borrow().prototype;
                }
                handle_eval_function(mc, env, eval_args)
            } else if let Some(method_name) = name.strip_prefix("console.") {
                crate::js_console::handle_console_method(mc, method_name, eval_args, env)
            } else if let Some(_method) = name.strip_prefix("os.") {
                #[cfg(feature = "os")]
                {
                    let default_this = Value::Object(*env);
                    let this_val = this_val.unwrap_or(&default_this);
                    Ok(crate::js_os::handle_os_method(mc, this_val, _method, eval_args, env)?)
                }
                #[cfg(not(feature = "os"))]
                {
                    Err(raise_eval_error!("os module not enabled. Recompile with --features os").into())
                }
            } else if let Some(_method) = name.strip_prefix("std.") {
                #[cfg(feature = "std")]
                {
                    match _method {
                        "sprintf" => Ok(crate::js_std::sprintf::handle_sprintf_call(eval_args)?),
                        "tmpfile" => Ok(crate::js_std::tmpfile::create_tmpfile(mc)?),
                        _ => Err(raise_eval_error!(format!("std method '{_method}' not implemented")).into()),
                    }
                }
                #[cfg(not(feature = "std"))]
                {
                    Err(raise_eval_error!("std module not enabled. Recompile with --features std").into())
                }
            } else if let Some(_method) = name.strip_prefix("tmp.") {
                #[cfg(feature = "std")]
                {
                    if let Some(Value::Object(this_obj)) = this_val {
                        Ok(crate::js_std::tmpfile::handle_file_method(this_obj, _method, eval_args)?)
                    } else {
                        Err(raise_eval_error!("TypeError: tmp method called on incompatible receiver").into())
                    }
                }
                #[cfg(not(feature = "std"))]
                {
                    Err(raise_eval_error!("std module (tmpfile) not enabled. Recompile with --features std").into())
                }
            } else if let Some(method) = name.strip_prefix("Boolean.prototype.") {
                let this_v = this_val.unwrap_or(&Value::Undefined);
                Ok(crate::js_boolean::handle_boolean_prototype_method(this_v, method)?)
            } else if let Some(method) = name.strip_prefix("BigInt.prototype.") {
                let this_v = this_val.unwrap_or(&Value::Undefined);
                Ok(crate::js_bigint::handle_bigint_object_method(this_v, method, eval_args)?)
            } else if let Some(method) = name.strip_prefix("TypedArray.prototype.") {
                let this_v = this_val.unwrap_or(&Value::Undefined);
                Ok(crate::js_typedarray::handle_typedarray_method(mc, this_v, method, eval_args, env)?)
            } else if name == "TypedArrayIterator.prototype.next" {
                let this_v = this_val.unwrap_or(&Value::Undefined);
                Ok(crate::js_typedarray::handle_typedarray_iterator_next(mc, this_v)?)
            } else if name == "Object.prototype.toString" {
                let this_v = this_val.unwrap_or(&Value::Undefined);
                Ok(handle_object_prototype_to_string(mc, this_v, env))
            } else if name == "Error.prototype.toString" {
                let this_v = this_val.unwrap_or(&Value::Undefined);
                // Delegate to Error.prototype.toString implementation
                Ok(crate::js_object::handle_error_to_string_method(mc, this_v, eval_args)?)
            } else if let Some(method) = name.strip_prefix("BigInt.") {
                Ok(crate::js_bigint::handle_bigint_static_method(mc, method, eval_args, env)?)
            } else if let Some(method) = name.strip_prefix("Number.prototype.") {
                Ok(handle_number_prototype_method(this_val, method, eval_args)?)
            } else if let Some(method) = name.strip_prefix("Number.") {
                Ok(handle_number_static_method(method, eval_args)?)
            } else if let Some(method) = name.strip_prefix("Math.") {
                Ok(handle_math_call(mc, method, eval_args, env)?)
            } else if let Some(method) = name.strip_prefix("JSON.") {
                Ok(handle_json_method(mc, method, eval_args, env)?)
            } else if let Some(method) = name.strip_prefix("Reflect.") {
                Ok(crate::js_reflect::handle_reflect_method(mc, method, eval_args, env)?)
            } else if let Some(method) = name.strip_prefix("Date.prototype.") {
                if let Some(this_obj) = this_val {
                    Ok(handle_date_method(mc, this_obj, method, eval_args, env)?)
                } else {
                    Err(raise_eval_error!("TypeError: Date method called on incompatible receiver").into())
                }
            } else if let Some(method) = name.strip_prefix("Date.") {
                Ok(handle_date_static_method(method, eval_args)?)
            } else if name.starts_with("String.") {
                if name == "String.fromCharCode" {
                    Ok(string_from_char_code(eval_args)?)
                } else if name == "String.fromCodePoint" {
                    Ok(string_from_code_point(eval_args)?)
                } else if name == "String.raw" {
                    Ok(string_raw(eval_args)?)
                } else if let Some(method) = name.strip_prefix("String.prototype.") {
                    // String instance methods need a 'this' value which should be the first argument if called directly?
                    // But here we are calling the function object directly.
                    // Usually instance methods are called via method call syntax (obj.method()), which sets 'this'.
                    // If we are here, it means we called the function object directly, e.g. String.prototype.slice.call(str, ...)
                    // But our current implementation of function calls doesn't handle 'this' binding for native functions well yet
                    // unless it's a method call.
                    // However, if we are calling it as a method of String.prototype, 'this' should be passed.
                    // But here 'name' is just a string identifier we assigned to the function.
                    // We need to know the 'this' value.
                    // For now, let's assume the first argument is 'this' if it's called as a standalone function?
                    // No, that's not how it works.
                    // If we are here, it means we are executing the native function body.
                    // We need to access the 'this' binding from the environment or context.
                    // But our native functions don't have a captured environment with 'this'.
                    // We need to change how we handle native function calls to include 'this'.

                    // Wait, the current architecture seems to rely on the caller to handle 'this' or pass it?
                    // In `evaluate_expr` for `Expr::Call`, we don't seem to pass 'this' explicitly for native functions
                    // unless it was a method call.

                    // Let's look at how `Expr::Call` handles method calls.
                    // It evaluates `func_expr`. If it's a property access, it sets `this`.
                    // But `evaluate_expr` returns a `Value`, not a reference.
                    // So we lose the `this` context unless we handle `Expr::Call` specially for property access.

                    // Actually, `Expr::Call` implementation in `eval.rs` (lines 600+) just evaluates `func_expr`.
                    // It doesn't seem to handle `this` binding for method calls properly yet?
                    // Ah, I see `Expr::Call` logic is split.
                    // Let's check `Expr::Call` implementation again.

                    // Use the provided `this` value (from method call) as the receiver; fall back to ToString conversion
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    let s_vec = match this_v {
                        Value::String(s) => s.clone(),
                        Value::Object(obj) => {
                            if let Some(val_rc) = object_get_key_value(obj, "__value__") {
                                if let Value::String(s2) = &*val_rc.borrow() {
                                    s2.clone()
                                } else {
                                    utf8_to_utf16(&value_to_string(this_v))
                                }
                            } else {
                                utf8_to_utf16(&value_to_string(this_v))
                            }
                        }
                        _ => utf8_to_utf16(&value_to_string(this_v)),
                    };
                    Ok(handle_string_method(mc, &s_vec, method, eval_args, env)?)
                } else {
                    Err(raise_eval_error!(format!("Unknown String function: {}", name)).into())
                }
            } else if let Some(suffix) = name.strip_prefix("Object.") {
                if let Some(method) = suffix.strip_prefix("prototype.") {
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    match method {
                        "valueOf" => Ok(crate::js_object::handle_value_of_method(mc, this_v, eval_args, env)?),
                        "toString" => Ok(crate::js_object::handle_to_string_method(mc, this_v, eval_args, env)?),
                        "toLocaleString" => Ok(crate::js_object::handle_to_string_method(mc, this_v, eval_args, env)?),
                        "hasOwnProperty" | "isPrototypeOf" | "propertyIsEnumerable" | "__lookupGetter__" | "__lookupSetter__" => {
                            // Need object wrapper
                            if let Value::Object(o) = this_v {
                                let res_opt = crate::js_object::handle_object_prototype_builtin(mc, name, o, eval_args, env)?;
                                Ok(res_opt.unwrap_or(Value::Undefined))
                            } else {
                                Err(raise_type_error!("Object.prototype method called on non-object receiver").into())
                            }
                        }
                        _ => Err(raise_eval_error!(format!("Unknown Object function: {}", name)).into()),
                    }
                } else {
                    Ok(crate::js_object::handle_object_method(mc, suffix, eval_args, env)?)
                }
            } else if let Some(suffix) = name.strip_prefix("Array.") {
                if let Some(method) = suffix.strip_prefix("prototype.") {
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        Ok(crate::js_array::handle_array_instance_method(mc, obj, method, eval_args, env)?)
                    } else {
                        Err(raise_eval_error!("TypeError: Array method called on non-object receiver").into())
                    }
                } else {
                    Ok(handle_array_static_method(mc, suffix, eval_args, env)?)
                }
            } else if name.starts_with("RegExp.") {
                if let Some(method) = name.strip_prefix("RegExp.prototype.") {
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        Ok(crate::js_regexp::handle_regexp_method(mc, obj, method, eval_args, env)?)
                    } else {
                        Err(raise_type_error!("RegExp.prototype method called on non-object receiver").into())
                    }
                } else {
                    Err(raise_eval_error!(format!("Unknown RegExp function: {}", name)).into())
                }
            } else if name.starts_with("Generator.") {
                if let Some(method) = name.strip_prefix("Generator.prototype.") {
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(gen_rc) = object_get_key_value(obj, "__generator__") {
                            let gen_val = gen_rc.borrow().clone();
                            if let Value::Generator(gen_ptr) = gen_val {
                                // Special-case iterator: generator[Symbol.iterator]() should return the generator object itself
                                if method == "iterator" {
                                    return Ok(Value::Object(*obj));
                                }
                                return crate::js_generator::handle_generator_instance_method(mc, &gen_ptr, method, eval_args, env);
                            }
                        }
                        Err(raise_eval_error!("TypeError: Generator.prototype method called on incompatible receiver").into())
                    } else {
                        Err(raise_eval_error!("TypeError: Generator.prototype method called on incompatible receiver").into())
                    }
                } else {
                    Err(raise_eval_error!(format!("Unknown Generator function: {}", name)).into())
                }
            } else if name.starts_with("Map.") {
                if let Some(method) = name.strip_prefix("Map.prototype.") {
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(map_val) = object_get_key_value(obj, "__map__") {
                            if let Value::Map(map_ptr) = &*map_val.borrow() {
                                Ok(crate::js_map::handle_map_instance_method(mc, map_ptr, method, eval_args, env)?)
                            } else {
                                Err(raise_eval_error!("TypeError: Map.prototype method called on incompatible receiver").into())
                            }
                        } else {
                            Err(raise_eval_error!("TypeError: Map.prototype method called on incompatible receiver").into())
                        }
                    } else if let Value::Map(map_ptr) = this_v {
                        Ok(crate::js_map::handle_map_instance_method(mc, map_ptr, method, eval_args, env)?)
                    } else {
                        Err(raise_eval_error!("TypeError: Map.prototype method called on non-object receiver").into())
                    }
                } else {
                    Err(raise_eval_error!(format!("Unknown Map function: {}", name)).into())
                }
            } else if name.starts_with("Map.") {
                if let Some(method) = name.strip_prefix("Map.prototype.") {
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(map_val) = object_get_key_value(obj, "__map__") {
                            if let Value::Map(map_ptr) = &*map_val.borrow() {
                                Ok(crate::js_map::handle_map_instance_method(mc, map_ptr, method, eval_args, env)?)
                            } else {
                                Err(raise_eval_error!("TypeError: Map.prototype method called on incompatible receiver").into())
                            }
                        } else {
                            Err(raise_eval_error!("TypeError: Map.prototype method called on incompatible receiver").into())
                        }
                    } else if let Value::Map(map_ptr) = this_v {
                        Ok(crate::js_map::handle_map_instance_method(mc, map_ptr, method, eval_args, env)?)
                    } else {
                        Err(raise_eval_error!("TypeError: Map.prototype method called on non-object receiver").into())
                    }
                } else {
                    Err(raise_eval_error!(format!("Unknown Map function: {}", name)).into())
                }
            } else if name.starts_with("WeakMap.") {
                if let Some(method) = name.strip_prefix("WeakMap.prototype.") {
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(wm_val) = object_get_key_value(obj, "__weakmap__") {
                            if let Value::WeakMap(wm_ptr) = &*wm_val.borrow() {
                                Ok(crate::js_weakmap::handle_weakmap_instance_method(
                                    mc, wm_ptr, method, eval_args, env,
                                )?)
                            } else {
                                Err(raise_eval_error!("TypeError: WeakMap.prototype method called on incompatible receiver").into())
                            }
                        } else {
                            Err(raise_eval_error!("TypeError: WeakMap.prototype method called on incompatible receiver").into())
                        }
                    } else if let Value::WeakMap(wm_ptr) = this_v {
                        Ok(crate::js_weakmap::handle_weakmap_instance_method(
                            mc, wm_ptr, method, eval_args, env,
                        )?)
                    } else {
                        Err(raise_eval_error!("TypeError: WeakMap.prototype method called on non-object receiver").into())
                    }
                } else {
                    Err(raise_eval_error!(format!("Unknown Map function: {}", name)).into())
                }
            } else if name.starts_with("WeakSet.") {
                if let Some(method) = name.strip_prefix("WeakSet.prototype.") {
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(ws_val) = object_get_key_value(obj, "__weakset__") {
                            if let Value::WeakSet(ws_ptr) = &*ws_val.borrow() {
                                Ok(crate::js_weakset::handle_weakset_instance_method(mc, ws_ptr, method, eval_args)?)
                            } else {
                                Err(raise_eval_error!("TypeError: WeakSet.prototype method called on incompatible receiver").into())
                            }
                        } else {
                            Err(raise_eval_error!("TypeError: WeakSet.prototype method called on incompatible receiver").into())
                        }
                    } else if let Value::WeakSet(ws_ptr) = this_v {
                        Ok(crate::js_weakset::handle_weakset_instance_method(mc, ws_ptr, method, eval_args)?)
                    } else {
                        Err(raise_eval_error!("TypeError: WeakSet.prototype method called on non-object receiver").into())
                    }
                } else {
                    Err(raise_eval_error!(format!("Unknown Map function: {}", name)).into())
                }
            } else if name.starts_with("Set.") {
                if let Some(method) = name.strip_prefix("Set.prototype.") {
                    let this_v = this_val.unwrap_or(&Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(set_val) = object_get_key_value(obj, "__set__") {
                            if let Value::Set(set_ptr) = &*set_val.borrow() {
                                Ok(handle_set_instance_method(mc, set_ptr, this_v, method, eval_args, env)?)
                            } else {
                                Err(raise_eval_error!("TypeError: Set.prototype method called on incompatible receiver").into())
                            }
                        } else {
                            Err(raise_eval_error!("TypeError: Set.prototype method called on incompatible receiver").into())
                        }
                    } else if let Value::Set(set_ptr) = this_v {
                        Ok(handle_set_instance_method(mc, set_ptr, this_v, method, eval_args, env)?)
                    } else {
                        // Fallback: if `this_v` is an object, check if it has the Set internal slot on the underlying object
                        // This happens when `this_v` is a JSObject wrapping the Set pointer? No, `Value::Set` is separate.
                        // Actually, `this_val` from `Expr::Call` might be just `obj_val`.
                        // If `this_val` is `Value::Object` (which it defaults to in `Expr::Call` matching logic),
                        // it might still fail the `__set__` check if it's not set up yet?

                        // Debug:
                        // println!("Set method call debug: method={}, this_val={:?}", method, this_v);

                        Err(raise_eval_error!("TypeError: Set.prototype method called on non-object receiver").into())
                    }
                } else {
                    Err(raise_eval_error!(format!("Unknown Set function: {}", name)).into())
                }
            } else if name.starts_with("Function.") {
                if let Some(method) = name.strip_prefix("Function.prototype.") {
                    if method == "call" {
                        let call_env = prepare_call_env_with_this(mc, Some(env), this_val, None, &[], None, Some(env), None)?;
                        Ok(crate::js_function::handle_global_function(mc, name, eval_args, &call_env)?)
                    } else {
                        let this_v = this_val.unwrap_or(&Value::Undefined);
                        handle_function_prototype_method(mc, this_v, method, eval_args, env)
                    }
                } else {
                    Err(raise_eval_error!(format!("Unknown Function method: {}", name)).into())
                }
            } else {
                let call_env = prepare_call_env_with_this(mc, Some(env), this_val, None, &[], None, Some(env), None)?;
                Ok(crate::js_function::handle_global_function(mc, name, eval_args, &call_env)?)
            }
        }
        Value::Object(obj) => {
            if let Some(cl_ptr) = obj.borrow().get_closure() {
                match &*cl_ptr.borrow() {
                    Value::Closure(cl) => {
                        let res = call_closure(mc, cl, this_val, eval_args, env, Some(*obj));
                        match res {
                            Ok(v) => Ok(v),
                            Err(mut e) => {
                                let name_opt = obj.borrow().get_property("name");
                                if let Some(name_str) = name_opt
                                    && let EvalError::Js(js_err) = &mut e
                                    && let Some(last_frame) = js_err.inner.stack.last_mut()
                                    && last_frame.contains("<anonymous>")
                                {
                                    *last_frame = last_frame.replace("<anonymous>", &name_str);
                                }
                                Err(e)
                            }
                        }
                    }
                    Value::AsyncClosure(cl) => Ok(handle_async_closure_call(mc, cl, this_val, eval_args, env, Some(*obj))?),
                    Value::GeneratorFunction(_, cl) => {
                        // Do not pre-read the function object's 'prototype' here because
                        // parameter default initializers may mutate it; let the
                        // generator call handler resolve the constructor prototype
                        // after parameter initialization by passing the function
                        // object so it can be observed at the correct time.
                        Ok(handle_generator_function_call(mc, cl, eval_args, this_val, None, Some(*obj))?)
                    }
                    // Async generator functions: create AsyncGenerator instance
                    Value::AsyncGeneratorFunction(_, cl) => Ok(handle_async_generator_function_call(mc, cl, eval_args, Some(*obj))?),
                    _ => Err(raise_type_error!("Not a function").into()),
                }
            } else if obj.borrow().class_def.is_some() {
                Err(raise_type_error!("Class constructor cannot be invoked without 'new'").into())
            } else if let Some(native_name) = object_get_key_value(obj, "__native_ctor") {
                match &*native_name.borrow() {
                    Value::String(name) => {
                        if name == &crate::unicode::utf8_to_utf16("Object") {
                            Ok(crate::js_class::handle_object_constructor(mc, eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("String") {
                            Ok(crate::js_string::string_constructor(mc, eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Boolean") {
                            Ok(crate::js_boolean::boolean_constructor(eval_args)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Number") {
                            Ok(number_constructor(mc, eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("BigInt") {
                            Ok(bigint_constructor(mc, eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Symbol") {
                            Ok(crate::js_symbol::handle_symbol_call(mc, eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Array") {
                            Ok(crate::js_array::handle_array_constructor(mc, eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Function") {
                            Ok(crate::js_function::handle_global_function(mc, "Function", eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Error")
                            || name == &crate::unicode::utf8_to_utf16("TypeError")
                            || name == &crate::unicode::utf8_to_utf16("ReferenceError")
                            || name == &crate::unicode::utf8_to_utf16("RangeError")
                            || name == &crate::unicode::utf8_to_utf16("SyntaxError")
                            || name == &crate::unicode::utf8_to_utf16("EvalError")
                            || name == &crate::unicode::utf8_to_utf16("URIError")
                        {
                            // For native Error constructors, calling them as a function
                            // should produce a new Error object with the provided message.
                            let msg_val = eval_args.first().cloned().unwrap_or(Value::Undefined);
                            // The constructor's "prototype" property points to the error prototype
                            if let Some(prototype_rc) = object_get_key_value(obj, "prototype")
                                && let Value::Object(proto_ptr) = &*prototype_rc.borrow()
                            {
                                let err = crate::core::create_error(mc, Some(*proto_ptr), msg_val)?;
                                Ok(err)
                            } else {
                                // Fallback: create error with no prototype
                                let err = crate::core::create_error(mc, None, msg_val)?;
                                Ok(err)
                            }
                        } else {
                            Err(raise_type_error!("Not a function").into())
                        }
                    }
                    _ => Err(raise_type_error!("Not a function").into()),
                }
            } else {
                Err(raise_type_error!("Not a function").into())
            }
        }
        _ => Err(raise_type_error!("Not a function").into()),
    }
}

fn lookup_or_create_import_meta<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    log::trace!("lookup_or_create_import_meta: env_ptr={:p}", env);
    // `import.meta` as a per-module ordinary object stored under env.__import_meta.
    // Prefer the immediate environment but fall back to the root global environment.
    let meta = object_get_key_value(env, "__import_meta").or_else(|| {
        let mut root = *env;
        while let Some(proto) = root.borrow().prototype {
            root = proto;
        }
        object_get_key_value(&root, "__import_meta")
    });
    if let Some(meta_rc) = meta
        && let Value::Object(meta_obj) = &*meta_rc.borrow()
    {
        log::trace!("lookup_or_create_import_meta: found existing import.meta object ptr={:p}", meta_obj);
        return Ok(Value::Object(*meta_obj));
    }

    // Fallback: create an import.meta object on the root environment
    log::trace!("lookup_or_create_import_meta: creating fallback import.meta on root env");
    let mut root = *env;
    while let Some(proto) = root.borrow().prototype {
        root = proto;
    }
    let import_meta = new_js_object_data(mc);
    if let Some(cell) = env_get(&root, "__filepath")
        && let Value::String(s) = cell.borrow().clone()
    {
        object_set_key_value(mc, &import_meta, "url", &Value::String(s))?;
    }
    object_set_key_value(mc, &root, "__import_meta", &Value::Object(import_meta))?;
    Ok(Value::Object(import_meta))
}

fn evaluate_expr_call<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    func_expr: &Expr,
    args: &[Expr],
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Special-case: direct eval of a single `new.target` string literal
    // (e.g., eval('new.target;')) — do early detection using the call-site env
    // to allow it only when it's a direct eval inside a non-arrow function and
    // throw a SyntaxError (before any argument side-effects) when disallowed.
    if let Expr::Var(name, ..) = func_expr
        && name == "eval"
        && args.len() == 1
        && let Expr::StringLit(s) = &args[0]
    {
        let code = crate::unicode::utf16_to_utf8(s);
        let trim = code.trim();
        if trim == "new.target;" || trim == "new.target" {
            // direct eval (callee is IdentifierReference) so is_indirect_eval=false
            let is_indirect_eval = false;
            // Find nearest function scope and whether it's an arrow
            // NOTE: do not treat the global environment (prototype == None) as a function scope
            let mut cur = Some(*env);
            let mut in_function = false;
            let mut in_arrow = false;
            while let Some(e) = cur {
                if e.borrow().is_function_scope && e.borrow().prototype.is_some() {
                    in_function = true;
                    if let Some(flag_rc) = crate::core::object_get_key_value(&e, "__is_arrow_function") {
                        in_arrow = matches!(*flag_rc.borrow(), crate::core::Value::Boolean(true));
                    }
                    break;
                }
                cur = e.borrow().prototype;
            }
            if !(!is_indirect_eval && in_function && !in_arrow) {
                let msg = "Invalid use of 'new.target' in eval code";
                let msg_val = crate::core::Value::String(crate::unicode::utf8_to_utf16(msg));
                let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                    v.borrow().clone()
                } else {
                    return Err(raise_syntax_error!(msg).into());
                };
                match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                    Ok(Value::Object(obj)) => {
                        return Err(EvalError::Throw(Value::Object(obj), None, None));
                    }
                    Ok(other) => return Err(EvalError::Throw(other, None, None)),
                    Err(_) => return Err(raise_syntax_error!(msg).into()),
                }
            }
            // Diagnostic: report env chain findings for direct new.target eval
            if in_function {
                // If __instance is present and not undefined, return the function stored in __function
                if let Some(inst_val_rc) = object_get_key_value(&cur.unwrap(), "__instance")
                    && !matches!(*inst_val_rc.borrow(), Value::Undefined)
                    && let Some(func_val_rc) = object_get_key_value(&cur.unwrap(), "__function")
                {
                    log::trace!("FAST-SPECIAL-NEWTARGET: returning __function");
                    return Ok(func_val_rc.borrow().clone());
                }
            } else {
                log::trace!("EVAL-FAST-NEWTARGET DIAG: in_function=false");
            }
            log::trace!("FAST-SPECIAL-NEWTARGET: returning undefined");
            return Ok(Value::Undefined);
        }
    }

    log::trace!("DEBUG-EVALUATE-CALL: func_expr={:?} args={:?}", func_expr, args);
    OPT_CHAIN_RECURSION_DEPTH.with(|c| log::trace!("ENTER evaluate_expr_call opt_depth={} func_expr={:?}", c.get(), func_expr));
    let (func_val, this_val) = match func_expr {
        Expr::OptionalProperty(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if obj_val.is_null_or_undefined() {
                // Short-circuit optional chain for method call on null/undefined
                return Ok(Value::Undefined);
            }
            let f_val = if let Value::Object(obj) = &obj_val {
                // Use accessor-aware property lookup so getters are executed
                let prop_val = get_property_with_accessors(mc, env, obj, key)?;
                if !matches!(prop_val, Value::Undefined) {
                    prop_val
                } else if (key.as_str() == "call" || key.as_str() == "apply") && obj.borrow().get_closure().is_some() {
                    let name = if key.as_str() == "call" {
                        "Function.prototype.call"
                    } else {
                        "Function.prototype.apply"
                    };
                    Value::Function(name.to_string())
                } else {
                    Value::Undefined
                }
            } else if let Value::String(s) = &obj_val
                && key == "length"
            {
                Value::Number(s.len() as f64)
            } else if matches!(
                obj_val,
                Value::Closure(_) | Value::Function(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..)
            ) && (key == "call" || key == "apply")
            {
                let name = if key == "call" {
                    "Function.prototype.call"
                } else {
                    "Function.prototype.apply"
                };
                Value::Function(name.to_string())
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, key)?
            };
            (f_val, Some(obj_val))
        }
        Expr::OptionalPrivateMember(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if obj_val.is_null_or_undefined() {
                return Ok(Value::Undefined);
            }
            let pv = evaluate_var(mc, env, key)?;
            let f_val = if let Value::Object(obj) = &obj_val {
                if let Value::PrivateName(n, id) = pv {
                    get_property_with_accessors(mc, env, obj, PropertyKey::Private(n, id))?
                } else {
                    return Err(raise_syntax_error!(format!("Private field '{}' must be declared in an enclosing class", key)).into());
                }
            } else {
                return Err(raise_type_error!("Cannot access private field on non-object").into());
            };
            (f_val, Some(obj_val))
        }
        Expr::OptionalIndex(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if obj_val.is_null_or_undefined() {
                // Short-circuit optional chain for computed method call on null/undefined
                return Ok(Value::Undefined);
            }
            let key_val = evaluate_expr(mc, env, key_expr)?;

            let key = match key_val {
                Value::Symbol(s) => PropertyKey::Symbol(s),
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::from(n.to_string()),
                _ => PropertyKey::from(value_to_string(&key_val)),
            };

            let f_val = if let Value::Object(obj) = &obj_val {
                if let Some(val) = object_get_key_value(obj, &key) {
                    val.borrow().clone()
                } else {
                    Value::Undefined
                }
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, &key)?
            };
            (f_val, Some(obj_val))
        }
        Expr::Property(obj_expr, key) => {
            // Fast special-case for `import.meta` when used as a callee (e.g. `import.meta()`)
            // to avoid evaluating the base `import` (which would throw ReferenceError).
            if let crate::core::Expr::Var(name, ..) = &**obj_expr
                && name == "import"
                && key == "meta"
            {
                log::trace!("CALL-SPECIAL-IMPORT_META: matched import.meta as callee");
                let import_meta_val = lookup_or_create_import_meta(mc, env)?;
                // Use the import.meta object as the call target. For such calls, there is no
                // meaningful 'this' base value we can evaluate safely, so use `None` to indicate
                // a plain function call (this -> undefined in strict mode).
                (import_meta_val, None)
            } else {
                let obj_val = if expr_contains_optional_chain(obj_expr) {
                    match evaluate_optional_chain_base(mc, env, obj_expr)? {
                        Some(val) => val,
                        None => return Ok(Value::Undefined),
                    }
                } else {
                    evaluate_expr(mc, env, obj_expr)?
                };
                let f_val = if let Value::Object(obj) = &obj_val {
                    // Use accessor-aware property lookup so getters are executed.
                    let prop_val = get_property_with_accessors(mc, env, obj, key)?;
                    if !matches!(prop_val, Value::Undefined) {
                        prop_val
                    } else if (key.as_str() == "call" || key.as_str() == "apply") && obj.borrow().get_closure().is_some() {
                        let name = if key.as_str() == "call" {
                            "Function.prototype.call"
                        } else {
                            "Function.prototype.apply"
                        };
                        Value::Function(name.to_string())
                    } else {
                        Value::Undefined
                    }
                } else if let Value::String(s) = &obj_val
                    && key == "length"
                {
                    Value::Number(s.len() as f64)
                } else if matches!(obj_val, Value::Undefined | Value::Null) {
                    return Err(raise_type_error!("Cannot read properties of null or undefined").into());
                } else if matches!(
                    obj_val,
                    Value::Closure(_) | Value::Function(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..)
                ) && (key == "call" || key == "apply")
                {
                    let name = if key == "call" {
                        "Function.prototype.call"
                    } else {
                        "Function.prototype.apply"
                    };
                    Value::Function(name.to_string())
                } else {
                    get_primitive_prototype_property(mc, env, &obj_val, key)?
                };

                (f_val, Some(obj_val))
            }
        }
        Expr::PrivateMember(obj_expr, key) => {
            let obj_val = if expr_contains_optional_chain(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };
            let pv = evaluate_var(mc, env, key)?;
            let f_val = if let Value::Object(obj) = &obj_val {
                if let Value::PrivateName(n, id) = pv {
                    get_property_with_accessors(mc, env, obj, PropertyKey::Private(n, id))?
                } else {
                    return Err(raise_syntax_error!(format!("Private field '{}' must be declared in an enclosing class", key)).into());
                }
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot read properties of null or undefined").into());
            } else {
                return Err(raise_type_error!("Cannot access private field on non-object").into());
            };
            (f_val, Some(obj_val))
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = if is_optional_chain_expr(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };
            let key_val = evaluate_expr(mc, env, key_expr)?;

            let key = match key_val {
                Value::Symbol(s) => PropertyKey::Symbol(s),
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::from(n.to_string()),
                _ => PropertyKey::from(value_to_string(&key_val)),
            };

            let f_val = if let Value::Object(obj) = &obj_val {
                // Use accessor-aware lookup so getters are invoked
                let prop_val = get_property_with_accessors(mc, env, obj, &key)?;
                if !matches!(prop_val, Value::Undefined) {
                    prop_val
                } else {
                    Value::Undefined
                }
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot read properties of null or undefined").into());
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, &key)?
            };
            (f_val, Some(obj_val))
        }
        _ => (evaluate_expr(mc, env, func_expr)?, None),
    };

    let mut eval_args = Vec::new();
    for arg_expr in args {
        if let Expr::Spread(target) = arg_expr {
            let val = evaluate_expr(mc, env, target)?;
            if let Value::Object(obj) = val {
                if is_array(mc, &obj) {
                    let len_val = object_get_key_value(&obj, "length").unwrap_or(new_gc_cell_ptr(mc, Value::Undefined));
                    let len = if let Value::Number(n) = *len_val.borrow() { n as usize } else { 0 };
                    for k in 0..len {
                        let item = object_get_key_value(&obj, k).unwrap_or(new_gc_cell_ptr(mc, Value::Undefined));
                        eval_args.push(item.borrow().clone());
                    }
                } else {
                    // Support generic iterables via Symbol.iterator
                    let mut iter_fn_opt: Option<crate::core::GcPtr<'gc, Value<'gc>>> = None;
                    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                    {
                        let iter_sym_val_opt = object_get_key_value(sym_obj, "iterator");
                        if let Some(iter_sym_val) = iter_sym_val_opt
                            && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                        {
                            // Use accessor-aware property read to support getters that return the iterator method
                            let iter_fn_val = get_property_with_accessors(mc, env, &obj, iter_sym)?;
                            // If the accessor returned undefined, there's no iterator
                            if !matches!(iter_fn_val, crate::core::Value::Undefined) {
                                // Wrap the returned Value into a GC pointer so code below can inspect/call it uniformly
                                iter_fn_opt = Some(crate::core::new_gc_cell_ptr(mc, iter_fn_val));
                            }
                        }
                    }
                    if let Some(iter_fn_val) = iter_fn_opt {
                        // Call iterator method on the object to get an iterator
                        let iterator = match &*iter_fn_val.borrow() {
                            Value::Function(name) => {
                                let call_env = crate::js_class::prepare_call_env_with_this(
                                    mc,
                                    Some(env),
                                    Some(&Value::Object(obj)),
                                    None,
                                    &[],
                                    None,
                                    Some(env),
                                    None,
                                )?;
                                evaluate_call_dispatch(mc, &call_env, &Value::Function(name.clone()), Some(&Value::Object(obj)), &[])?
                            }
                            Value::Closure(cl) => crate::core::call_closure(mc, cl, Some(&Value::Object(obj)), &[], env, None)?,
                            Value::Object(o) => {
                                // Support function objects that wrap a closure (get_closure)
                                if let Some(cl_ptr) = o.borrow().get_closure() {
                                    match &*cl_ptr.borrow() {
                                        Value::Closure(cl) => crate::core::call_closure(mc, cl, Some(&Value::Object(*o)), &[], env, None)?,
                                        Value::Function(name) => {
                                            let call_env = crate::js_class::prepare_call_env_with_this(
                                                mc,
                                                Some(env),
                                                Some(&Value::Object(*o)),
                                                None,
                                                &[],
                                                None,
                                                Some(env),
                                                None,
                                            )?;
                                            evaluate_call_dispatch(
                                                mc,
                                                &call_env,
                                                &Value::Function(name.clone()),
                                                Some(&Value::Object(*o)),
                                                &[],
                                            )?
                                        }
                                        _ => return Err(raise_type_error!("Spread target is not iterable").into()),
                                    }
                                } else {
                                    log::trace!("Spread target is not iterable: iterator property is object but not callable");
                                    return Err(raise_type_error!("Spread target is not iterable").into());
                                }
                            }
                            _ => {
                                log::trace!("Spread target is not iterable: iterator property exists but not callable");
                                return Err(raise_type_error!("Spread target is not iterable").into());
                            }
                        };

                        // Consume iterator by repeatedly calling its next() method
                        if let Value::Object(iter_obj) = iterator {
                            loop {
                                if let Some(next_val) = object_get_key_value(&iter_obj, "next") {
                                    let next_fn = next_val.borrow().clone();

                                    let res = match &next_fn {
                                        Value::Function(name) => {
                                            let call_env = crate::js_class::prepare_call_env_with_this(
                                                mc,
                                                Some(env),
                                                Some(&Value::Object(iter_obj)),
                                                None,
                                                &[],
                                                None,
                                                Some(env),
                                                None,
                                            )?;
                                            evaluate_call_dispatch(
                                                mc,
                                                &call_env,
                                                &Value::Function(name.clone()),
                                                Some(&Value::Object(iter_obj)),
                                                &[],
                                            )?
                                        }
                                        Value::Closure(cl) => call_closure(mc, cl, Some(&Value::Object(iter_obj)), &[], env, None)?,
                                        Value::Object(o) => {
                                            if let Some(cl_ptr) = o.borrow().get_closure() {
                                                match &*cl_ptr.borrow() {
                                                    Value::Closure(cl) => call_closure(mc, cl, Some(&Value::Object(*o)), &[], env, None)?,
                                                    Value::Function(name) => {
                                                        let call_env = crate::js_class::prepare_call_env_with_this(
                                                            mc,
                                                            Some(env),
                                                            Some(&Value::Object(*o)),
                                                            None,
                                                            &[],
                                                            None,
                                                            Some(env),
                                                            None,
                                                        )?;
                                                        evaluate_call_dispatch(
                                                            mc,
                                                            &call_env,
                                                            &Value::Function(name.clone()),
                                                            Some(&Value::Object(*o)),
                                                            &[],
                                                        )?
                                                    }
                                                    _ => return Err(raise_type_error!("Iterator.next is not callable").into()),
                                                }
                                            } else {
                                                return Err(raise_type_error!("Iterator.next is not callable").into());
                                            }
                                        }
                                        _ => {
                                            return Err(raise_type_error!("Iterator.next is not callable").into());
                                        }
                                    };

                                    if let Value::Object(res_obj) = res {
                                        // Access 'done' and 'value' using accessor-aware property reads so getters can run
                                        let done = match get_property_with_accessors(mc, env, &res_obj, "done") {
                                            Ok(v) => {
                                                if let Value::Boolean(b) = v {
                                                    b
                                                } else {
                                                    false
                                                }
                                            }
                                            Err(e) => return Err(e),
                                        };

                                        if done {
                                            break;
                                        }

                                        let value = get_property_with_accessors(mc, env, &res_obj, "value")?;

                                        eval_args.push(value);

                                        continue;
                                    } else {
                                        return Err(raise_type_error!("Iterator.next did not return an object").into());
                                    }
                                } else {
                                    return Err(raise_type_error!("Iterator has no next method").into());
                                }
                            }
                        } else {
                            return Err(raise_type_error!("Iterator call did not return an object").into());
                        }
                    }
                }
            } else {
                return Err(raise_type_error!("Spread only implemented for Objects").into());
            }
        } else {
            let val = evaluate_expr(mc, env, arg_expr)?;
            eval_args.push(val);
        }
    }

    // If callee appears to be a non-callable primitive and the callee expression is a variable,
    // prefer a TypeError with the variable name (e.g., "a is not a function").
    // Include generator/async-generator function and async closures as callable values too.
    if !matches!(
        func_val,
        Value::Closure(_)
            | Value::Function(_)
            | Value::Object(_)
            | Value::GeneratorFunction(..)
            | Value::AsyncGeneratorFunction(..)
            | Value::AsyncClosure(_)
    ) {
        if let Expr::Var(name, ..) = func_expr {
            return Err(raise_type_error!(format!("{} is not a function", name)).into());
        } else {
            return Err(raise_type_error!("Not a function").into());
        }
    }

    // Is this a *direct* eval call? (IsDirectEvalCall: callee is an IdentifierReference named "eval")
    let is_direct_eval = matches!(func_expr, Expr::Var(name, ..) if name == "eval");

    // If this is an *indirect* call to the builtin "eval", execute it in the caller's global environment
    // (the environment provided to the call), not necessarily the topmost realm object. Use the
    // passed-in `env` as the env_for_call when indirect. This allows detecting global lexical
    // declarations that live in the caller's global scope.
    let is_indirect_eval_call = matches!(func_val, Value::Function(ref name) if name == "eval") && !is_direct_eval;
    let env_for_call = if is_indirect_eval_call {
        let mut t = *env;
        while let Some(proto) = t.borrow().prototype {
            t = proto;
        }
        t
    } else {
        *env
    };

    // Dispatch, handling indirect `eval` marker on the global env when necessary.
    dispatch_with_indirect_eval_marker(mc, &env_for_call, &func_val, this_val.as_ref(), &eval_args, is_indirect_eval_call)
}

fn is_optional_chain_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::OptionalProperty(..) | Expr::OptionalIndex(..) | Expr::OptionalPrivateMember(..) | Expr::OptionalCall(..)
    )
}

// Recursively detect whether an expression contains an optional chain node anywhere in its subtree.
fn expr_contains_optional_chain(expr: &Expr) -> bool {
    use Expr::*;
    match expr {
        OptionalProperty(..) | OptionalIndex(..) | OptionalPrivateMember(..) | OptionalCall(..) => true,
        Property(obj, _) | Index(obj, _) | PrivateMember(obj, _) => expr_contains_optional_chain(obj),
        Call(func, args) | New(func, args) => {
            if expr_contains_optional_chain(func) {
                return true;
            }
            for a in args.iter() {
                if expr_contains_optional_chain(a) {
                    return true;
                }
            }
            false
        }
        SuperCall(args) | SuperMethod(_, args) => args.iter().any(expr_contains_optional_chain),
        Assign(l, r)
        | Comma(l, r)
        | Binary(l, _, r)
        | LogicalAnd(l, r)
        | LogicalOr(l, r)
        | NullishCoalescing(l, r)
        | Mod(l, r)
        | Pow(l, r)
        | LogicalAndAssign(l, r)
        | LogicalOrAssign(l, r)
        | NullishAssign(l, r)
        | AddAssign(l, r)
        | SubAssign(l, r)
        | PowAssign(l, r)
        | MulAssign(l, r)
        | DivAssign(l, r)
        | ModAssign(l, r)
        | BitXorAssign(l, r)
        | BitAndAssign(l, r)
        | BitOrAssign(l, r)
        | LeftShiftAssign(l, r)
        | RightShiftAssign(l, r)
        | UnsignedRightShiftAssign(l, r) => expr_contains_optional_chain(l) || expr_contains_optional_chain(r),
        // single-expression variants
        TypeOf(e) | Delete(e) | Void(e) | Await(e) | YieldStar(e) | Getter(e) | Setter(e) | UnaryNeg(e) | UnaryPlus(e) | BitNot(e)
        | LogicalNot(e) | Increment(e) | Decrement(e) | PostIncrement(e) | PostDecrement(e) | Spread(e) => expr_contains_optional_chain(e),
        Yield(opt) => opt.as_ref().is_some_and(|e| expr_contains_optional_chain(e)),
        Conditional(c, t, e) => expr_contains_optional_chain(c) || expr_contains_optional_chain(t) || expr_contains_optional_chain(e),
        Array(elements) => elements.iter().any(|opt| opt.as_ref().is_some_and(expr_contains_optional_chain)),
        Object(props) => props
            .iter()
            .any(|(k, v, _, _)| expr_contains_optional_chain(k) || expr_contains_optional_chain(v)),
        _ => false,
    }
}

// Helper: evaluate call arguments, expanding spread elements into the final argument list.
fn collect_call_args<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, args: &[Expr]) -> Result<Vec<Value<'gc>>, EvalError<'gc>> {
    let mut eval_args: Vec<Value<'gc>> = Vec::new();
    for arg_expr in args.iter() {
        if let Expr::Spread(target) = arg_expr {
            let val = evaluate_expr(mc, env, target)?;
            if let Value::Object(obj) = val {
                if is_array(mc, &obj) {
                    let len_val = object_get_key_value(&obj, "length").unwrap_or(new_gc_cell_ptr(mc, Value::Undefined));
                    let len = if let Value::Number(n) = *len_val.borrow() { n as usize } else { 0 };
                    for k in 0..len {
                        let item = object_get_key_value(&obj, k).unwrap_or(new_gc_cell_ptr(mc, Value::Undefined));
                        eval_args.push(item.borrow().clone());
                    }
                } else {
                    // Support generic iterables via Symbol.iterator
                    let mut iter_fn_opt: Option<crate::core::GcPtr<'gc, Value<'gc>>> = None;
                    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                    {
                        let iter_sym_val_opt = object_get_key_value(sym_obj, "iterator");
                        if let Some(iter_sym_val) = iter_sym_val_opt
                            && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                        {
                            // Use accessor-aware property read to support getters that return the iterator method
                            let iter_fn_val = get_property_with_accessors(mc, env, &obj, iter_sym)?;
                            // If the accessor returned undefined, there's no iterator
                            if !matches!(iter_fn_val, crate::core::Value::Undefined) {
                                // Wrap the returned Value into a GC pointer so code below can inspect/call it uniformly
                                iter_fn_opt = Some(crate::core::new_gc_cell_ptr(mc, iter_fn_val));
                            }
                        }
                    }
                    if let Some(iter_fn_val) = iter_fn_opt {
                        // Call iterator method on the object to get an iterator
                        let iterator = match &*iter_fn_val.borrow() {
                            Value::Function(name) => {
                                let call_env = crate::js_class::prepare_call_env_with_this(
                                    mc,
                                    Some(env),
                                    Some(&Value::Object(obj)),
                                    None,
                                    &[],
                                    None,
                                    Some(env),
                                    None,
                                )?;
                                evaluate_call_dispatch(mc, &call_env, &Value::Function(name.clone()), Some(&Value::Object(obj)), &[])?
                            }
                            Value::Closure(cl) => crate::core::call_closure(mc, cl, Some(&Value::Object(obj)), &[], env, None)?,
                            Value::Object(o) => {
                                // Support function objects that wrap a closure (get_closure)
                                if let Some(cl_ptr) = o.borrow().get_closure() {
                                    match &*cl_ptr.borrow() {
                                        Value::Closure(cl) => crate::core::call_closure(mc, cl, Some(&Value::Object(*o)), &[], env, None)?,
                                        Value::Function(name) => {
                                            let call_env = crate::js_class::prepare_call_env_with_this(
                                                mc,
                                                Some(env),
                                                Some(&Value::Object(*o)),
                                                None,
                                                &[],
                                                None,
                                                Some(env),
                                                None,
                                            )?;
                                            evaluate_call_dispatch(
                                                mc,
                                                &call_env,
                                                &Value::Function(name.clone()),
                                                Some(&Value::Object(*o)),
                                                &[],
                                            )?
                                        }
                                        _ => return Err(raise_type_error!("Spread target is not iterable").into()),
                                    }
                                } else {
                                    log::trace!("Spread target is not iterable: iterator property is object but not callable");
                                    return Err(raise_type_error!("Spread target is not iterable").into());
                                }
                            }
                            _ => {
                                log::trace!("Spread target is not iterable: iterator property exists but not callable");
                                return Err(raise_type_error!("Spread target is not iterable").into());
                            }
                        };

                        // Consume iterator by repeatedly calling its next() method
                        if let Value::Object(iter_obj) = iterator {
                            loop {
                                if let Some(next_val) = object_get_key_value(&iter_obj, "next") {
                                    let next_fn = next_val.borrow().clone();

                                    let res = match &next_fn {
                                        Value::Function(name) => {
                                            let call_env = crate::js_class::prepare_call_env_with_this(
                                                mc,
                                                Some(env),
                                                Some(&Value::Object(iter_obj)),
                                                None,
                                                &[],
                                                None,
                                                Some(env),
                                                None,
                                            )?;
                                            evaluate_call_dispatch(
                                                mc,
                                                &call_env,
                                                &Value::Function(name.clone()),
                                                Some(&Value::Object(iter_obj)),
                                                &[],
                                            )?
                                        }
                                        Value::Closure(cl) => call_closure(mc, cl, Some(&Value::Object(iter_obj)), &[], env, None)?,
                                        Value::Object(o) => {
                                            if let Some(cl_ptr) = o.borrow().get_closure() {
                                                match &*cl_ptr.borrow() {
                                                    Value::Closure(cl) => call_closure(mc, cl, Some(&Value::Object(*o)), &[], env, None)?,
                                                    Value::Function(name) => {
                                                        let call_env = crate::js_class::prepare_call_env_with_this(
                                                            mc,
                                                            Some(env),
                                                            Some(&Value::Object(*o)),
                                                            None,
                                                            &[],
                                                            None,
                                                            Some(env),
                                                            None,
                                                        )?;
                                                        evaluate_call_dispatch(
                                                            mc,
                                                            &call_env,
                                                            &Value::Function(name.clone()),
                                                            Some(&Value::Object(*o)),
                                                            &[],
                                                        )?
                                                    }
                                                    _ => return Err(raise_type_error!("Iterator.next is not callable").into()),
                                                }
                                            } else {
                                                return Err(raise_type_error!("Iterator.next is not callable").into());
                                            }
                                        }
                                        _ => {
                                            return Err(raise_type_error!("Iterator.next is not callable").into());
                                        }
                                    };

                                    if let Value::Object(res_obj) = res {
                                        // Access 'done' and 'value' using accessor-aware property reads so getters can run
                                        let done = match get_property_with_accessors(mc, env, &res_obj, "done") {
                                            Ok(v) => {
                                                if let Value::Boolean(b) = v {
                                                    b
                                                } else {
                                                    false
                                                }
                                            }
                                            Err(e) => return Err(e),
                                        };

                                        if done {
                                            break;
                                        }

                                        let value = get_property_with_accessors(mc, env, &res_obj, "value")?;

                                        eval_args.push(value);

                                        continue;
                                    } else {
                                        return Err(raise_type_error!("Iterator.next did not return an object").into());
                                    }
                                } else {
                                    return Err(raise_type_error!("Iterator has no next method").into());
                                }
                            }
                        } else {
                            return Err(raise_type_error!("Iterator call did not return an object").into());
                        }
                    }
                }
            } else {
                return Err(raise_type_error!("Spread only implemented for Objects").into());
            }
        } else {
            let val = evaluate_expr(mc, env, arg_expr)?;
            eval_args.push(val);
        }
    }

    Ok(eval_args)
}

fn evaluate_optional_chain_base<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    expr: &Expr,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    // Guard recursion depth to avoid stack overflows from unexpected cycles
    struct RecursionGuard;
    impl Drop for RecursionGuard {
        fn drop(&mut self) {
            OPT_CHAIN_RECURSION_DEPTH.with(|c| {
                let new = c.get().saturating_sub(1);
                c.set(new);
                log::trace!("EXIT evaluate_optional_chain_base depth={new}");
            });
        }
    }

    OPT_CHAIN_RECURSION_DEPTH.with(|c| c.set(c.get().saturating_add(1)));
    let _guard = RecursionGuard;
    // Quick safety gate: fail early if recursion grows unexpectedly large.
    // This is stricter than the global limit and helps surface runaway
    // recursion during debugging without affecting normal shallow chains.
    const OPT_CHAIN_SAFE_LIMIT: u32 = 256;
    let depth_now = OPT_CHAIN_RECURSION_DEPTH.with(|c| c.get());
    if depth_now > OPT_CHAIN_SAFE_LIMIT {
        log::error!(
            "optional chain recursion safety triggered depth={} (limit={})",
            depth_now,
            OPT_CHAIN_SAFE_LIMIT
        );
        return Err(raise_eval_error!("optional chain recursion safety triggered").into());
    }
    if depth_now > OPT_CHAIN_RECURSION_LIMIT {
        return Err(raise_eval_error!("optional chain recursion limit exceeded").into());
    }
    OPT_CHAIN_RECURSION_DEPTH.with(|c| log::trace!("ENTER evaluate_optional_chain_base depth={} expr={:?}", c.get(), expr));
    match expr {
        Expr::OptionalProperty(obj_expr, prop) => {
            log::trace!(
                "OPT_CHAIN: OptionalProperty -> recursing into obj_expr={:?} depth={}",
                obj_expr,
                OPT_CHAIN_RECURSION_DEPTH.with(|c| c.get())
            );
            let base_val = match evaluate_optional_chain_base(mc, env, obj_expr)? {
                Some(val) => val,
                None => return Ok(None),
            };
            if base_val.is_null_or_undefined() {
                log::trace!("OPT_CHAIN: OptionalProperty short-circuit (base nullish) expr={:?}", obj_expr);
                Ok(None)
            } else if let Value::Object(obj) = &base_val {
                // Use accessor-aware property read so getters are executed.
                let prop_val = get_property_with_accessors(mc, env, obj, prop)?;
                log::trace!("OPT_CHAIN: OptionalProperty got prop_val={:?} for prop='{}'", prop_val, prop);
                if !matches!(prop_val, Value::Undefined) {
                    Ok(Some(prop_val))
                } else {
                    Ok(Some(Value::Undefined))
                }
            } else {
                Ok(Some(get_primitive_prototype_property(mc, env, &base_val, prop)?))
            }
        }
        Expr::OptionalIndex(obj_expr, index_expr) => {
            log::trace!(
                "OPT_CHAIN: OptionalIndex -> recursing into obj_expr={:?} depth={}",
                obj_expr,
                OPT_CHAIN_RECURSION_DEPTH.with(|c| c.get())
            );
            let base_val = match evaluate_optional_chain_base(mc, env, obj_expr)? {
                Some(val) => val,
                None => return Ok(None),
            };
            if base_val.is_null_or_undefined() {
                log::trace!("OPT_CHAIN: OptionalIndex short-circuit (base nullish) expr={:?}", obj_expr);
                return Ok(None);
            }
            let index_val = evaluate_expr(mc, env, index_expr)?;
            let prop_key = match index_val {
                Value::Symbol(s) => crate::core::PropertyKey::Symbol(s),
                Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                val => {
                    let s = match val {
                        Value::Number(n) => n.to_string(),
                        Value::Boolean(b) => b.to_string(),
                        Value::Undefined => "undefined".to_string(),
                        Value::Null => "null".to_string(),
                        _ => crate::core::value_to_string(&val),
                    };
                    crate::core::PropertyKey::String(s)
                }
            };
            if let Value::Object(obj) = &base_val {
                // Use accessor-aware lookup so getters are invoked
                let prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                if !matches!(prop_val, Value::Undefined) {
                    Ok(Some(prop_val))
                } else {
                    Ok(Some(Value::Undefined))
                }
            } else {
                Ok(Some(get_primitive_prototype_property(mc, env, &base_val, &prop_key)?))
            }
        }
        Expr::OptionalPrivateMember(obj_expr, key) => {
            log::trace!(
                "OPT_CHAIN: OptionalPrivateMember -> recursing into obj_expr={:?} depth={}",
                obj_expr,
                OPT_CHAIN_RECURSION_DEPTH.with(|c| c.get())
            );
            let base_val = match evaluate_optional_chain_base(mc, env, obj_expr)? {
                Some(val) => val,
                None => return Ok(None),
            };
            if base_val.is_null_or_undefined() {
                log::trace!("OPT_CHAIN: OptionalPrivateMember short-circuit (base nullish) expr={:?}", obj_expr);
                Ok(None)
            } else if let Value::Object(obj) = &base_val {
                let pv = evaluate_var(mc, env, key)?;
                if let Value::PrivateName(n, id) = pv {
                    Ok(Some(get_property_with_accessors(mc, env, obj, PropertyKey::Private(n, id))?))
                } else {
                    Err(raise_syntax_error!(format!("Private field '{}' must be declared in an enclosing class", key)).into())
                }
            } else {
                Err(raise_type_error!("Cannot access private field on non-object").into())
            }
        }
        Expr::Property(obj_expr, key) => {
            log::trace!(
                "OPT_CHAIN: Property -> evaluating base for obj_expr={:?} key={} depth={}",
                obj_expr,
                key,
                OPT_CHAIN_RECURSION_DEPTH.with(|c| c.get())
            );
            // Propagate short-circuiting from nested optional chains (e.g., a?.b.c)
            let base_val = match evaluate_optional_chain_base(mc, env, obj_expr)? {
                Some(v) => v,
                None => return Ok(None),
            };
            if base_val.is_null_or_undefined() {
                log::trace!("OPT_CHAIN: Property base nullish -> return undefined for key='{key}'");
                Ok(Some(Value::Undefined))
            } else if let Value::Object(obj) = &base_val {
                let prop_val = get_property_with_accessors(mc, env, obj, key)?;
                if !matches!(prop_val, Value::Undefined) {
                    Ok(Some(prop_val))
                } else {
                    Ok(Some(Value::Undefined))
                }
            } else {
                Ok(Some(get_primitive_prototype_property(mc, env, &base_val, key)?))
            }
        }
        Expr::Index(obj_expr, index_expr) => {
            // Propagate short-circuiting from nested optional chains (e.g., a?.b['c'])
            let base_val = match evaluate_optional_chain_base(mc, env, obj_expr)? {
                Some(v) => v,
                None => return Ok(None),
            };
            if base_val.is_null_or_undefined() {
                return Ok(Some(Value::Undefined));
            }
            let index_val = evaluate_expr(mc, env, index_expr)?;
            let prop_key = match index_val {
                Value::Symbol(s) => crate::core::PropertyKey::Symbol(s),
                Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                val => {
                    let s = match val {
                        Value::Number(n) => n.to_string(),
                        Value::Boolean(b) => b.to_string(),
                        Value::Undefined => "undefined".to_string(),
                        Value::Null => "null".to_string(),
                        _ => crate::core::value_to_string(&val),
                    };
                    crate::core::PropertyKey::String(s)
                }
            };
            if let Value::Object(obj) = &base_val {
                // Use accessor-aware lookup so getters are invoked
                let prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                if !matches!(prop_val, Value::Undefined) {
                    Ok(Some(prop_val))
                } else {
                    Ok(Some(Value::Undefined))
                }
            } else {
                Ok(Some(get_primitive_prototype_property(mc, env, &base_val, &prop_key)?))
            }
        }
        Expr::OptionalCall(lhs, args) => match &**lhs {
            Expr::Property(obj_expr, key) => {
                let obj_val = evaluate_expr(mc, env, obj_expr)?;
                log::debug!(
                    "EVAL_OPT_CALL_BASE(PROPERTY): obj_expr={:?} obj_val={:?} key={}",
                    obj_expr,
                    obj_val,
                    key
                );
                if obj_val.is_null_or_undefined() {
                    log::debug!("EVAL_OPT_CALL_BASE: short-circuiting property call because base is null/undefined");
                    return Ok(None);
                }
                let eval_args = collect_call_args(mc, env, args)?;

                let f_val = if let Value::Object(obj) = &obj_val {
                    // Use accessor-aware property read so getters are executed.
                    let prop_val = get_property_with_accessors(mc, env, obj, key)?;
                    if !matches!(prop_val, Value::Undefined) {
                        prop_val
                    } else if (key.as_str() == "call" || key.as_str() == "apply") && obj.borrow().get_closure().is_some() {
                        Value::Function(key.to_string())
                    } else {
                        Value::Undefined
                    }
                } else if let Value::String(s) = &obj_val
                    && key == "length"
                {
                    Value::Number(s.len() as f64)
                } else if matches!(
                    obj_val,
                    Value::Closure(_) | Value::Function(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..)
                ) && key == "call"
                {
                    Value::Function("call".to_string())
                } else {
                    get_primitive_prototype_property(mc, env, &obj_val, key)?
                };

                log::debug!("OPTIONALCALL-PROPERTY: f_val={:?} obj_val={:?} key={}", f_val, obj_val, key);

                // If the f_val is nullish then optional call should short-circuit and return undefined
                if f_val.is_null_or_undefined() {
                    log::debug!("EVAL_OPT_CALL_BASE: short-circuiting property call because target is null/undefined");
                    return Ok(None);
                }

                let result = match f_val {
                    Value::Function(name) => {
                        // Optional call invoking `eval` is always an indirect eval — use global env.
                        let env_for_call = if name == "eval" {
                            let mut t = *env;
                            while let Some(proto) = t.borrow().prototype {
                                t = proto;
                            }
                            t
                        } else {
                            *env
                        };
                        let call_env = crate::js_class::prepare_call_env_with_this(
                            mc,
                            Some(&env_for_call),
                            Some(&obj_val),
                            None,
                            &[],
                            None,
                            Some(&env_for_call),
                            None,
                        )?;
                        // Dispatch, handling indirect global `eval` marking when necessary.
                        call_named_eval_or_dispatch(mc, &env_for_call, &call_env, &name, Some(&obj_val), &eval_args)?
                    }
                    Value::Closure(c) => call_closure(mc, &c, Some(&obj_val), &eval_args, env, None)?,
                    Value::Object(o) => {
                        let call_env =
                            crate::js_class::prepare_call_env_with_this(mc, Some(env), Some(&obj_val), None, &[], None, Some(env), None)?;
                        evaluate_call_dispatch(mc, &call_env, &Value::Object(o), Some(&obj_val), &eval_args)?
                    }
                    _ => return Err(raise_type_error!("OptionalCall target is not a function").into()),
                };
                Ok(Some(result))
            }
            Expr::OptionalProperty(obj_expr, key) => {
                // Handle optional property followed by optional call (e.g., a?.b?.())
                log::debug!("EVAL_OPT_CALL_BASE(OPTIONAL_PROPERTY): obj_expr={:?} key={}", obj_expr, key);
                log::trace!(
                    "OPT_CHAIN: OptionalCall->OptionalProperty recursing into obj_expr={:?} depth={}",
                    obj_expr,
                    OPT_CHAIN_RECURSION_DEPTH.with(|c| c.get())
                );
                let base_val = match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(None),
                };
                if base_val.is_null_or_undefined() {
                    log::debug!("EVAL_OPT_CALL_BASE: short-circuiting optional property call because base is null/undefined");
                    return Ok(None);
                }

                let eval_args = collect_call_args(mc, env, args)?;

                // Get the property value, but if the property itself is nullish then optional call should short-circuit
                let f_val = if let Value::Object(obj) = &base_val {
                    let prop_val = get_property_with_accessors(mc, env, obj, key)?;
                    if prop_val.is_null_or_undefined() {
                        log::debug!("EVAL_OPT_CALL_BASE: optional property is nullish, short-circuiting call");
                        return Ok(None);
                    }
                    prop_val
                } else {
                    let proto_val = get_primitive_prototype_property(mc, env, &base_val, key)?;
                    if proto_val.is_null_or_undefined() {
                        log::debug!("EVAL_OPT_CALL_BASE: primitive optional property is nullish, short-circuiting call");
                        return Ok(None);
                    }
                    proto_val
                };

                let result = match f_val {
                    Value::Function(name) => {
                        // Optional call invoking `eval` is always an indirect eval — use global env.
                        let env_for_call = if name == "eval" {
                            let mut t = *env;
                            while let Some(proto) = t.borrow().prototype {
                                t = proto;
                            }
                            t
                        } else {
                            *env
                        };
                        let call_env = crate::js_class::prepare_call_env_with_this(
                            mc,
                            Some(&env_for_call),
                            Some(&base_val),
                            None,
                            &[],
                            None,
                            Some(&env_for_call),
                            None,
                        )?;
                        if name == "eval" {
                            let key = PropertyKey::String("__is_indirect_eval".to_string());
                            object_set_key_value(mc, &env_for_call, &key, &Value::Boolean(true))?;
                            let res = evaluate_call_dispatch(mc, &call_env, &Value::Function(name.clone()), Some(&base_val), &eval_args);
                            let _ = env_for_call.borrow_mut(mc).properties.shift_remove(&key);
                            res?
                        } else {
                            evaluate_call_dispatch(mc, &call_env, &Value::Function(name.clone()), Some(&base_val), &eval_args)?
                        }
                    }
                    Value::Closure(c) => call_closure(mc, &c, Some(&base_val), &eval_args, env, None)?,
                    Value::Object(o) => {
                        let call_env = prepare_call_env_with_this(mc, Some(env), Some(&base_val), None, &[], None, Some(env), None)?;
                        evaluate_call_dispatch(mc, &call_env, &Value::Object(o), Some(&base_val), &eval_args)?
                    }
                    _ => return Err(raise_type_error!("OptionalCall target is not a function").into()),
                };
                Ok(Some(result))
            }

            Expr::OptionalIndex(obj_expr, index_expr) => {
                // Handle optional index followed by optional call (e.g., a?.[i]?.())
                log::debug!("EVAL_OPT_CALL_BASE(OPTIONAL_INDEX): obj_expr={:?}", obj_expr);
                let base_val = match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(None),
                };
                if base_val.is_null_or_undefined() {
                    log::debug!("EVAL_OPT_CALL_BASE: short-circuiting optional index call because base is null/undefined");
                    return Ok(None);
                }

                let index_val = evaluate_expr(mc, env, index_expr)?;
                let prop_key = match index_val {
                    Value::Symbol(s) => crate::core::PropertyKey::Symbol(s),
                    Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                    val => {
                        let s = match val {
                            Value::Number(n) => n.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            Value::Undefined => "undefined".to_string(),
                            Value::Null => "null".to_string(),
                            _ => crate::core::value_to_string(&val),
                        };
                        crate::core::PropertyKey::String(s)
                    }
                };

                let eval_args = collect_call_args(mc, env, args)?;

                // If the property itself is nullish then optional call should short-circuit
                let f_val = if let Value::Object(obj) = &base_val {
                    let prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                    if prop_val.is_null_or_undefined() {
                        log::debug!("EVAL_OPT_CALL_BASE: optional index is nullish, short-circuiting call");
                        return Ok(None);
                    }
                    prop_val
                } else {
                    let proto_val = get_primitive_prototype_property(mc, env, &base_val, &prop_key)?;
                    if proto_val.is_null_or_undefined() {
                        log::debug!("EVAL_OPT_CALL_BASE: primitive optional index is nullish, short-circuiting call");
                        return Ok(None);
                    }
                    proto_val
                };

                let result = match f_val {
                    Value::Function(name) => {
                        // Optional call invoking `eval` is always an indirect eval — use global env.
                        let env_for_call = if name == "eval" {
                            let mut t = *env;
                            while let Some(proto) = t.borrow().prototype {
                                t = proto;
                            }
                            t
                        } else {
                            *env
                        };
                        let call_env = prepare_call_env_with_this(
                            mc,
                            Some(&env_for_call),
                            Some(&base_val),
                            None,
                            &[],
                            None,
                            Some(&env_for_call),
                            None,
                        )?;
                        if name == "eval" {
                            let key = PropertyKey::String("__is_indirect_eval".to_string());
                            object_set_key_value(mc, &env_for_call, &key, &Value::Boolean(true))?;
                            let res = evaluate_call_dispatch(mc, &call_env, &Value::Function(name.clone()), Some(&base_val), &eval_args);
                            let _ = env_for_call.borrow_mut(mc).properties.shift_remove(&key);
                            res?
                        } else {
                            evaluate_call_dispatch(mc, &call_env, &Value::Function(name.clone()), Some(&base_val), &eval_args)?
                        }
                    }
                    Value::Closure(c) => call_closure(mc, &c, Some(&base_val), &eval_args, env, None)?,
                    Value::Object(o) => {
                        let call_env = prepare_call_env_with_this(mc, Some(env), Some(&base_val), None, &[], None, Some(env), None)?;
                        evaluate_call_dispatch(mc, &call_env, &Value::Object(o), Some(&base_val), &eval_args)?
                    }
                    _ => return Err(raise_type_error!("OptionalCall target is not a function").into()),
                };
                Ok(Some(result))
            }

            Expr::Index(obj_expr, index_expr) => {
                let obj_val = evaluate_expr(mc, env, obj_expr)?;
                log::debug!("EVAL_OPT_CALL_BASE(INDEX): obj_expr={:?} obj_val={:?}", obj_expr, obj_val);
                if obj_val.is_null_or_undefined() {
                    log::debug!("EVAL_OPT_CALL_BASE: short-circuiting index call because base is null/undefined");
                    return Ok(None);
                }

                let index_val = evaluate_expr(mc, env, index_expr)?;
                let prop_key = match index_val {
                    Value::Symbol(s) => crate::core::PropertyKey::Symbol(s),
                    Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                    val => {
                        let s = match val {
                            Value::Number(n) => n.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            Value::Undefined => "undefined".to_string(),
                            Value::Null => "null".to_string(),
                            _ => crate::core::value_to_string(&val),
                        };
                        crate::core::PropertyKey::String(s)
                    }
                };

                let eval_args = collect_call_args(mc, env, args)?;

                let f_val = if let Value::Object(obj) = &obj_val {
                    // Use accessor-aware lookup so getters are invoked
                    let prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                    if !matches!(prop_val, Value::Undefined) {
                        prop_val
                    } else {
                        Value::Undefined
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &obj_val, &prop_key)?
                };

                let result = match f_val {
                    Value::Function(name) => {
                        // Optional call invoking `eval` is always an indirect eval — use global env.
                        let env_for_call = if name == "eval" {
                            let mut t = *env;
                            while let Some(proto) = t.borrow().prototype {
                                t = proto;
                            }
                            t
                        } else {
                            *env
                        };
                        let call_env = prepare_call_env_with_this(
                            mc,
                            Some(&env_for_call),
                            Some(&obj_val),
                            None,
                            &[],
                            None,
                            Some(&env_for_call),
                            None,
                        )?;
                        // Dispatch, handling indirect global `eval` marking when necessary.
                        call_named_eval_or_dispatch(mc, &env_for_call, &call_env, &name, Some(&obj_val), &eval_args)?
                    }
                    Value::Closure(c) => call_closure(mc, &c, Some(&obj_val), &eval_args, env, None)?,
                    Value::Object(o) => {
                        let call_env = prepare_call_env_with_this(mc, Some(env), Some(&obj_val), None, &[], None, Some(env), None)?;
                        evaluate_call_dispatch(mc, &call_env, &Value::Object(o), Some(&obj_val), &eval_args)?
                    }
                    _ => return Err(raise_type_error!("OptionalCall target is not a function").into()),
                };
                Ok(Some(result))
            }
            _ => {
                let left_val = evaluate_expr(mc, env, lhs)?;
                if left_val.is_null_or_undefined() {
                    return Ok(None);
                }
                let eval_args = collect_call_args(mc, env, args)?;
                let result = match left_val {
                    Value::Function(name) => {
                        // Optional call invoking `eval` is always an indirect eval — use global env.
                        let env_4_call = if name == "eval" {
                            let mut t = *env;
                            while let Some(proto) = t.borrow().prototype {
                                t = proto;
                            }
                            t
                        } else {
                            *env
                        };
                        let call_env = prepare_call_env_with_this(mc, Some(&env_4_call), None, None, &[], None, Some(&env_4_call), None)?;
                        // Dispatch, handling indirect global `eval` marking when necessary.
                        call_named_eval_or_dispatch(mc, &env_4_call, &call_env, &name, None, &eval_args)?
                    }
                    Value::Closure(c) => call_closure(mc, &c, None, &eval_args, env, None)?,
                    Value::Object(o) => {
                        let call_env = prepare_call_env_with_this(mc, Some(env), None, None, &[], None, Some(env), None)?;
                        evaluate_call_dispatch(mc, &call_env, &Value::Object(o), None, &eval_args)?
                    }

                    _ => return Err(raise_type_error!("OptionalCall target is not a function").into()),
                };
                Ok(Some(result))
            }
        },
        Expr::Call(func, _args) | Expr::New(func, _args) => {
            if expr_contains_optional_chain(func) {
                match evaluate_optional_chain_base(mc, env, func)? {
                    Some(_) => Ok(Some(evaluate_expr(mc, env, expr)?)),
                    None => Ok(None),
                }
            } else {
                Ok(Some(evaluate_expr(mc, env, expr)?))
            }
        }
        _ => Ok(Some(evaluate_expr(mc, env, expr)?)),
    }
}

fn await_promise_value<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let promise_ref_opt = match value {
        Value::Object(obj) => crate::js_promise::get_promise_from_js_object(obj),
        Value::Promise(p) => Some(*p),
        _ => None,
    };

    let promise_ref = if let Some(p) = promise_ref_opt {
        p
    } else {
        // Wrap non-promise values in a resolved promise to ensure microtask interleaving
        let (p, resolve, _) = crate::js_promise::create_promise_capability(mc, env)?;
        crate::js_promise::call_function(mc, &resolve, std::slice::from_ref(value), env)?;
        p
    };

    match value {
        Value::Object(obj) => {
            if crate::js_promise::get_promise_from_js_object(obj).is_some() {
                crate::js_promise::mark_promise_handled(mc, promise_ref, env).expect("Marking promise as handled failed");
            }
        }
        _ => crate::js_promise::mark_promise_handled(mc, promise_ref, env).expect("Marking promise as handled failed"),
    }

    loop {
        let state = promise_ref.borrow().state.clone();
        match state {
            crate::core::PromiseState::Pending => {
                match crate::js_promise::run_event_loop(mc)? {
                    crate::js_promise::PollResult::Executed => continue,
                    crate::js_promise::PollResult::Wait(d) => {
                        std::thread::sleep(d);
                        continue;
                    }
                    crate::js_promise::PollResult::Empty => {
                        // Process any matured runtime pending unhandleds in this env
                        if crate::js_promise::process_runtime_pending_unhandled(mc, env, false)? {
                            continue;
                        }
                        std::thread::yield_now();
                        continue;
                    }
                }
            }
            crate::core::PromiseState::Fulfilled(v) => {
                // Await always yields to the microtask queue once, even when already fulfilled.
                let _ = crate::js_promise::run_event_loop(mc);
                return Ok(v.clone());
            }
            crate::core::PromiseState::Rejected(r) => {
                let _ = crate::js_promise::run_event_loop(mc);
                return Err(EvalError::Throw(r.clone(), None, None));
            }
        }
    }
}

#[allow(dead_code)]
fn await_promise_value_if_pending<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let promise_ref_opt = match value {
        Value::Object(obj) => crate::js_promise::get_promise_from_js_object(obj),
        Value::Promise(p) => Some(*p),
        _ => None,
    };

    let promise_ref = if let Some(p) = promise_ref_opt {
        p
    } else {
        let (p, resolve, _) = crate::js_promise::create_promise_capability(mc, env)?;
        crate::js_promise::call_function(mc, &resolve, std::slice::from_ref(value), env).expect("call_function failed");
        p
    };

    match &value {
        Value::Object(obj) => {
            if crate::js_promise::get_promise_from_js_object(obj).is_some() {
                crate::js_promise::mark_promise_handled(mc, promise_ref, env).expect("Marking promise as handled failed");
            }
        }
        _ => crate::js_promise::mark_promise_handled(mc, promise_ref, env).expect("Marking promise as handled failed"),
    }

    loop {
        let state = promise_ref.borrow().state.clone();
        match state {
            crate::core::PromiseState::Pending => {
                match crate::js_promise::run_event_loop(mc)? {
                    crate::js_promise::PollResult::Executed => continue,
                    crate::js_promise::PollResult::Wait(d) => {
                        std::thread::sleep(d);
                        continue;
                    }
                    crate::js_promise::PollResult::Empty => {
                        // Process any matured runtime pending unhandleds in this env
                        if crate::js_promise::process_runtime_pending_unhandled(mc, env, false)? {
                            continue;
                        }
                        std::thread::yield_now();
                        continue;
                    }
                }
            }
            crate::core::PromiseState::Fulfilled(v) => return Ok(v.clone()),
            crate::core::PromiseState::Rejected(r) => return Err(EvalError::Throw(r.clone(), None, None)),
        }
    }
}

fn evaluate_expr_binary<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let l_val = evaluate_expr(mc, env, left)?;
    let r_val = evaluate_expr(mc, env, right)?;
    match op {
        BinaryOp::Add => {
            let l_prim = crate::core::to_primitive(mc, &l_val, "default", env)?;
            let r_prim = crate::core::to_primitive(mc, &r_val, "default", env)?;

            // If either is String, concatenate with ToString semantics
            if matches!(l_prim, Value::String(_)) || matches!(r_prim, Value::String(_)) {
                let mut res = match &l_prim {
                    Value::String(ls) => ls.clone(),
                    _ => utf8_to_utf16(&to_string_for_concat(mc, env, &l_prim)?),
                };
                match &r_prim {
                    Value::String(rs) => res.extend(rs.clone()),
                    _ => res.extend(utf8_to_utf16(&to_string_for_concat(mc, env, &r_prim)?)),
                }
                return Ok(Value::String(res));
            }

            match (l_prim, r_prim) {
                (Value::BigInt(ln), Value::BigInt(rn)) => Ok(Value::BigInt(Box::new(*ln + *rn))),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln + rn)),
                (lprim, rprim) => Ok(Value::Number(to_number(&lprim)? + to_number(&rprim)?)),
            }
        }
        BinaryOp::Sub => {
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(ln), Value::BigInt(rn)) => Ok(Value::BigInt(Box::new(*ln - *rn))),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln - rn)),
                (l, r) => Ok(Value::Number(to_number(&l)? - to_number(&r)?)),
            }
        }
        BinaryOp::Mul => {
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(ln), Value::BigInt(rn)) => Ok(Value::BigInt(Box::new(*ln * *rn))),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln * rn)),
                (l, r) => Ok(Value::Number(to_number(&l)? * to_number(&r)?)),
            }
        }
        BinaryOp::Div => {
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    if rn.is_zero() {
                        return Err(raise_range_error!("Division by zero").into());
                    }
                    Ok(Value::BigInt(Box::new(*ln / *rn)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln / rn)),
                (l, r) => Ok(Value::Number(to_number(&l)? / to_number(&r)?)),
            }
        }
        BinaryOp::LeftShift => {
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    // Handle negative shift counts: perform division by 2^(-rn), rounding down
                    if rn.sign() == num_bigint::Sign::Minus {
                        let neg = -(&*rn);
                        let shift = match neg.to_usize() {
                            Some(s) => s,
                            None => return Err(raise_eval_error!("invalid bigint shift").into()),
                        };
                        if shift == 0 {
                            return Ok(Value::BigInt(Box::new((*ln).clone())));
                        }
                        let divisor = BigInt::from(1u8) << shift;
                        let q = &*ln / &divisor;
                        let r = &*ln % &divisor;
                        if (*ln).sign() == num_bigint::Sign::Minus && !r.is_zero() {
                            return Ok(Value::BigInt(Box::new(q - BigInt::from(1u8))));
                        }
                        return Ok(Value::BigInt(Box::new(q)));
                    }
                    let shift = match rn.to_usize() {
                        Some(s) => s,
                        None => return Err(raise_eval_error!("invalid bigint shift").into()),
                    };
                    Ok(Value::BigInt(Box::new((*ln).clone() << shift)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (l, r) => {
                    let l = to_int32_value_with_env(mc, env, &l)?;
                    let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                    Ok(Value::Number(((l << r) as i32) as f64))
                }
            }
        }
        BinaryOp::RightShift => {
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    // signedRightShift(x, y) is defined as leftShift(x, -y)
                    if rn.sign() == num_bigint::Sign::Minus {
                        let pos = match (-&*rn).to_usize() {
                            Some(s) => s,
                            None => return Err(raise_eval_error!("invalid bigint shift").into()),
                        };
                        Ok(Value::BigInt(Box::new((*ln).clone() << pos)))
                    } else {
                        let shift = match rn.to_usize() {
                            Some(s) => s,
                            None => return Err(raise_eval_error!("invalid bigint shift").into()),
                        };
                        Ok(Value::BigInt(Box::new((*ln).clone() >> shift)))
                    }
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (l, r) => {
                    let l = to_int32_value_with_env(mc, env, &l)?;
                    let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                    Ok(Value::Number((l >> r) as f64))
                }
            }
        }
        BinaryOp::UnsignedRightShift => match (l_val, r_val) {
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("BigInt does not support >>>").into()),
            (l, r) => {
                let l = to_uint32_value_with_env(mc, env, &l)?;
                let r = (to_uint32_value_with_env(mc, env, &r)? & 0x1F) as u32;
                Ok(Value::Number((l >> r) as f64))
            }
        },
        BinaryOp::StrictEqual => {
            let eq = match (l_val, r_val) {
                (Value::Number(l), Value::Number(r)) => l == r,
                (Value::BigInt(l), Value::BigInt(r)) => l == r,
                (Value::String(l), Value::String(r)) => l == r,
                (Value::Boolean(l), Value::Boolean(r)) => l == r,
                (Value::Null, Value::Null) => true,
                (Value::Undefined, Value::Undefined) => true,
                (Value::Object(l), Value::Object(r)) => Gc::ptr_eq(l, r),
                (Value::Closure(l), Value::Closure(r)) => Gc::ptr_eq(l, r),
                (Value::Function(l), Value::Function(r)) => l == r,
                (Value::Symbol(l), Value::Symbol(r)) => Gc::ptr_eq(l, r),
                _ => false,
            };
            Ok(Value::Boolean(eq))
        }
        BinaryOp::StrictNotEqual => {
            let eq = match (l_val, r_val) {
                (Value::Number(l), Value::Number(r)) => l == r,
                (Value::BigInt(l), Value::BigInt(r)) => l == r,
                (Value::String(l), Value::String(r)) => l == r,
                (Value::Boolean(l), Value::Boolean(r)) => l == r,
                (Value::Null, Value::Null) => true,
                (Value::Undefined, Value::Undefined) => true,
                (Value::Object(l), Value::Object(r)) => Gc::ptr_eq(l, r),
                (Value::Closure(l), Value::Closure(r)) => Gc::ptr_eq(l, r),
                (Value::Function(l), Value::Function(r)) => l == r,
                (Value::Symbol(l), Value::Symbol(r)) => Gc::ptr_eq(l, r),
                _ => false,
            };
            Ok(Value::Boolean(!eq))
        }
        BinaryOp::In => {
            log::trace!("DEBUG-IN: evaluating In operator: l_val={:?}, r_val={:?}", l_val, r_val);
            if let Value::Object(obj) = r_val {
                if let Value::PrivateName(name, id) = l_val {
                    log::trace!("DEBUG-IN: private-in detected: name={}, id={}", name, id);
                    let key = PropertyKey::Private(name, id);
                    // Check for existence of private property anywhere in the prototype chain
                    // (though private fields/methods are usually own properties or on specific prototypes)
                    let present = object_get_key_value(&obj, key).is_some();
                    return Ok(Value::Boolean(present));
                }

                let key = match l_val {
                    Value::String(s) => utf16_to_utf8(&s),
                    _ => value_to_string(&l_val),
                };
                // Handle Proxy's has trap if present
                if let Some(proxy_ptr) = get_own_property(&obj, "__proxy__")
                    && let Value::Proxy(p) = &*proxy_ptr.borrow()
                {
                    let present = crate::js_proxy::proxy_has_property(mc, p, &key)?;
                    return Ok(Value::Boolean(present));
                }
                let present = object_get_key_value(&obj, key).is_some();
                Ok(Value::Boolean(present))
            } else {
                log::trace!("DEBUG-IN: RHS is not object: {:?}", r_val);
                Err(raise_type_error!("Right-hand side of 'in' must be an object").into())
            }
        }
        BinaryOp::Equal => {
            let eq = loose_equal(mc, &l_val, &r_val, env)?;
            Ok(Value::Boolean(eq))
        }
        BinaryOp::NotEqual => {
            let eq = loose_equal(mc, &l_val, &r_val, env)?;
            Ok(Value::Boolean(!eq))
        }
        BinaryOp::GreaterThan => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::Boolean(l > r)),
            (Value::String(l), Value::String(r)) => {
                // Compare UTF-16 code unit sequences directly per ECMAScript
                // relational comparison rules (do NOT lossily convert to UTF-8).
                Ok(Value::Boolean(crate::unicode::utf16_cmp(&l, &r) == std::cmp::Ordering::Greater))
            }
            (Value::BigInt(l), other) => {
                // If the other operand is a string, attempt to parse it as an
                // integer decimal BigInt literal (no fractional/exponent parts).
                // If parsing fails, the comparison is undefined per Test262 and
                // should yield false for relational comparisons between BigInt
                // and an incomparable string.
                if let Value::String(s) = other {
                    let ss = crate::unicode::utf16_to_utf8(&s);
                    return Ok(string_to_bigint_for_eq(&ss).map(|rb| *l > rb).unwrap_or(false).into());
                }

                // For other cases (numbers, etc.) perform spec-aligned
                // BigInt/Number relational comparison:
                // - If the RHS (after ToNumber) is NaN -> false
                // - If the RHS is a finite integer value, attempt to convert it to
                //   a BigInt and compare as BigInt; otherwise compare by
                //   converting BigInt to f64 and comparing as numbers.
                let rn = to_number_with_env(mc, env, &other)?;
                if let Some(ord) = compare_bigint_and_number(&l, rn) {
                    return Ok(Value::Boolean(ord == std::cmp::Ordering::Greater));
                }
                Ok(Value::Boolean(false))
            }
            (other, Value::BigInt(r)) => {
                // If the left operand is a string, try parsing it as an integer
                // decimal BigInt. If parsing fails, the comparison yields false.
                if let Value::String(s) = other {
                    let ss = crate::unicode::utf16_to_utf8(&s);
                    return Ok(string_to_bigint_for_eq(&ss).map(|lb| lb > *r).unwrap_or(false).into());
                }

                // For other cases, attempt to convert LHS to Number and then
                // perform the symmetric BigInt/Number comparison rules used above.
                let ln = to_number_with_env(mc, env, &other)?;
                if let Some(ord) = compare_bigint_and_number(&r, ln) {
                    return Ok(Value::Boolean(ord == std::cmp::Ordering::Less));
                }
                Ok(Value::Boolean(false))
            }
            (l, r) => {
                // If either side is an object, ToPrimitive with hint "number" must
                // be applied first. If both primitives are strings, compare using
                // UTF-16 code unit sequence comparison per ECMAScript string
                // relational comparison rules.
                let lprim = if let Value::Object(_) = l {
                    crate::core::to_primitive(mc, &l, "number", env)?
                } else {
                    l.clone()
                };
                let rprim = if let Value::Object(_) = r {
                    crate::core::to_primitive(mc, &r, "number", env)?
                } else {
                    r.clone()
                };
                if let (Value::String(ls), Value::String(rs)) = (&lprim, &rprim) {
                    return Ok(Value::Boolean(crate::unicode::utf16_cmp(ls, rs) == std::cmp::Ordering::Greater));
                }
                let ln = to_number_with_env(mc, env, &lprim)?;
                let rn = to_number_with_env(mc, env, &rprim)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln > rn))
            }
        },
        BinaryOp::LessThan => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::Boolean(l < r)),
            (Value::String(l), Value::String(r)) => Ok(Value::Boolean(crate::unicode::utf16_cmp(&l, &r) == std::cmp::Ordering::Less)),
            (Value::BigInt(l), other) => {
                if let Value::String(s) = other {
                    let ss = crate::unicode::utf16_to_utf8(&s);
                    return Ok(string_to_bigint_for_eq(&ss).map(|rb| *l < rb).unwrap_or(false).into());
                }

                let rn = to_number_with_env(mc, env, &other)?;
                if let Some(ord) = compare_bigint_and_number(&l, rn) {
                    return Ok(Value::Boolean(ord == std::cmp::Ordering::Less));
                }
                Ok(Value::Boolean(false))
            }
            (other, Value::BigInt(r)) => {
                if let Value::String(s) = other {
                    let ss = crate::unicode::utf16_to_utf8(&s);
                    return Ok(string_to_bigint_for_eq(&ss).map(|lb| lb < *r).unwrap_or(false).into());
                }

                let ln = to_number_with_env(mc, env, &other)?;
                if ln.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                if ln.is_finite()
                    && ln.fract() == 0.0
                    && let Some(lb) = num_bigint::BigInt::from_f64(ln)
                {
                    return Ok(Value::Boolean(lb < *r));
                }
                let rn = r.to_f64().unwrap_or(f64::NAN);
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln < rn))
            }
            (l, r) => {
                let lprim = if let Value::Object(_) = l {
                    crate::core::to_primitive(mc, &l, "number", env)?
                } else {
                    l.clone()
                };
                let rprim = if let Value::Object(_) = r {
                    crate::core::to_primitive(mc, &r, "number", env)?
                } else {
                    r.clone()
                };
                if let (Value::String(ls), Value::String(rs)) = (&lprim, &rprim) {
                    return Ok(Value::Boolean(crate::unicode::utf16_cmp(ls, rs) == std::cmp::Ordering::Less));
                }
                let ln = to_number_with_env(mc, env, &lprim)?;
                let rn = to_number_with_env(mc, env, &rprim)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln < rn))
            }
        },
        BinaryOp::GreaterEqual => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::Boolean(l >= r)),
            (Value::String(l), Value::String(r)) => Ok(Value::Boolean(crate::unicode::utf16_cmp(&l, &r) != std::cmp::Ordering::Less)),
            (Value::BigInt(l), other) => {
                if let Value::String(s) = other {
                    let ss = crate::unicode::utf16_to_utf8(&s);
                    return Ok(string_to_bigint_for_eq(&ss).map(|rb| *l >= rb).unwrap_or(false).into());
                }

                let rn = to_number_with_env(mc, env, &other)?;
                if rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                if rn.is_finite()
                    && rn.fract() == 0.0
                    && let Some(rb) = num_bigint::BigInt::from_f64(rn)
                {
                    return Ok(Value::Boolean(*l >= rb));
                }
                let ln = l.to_f64().unwrap_or(f64::NAN);
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln >= rn))
            }
            (other, Value::BigInt(r)) => {
                if let Value::String(s) = other {
                    let ss = crate::unicode::utf16_to_utf8(&s);
                    return Ok(string_to_bigint_for_eq(&ss).map(|lb| lb >= *r).unwrap_or(false).into());
                }

                let ln = to_number_with_env(mc, env, &other)?;
                if ln.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                if ln.is_finite()
                    && ln.fract() == 0.0
                    && let Some(lb) = num_bigint::BigInt::from_f64(ln)
                {
                    return Ok(Value::Boolean(lb >= *r));
                }
                let rn = r.to_f64().unwrap_or(f64::NAN);
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln >= rn))
            }
            (l, r) => {
                let lprim = if let Value::Object(_) = l {
                    crate::core::to_primitive(mc, &l, "number", env)?
                } else {
                    l.clone()
                };
                let rprim = if let Value::Object(_) = r {
                    crate::core::to_primitive(mc, &r, "number", env)?
                } else {
                    r.clone()
                };
                if let (Value::String(ls), Value::String(rs)) = (&lprim, &rprim) {
                    return Ok(Value::Boolean(crate::unicode::utf16_cmp(ls, rs) != std::cmp::Ordering::Less));
                }
                let ln = to_number_with_env(mc, env, &lprim)?;
                let rn = to_number_with_env(mc, env, &rprim)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln >= rn))
            }
        },
        BinaryOp::LessEqual => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::Boolean(l <= r)),
            (Value::String(l), Value::String(r)) => Ok(Value::Boolean(crate::unicode::utf16_cmp(&l, &r) != std::cmp::Ordering::Greater)),
            (Value::BigInt(l), other) => {
                if let Value::String(s) = other {
                    let ss = crate::unicode::utf16_to_utf8(&s);
                    return Ok(string_to_bigint_for_eq(&ss).map(|rb| *l <= rb).unwrap_or(false).into());
                }

                let rn = to_number_with_env(mc, env, &other)?;
                if rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                if rn.is_finite()
                    && rn.fract() == 0.0
                    && let Some(rb) = num_bigint::BigInt::from_f64(rn)
                {
                    return Ok(Value::Boolean(*l <= rb));
                }
                let ln = l.to_f64().unwrap_or(f64::NAN);
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln <= rn))
            }
            (other, Value::BigInt(r)) => {
                if let Value::String(s) = other {
                    let ss = crate::unicode::utf16_to_utf8(&s);
                    return Ok(string_to_bigint_for_eq(&ss).map(|lb| lb <= *r).unwrap_or(false).into());
                }

                let ln = to_number_with_env(mc, env, &other)?;
                if ln.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                if ln.is_finite()
                    && ln.fract() == 0.0
                    && let Some(lb) = num_bigint::BigInt::from_f64(ln)
                {
                    return Ok(Value::Boolean(lb <= *r));
                }
                let rn = r.to_f64().unwrap_or(f64::NAN);
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln <= rn))
            }
            (l, r) => {
                let lprim = if let Value::Object(_) = l {
                    crate::core::to_primitive(mc, &l, "number", env)?
                } else {
                    l.clone()
                };
                let rprim = if let Value::Object(_) = r {
                    crate::core::to_primitive(mc, &r, "number", env)?
                } else {
                    r.clone()
                };
                if let (Value::String(ls), Value::String(rs)) = (&lprim, &rprim) {
                    return Ok(Value::Boolean(crate::unicode::utf16_cmp(ls, rs) != std::cmp::Ordering::Greater));
                }
                let ln = to_number_with_env(mc, env, &lprim)?;
                let rn = to_number_with_env(mc, env, &rprim)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln <= rn))
            }
        },
        BinaryOp::Mod => {
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    if rn.is_zero() {
                        return Err(raise_range_error!("Division by zero").into());
                    }
                    Ok(Value::BigInt(Box::new(*ln % *rn)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln % rn)),
                (l, r) => Ok(Value::Number(to_number(&l)? % to_number(&r)?)),
            }
        }
        BinaryOp::Pow => {
            // Perform ToNumeric on both operands first (handles wrapped primitives)
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(base), Value::BigInt(exp)) => {
                    if exp.sign() == num_bigint::Sign::Minus {
                        return Err(raise_range_error!("Exponent must be non-negative").into());
                    }
                    let e = exp
                        .to_u32()
                        .ok_or_else(|| EvalError::Js(raise_range_error!("Exponent too large")))?;
                    Ok(Value::BigInt(Box::new(base.pow(e))))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(lf), Value::Number(rf)) => {
                    // If abs(base) is 1 and exponent is +Infinity or -Infinity, result is NaN
                    if lf.abs() == 1.0 && rf.is_infinite() {
                        Ok(Value::Number(f64::NAN))
                    } else {
                        Ok(Value::Number(lf.powf(rf)))
                    }
                }
                _ => unreachable!("ToNumeric returned non-numeric primitive"),
            }
        }
        BinaryOp::BitAnd => {
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::BigInt(Box::new(*l & *r))),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(l), Value::Number(r)) => {
                    let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                    let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                    Ok(Value::Number((l & r) as f64))
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise AND").into()),
            }
        }
        BinaryOp::BitOr => {
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::BigInt(Box::new(*l | *r))),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(l), Value::Number(r)) => {
                    let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                    let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                    Ok(Value::Number((l | r) as f64))
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise OR").into()),
            }
        }
        BinaryOp::BitXor => {
            let lnum = to_numeric_with_env(mc, env, &l_val)?;
            let rnum = to_numeric_with_env(mc, env, &r_val)?;
            match (lnum, rnum) {
                (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::BigInt(Box::new(*l ^ *r))),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types").into()),
                (Value::Number(l), Value::Number(r)) => {
                    let l = to_int32_value_with_env(mc, env, &Value::Number(l))?;
                    let r = to_int32_value_with_env(mc, env, &Value::Number(r))?;
                    Ok(Value::Number((l ^ r) as f64))
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise XOR").into()),
            }
        }
        BinaryOp::NullishCoalescing => {
            if l_val.is_null_or_undefined() {
                Ok(r_val)
            } else {
                Ok(l_val)
            }
        }
        BinaryOp::InstanceOf => match r_val {
            Value::Object(ctor) => {
                // Per ECMAScript: if RHS has a @@hasInstance method, call it (GetMethod)
                if let Some(sym_ctor_rc) = crate::core::object_get_key_value(env, "Symbol")
                    && let Value::Object(sym_ctor_obj) = &*sym_ctor_rc.borrow()
                    && let Some(has_inst_rc) = crate::core::object_get_key_value(sym_ctor_obj, "hasInstance")
                    && let Value::Symbol(has_inst_sym) = &*has_inst_rc.borrow()
                {
                    // Get method = ctor[Symbol.hasInstance] using Get (accessors invoked)
                    let method_val =
                        crate::core::get_property_with_accessors(mc, env, &ctor, crate::core::PropertyKey::Symbol(*has_inst_sym))?;
                    match method_val {
                        Value::Undefined | Value::Null => { /* treat as absent */ }
                        ref method_val => {
                            // If present but not callable -> TypeError per spec
                            let is_callable = matches!(method_val, Value::Closure(_) | Value::Function(_) | Value::Object(_));
                            if !is_callable {
                                return Err(raise_type_error!("Symbol.hasInstance method is not callable").into());
                            }
                            // Call method with this = ctor and argument = left value
                            let call_res =
                                crate::js_promise::call_function_with_this(mc, method_val, Some(&Value::Object(ctor)), &[l_val], env)?;
                            return Ok(Value::Boolean(call_res.to_truthy()));
                        }
                    }
                }

                // No @@hasInstance -> ordinary behavior
                // If left-hand side is an object, per tests we must attempt to fetch
                // `ctor.prototype` (Get semantics) *before* throwing for non-callable C
                if let Value::Object(obj) = l_val {
                    // Attempt to read prototype (this may throw and should propagate)
                    let prototype_val = crate::core::get_property_with_accessors(mc, env, &ctor, "prototype")?;

                    // Now ensure constructor is callable
                    let is_callable_ctor = ctor.borrow().get_closure().is_some()
                        || ctor.borrow().class_def.is_some()
                        || crate::core::object_get_key_value(&ctor, "__is_constructor").is_some();
                    if !is_callable_ctor {
                        return Err(raise_type_error!("Only Function objects implement [[HasInstance]] and consequently can be proper ShiftExpression for The instanceof operator").into());
                    }

                    // Prototype must be an object
                    if let Value::Object(constructor_proto_obj) = prototype_val {
                        // Walk the internal prototype chain
                        let mut current_proto_opt: Option<crate::core::JSObjectDataPtr> = obj.borrow().prototype;
                        while let Some(proto_obj) = current_proto_opt {
                            if Gc::ptr_eq(proto_obj, constructor_proto_obj) {
                                return Ok(Value::Boolean(true));
                            }
                            current_proto_opt = proto_obj.borrow().prototype;
                        }
                        Ok(Value::Boolean(false))
                    } else {
                        Err(raise_type_error!("Right-hand side of 'instanceof' is not an object").into())
                    }
                } else {
                    // If LHS is not object we still must check whether constructor is callable
                    let is_callable_ctor = ctor.borrow().get_closure().is_some()
                        || ctor.borrow().class_def.is_some()
                        || crate::core::object_get_key_value(&ctor, "__is_constructor").is_some();
                    if !is_callable_ctor {
                        return Err(raise_type_error!("Only Function objects implement [[HasInstance]] and consequently can be proper ShiftExpression for The instanceof operator").into());
                    }
                    Ok(Value::Boolean(false))
                }
            }
            _ => Err(raise_type_error!("Right-hand side of 'instanceof' is not an object").into()),
        },
    }
}

enum LogicalAssignOp {
    And,
    Or,
    Nullish,
}

fn evaluate_expr_logical_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
    op: LogicalAssignOp,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            // Support NamedEvaluation for logical assignment similar to simple assignment
            let mut maybe_name_to_set: Option<String> = None;
            match value_expr {
                crate::core::Expr::Function(name_opt, ..) if name_opt.is_none() => {
                    maybe_name_to_set = Some(candidate_name_from_target(env, target));
                }
                crate::core::Expr::ArrowFunction(..) | crate::core::Expr::AsyncArrowFunction(..) => {
                    maybe_name_to_set = Some(candidate_name_from_target(env, target));
                }
                crate::core::Expr::GeneratorFunction(name_opt, ..) if name_opt.is_none() => {
                    maybe_name_to_set = Some(candidate_name_from_target(env, target));
                }
                crate::core::Expr::AsyncFunction(name_opt, ..) if name_opt.is_none() => {
                    maybe_name_to_set = Some(candidate_name_from_target(env, target));
                }
                crate::core::Expr::Class(class_def) if class_def.name.is_empty() => {
                    maybe_name_to_set = Some(candidate_name_from_target(env, target));
                }
                _ => {}
            }

            let current = evaluate_var(mc, env, name)?;
            let should_assign = match op {
                LogicalAssignOp::And => current.to_truthy(),
                LogicalAssignOp::Or => !current.to_truthy(),
                LogicalAssignOp::Nullish => matches!(current, Value::Null | Value::Undefined),
            };

            if should_assign {
                // Handle NamedEvaluation special-case for class expressions
                let val = if let Expr::Class(class_def) = value_expr
                    && class_def.name.is_empty()
                    && maybe_name_to_set.is_some()
                {
                    let inferred_name = maybe_name_to_set.clone().unwrap();
                    create_class_object(mc, &inferred_name, &class_def.extends, &class_def.members, env, false)?
                } else {
                    evaluate_expr(mc, env, value_expr)?
                };

                // NamedEvaluation: after RHS evaluated, set function 'name' if appropriate
                if let Some(nm) = maybe_name_to_set.clone()
                    && let Value::Object(obj) = &val
                {
                    let mut should_set = false;
                    let force_set_for_arrow = matches!(
                        value_expr,
                        crate::core::Expr::ArrowFunction(..) | crate::core::Expr::AsyncArrowFunction(..)
                    ) || format!("{:?}", value_expr).contains("ArrowFunction");
                    if force_set_for_arrow {
                        should_set = true;
                    } else if let Some(name_rc) = object_get_key_value(obj, "name") {
                        let existing_val = match &*name_rc.borrow() {
                            Value::Property { value: Some(v), .. } => v.borrow().clone(),
                            other => other.clone(),
                        };
                        let name_str = crate::core::value_to_string(&existing_val);
                        if name_str.is_empty() {
                            should_set = true;
                        }
                    } else {
                        should_set = true;
                    }

                    if should_set {
                        let desc = create_descriptor_object(mc, &Value::String(crate::unicode::utf8_to_utf16(&nm)), false, false, true)?;
                        crate::js_object::define_property_internal(mc, obj, "name", &desc)?;
                    }
                }

                env_set_recursive(mc, env, name, &val)?;
                Ok(val)
            } else {
                Ok(current)
            }
        }
        Expr::Property(obj_expr, key_str) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            // Property access on primitives is allowed but assignment strictness might vary.
            // But usually assignment to primitive fails or ignored.
            // evaluate_expr_assign handles primitives by Error "Cannot assign to property of non-object".
            // We'll mimic that.
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, key_str)?;

                let should_assign = match op {
                    LogicalAssignOp::And => current.to_truthy(),
                    LogicalAssignOp::Or => !current.to_truthy(),
                    LogicalAssignOp::Nullish => matches!(current, Value::Null | Value::Undefined),
                };

                if should_assign {
                    let val = evaluate_expr(mc, env, value_expr)?;
                    set_property_with_accessors(mc, env, &obj, key_str, &val)?;
                    Ok(val)
                } else {
                    Ok(current)
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val_res = evaluate_expr(mc, env, key_expr)?;

            let key = match key_val_res {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                _ => PropertyKey::from(value_to_string(&key_val_res)),
            };

            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;

                let should_assign = match op {
                    LogicalAssignOp::And => current.to_truthy(),
                    LogicalAssignOp::Or => !current.to_truthy(),
                    LogicalAssignOp::Nullish => matches!(current, Value::Null | Value::Undefined),
                };

                if should_assign {
                    let val = evaluate_expr(mc, env, value_expr)?;
                    set_property_with_accessors(mc, env, &obj, &key, &val)?;
                    Ok(val)
                } else {
                    Ok(current)
                }
            } else {
                Err(raise_type_error!("Cannot assign to property of non-object").into())
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            // Evaluate base and resolve private key; this will throw on invalid base or missing private name
            let (obj, prop_key) = eval_private_member_ref(mc, env, obj_expr, name)?;

            let current = get_property_with_accessors(mc, env, &obj, prop_key.clone())?;

            let should_assign = match op {
                LogicalAssignOp::And => current.to_truthy(),
                LogicalAssignOp::Or => !current.to_truthy(),
                LogicalAssignOp::Nullish => matches!(current, Value::Null | Value::Undefined),
            };

            if should_assign {
                let val = evaluate_expr(mc, env, value_expr)?;
                set_property_with_accessors(mc, env, &obj, prop_key, &val)?;
                Ok(val)
            } else {
                Ok(current)
            }
        }
        _ => Err(raise_eval_error!("Invalid assignment target").into()),
    }
}

pub fn evaluate_expr<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, expr: &Expr) -> Result<Value<'gc>, EvalError<'gc>> {
    match expr {
        Expr::Number(n) => {
            log::trace!("DEBUG: evaluate_expr Number -> {n}");
            Ok(Value::Number(*n))
        }
        Expr::BigInt(chars) => {
            let s = utf16_to_utf8(chars);
            let bi = crate::js_bigint::parse_bigint_string(&s)?;
            Ok(Value::BigInt(Box::new(bi)))
        }
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::Boolean(b) => Ok(Value::Boolean(*b)),
        Expr::Null => Ok(Value::Null),
        Expr::Undefined => Ok(Value::Undefined),
        Expr::PrivateName(name) => {
            // PrivateName is stored without '#'; resolve using the '#name' binding.
            let lookup = format!("#{name}");
            Ok(evaluate_var(mc, env, &lookup)?)
        }
        Expr::Var(name, _, _) => Ok(evaluate_var(mc, env, name)?),
        Expr::Comma(left, right) => {
            evaluate_expr(mc, env, left)?;
            evaluate_expr(mc, env, right)
        }
        Expr::Assign(target, value_expr) => {
            // Diagnostic: log the assignment target variant to help identify unsupported targets
            let target_variant = match &**target {
                Expr::Var(_, _, _) => "Var",
                Expr::Property(_, _) => "Property",
                Expr::Index(_, _) => "Index",
                Expr::Spread(inner) => {
                    // log inner variant for better diagnostics
                    let inner_variant = match &**inner {
                        Expr::Array(_) => "Array",
                        Expr::Index(_, _) => "Index",
                        Expr::Property(_, _) => "Property",
                        Expr::Var(_, _, _) => "Var",
                        Expr::Call(_, _) => "Call",
                        _ => "Other",
                    };
                    log::debug!("evaluate_expr: Assign target is Spread with inner variant = {}", inner_variant);
                    "Spread"
                }
                Expr::Array(_) => "Array",
                Expr::Object(_) => "Object",
                _ => "Other",
            };
            log::debug!("evaluate_expr: Assign target variant = {}", target_variant);
            evaluate_expr_assign(mc, env, target, value_expr)
        }
        Expr::AddAssign(target, value_expr) => evaluate_expr_add_assign(mc, env, target, value_expr),
        Expr::SubAssign(target, value_expr) => evaluate_expr_sub_assign(mc, env, target, value_expr),
        Expr::MulAssign(target, value_expr) => evaluate_expr_mul_assign(mc, env, target, value_expr),
        Expr::DivAssign(target, value_expr) => evaluate_expr_div_assign(mc, env, target, value_expr),
        Expr::ModAssign(target, value_expr) => evaluate_expr_mod_assign(mc, env, target, value_expr),
        Expr::PowAssign(target, value_expr) => evaluate_expr_pow_assign(mc, env, target, value_expr),
        Expr::BitAndAssign(target, value_expr) => evaluate_expr_bitand_assign(mc, env, target, value_expr),
        Expr::BitOrAssign(target, value_expr) => evaluate_expr_bitor_assign(mc, env, target, value_expr),
        Expr::BitXorAssign(target, value_expr) => evaluate_expr_bitxor_assign(mc, env, target, value_expr),
        Expr::LeftShiftAssign(target, value_expr) => evaluate_expr_leftshift_assign(mc, env, target, value_expr),
        Expr::RightShiftAssign(target, value_expr) => evaluate_expr_rightshift_assign(mc, env, target, value_expr),
        Expr::UnsignedRightShiftAssign(target, value_expr) => evaluate_expr_urightshift_assign(mc, env, target, value_expr),
        Expr::LogicalAndAssign(target, value_expr) => evaluate_expr_logical_assign(mc, env, target, value_expr, LogicalAssignOp::And),
        Expr::LogicalOrAssign(target, value_expr) => evaluate_expr_logical_assign(mc, env, target, value_expr, LogicalAssignOp::Or),
        Expr::NullishAssign(target, value_expr) => evaluate_expr_logical_assign(mc, env, target, value_expr, LogicalAssignOp::Nullish),
        Expr::Binary(left, op, right) => evaluate_expr_binary(mc, env, left, op, right),
        Expr::LogicalNot(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            Ok(Value::Boolean(!val.to_truthy()))
        }
        Expr::Conditional(cond, then_expr, else_expr) => {
            let val = evaluate_expr(mc, env, cond)?;
            if val.to_truthy() {
                let r = evaluate_expr(mc, env, then_expr)?;
                Ok(r)
            } else {
                let r = evaluate_expr(mc, env, else_expr)?;
                Ok(r)
            }
        }
        Expr::Object(properties) => evaluate_expr_object(mc, env, properties),
        Expr::Regex(pattern, flags) => {
            // Instantiate a RegExp object for a regex literal
            let pattern_utf16 = crate::unicode::utf8_to_utf16(pattern);
            let flags_u16 = crate::unicode::utf8_to_utf16(flags);
            let arg1 = Value::String(pattern_utf16);
            let arg2 = Value::String(flags_u16);
            Ok(crate::js_regexp::handle_regexp_constructor(mc, &[arg1, arg2])?)
        }

        Expr::Array(elements) => evaluate_expr_array(mc, env, elements),

        Expr::Function(name, params, body) => {
            let mut body_clone = body.clone();
            Ok(evaluate_function_expression(mc, env, name.clone(), params, &mut body_clone)?)
        }
        Expr::GeneratorFunction(name, params, body) => {
            // Similar to Function but produces a GeneratorFunction value
            let func_obj = crate::core::new_js_object_data(mc);
            // Set internal [[Prototype]] to `GeneratorFunction.prototype` when available
            // so generator functions inherit from a distinct function prototype.
            if let Some(gen_func_ctor_val) = env_get(env, "GeneratorFunction")
                && let Value::Object(gen_func_ctor) = &*gen_func_ctor_val.borrow()
                && let Some(gen_func_proto_val) = object_get_key_value(gen_func_ctor, "prototype")
            {
                let proto_opt = match &*gen_func_proto_val.borrow() {
                    Value::Object(o) => Some(*o),
                    Value::Property { value: Some(v), .. } => {
                        let inner = v.borrow().clone();
                        if let Value::Object(o2) = inner { Some(o2) } else { None }
                    }
                    _ => None,
                };
                if let Some(gen_func_proto) = proto_opt {
                    func_obj.borrow_mut(mc).prototype = Some(gen_func_proto);
                    // DEBUG: report that the new generator function object's [[Prototype]] was set
                    log::trace!(
                        "eval: created generator function func_obj.ptr={:p} [[Prototype]] -> {:p}",
                        Gc::as_ptr(func_obj),
                        Gc::as_ptr(gen_func_proto)
                    );
                }
            } else if let Some(func_ctor_val) = env_get(env, "Function")
                && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                && let Value::Object(proto) = &*proto_val.borrow()
            {
                // Fallback: use Function.prototype
                func_obj.borrow_mut(mc).prototype = Some(*proto);
            }

            let is_strict = body.first()
                .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
                .unwrap_or(false);
            let closure_data = ClosureData {
                params: params.to_vec(),
                body: body.clone(),
                env: Some(*env),
                is_strict,
                enforce_strictness_inheritance: true,
                ..ClosureData::default()
            };
            let closure_val = Value::GeneratorFunction(name.clone(), Gc::new(mc, closure_data));
            func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
            match name {
                Some(n) if !n.is_empty() => {
                    let desc = create_descriptor_object(mc, &Value::String(utf8_to_utf16(n)), false, false, true)?;
                    crate::js_object::define_property_internal(mc, &func_obj, "name", &desc)?;
                }
                _ => {
                    // Anonymous generator functions should expose an own 'name' property with the empty string
                    // and the standard attributes: writable:false, enumerable:false, configurable:true
                    let desc = crate::core::create_descriptor_object(mc, &Value::String(vec![]), false, false, true)?;
                    crate::js_object::define_property_internal(mc, &func_obj, "name", &desc)?;
                }
            }

            // Set 'length' property for generator function
            let mut fn_length = 0_usize;
            for p in params.iter() {
                match p {
                    crate::core::DestructuringElement::Variable(_, default_opt) => {
                        if default_opt.is_some() {
                            break;
                        }
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Rest(_) => break,
                    crate::core::DestructuringElement::NestedArray(..) | crate::core::DestructuringElement::NestedObject(..) => {
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Empty => {}
                    _ => {}
                }
            }
            let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(fn_length as f64), false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "length", &desc_len)?;

            // Create prototype object
            let proto_obj = crate::core::new_js_object_data(mc);
            // Set prototype of prototype object to Generator.prototype if available
            // (fallback to Object.prototype otherwise).
            if let Some(gen_val) = env_get(env, "Generator")
                && let Value::Object(gen_ctor) = &*gen_val.borrow()
                && let Some(gen_proto_val) = object_get_key_value(gen_ctor, "prototype")
            {
                log::debug!("GeneratorFunction: found Generator constructor ptr = {:p}", Gc::as_ptr(*gen_ctor));
                log::debug!("GeneratorFunction: raw prototype value ptr = {:p}", Gc::as_ptr(gen_proto_val));
                let proto_value = match &*gen_proto_val.borrow() {
                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                    other => other.clone(),
                };
                if let Value::Object(gen_proto) = proto_value {
                    log::debug!(
                        "GeneratorFunction: setting proto_obj.prototype to Generator.prototype {:p}",
                        Gc::as_ptr(gen_proto)
                    );
                    proto_obj.borrow_mut(mc).prototype = Some(gen_proto);
                } else {
                    log::debug!("GeneratorFunction: Generator.prototype is not an object");
                }
            } else {
                log::debug!("GeneratorFunction: could not find Generator.prototype");
                if let Some(obj_val) = env_get(env, "Object")
                    && let Value::Object(obj_ctor) = &*obj_val.borrow()
                    && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
                {
                    let proto_value = match &*obj_proto_val.borrow() {
                        Value::Property { value: Some(v), .. } => v.borrow().clone(),
                        other => other.clone(),
                    };
                    if let Value::Object(obj_proto) = proto_value {
                        log::debug!("GeneratorFunction: falling back to Object.prototype {:p}", Gc::as_ptr(obj_proto));
                        proto_obj.borrow_mut(mc).prototype = Some(obj_proto);
                    }
                }
            }

            // For generator functions, do NOT create an own 'constructor'
            // property on the prototype object (per test262 expectations).
            // Only define the `prototype` property on the function itself
            // with writable:true, enumerable:false, configurable:false.
            let desc_proto = crate::core::create_descriptor_object(mc, &Value::Object(proto_obj), true, false, false)?;
            crate::js_object::define_property_internal(mc, &func_obj, "prototype", &desc_proto)?;
            // DEBUG: report the enumerable flag for the newly-defined `prototype` property
            log::trace!(
                "eval: func_obj.ptr={:p} prototype enumerable={} non_enumerable={}",
                Gc::as_ptr(func_obj),
                func_obj.borrow().is_enumerable("prototype"),
                func_obj
                    .borrow()
                    .non_enumerable
                    .contains(&crate::core::PropertyKey::String("prototype".to_string()))
            );
            // Ensure non-enumerable as spec requires for function `prototype` property
            func_obj.borrow_mut(mc).set_non_enumerable("prototype");

            Ok(Value::Object(func_obj))
        }
        Expr::AsyncGeneratorFunction(name, params, body) => {
            // Async generator functions are represented as objects with an AsyncGeneratorFunction stored
            // in an internal closure slot. They inherit from Function.prototype.
            let func_obj = crate::core::new_js_object_data(mc);

            // Set __proto__ to Function.prototype
            if let Some(func_ctor_val) = env_get(env, "Function")
                && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                && let Value::Object(proto) = &*proto_val.borrow()
            {
                func_obj.borrow_mut(mc).prototype = Some(*proto);
            }

            let is_strict = body.first()
                .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
                .unwrap_or(false);
            let closure_data = ClosureData {
                params: params.to_vec(),
                body: body.clone(),
                env: Some(*env),
                is_strict,
                enforce_strictness_inheritance: true,
                ..ClosureData::default()
            };
            let closure_val = Value::AsyncGeneratorFunction(name.clone(), Gc::new(mc, closure_data));
            func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
            let name_val = match name {
                Some(n) if !n.is_empty() => Value::String(utf8_to_utf16(n)),
                _ => Value::String(vec![]),
            };
            let desc = create_descriptor_object(mc, &name_val, false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "name", &desc)?;

            // Set 'length' property for async generator function
            let mut fn_length = 0_usize;
            for p in params.iter() {
                match p {
                    crate::core::DestructuringElement::Variable(_, default_opt) => {
                        if default_opt.is_some() {
                            break;
                        }
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Rest(_) => break,
                    crate::core::DestructuringElement::NestedArray(..) | crate::core::DestructuringElement::NestedObject(..) => {
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Empty => {}
                    _ => {}
                }
            }
            let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(fn_length as f64), false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "length", &desc_len)?;

            // Create prototype object
            let proto_obj = crate::core::new_js_object_data(mc);
            // Set prototype of prototype object to AsyncGenerator.prototype if available
            // (fallback to Object.prototype otherwise).
            if let Some(async_gen_val) = env_get(env, "AsyncGenerator")
                && let Value::Object(async_gen_ctor) = &*async_gen_val.borrow()
                && let Some(async_proto_val) = object_get_key_value(async_gen_ctor, "prototype")
            {
                let proto_value = match &*async_proto_val.borrow() {
                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                    other => other.clone(),
                };
                if let Value::Object(async_proto) = proto_value {
                    proto_obj.borrow_mut(mc).prototype = Some(async_proto);
                }
            } else if let Some(obj_val) = env_get(env, "Object")
                && let Value::Object(obj_ctor) = &*obj_val.borrow()
                && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
            {
                let proto_value = match &*obj_proto_val.borrow() {
                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                    other => other.clone(),
                };
                if let Value::Object(obj_proto) = proto_value {
                    proto_obj.borrow_mut(mc).prototype = Some(obj_proto);
                }
            }
            // Record the intrinsic async generator prototype (if any) for fallback
            if let Some(proto) = proto_obj.borrow().prototype {
                object_set_key_value(mc, &func_obj, "__async_generator_proto", &Value::Object(proto))?;
            }

            // For async generator functions, do NOT create an own 'constructor'
            // property on the prototype object (per Test262 expectations).
            // Only define the `prototype` property on the function itself
            // with writable:true, enumerable:false, configurable:false.
            let desc_proto = crate::core::create_descriptor_object(mc, &Value::Object(proto_obj), true, false, false)?;
            crate::js_object::define_property_internal(mc, &func_obj, "prototype", &desc_proto)?;

            Ok(Value::Object(func_obj))
        }
        Expr::AsyncFunction(name, params, body) => {
            // Async functions are represented as objects with an AsyncClosure stored
            // in an internal closure slot. They inherit from Function.prototype.
            let func_obj = new_js_object_data(mc);

            // Set __proto__ to Function.prototype
            if let Some(func_ctor_val) = env_get(env, "Function")
                && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                && let Value::Object(proto) = &*proto_val.borrow()
            {
                func_obj.borrow_mut(mc).prototype = Some(*proto);
            }

            let is_strict = body.first()
                .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
                .unwrap_or(false);
            let closure_data = ClosureData {
                params: params.to_vec(),
                body: body.clone(),
                env: Some(*env),
                is_strict,
                enforce_strictness_inheritance: true,
                ..ClosureData::default()
            };
            let closure_val = Value::AsyncClosure(Gc::new(mc, closure_data));
            func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
            let val = match name {
                Some(n) if !n.is_empty() => Value::String(utf8_to_utf16(n)),
                _ => Value::String(vec![]),
            };
            let desc = create_descriptor_object(mc, &val, false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "name", &desc)?;

            // Set 'length' property for async functions
            let mut fn_length = 0_usize;
            for p in params.iter() {
                match p {
                    crate::core::DestructuringElement::Variable(_, default_opt) => {
                        if default_opt.is_some() {
                            break;
                        }
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Rest(_) => break,
                    crate::core::DestructuringElement::NestedArray(..) | crate::core::DestructuringElement::NestedObject(..) => {
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Empty => {}
                    _ => {}
                }
            }
            let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(fn_length as f64), false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "length", &desc_len)?;

            Ok(Value::Object(func_obj))
        }
        Expr::ArrowFunction(params, body) => {
            // Create an arrow function object which captures the current `this` lexically
            let func_obj = crate::core::new_js_object_data(mc);
            // Set __proto__ to Function.prototype
            if let Some(func_ctor_val) = env_get(env, "Function")
                && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                && let Value::Object(proto) = &*proto_val.borrow()
            {
                func_obj.borrow_mut(mc).prototype = Some(*proto);
            }

            let is_strict = body.first()
                .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
                .unwrap_or(false);

            let closure_data = ClosureData {
                params: params.to_vec(),
                body: body.clone(),
                env: Some(*env),
                bound_this: None,
                is_arrow: true,
                is_strict,
                enforce_strictness_inheritance: true,
                home_object: env.borrow().get_home_object(),
                ..ClosureData::default()
            };
            let closure_val = Value::Closure(Gc::new(mc, closure_data));
            func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));

            // Set 'length' for arrow functions
            let mut fn_length = 0_usize;
            for p in params.iter() {
                match p {
                    crate::core::DestructuringElement::Variable(_, default_opt) => {
                        if default_opt.is_some() {
                            break;
                        }
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Rest(_) => break,
                    crate::core::DestructuringElement::NestedArray(..) | crate::core::DestructuringElement::NestedObject(..) => {
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Empty => {}
                    _ => {}
                }
            }
            let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(fn_length as f64), false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "length", &desc_len)?;

            // Anonymous arrow functions should expose an own 'name' property with the
            // empty string and standard attributes: writable:false, enumerable:false, configurable:true
            let desc = create_descriptor_object(mc, &Value::String(vec![]), false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "name", &desc)?;

            // Arrow functions do not have a 'prototype' property (not constructible)

            Ok(Value::Object(func_obj))
        }
        Expr::Call(func_expr, args) => evaluate_expr_call(mc, env, func_expr, args),
        Expr::New(ctor, args) => evaluate_expr_new(mc, env, ctor, args),
        Expr::DynamicImport(specifier) => {
            // Evaluate the specifier before creating the promise capability so
            // abrupt completion at this stage throws synchronously.
            let spec_val = evaluate_expr(mc, env, specifier)?;

            let promise = crate::core::new_gc_cell_ptr(mc, crate::core::JSPromise::new());
            let promise_obj = crate::js_promise::make_promise_js_object(mc, promise, Some(*env))?;

            let module_result: Result<Value<'gc>, EvalError<'gc>> = (|| {
                let prim = crate::core::to_primitive(mc, &spec_val, "string", env)?;
                let module_name = match prim {
                    Value::Symbol(_) => {
                        return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                    }
                    _ => crate::core::value_to_string(&prim),
                };

                let base_path = if let Some(cell) = env_get(env, "__filepath")
                    && let Value::String(s) = cell.borrow().clone()
                {
                    Some(utf16_to_utf8(&s))
                } else {
                    None
                };

                crate::js_module::load_module(mc, &module_name, base_path.as_deref(), Some(*env))
            })();

            match module_result {
                Ok(module_value) => {
                    crate::js_promise::resolve_promise(mc, &promise, module_value, env);
                }
                Err(err) => {
                    let reason = match err {
                        EvalError::Throw(val, _line, _column) => val,
                        EvalError::Js(js_err) => js_error_to_value(mc, env, &js_err),
                    };
                    crate::js_promise::reject_promise(mc, &promise, reason, env);
                }
            }
            Ok(Value::Object(promise_obj))
        }

        Expr::Property(obj_expr, key) => {
            // Special-case `import.meta` when executing in a module environment. We treat
            // `import.meta` as a per-module ordinary object stored under env.__import_meta.
            if let crate::core::Expr::Var(name, _, _) = &**obj_expr
                && name == "import"
                && key == "meta"
            {
                // Prefer the immediate environment but fall back to the root global environment
                let meta_rc = crate::core::object_get_key_value(env, "__import_meta").or_else(|| {
                    let mut root = *env;
                    while let Some(proto) = root.borrow().prototype {
                        root = proto;
                    }
                    crate::core::object_get_key_value(&root, "__import_meta")
                });
                if let Some(meta_rc) = meta_rc
                    && let Value::Object(meta_obj) = &*meta_rc.borrow()
                {
                    log::trace!("eval Expr::Property: special-case import.meta matched, returning module import.meta object");
                    return Ok(Value::Object(*meta_obj));
                } else {
                    log::trace!(
                        "eval Expr::Property: special-case import.meta not matched in current env chain, creating fallback import.meta on root env"
                    );
                    // Fallback: create an import.meta object on the root environment so
                    // `import.meta` accessors always produce an ordinary object in
                    // module contexts. Use __filepath on the root env to populate the
                    // 'url' property when available.
                    let mut root = *env;
                    while let Some(proto) = root.borrow().prototype {
                        root = proto;
                    }
                    // Create a new ordinary object for import.meta
                    let import_meta = new_js_object_data(mc);
                    if let Some(cell) = env_get(&root, "__filepath")
                        && let Value::String(s) = cell.borrow().clone()
                    {
                        object_set_key_value(mc, &import_meta, "url", &Value::String(s))?;
                    }
                    object_set_key_value(mc, &root, "__import_meta", &Value::Object(import_meta))?;
                    return Ok(Value::Object(import_meta));
                }
            }

            let obj_val = if expr_contains_optional_chain(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };

            if let Value::Object(obj) = &obj_val {
                let val = get_property_with_accessors(mc, env, obj, key)?;
                // Special-case `__proto__` getter: if not present as an own property, return
                // the internal prototype pointer (or null).
                if key == "__proto__" {
                    let proto_key = PropertyKey::String("__proto__".to_string());
                    if get_own_property(obj, &proto_key).is_none() {
                        if let Some(proto_ptr) = obj.borrow().prototype {
                            return Ok(Value::Object(proto_ptr));
                        } else {
                            return Ok(Value::Null);
                        }
                    }
                }
                Ok(val)
            } else if let Value::String(s) = &obj_val
                && key == "length"
            {
                Ok(Value::Number(s.len() as f64))
            } else if key == "length"
                && matches!(
                    obj_val,
                    Value::Closure(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..)
                )
            {
                let params = match obj_val {
                    Value::Closure(ref c) => &c.params,
                    Value::AsyncClosure(ref c) => &c.params,
                    Value::GeneratorFunction(_, ref c) => &c.params,
                    Value::AsyncGeneratorFunction(_, ref c) => &c.params,
                    _ => unreachable!(),
                };
                let mut len = 0.0;
                for p in params {
                    match p {
                        DestructuringElement::Rest(_) | DestructuringElement::RestPattern(_) => break,
                        DestructuringElement::Variable(_, Some(_)) => break,
                        DestructuringElement::NestedArray(_, Some(_)) => break,
                        DestructuringElement::NestedObject(_, Some(_)) => break,
                        _ => len += 1.0,
                    }
                }
                Ok(Value::Number(len))
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                log::debug!("Expr::Property: attempting property access on nullish base: obj_expr={obj_expr:?}");
                Err(raise_type_error!("Cannot read properties of null or undefined").into())
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, key)
            }
        }
        Expr::PrivateMember(obj_expr, key) => {
            let obj_val = if is_optional_chain_expr(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };
            let pv = evaluate_var(mc, env, key)?;
            if let Value::Object(obj) = &obj_val {
                if let Value::PrivateName(n, id) = pv {
                    get_property_with_accessors(mc, env, obj, PropertyKey::Private(n, id))
                } else {
                    Err(raise_syntax_error!(format!("Private field '{}' must be declared in an enclosing class", key)).into())
                }
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                Err(raise_type_error!("Cannot read properties of null or undefined").into())
            } else {
                Err(raise_type_error!("Cannot access private field on non-object").into())
            }
        }
        Expr::OptionalPrivateMember(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if obj_val.is_null_or_undefined() {
                Ok(Value::Undefined)
            } else if let Value::Object(obj) = &obj_val {
                let pv = evaluate_var(mc, env, key)?;
                if let Value::PrivateName(n, id) = pv {
                    get_property_with_accessors(mc, env, obj, PropertyKey::Private(n, id))
                } else {
                    Err(raise_syntax_error!(format!("Private field '{}' must be declared in an enclosing class", key)).into())
                }
            } else {
                Err(raise_type_error!("Cannot access private field on non-object").into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            if let Expr::Super = &**obj_expr {
                let key_val = evaluate_expr(mc, env, key_expr)?;
                let key = match key_val {
                    Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                    Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                    Value::Symbol(s) => PropertyKey::Symbol(s),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &key_val, "string", env)?;
                        match prim {
                            Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                            Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                            Value::Symbol(s) => PropertyKey::Symbol(s),
                            other => PropertyKey::String(value_to_string(&other)),
                        }
                    }
                    _ => PropertyKey::String(value_to_string(&key_val)),
                };
                // Capture backtrace at the evaluator call-site delegating to
                // `evaluate_super_computed_property` so we can correlate the
                // lookup-time environment with where the evaluator invoked it.
                let bt = std::backtrace::Backtrace::capture();
                log::trace!("eval::Index(super): backtrace before evaluate_super_computed_property: {:?}", bt);
                return Ok(crate::js_class::evaluate_super_computed_property(mc, env, key)?);
            }

            let obj_val = if expr_contains_optional_chain(obj_expr) {
                match evaluate_optional_chain_base(mc, env, obj_expr)? {
                    Some(val) => val,
                    None => return Ok(Value::Undefined),
                }
            } else {
                evaluate_expr(mc, env, obj_expr)?
            };

            // Per spec, ToObject(base) (i.e. checking for null/undefined base) must occur
            // before ToPropertyKey(key) and any evaluation of the key's coercion, so
            // if base is nullish we should throw a TypeError immediately and not
            // evaluate `key_expr` which could have side effects.
            if matches!(obj_val, Value::Undefined | Value::Null) {
                log::debug!("Expr::Index: attempting property access on nullish base: obj_expr={obj_expr:?}");
                return Err(raise_type_error!("Cannot read properties of null or undefined").into());
            }

            let key_val = evaluate_expr(mc, env, key_expr)?;

            let key = match key_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                Value::Object(_) => {
                    // ToPropertyKey semantics: ToPrimitive with hint 'string'
                    let prim = crate::core::to_primitive(mc, &key_val, "string", env)?;
                    match prim {
                        Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
                        Value::Symbol(s) => PropertyKey::Symbol(s),
                        other => PropertyKey::String(value_to_string(&other)),
                    }
                }
                _ => PropertyKey::String(value_to_string(&key_val)),
            };

            if let Value::Object(obj) = &obj_val {
                get_property_with_accessors(mc, env, obj, &key)
            } else if let Value::String(s) = &obj_val {
                if let PropertyKey::String(k_str) = &key {
                    if k_str == "length" {
                        Ok(Value::Number(s.len() as f64))
                    } else if let Ok(idx) = k_str.parse::<usize>() {
                        if idx < s.len() {
                            Ok(Value::String(vec![s[idx]]))
                        } else {
                            Ok(Value::Undefined)
                        }
                    } else {
                        get_primitive_prototype_property(mc, env, &obj_val, &key)
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &obj_val, &key)
                }
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, &key)
            }
        }
        Expr::PostIncrement(target) => evaluate_update_expression(mc, env, target, 1.0, true),
        Expr::PostDecrement(target) => evaluate_update_expression(mc, env, target, -1.0, true),
        Expr::Increment(target) => evaluate_update_expression(mc, env, target, 1.0, false),
        Expr::Decrement(target) => evaluate_update_expression(mc, env, target, -1.0, false),
        Expr::TemplateString(parts) => {
            let mut result = Vec::new();
            for part in parts {
                match part {
                    crate::core::TemplatePart::String(s) => result.extend(s),
                    crate::core::TemplatePart::Expr(tokens) => {
                        let (expr, _) = crate::core::parse_simple_expression(tokens, 0)?;
                        let val = evaluate_expr(mc, env, &expr)?;
                        // Template strings perform ToString coercion.
                        // Objects are converted via ToPrimitive with hint 'string'.
                        match val {
                            Value::String(s) => result.extend(s),
                            Value::Number(n) => result.extend(crate::unicode::utf8_to_utf16(&value_to_string(&Value::Number(n)))),
                            Value::BigInt(b) => result.extend(crate::unicode::utf8_to_utf16(&b.to_string())),
                            Value::Boolean(b) => result.extend(crate::unicode::utf8_to_utf16(&b.to_string())),
                            Value::Undefined => result.extend(crate::unicode::utf8_to_utf16("undefined")),
                            Value::Null => result.extend(crate::unicode::utf8_to_utf16("null")),
                            Value::Object(_) => {
                                // Template strings perform ToString coercion.
                                // We call toString() explicitly if it exists, otherwise use value_to_string.
                                let mut s = "[object Object]".to_string();
                                if let Value::Object(obj) = &val {
                                    if let Some(method_rc) = object_get_key_value(obj, "toString") {
                                        let method_val = method_rc.borrow().clone();
                                        if matches!(
                                            method_val,
                                            Value::Closure(_) | Value::AsyncClosure(_) | Value::Function(_) | Value::Object(_)
                                        ) {
                                            if let Ok(res) = evaluate_call_dispatch(mc, env, &method_val, Some(&val), &Vec::new()) {
                                                s = crate::core::value_to_string(&res);
                                            }
                                        } else {
                                            // Regular ToPrimitive for other objects?
                                            // The fallback below handles it.
                                            let prim = crate::core::to_primitive(mc, &val, "string", env)?;
                                            s = match &prim {
                                                Value::String(vs) => crate::unicode::utf16_to_utf8(vs),
                                                _ => value_to_string(&prim),
                                            };
                                        }
                                    } else {
                                        let prim = crate::core::to_primitive(mc, &val, "string", env)?;
                                        s = match &prim {
                                            Value::String(vs) => crate::unicode::utf16_to_utf8(vs),
                                            _ => value_to_string(&prim),
                                        };
                                    }
                                }
                                result.extend(crate::unicode::utf8_to_utf16(&s));
                            }
                            _ => {
                                // Fallback to the generic representation
                                let s = value_to_string(&val);
                                result.extend(crate::unicode::utf8_to_utf16(&s));
                            }
                        }
                    }
                }
            }
            Ok(Value::String(result))
        }
        Expr::Class(class_def) => {
            if class_def.name.is_empty() {
                let class_obj = create_class_object(mc, &class_def.name, &class_def.extends, &class_def.members, env, false)?;
                Ok(class_obj)
            } else {
                // Create a class scope for the class name binding (lexical, immutable)
                let class_scope = new_js_object_data(mc);
                class_scope.borrow_mut(mc).prototype = Some(*env);
                env_set(mc, &class_scope, &class_def.name, &Value::Uninitialized)?;

                let class_obj = create_class_object(mc, &class_def.name, &class_def.extends, &class_def.members, &class_scope, false)?;

                // Initialize the class name binding to the class object and mark it immutable
                env_set(mc, &class_scope, &class_def.name, &class_obj)?;
                class_scope.borrow_mut(mc).set_const(class_def.name.clone());

                Ok(class_obj)
            }
        }
        Expr::UnaryNeg(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            match val {
                Value::BigInt(b) => Ok(Value::BigInt(Box::new(-*b))),
                other => Ok(Value::Number(-to_number_with_env(mc, env, &other)?)),
            }
        }
        Expr::UnaryPlus(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            Ok(Value::Number(to_number_with_env(mc, env, &val)?))
        }
        Expr::BitNot(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            let num = to_numeric_with_env(mc, env, &val)?;
            match num {
                Value::BigInt(b) => Ok(Value::BigInt(Box::new(!*b))),
                Value::Number(n) => {
                    let i = to_int32_value_with_env(mc, env, &Value::Number(n))?;
                    Ok(Value::Number((!i) as f64))
                }
                _ => Err(raise_eval_error!("Invalid numeric conversion for bitwise NOT").into()),
            }
        }
        Expr::Void(expr) => {
            // Evaluate for side effects, then discard and return undefined.
            let _ = evaluate_expr(mc, env, expr)?;
            Ok(Value::Undefined)
        }
        Expr::TypeOf(expr) => {
            // typeof has special semantics: if the evaluation of the operand throws a
            // ReferenceError due to an *unresolvable* reference (identifier not found),
            // the result is "undefined" instead of throwing. However, if the ReferenceError
            // is due to accessing an uninitialized lexical binding (TDZ), the ReferenceError
            // must be propagated. Distinguish these cases here.
            let val_result = evaluate_expr(mc, env, expr);
            let val = match val_result {
                Ok(v) => v,
                Err(e) => {
                    match e {
                        EvalError::Js(js_err) => {
                            // If this is a ReferenceError for an unresolvable reference ("is not defined"),
                            // treat it as undefined. Otherwise rethrow the error (propagate it).
                            let msg = js_err.message();
                            if msg.ends_with(" is not defined") {
                                Value::Undefined
                            } else {
                                return Err(EvalError::Js(js_err));
                            }
                        }
                        // For thrown values (EvalError::Throw) or other EvalError variants,
                        // propagate the error so semantics are preserved.
                        other => return Err(other),
                    }
                }
            };

            let type_str = match val {
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Boolean(_) => "boolean",
                Value::Undefined | Value::Uninitialized => "undefined",
                Value::Null => "object",
                Value::Symbol(_) => "symbol",
                Value::BigInt(_) => "bigint",
                Value::Function(_)
                | Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::GeneratorFunction(..)
                | Value::ClassDefinition(_)
                | Value::Getter(..)
                | Value::Setter(..) => "function",
                Value::Object(obj) => {
                    if obj.borrow().get_closure().is_some() {
                        "function"
                    } else if let Some(is_ctor) = object_get_key_value(&obj, "__is_constructor") {
                        if matches!(*is_ctor.borrow(), Value::Boolean(true)) {
                            "function"
                        } else {
                            "object"
                        }
                    } else {
                        "object"
                    }
                }
                _ => "undefined",
            };
            Ok(Value::String(utf8_to_utf16(type_str)))
        }
        Expr::LogicalAnd(left, right) => {
            let lhs = evaluate_expr(mc, env, left)?;
            let is_truthy = lhs.to_truthy();
            if !is_truthy { Ok(lhs) } else { evaluate_expr(mc, env, right) }
        }
        Expr::LogicalOr(left, right) => {
            let lhs = evaluate_expr(mc, env, left)?;
            let is_truthy = lhs.to_truthy();
            if is_truthy { Ok(lhs) } else { evaluate_expr(mc, env, right) }
        }
        Expr::NullishCoalescing(left, right) => {
            let lhs = evaluate_expr(mc, env, left)?;
            if lhs.is_null_or_undefined() {
                evaluate_expr(mc, env, right)
            } else {
                Ok(lhs)
            }
        }
        Expr::This => Ok(crate::js_class::evaluate_this(mc, env)?),
        Expr::NewTarget => {
            // Runtime runtime for `new.target`: walk environment chain to find the nearest
            // function scope. If the function was invoked as a constructor (has `__instance`),
            // return the function object stored in `__function` (if present). Otherwise return `undefined`.
            // Important: Arrow functions do not have their own `new.target` — skip their function
            // scopes and continue walking up the environment chain.
            let mut cur = Some(*env);
            while let Some(e) = cur {
                if e.borrow().is_function_scope {
                    // Arrow functions do not have `new.target` of their own — skip them
                    if let Some(flag_rc) = object_get_key_value(&e, "__is_arrow_function")
                        && matches!(*flag_rc.borrow(), Value::Boolean(true))
                    {
                        cur = e.borrow().prototype;
                        continue;
                    }
                    if let Some(inst_rc) = object_get_key_value(&e, "__instance")
                        && !matches!(*inst_rc.borrow(), Value::Undefined)
                    {
                        if let Some(nt_rc) = object_get_key_value(&e, "__new_target") {
                            return Ok(nt_rc.borrow().clone());
                        }
                        if let Some(func_rc) = object_get_key_value(&e, "__function") {
                            return Ok(func_rc.borrow().clone());
                        }
                    }
                    return Ok(Value::Undefined);
                }
                cur = e.borrow().prototype;
            }
            Ok(Value::Undefined)
        }

        Expr::SuperCall(args) => {
            let eval_args = collect_call_args(mc, env, args)?;
            Ok(crate::js_class::evaluate_super_call(mc, env, &eval_args)?)
        }
        Expr::SuperProperty(prop) => {
            // Delegate to `evaluate_super_computed_property` which implements the full semantics
            // (handles accessors/getters and the legacy fallback). Convert any JSError into EvalError::Js.
            Ok(crate::js_class::evaluate_super_computed_property(mc, env, prop)?)
        }
        Expr::SuperMethod(prop, args) => {
            let eval_args = collect_call_args(mc, env, args)?;
            Ok(crate::js_class::evaluate_super_method(mc, env, prop, &eval_args)?)
        }
        Expr::Super => Ok(crate::js_class::evaluate_super(mc, env)?),
        Expr::OptionalProperty(lhs, prop) => {
            let left_val = evaluate_expr(mc, env, lhs)?;
            if left_val.is_null_or_undefined() {
                Ok(Value::Undefined)
            } else if let Value::Object(obj) = &left_val {
                // Use accessor-aware property read so getters are executed.
                let prop_val = get_property_with_accessors(mc, env, obj, prop)?;
                if !matches!(prop_val, Value::Undefined) {
                    Ok(prop_val)
                } else {
                    Ok(Value::Undefined)
                }
            } else if let Value::String(s) = &left_val
                && prop == "length"
            {
                Ok(Value::Number(s.len() as f64))
            } else {
                get_primitive_prototype_property(mc, env, &left_val, prop)
            }
        }
        Expr::OptionalIndex(lhs, index_expr) => {
            let left_val = evaluate_expr(mc, env, lhs)?;
            if left_val.is_null_or_undefined() {
                Ok(Value::Undefined)
            } else {
                let index_val = evaluate_expr(mc, env, index_expr)?;
                let prop_key = match index_val {
                    Value::Symbol(s) => crate::core::PropertyKey::Symbol(s),
                    Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                    val => {
                        let s = match val {
                            Value::Number(n) => n.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            Value::Undefined => "undefined".to_string(),
                            Value::Null => "null".to_string(),
                            _ => crate::core::value_to_string(&val),
                        };
                        crate::core::PropertyKey::String(s)
                    }
                };
                if let Value::Object(obj) = &left_val {
                    // Use accessor-aware lookup so getters are invoked
                    let prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                    if !matches!(prop_val, Value::Undefined) {
                        Ok(prop_val)
                    } else {
                        Ok(Value::Undefined)
                    }
                } else {
                    get_primitive_prototype_property(mc, env, &left_val, &prop_key)
                }
            }
        }
        Expr::OptionalCall(lhs, args) => {
            // Handle optional call specially for property/index targets so we can short-circuit
            // when the base is null/undefined without raising a "Cannot read properties of null or undefined" error.
            match &**lhs {
                Expr::Property(obj_expr, key) => {
                    // Special-case `super.method?.()` semantics: evaluate the property on the
                    // parent prototype without evaluating call arguments, short-circuit to
                    // undefined when absent, and ensure the call receives the correct `this`.
                    if matches!(&**obj_expr, Expr::Super) {
                        // Get the super property (handles getters and accessors)
                        let prop_val = crate::js_class::evaluate_super_computed_property(mc, env, key)?;
                        if matches!(prop_val, Value::Undefined) {
                            return Ok(Value::Undefined);
                        }
                        // Only evaluate arguments once we know the method exists
                        let eval_args = collect_call_args(mc, env, args)?;
                        // Resolve the current `this` binding for the call.
                        // Prefer any direct `this` binding on the current env (so we
                        // preserve the exact receiver observed by `evaluate_super_computed_property`),
                        // otherwise fall back to the general `evaluate_this` walker.
                        let this_val = if let Some(tv_rc) = crate::core::object_get_key_value(env, "this") {
                            tv_rc.borrow().clone()
                        } else {
                            crate::js_class::evaluate_this(mc, env)?
                        };
                        log::trace!("DBG OPT_SUPER_CALL: this_val={:?} prop_val={:?}", this_val, prop_val);

                        match prop_val {
                            Value::Function(name) => {
                                let env_for_call = if name == "eval" {
                                    let mut t = *env;
                                    while let Some(proto) = t.borrow().prototype {
                                        t = proto;
                                    }
                                    t
                                } else {
                                    *env
                                };
                                let call_env = prepare_call_env_with_this(
                                    mc,
                                    Some(&env_for_call),
                                    Some(&this_val),
                                    None,
                                    &[],
                                    None,
                                    Some(&env_for_call),
                                    None,
                                )?;
                                // Dispatch, handling indirect global `eval` marking when necessary.
                                call_named_eval_or_dispatch(mc, &env_for_call, &call_env, &name, Some(&this_val), &eval_args)
                            }
                            Value::Closure(c) => call_closure(mc, &c, Some(&this_val), &eval_args, env, None),
                            Value::Object(o) => {
                                let c_e = prepare_call_env_with_this(mc, Some(env), Some(&this_val), None, &[], None, Some(env), Some(o))?;
                                evaluate_call_dispatch(mc, &c_e, &Value::Object(o), Some(&this_val), &eval_args)
                            }
                            _ => Err(raise_type_error!("OptionalCall target is not a function").into()),
                        }
                    } else {
                        let obj_val = evaluate_expr(mc, env, obj_expr)?;
                        if obj_val.is_null_or_undefined() {
                            return Ok(Value::Undefined);
                        }
                        let eval_args = collect_call_args(mc, env, args)?;

                        let f_val = if let Value::Object(obj) = &obj_val {
                            // Use accessor-aware lookup so getters are executed
                            let prop_val = get_property_with_accessors(mc, env, obj, key)?;
                            if !matches!(prop_val, Value::Undefined) {
                                prop_val
                            } else if (key.as_str() == "call" || key.as_str() == "apply") && obj.borrow().get_closure().is_some() {
                                Value::Function(key.to_string())
                            } else {
                                Value::Undefined
                            }
                        } else if let Value::String(s) = &obj_val
                            && key == "length"
                        {
                            Value::Number(s.len() as f64)
                        } else if matches!(
                            obj_val,
                            Value::Closure(_) | Value::Function(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..)
                        ) && key == "call"
                        {
                            Value::Function("call".to_string())
                        } else {
                            get_primitive_prototype_property(mc, env, &obj_val, key)?
                        };

                        match f_val {
                            Value::Function(name) => {
                                // Optional call invoking `eval` is always an indirect eval — use global env.
                                let env_for_call = if name == "eval" {
                                    let mut t = *env;
                                    while let Some(proto) = t.borrow().prototype {
                                        t = proto;
                                    }
                                    t
                                } else {
                                    *env
                                };
                                let call_env = prepare_call_env_with_this(
                                    mc,
                                    Some(&env_for_call),
                                    Some(&obj_val),
                                    None,
                                    &[],
                                    None,
                                    Some(&env_for_call),
                                    None,
                                )?;
                                // Dispatch, handling indirect global `eval` marking when necessary.
                                Ok(call_named_eval_or_dispatch(
                                    mc,
                                    &env_for_call,
                                    &call_env,
                                    &name,
                                    Some(&obj_val),
                                    &eval_args,
                                )?)
                            }
                            Value::Closure(c) => call_closure(mc, &c, Some(&obj_val), &eval_args, env, None),
                            Value::Object(o) => {
                                let call_env = prepare_call_env_with_this(mc, Some(env), Some(&obj_val), None, &[], None, Some(env), None)?;
                                evaluate_call_dispatch(mc, &call_env, &Value::Object(o), Some(&obj_val), &eval_args)
                            }
                            _ => Err(raise_type_error!("OptionalCall target is not a function").into()),
                        }
                    }
                }
                Expr::SuperProperty(prop) => {
                    // Special-case `super.prop?.()` where the parser produces a SuperProperty
                    // node directly (not Property(Super, ...)).
                    let prop_key = (*prop).clone();
                    let prop_val = crate::js_class::evaluate_super_computed_property(mc, env, prop_key)?;
                    if matches!(prop_val, Value::Undefined) {
                        return Ok(Value::Undefined);
                    }
                    // Only evaluate arguments once we know the method exists.
                    let eval_args = collect_call_args(mc, env, args)?;
                    // Resolve the current `this` binding for the call.
                    let this_val = if let Some(tv_rc) = object_get_key_value(env, "this") {
                        tv_rc.borrow().clone()
                    } else {
                        crate::js_class::evaluate_this(mc, env)?
                    };

                    match prop_val {
                        Value::Function(name) => {
                            let env_4_call = if name == "eval" {
                                let mut t = *env;
                                while let Some(proto) = t.borrow().prototype {
                                    t = proto;
                                }
                                t
                            } else {
                                *env
                            };
                            let c_e = prepare_call_env_with_this(
                                mc,
                                Some(&env_4_call),
                                Some(&this_val),
                                None,
                                &[],
                                None,
                                Some(&env_4_call),
                                None,
                            )?;
                            if name == "eval" {
                                let key = PropertyKey::String("__is_indirect_eval".to_string());
                                object_set_key_value(mc, &env_4_call, &key, &Value::Boolean(true))?;
                                let res = evaluate_call_dispatch(mc, &c_e, &Value::Function(name.clone()), Some(&this_val), &eval_args);
                                let _ = env_4_call.borrow_mut(mc).properties.shift_remove(&key);
                                res
                            } else {
                                evaluate_call_dispatch(mc, &c_e, &Value::Function(name.clone()), Some(&this_val), &eval_args)
                            }
                        }
                        Value::Closure(c) => call_closure(mc, &c, Some(&this_val), &eval_args, env, None),
                        Value::Object(o) => {
                            let call_env = prepare_call_env_with_this(mc, Some(env), Some(&this_val), None, &[], None, Some(env), Some(o))?;
                            evaluate_call_dispatch(mc, &call_env, &Value::Object(o), Some(&this_val), &eval_args)
                        }
                        _ => Err(raise_type_error!("OptionalCall target is not a function").into()),
                    }
                }
                Expr::Index(obj_expr, index_expr) => {
                    let obj_val = evaluate_expr(mc, env, obj_expr)?;
                    if obj_val.is_null_or_undefined() {
                        return Ok(Value::Undefined);
                    }

                    let index_val = evaluate_expr(mc, env, index_expr)?;
                    let prop_key = match index_val {
                        Value::Symbol(s) => crate::core::PropertyKey::Symbol(s),
                        Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        val => {
                            let s = match val {
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Undefined => "undefined".to_string(),
                                Value::Null => "null".to_string(),
                                _ => crate::core::value_to_string(&val),
                            };
                            crate::core::PropertyKey::String(s)
                        }
                    };

                    let eval_args = collect_call_args(mc, env, args)?;

                    let f_val = if let Value::Object(obj) = &obj_val {
                        // Use accessor-aware lookup so getters are invoked
                        let prop_val = get_property_with_accessors(mc, env, obj, &prop_key)?;
                        if !matches!(prop_val, Value::Undefined) {
                            prop_val
                        } else {
                            Value::Undefined
                        }
                    } else {
                        get_primitive_prototype_property(mc, env, &obj_val, &prop_key)?
                    };

                    match f_val {
                        Value::Function(name) => {
                            // Optional call invoking `eval` is always an indirect eval — use global env.
                            let env_4_call = if name == "eval" {
                                let mut t = *env;
                                while let Some(proto) = t.borrow().prototype {
                                    t = proto;
                                }
                                t
                            } else {
                                *env
                            };
                            let c_e = prepare_call_env_with_this(
                                mc,
                                Some(&env_4_call),
                                Some(&obj_val),
                                None,
                                &[],
                                None,
                                Some(&env_4_call),
                                None,
                            )?;
                            Ok(call_named_eval_or_dispatch(
                                mc,
                                &env_4_call,
                                &c_e,
                                &name,
                                Some(&obj_val),
                                &eval_args,
                            )?)
                        }
                        Value::Closure(c) => call_closure(mc, &c, Some(&obj_val), &eval_args, env, None),
                        Value::Object(o) => {
                            let call_env = prepare_call_env_with_this(mc, Some(env), Some(&obj_val), None, &[], None, Some(env), None)?;
                            evaluate_call_dispatch(mc, &call_env, &Value::Object(o), Some(&obj_val), &eval_args)
                        }
                        _ => Err(raise_type_error!("OptionalCall target is not a function").into()),
                    }
                }
                _ => {
                    // Fallback: evaluate the lhs; if it throws because of reading properties of null/undefined,
                    // treat it as short-circuit for optional chaining and return undefined.
                    match evaluate_expr(mc, env, lhs) {
                        Ok(left_val) => {
                            if left_val.is_null_or_undefined() {
                                Ok(Value::Undefined)
                            } else {
                                let eval_args = collect_call_args(mc, env, args)?;
                                match left_val {
                                    Value::Function(name) => {
                                        // Optional call invoking `eval` is always an indirect eval — use global env.
                                        let e_4_c = if name == "eval" {
                                            let mut t = *env;
                                            while let Some(proto) = t.borrow().prototype {
                                                t = proto;
                                            }
                                            t
                                        } else {
                                            *env
                                        };
                                        let c_e = prepare_call_env_with_this(mc, Some(&e_4_c), None, None, &[], None, Some(&e_4_c), None)?;
                                        Ok(call_named_eval_or_dispatch(mc, &e_4_c, &c_e, &name, None, &eval_args)?)
                                    }
                                    Value::Closure(c) => call_closure(mc, &c, None, &eval_args, env, None),
                                    Value::Object(o) => {
                                        let call_env = prepare_call_env_with_this(mc, Some(env), None, None, &[], None, Some(env), None)?;
                                        evaluate_call_dispatch(mc, &call_env, &Value::Object(o), None, &eval_args)
                                    }
                                    _ => Err(raise_type_error!("OptionalCall target is not a function").into()),
                                }
                            }
                        }
                        Err(e) => {
                            if e.message() == "Cannot read properties of null or undefined" {
                                Ok(Value::Undefined)
                            } else {
                                Err(e)
                            }
                        }
                    }
                }
            }
        }
        Expr::Delete(target) => match &**target {
            Expr::SuperProperty(_) => {
                let _ = crate::js_class::evaluate_this(mc, env)?;
                Err(raise_reference_error!("Cannot delete a super property").into())
            }
            Expr::Property(obj_expr, _key) if matches!(&**obj_expr, Expr::Super) => {
                let _ = crate::js_class::evaluate_this(mc, env)?;
                Err(raise_reference_error!("Cannot delete a super property").into())
            }
            Expr::Index(obj_expr, key_expr) if matches!(&**obj_expr, Expr::Super) => {
                let _ = crate::js_class::evaluate_this(mc, env)?;
                let _ = evaluate_expr(mc, env, key_expr)?;
                Err(raise_reference_error!("Cannot delete a super property").into())
            }
            Expr::Property(obj_expr, key) => {
                let obj_val = evaluate_expr(mc, env, obj_expr)?;
                if obj_val.is_null_or_undefined() {
                    return Err(raise_type_error!("Cannot delete property of null or undefined").into());
                }
                if let Value::Object(obj) = obj_val {
                    let key_val = PropertyKey::from(key.to_string());
                    // Proxy wrapper: delegate to deleteProperty trap
                    if let Some(proxy_ptr) = get_own_property(&obj, "__proxy__")
                        && let Value::Proxy(p) = &*proxy_ptr.borrow()
                    {
                        let deleted = crate::js_proxy::proxy_delete_property(mc, p, &key_val)?;
                        return Ok(Value::Boolean(deleted));
                    }

                    if obj.borrow().non_configurable.contains(&key_val) {
                        Err(crate::raise_type_error!(format!("Cannot delete non-configurable property '{key}'",)).into())
                    } else {
                        let _ = obj.borrow_mut(mc).properties.shift_remove(&key_val);
                        // Deleting a non-existent property returns true per JS semantics
                        Ok(Value::Boolean(true))
                    }
                } else {
                    Ok(Value::Boolean(true))
                }
            }
            Expr::Index(obj_expr, key_expr) => {
                let obj_val = evaluate_expr(mc, env, obj_expr)?;
                if obj_val.is_null_or_undefined() {
                    return Err(raise_type_error!("Cannot delete property of null or undefined").into());
                }
                let key_val_res = evaluate_expr(mc, env, key_expr)?;
                let key = match &key_val_res {
                    Value::String(s) => PropertyKey::String(utf16_to_utf8(s)),
                    Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(*n))),
                    Value::Symbol(s) => PropertyKey::Symbol(*s),
                    _ => PropertyKey::from(value_to_string(&key_val_res)),
                };
                if let Value::Object(obj) = obj_val {
                    if obj.borrow().non_configurable.contains(&key) {
                        Err(crate::raise_type_error!(format!(
                            "Cannot delete non-configurable property '{}'",
                            value_to_string(&key_val_res)
                        ))
                        .into())
                    } else {
                        let _ = obj.borrow_mut(mc).properties.shift_remove(&key);
                        // Deleting a non-existent property returns true per JS semantics
                        Ok(Value::Boolean(true))
                    }
                } else {
                    Ok(Value::Boolean(true))
                }
            }
            Expr::Var(name, _, _) => {
                Err(raise_syntax_error!(format!("Delete of an unqualified identifier '{name}' in strict mode",)).into())
            }
            _ => {
                let _ = evaluate_expr(mc, env, target)?;
                Ok(Value::Boolean(true))
            }
        },
        Expr::Getter(func_expr) => {
            let val = evaluate_expr(mc, env, func_expr)?;
            let closure = match &val {
                Value::Object(obj) => {
                    let c_val = obj.borrow().get_closure();
                    if let Some(c_ptr) = c_val {
                        let c_ref = c_ptr.borrow();
                        if let Value::Closure(c) = &*c_ref {
                            *c
                        } else {
                            panic!("Getter function missing internal closure (not a closure)");
                        }
                    } else {
                        panic!("Getter function missing internal closure");
                    }
                }
                Value::Closure(c) => *c,
                _ => panic!("Expr::Getter evaluated to invalid value: {:?}", val),
            };
            Ok(Value::Getter(
                closure.body.clone(),
                closure.env.expect("Getter closure requires env"),
                None,
            ))
        }
        Expr::Setter(func_expr) => {
            let val = evaluate_expr(mc, env, func_expr)?;
            let closure = match &val {
                Value::Object(obj) => {
                    let c_val = obj.borrow().get_closure();
                    if let Some(c_ptr) = c_val {
                        let c_ref = c_ptr.borrow();
                        if let Value::Closure(c) = &*c_ref {
                            *c
                        } else {
                            panic!("Setter function missing internal closure (not a closure)");
                        }
                    } else {
                        panic!("Setter function missing internal closure");
                    }
                }
                Value::Closure(c) => *c,
                _ => panic!("Expr::Setter evaluated to invalid value: {:?}", val),
            };
            Ok(Value::Setter(
                closure.params.clone(),
                closure.body.clone(),
                closure.env.expect("Setter closure requires env"),
                None,
            ))
        }
        Expr::TaggedTemplate(tag_expr, strings, exprs) => {
            // Evaluate the tag function
            let func_val = evaluate_expr(mc, env, tag_expr.as_ref())?;

            // Create the "segments" (cooked) array and populate it
            let segments_arr = crate::js_array::create_array(mc, env)?;
            for (i, s) in strings.iter().enumerate() {
                object_set_key_value(mc, &segments_arr, i, &Value::String(s.clone()))?;
            }
            crate::js_array::set_array_length(mc, &segments_arr, strings.len())?;

            // Create the raw array (use same strings here; raw/cooked handling can be refined later)
            let raw_arr = crate::js_array::create_array(mc, env)?;
            for (i, s) in strings.iter().enumerate() {
                object_set_key_value(mc, &raw_arr, i, &Value::String(s.clone()))?;
            }
            crate::js_array::set_array_length(mc, &raw_arr, strings.len())?;

            // Attach raw as a property on segments
            object_set_key_value(mc, &segments_arr, "raw", &Value::Object(raw_arr))?;

            // Evaluate substitution expressions
            let mut call_args: Vec<Value<'gc>> = Vec::new();
            call_args.push(Value::Object(segments_arr));
            for e in exprs.iter() {
                let v = evaluate_expr(mc, env, e)?;
                call_args.push(v);
            }

            // Call the tag function with 'undefined' as this
            let res = evaluate_call_dispatch(mc, env, &func_val, Some(&Value::Undefined), &call_args)?;
            Ok(res)
        }
        Expr::Await(expr) => {
            log::trace!("DEBUG: Evaluating Await");
            let value = evaluate_expr(mc, env, expr)?;
            await_promise_value(mc, env, &value)
        }
        Expr::Yield(_) | Expr::YieldStar(_) => Err(raise_eval_error!("`yield` is only valid inside generator functions").into()),
        Expr::AsyncArrowFunction(params, body) => {
            // Create an async arrow function object which captures the current `this` lexically
            let func_obj = crate::core::new_js_object_data(mc);
            // Set __proto__ to Function.prototype
            if let Some(func_ctor_val) = env_get(env, "Function")
                && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                && let Value::Object(proto) = &*proto_val.borrow()
            {
                func_obj.borrow_mut(mc).prototype = Some(*proto);
            }

            let is_strict = body.first()
                .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
                .unwrap_or(false);
            let closure_data = ClosureData {
                params: params.to_vec(),
                body: body.clone(),
                env: Some(*env),
                bound_this: None,
                is_arrow: true,
                is_strict,
                enforce_strictness_inheritance: true,
                ..ClosureData::default()
            };
            let closure_val = Value::AsyncClosure(Gc::new(mc, closure_data));
            func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));

            // Anonymous async arrow functions should expose an own 'name' property
            // with the empty string as its value and the standard attributes: writable:false,
            // enumerable:false, configurable:true (per Test262 expectations).
            let desc = create_descriptor_object(mc, &Value::String(vec![]), false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "name", &desc)?;

            // Set 'length' for async arrow functions (number of positional params before first default/rest)
            let mut fn_length = 0_usize;
            for p in params.iter() {
                match p {
                    crate::core::DestructuringElement::Variable(_, default_opt) => {
                        if default_opt.is_some() {
                            break;
                        }
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Rest(_) => break,
                    crate::core::DestructuringElement::NestedArray(..) | crate::core::DestructuringElement::NestedObject(..) => {
                        fn_length += 1;
                    }
                    crate::core::DestructuringElement::Empty => {}
                    _ => {}
                }
            }
            let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(fn_length as f64), false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "length", &desc_len)?;

            Ok(Value::Object(func_obj))
        }
        Expr::ValuePlaceholder => Ok(Value::Undefined),
        _ => todo!("{expr:?}"),
    }
}

fn evaluate_var<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str) -> Result<Value<'gc>, EvalError<'gc>> {
    // env_get returns the raw stored slot. If a property descriptor (Value::Property) was installed
    // via DefinePropertyOrThrow, unwrap the stored value. For accessor descriptors, call accessor.
    if let Some(val_ptr) = env_get(env, name) {
        let val = val_ptr.borrow().clone();
        log::debug!("evaluate_var: name='{}' raw_value={:?}", name, val);
        if let Value::Uninitialized = val {
            return Err(raise_reference_error!(format!("Cannot access '{name}' before initialization")).into());
        }
        match val {
            Value::Property { value: Some(v), .. } => return Ok(v.borrow().clone()),
            // Handle accessor properties (Value::Property with no value) and raw accessors
            Value::Property { value: None, .. } | Value::Getter(..) | Value::Setter(..) => {
                return get_property_with_accessors(mc, env, env, name);
            }
            other => return Ok(other),
        }
    }
    if name.starts_with("#") {
        return Err(raise_syntax_error!(format!("Private field '{name}' must be declared in an enclosing class")).into());
    }
    if std::env::var("DEBUG_VAR_LOOKUP").is_ok() && (name == "i" || name == "newResult" || name == "optionalResult") {
        log::debug!("evaluate_var: '{}' not found; dumping env chain", name);
        let mut cur = Some(*env);
        let mut idx = 0;
        while let Some(e) = cur {
            let keys: Vec<String> = e.borrow().properties.keys().map(|k| format!("{:?}", k)).collect();
            log::debug!("  Env[{}]: ptr={:p} keys={:?}", idx, e, keys);
            idx += 1;
            cur = e.borrow().prototype;
        }
        log::debug!(
            "evaluate_var: backtrace for missing '{}':\n{:?}",
            name,
            std::backtrace::Backtrace::capture()
        );
    }
    Err(raise_reference_error!(format!("{name} is not defined")).into())
}

fn evaluate_function_expression<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    name: Option<String>,
    params: &[DestructuringElement],
    body: &mut [Statement],
) -> Result<Value<'gc>, JSError> {
    let func_obj = crate::core::new_js_object_data(mc);

    // Set __proto__ to Function.prototype
    if let Some(func_ctor_val) = env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        func_obj.borrow_mut(mc).prototype = Some(*proto);
    }

    // For Named Function Expressions (NFE), the function's name should be bound
    // only inside the function body. We implement this by setting the function's
    // closure environment to the surrounding lexical env (`*env`) and creating
    // a per-call binding (in `call_closure`) when the function is invoked.
    let has_body_use_strict = body.first()
        .map(|s| {
             matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict")
        })
        .unwrap_or(false);

    // Compute whether the lexical environment or any of its prototypes is marked strict.
    // We deliberately continue traversing the prototype chain even if an own property exists
    // and is false, to avoid transient masking by temporary non-strict markers (e.g., during
    // indirect eval clearing). This ensures child environments correctly inherit strictness.
    let mut proto_iter = Some(*env);
    let mut env_strict_ancestor = false;
    while let Some(cur) = proto_iter {
        if env_get_strictness(&cur) {
            env_strict_ancestor = true;
            break;
        }
        proto_iter = cur.borrow().prototype;
    }

    let is_strict = has_body_use_strict || env_strict_ancestor;

    let closure_data = ClosureData {
        params: params.to_vec(),
        body: body.to_vec(),
        env: Some(*env),
        is_strict,
        enforce_strictness_inheritance: true,
        ..ClosureData::default()
    };
    let closure_val = Value::Closure(Gc::new(mc, closure_data));
    func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
    match name {
        Some(n) if !n.is_empty() => {
            let desc = create_descriptor_object(mc, &Value::String(utf8_to_utf16(&n)), false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "name", &desc)?;
        }
        _ => {
            // Anonymous function expressions expose an own 'name' property with the empty string
            // Use direct insertion + flags to ensure non-writable / non-enumerable markers are set atomically
            object_set_key_value(mc, &func_obj, "name", &Value::String(crate::unicode::utf8_to_utf16("")))?;
            func_obj.borrow_mut(mc).set_non_writable("name");
            func_obj.borrow_mut(mc).set_non_enumerable("name");
            // Mark configurable=true explicitly (default is configurable unless non-configurable marker present)
            func_obj.borrow_mut(mc).set_configurable("name");
        }
    }

    // Set 'length' property for functions (number of positional params before first default/rest)
    let mut fn_length = 0_usize;
    for p in params.iter() {
        match p {
            crate::core::DestructuringElement::Variable(_, default_opt) => {
                if default_opt.is_some() {
                    break;
                }
                fn_length += 1;
            }
            crate::core::DestructuringElement::Rest(_) => break,
            crate::core::DestructuringElement::NestedArray(..) | crate::core::DestructuringElement::NestedObject(..) => {
                fn_length += 1;
            }
            crate::core::DestructuringElement::Empty => {}
            _ => {}
        }
    }
    let desc = create_descriptor_object(mc, &Value::Number(fn_length as f64), false, false, true)?;
    crate::js_object::define_property_internal(mc, &func_obj, "length", &desc)?;

    // Create prototype object
    let proto_obj = crate::core::new_js_object_data(mc);
    // Set prototype of prototype object to Object.prototype
    if let Some(obj_val) = env_get(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
    {
        proto_obj.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    // Set 'constructor' on prototype with proper attributes (writable:true, enumerable:false, configurable:true)
    let desc_ctor = crate::core::create_descriptor_object(mc, &Value::Object(func_obj), true, false, true)?;
    crate::js_object::define_property_internal(mc, &proto_obj, "constructor", &desc_ctor)?;
    // Set 'prototype' on function with proper attributes (writable:true, enumerable:false, configurable:false)
    let desc_proto = crate::core::create_descriptor_object(mc, &Value::Object(proto_obj), true, false, false)?;
    crate::js_object::define_property_internal(mc, &func_obj, "prototype", &desc_proto)?;
    // DEBUG: log pointers for function and its prototype so we can compare when constructing instances
    log::debug!(
        "evaluate_function_expression: func_obj={:p} proto_obj={:p}",
        Gc::as_ptr(func_obj),
        Gc::as_ptr(proto_obj)
    );

    Ok(Value::Object(func_obj))
}

pub(crate) fn get_property_with_accessors<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: impl Into<PropertyKey<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let key = &key.into();

    if let PropertyKey::Private(..) = key {
        // Private members are not inherited, so check own properties only.
        // Walking the prototype chain would incorrectly find private static methods
        // from a base class on a derived class constructor.
        if let Some(val_ptr) = crate::core::get_own_property(obj, key) {
            let val = val_ptr.borrow().clone();
            match val {
                Value::Property { getter, value, .. } => {
                    if let Some(g) = getter {
                        return call_accessor(mc, env, obj, &g);
                    } else if let Some(v) = value {
                        return Ok(v.borrow().clone());
                    }
                    return Err(raise_type_error!("Private accessor has no getter").into());
                }
                Value::Getter(..) => return call_accessor(mc, env, obj, &val),
                _ => return Ok(val),
            }
        } else {
            return Err(raise_type_error!("accessed private field from an ordinary object").into());
        }
    }

    if let PropertyKey::String(s) = key
        && s.starts_with('#')
    {
        // Legacy/Fallback for string-based private keys if any remain
        if let Some(val_ptr) = object_get_key_value(obj, key) {
            let val = val_ptr.borrow().clone();
            match val {
                Value::Property { getter, value, .. } => {
                    if let Some(g) = getter {
                        return call_accessor(mc, env, obj, &g);
                    } else if let Some(v) = value {
                        return Ok(v.borrow().clone());
                    } else {
                        return Ok(Value::Undefined);
                    }
                }
                Value::Getter(..) => return call_accessor(mc, env, obj, &val),
                _ => return Ok(val),
            }
        } else {
            return Err(raise_type_error!("accessed private field from an ordinary object").into());
        }
    }

    // If this object is a Proxy wrapper, delegate to proxy hooks
    if let Some(proxy_ptr) = get_own_property(obj, "__proxy__")
        && let Value::Proxy(p) = &*proxy_ptr.borrow()
    {
        let res_opt = crate::js_proxy::proxy_get_property(mc, p, key)?;
        if let Some(v) = res_opt {
            return Ok(v);
        } else {
            return Ok(Value::Undefined);
        }
    }

    let mut cur = Some(*obj);
    while let Some(cur_obj) = cur {
        if let Some(val_ptr) = object_get_key_value(&cur_obj, key) {
            let val = val_ptr.borrow().clone();
            return match val {
                Value::Property { getter, value, .. } => {
                    if let Some(g) = getter {
                        call_accessor(mc, env, obj, &g)
                    } else if let Some(v) = value {
                        Ok(v.borrow().clone())
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                Value::Getter(..) => call_accessor(mc, env, obj, &val),
                _ => Ok(val),
            };
        }

        // If not found in ordinary properties, check for TypedArray indexed elements when
        // the property key is a canonical numeric index string.
        if let PropertyKey::String(s) = key
            && let Ok(idx) = s.parse::<usize>()
            && let Some(ta_cell) = cur_obj.borrow().properties.get(&PropertyKey::String("__typedarray".to_string()))
            && let Value::TypedArray(ta) = &*ta_cell.borrow()
        {
            log::debug!("get_property_with_accessors: TypedArray property access for key={s} idx={idx}");
            // Compute current length for length-tracking views
            let cur_len = if ta.length_tracking {
                let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                if buf_len <= ta.byte_offset {
                    0
                } else {
                    (buf_len - ta.byte_offset) / ta.element_size()
                }
            } else {
                ta.length
            };
            if idx < cur_len {
                log::trace!("get_property_with_accessors: typedarray idx {} cur_len {}", idx, cur_len);
                match ta.kind {
                    crate::core::TypedArrayKind::BigInt64 | crate::core::TypedArrayKind::BigUint64 => {
                        // For BigInt arrays, read the raw bytes and create a BigInt
                        let size = ta.element_size();
                        let byte_offset = ta.byte_offset + idx * size;
                        let buffer = ta.buffer.borrow();
                        let data = buffer.data.lock().unwrap();
                        if byte_offset + size <= data.len() {
                            let bytes = &data[byte_offset..byte_offset + size];
                            let big_int = if matches!(ta.kind, crate::core::TypedArrayKind::BigInt64) {
                                let mut b = [0u8; 8];
                                b.copy_from_slice(bytes);
                                num_bigint::BigInt::from(i64::from_le_bytes(b))
                            } else {
                                let mut b = [0u8; 8];
                                b.copy_from_slice(bytes);
                                num_bigint::BigInt::from(u64::from_le_bytes(b))
                            };
                            log::debug!("get_property_with_accessors: returning BigInt {} for idx {}", big_int, idx);
                            return Ok(Value::BigInt(Box::new(big_int)));
                        } else {
                            log::debug!("get_property_with_accessors: out of bounds for idx {}", idx);
                            return Ok(Value::Undefined);
                        }
                    }
                    _ => {
                        let n = ta.get(idx)?;
                        log::debug!("get_property_with_accessors: returning Number {n} for idx {idx}");
                        return Ok(Value::Number(n));
                    }
                }
            } else {
                log::debug!("get_property_with_accessors: idx {idx} >= cur_len {cur_len} for TypedArray");
            }
        }

        cur = cur_obj.borrow().prototype;
    }
    log::debug!(
        "get_property_with_accessors: property not found, returning Undefined for key={:?}",
        key
    );
    Ok(Value::Undefined)
}

fn set_property_with_accessors<'gc>(
    mc: &MutationContext<'gc>,
    _env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: impl Into<PropertyKey<'gc>>,
    val: &Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    let key = &key.into();

    if let PropertyKey::Private(..) = key {
        // Check prototype chain for private property (fields are own, methods/accessors on prototype)
        if object_get_key_value(obj, key).is_none() {
            return Err(raise_type_error!("accessed private field from an ordinary object").into());
        }
    }

    if let PropertyKey::String(s) = key
        && s.starts_with('#')
        && object_get_key_value(obj, key).is_none()
    {
        return Err(raise_type_error!("accessed private field from an ordinary object").into());
    }

    // Locate owner (object on prototype chain that actually has the property)
    let mut owner_opt: Option<crate::core::JSObjectDataPtr> = None;
    {
        let mut cur = Some(*obj);
        while let Some(c) = cur {
            // Use `get_own_property` here so we detect the object which actually
            // *owns* the property, rather than `object_get_key_value` which
            // searches the prototype chain and can incorrectly return true for
            // the receiver when the property is inherited.
            if get_own_property(&c, key).is_some() {
                owner_opt = Some(c);
                break;
            }
            cur = c.borrow().prototype;
        }
    }
    let _owner_ptr = owner_opt.map(Gc::as_ptr);
    // Special-case assignment to `__proto__` to update the internal prototype pointer
    if let PropertyKey::String(s) = key
        && s == "__proto__"
    {
        match &val {
            Value::Object(proto_obj) => {
                // If object is non-extensible and the new prototype differs, throw TypeError
                if !obj.borrow().is_extensible() {
                    let differs = match obj.borrow().prototype {
                        Some(cur) => !Gc::ptr_eq(cur, *proto_obj),
                        None => true,
                    };
                    if differs {
                        return Err(raise_type_error!("Cannot change prototype of non-extensible object").into());
                    }
                }
                // Update internal prototype pointer; do NOT create an own enumerable '__proto__' property
                obj.borrow_mut(mc).prototype = Some(*proto_obj);
                return Ok(());
            }
            Value::Null => {
                if !obj.borrow().is_extensible() && obj.borrow().prototype.is_some() {
                    return Err(raise_type_error!("Cannot change prototype of non-extensible object").into());
                }
                obj.borrow_mut(mc).prototype = None;
                return Ok(());
            }
            _ => {
                // For non-object/null, just set the property (do not change internal prototype)
                object_set_key_value(mc, obj, key, val)?;
                return Ok(());
            }
        }
    }

    // Special-case assignment to 'length' on array objects so we can remove/resize indexed elements
    if let PropertyKey::String(s) = key
        && s == "length"
        && crate::js_array::is_array(mc, obj)
    {
        match &val {
            Value::Number(n) => {
                if !n.is_finite() {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if *n < 0.0 {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if *n != n.trunc() {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                let new_len = *n as usize;
                crate::core::object_set_length(mc, obj, new_len)?;
                return Ok(());
            }
            _ => {
                // In JS, setting length to non-number triggers ToUint32 conversion; for now, reject
                return Err(raise_range_error!("Invalid array length").into());
            }
        }
    }

    // If this object is a Proxy wrapper, delegate to its set trap
    if let Some(proxy_ptr) = get_own_property(obj, "__proxy__")
        && let Value::Proxy(p) = &*proxy_ptr.borrow()
    {
        // Proxy#set returns boolean; ignore the boolean here but propagate errors
        let _ok = crate::js_proxy::proxy_set_property(mc, p, key, val)?;
        return Ok(());
    }

    // Pre-check prototypes only if the receiver does NOT already have an own property for the key.
    // If the receiver owns the property, the semantics of assignment must first honor the
    // receiver's own property (data or accessor). Only when there is no own property should we
    // consult the prototype chain for inherited setters or non-writable inherited properties.

    if get_own_property(obj, key).is_none() {
        let mut proto_check = obj.borrow().prototype;
        while let Some(proto_obj) = proto_check {
            // Diagnostic: print prototype object pointer, whether it owns the key,
            // and its writability for the key. This helps verify that the pre-check
            // sees the same non-writable marker that `define_property_internal` set.

            if let Some(inherited_ptr) = object_get_key_value(&proto_obj, key) {
                let inherited = inherited_ptr.borrow().clone();
                match inherited {
                    Value::Property { setter, getter, .. } => {
                        if let Some(s) = setter {
                            // Clone setter out to avoid holding a borrow on the inherited property
                            // cell while calling into the setter, which may mutate prototypes/receiver.
                            let s_clone = (*s).clone();
                            return call_setter(mc, obj, &s_clone, val);
                        }
                        if getter.is_some() {
                            return Err(raise_type_error!("Cannot set property which has only a getter").into());
                        }
                        if !proto_obj.borrow().is_writable(key) {
                            let strict = crate::core::env_get_strictness(_env);
                            log::warn!(
                                "DBG proto_check: inherited non-writable on proto={:p} key={:?} strictness={} env_ptr={:p}",
                                Gc::as_ptr(proto_obj),
                                key,
                                strict,
                                Gc::as_ptr(*_env)
                            );
                            if !strict {
                                // Dump parent env chain and whether any ancestor has the __is_strict marker
                                let mut p = Some(*_env);
                                while let Some(cur) = p {
                                    let has_marker = get_own_property(&cur, "__is_strict").is_some();
                                    log::warn!("DBG env_chain: env_ptr={:p} has__is_strict={}", Gc::as_ptr(cur), has_marker);
                                    p = cur.borrow().prototype;
                                }
                            }
                            if strict {
                                return Err(raise_type_error!("Cannot assign to read-only property").into());
                            } else {
                                return Ok(());
                            }
                        }
                        // writable on prototype -> assignment will create an own property; allow it
                        break;
                    }
                    Value::Setter(params, body, captured_env, home_opt) => {
                        return call_setter_raw(mc, obj, &params, &body, &captured_env, home_opt.clone(), val);
                    }
                    Value::Getter(..) => {
                        return Err(raise_type_error!("Cannot set property which has only a getter").into());
                    }
                    _ => {
                        // Plain inherited value: treat as data property
                        if !proto_obj.borrow().is_writable(key) {
                            let strict = crate::core::env_get_strictness(_env);
                            log::warn!(
                                "DBG proto_check: inherited non-writable on proto={:p} key={:?} strictness={} env_ptr={:p}",
                                Gc::as_ptr(proto_obj),
                                key,
                                strict,
                                Gc::as_ptr(*_env)
                            );
                            if !strict {
                                // Dump parent env chain and whether any ancestor has the __is_strict marker
                                let mut p = Some(*_env);
                                while let Some(cur) = p {
                                    let has_marker = get_own_property(&cur, "__is_strict").is_some();
                                    log::warn!("DBG env_chain: env_ptr={:p} has__is_strict={}", Gc::as_ptr(cur), has_marker);
                                    p = cur.borrow().prototype;
                                }
                            }
                            if strict {
                                return Err(raise_type_error!("Cannot assign to read-only property").into());
                            } else {
                                return Ok(());
                            }
                        }
                        break;
                    }
                }
            }
            proto_check = proto_obj.borrow().prototype;
        }
    }

    // First, locate owner (object on the prototype chain that actually has the property)
    let mut owner_opt: Option<crate::core::JSObjectDataPtr> = None;
    {
        let mut cur = Some(*obj);
        while let Some(c) = cur {
            // Use `get_own_property` to ensure we only detect the object that actually
            // owns the property (not one that only has it via the prototype chain).
            if get_own_property(&c, key).is_some() {
                owner_opt = Some(c);
                break;
            }
            cur = c.borrow().prototype;
        }
    }

    if let Some(owner_obj) = owner_opt {
        // Found an owner in the chain
        if Gc::ptr_eq(owner_obj, *obj) {
            // Owner is the receiver (own property)
            let prop_ptr = object_get_key_value(obj, key).unwrap();
            let prop = prop_ptr.borrow().clone();
            match prop {
                Value::Property { setter, getter, .. } => {
                    if let Some(s) = setter {
                        if let Value::Setter(_, _, _, home_opt) = &*s {
                            if let Some(home_ptr) = home_opt {
                                // Avoid holding a borrow on the home object across the call
                                // (which may mutate the receiver). Extract the raw pointer
                                // first so the borrow doesn't live into the `call_setter`.
                                let home_addr = {
                                    let h = home_ptr.borrow();
                                    Gc::as_ptr(*h)
                                };
                                log::debug!(
                                    "DBG set_property: stored Property descriptor has setter with home_obj={:p}",
                                    home_addr
                                );
                            } else {
                                log::debug!("DBG set_property: stored Property descriptor has setter with no home_obj");
                            }
                        } else {
                            log::debug!("DBG set_property: stored Property descriptor setter is not a Setter variant");
                        }
                        // Clone setter value to avoid holding any borrows into the property cell
                        // while the setter executes and potentially mutates the receiver.
                        let s_clone = (*s).clone();
                        return call_setter(mc, obj, &s_clone, val);
                    }
                    if getter.is_some() {
                        return Err(raise_type_error!("Cannot set property which has only a getter").into());
                    }
                    // If the existing property is non-writable, TypeError should be thrown
                    let writable = { obj.borrow().is_writable(key) };
                    if !writable {
                        return Err(raise_type_error!("Cannot assign to read-only property").into());
                    }
                    let mut prop_mut = prop_ptr.borrow_mut(mc);
                    if let Value::Property { value, .. } = &mut *prop_mut {
                        *value = Some(new_gc_cell_ptr(mc, val.clone()));
                    } else {
                        *prop_mut = val.clone();
                    }
                    Ok(())
                }
                Value::Setter(params, body, captured_env, home_opt) => {
                    if let Some(hb) = &home_opt {
                        log::debug!("DBG set_property: calling setter with home_obj={:p}", Gc::as_ptr(*hb.borrow()));
                    } else {
                        log::debug!("DBG set_property: calling setter with no home_obj");
                    }
                    call_setter_raw(mc, obj, &params, &body, &captured_env, home_opt.clone(), val)
                }
                Value::Getter(..) => Err(raise_type_error!("Cannot set property which has only a getter").into()),
                _ => {
                    // For plain existing properties, respect writability
                    let writable = { obj.borrow().is_writable(key) };
                    if !writable {
                        return Err(raise_type_error!("Cannot assign to read-only property").into());
                    }
                    let mut prop_mut = prop_ptr.borrow_mut(mc);
                    *prop_mut = val.clone();
                    Ok(())
                }
            }
        } else {
            // Owner is on the prototype chain (inherited property)
            let proto_obj = owner_obj;
            let inherited_ptr = object_get_key_value(&proto_obj, key).unwrap();
            let inherited = inherited_ptr.borrow().clone();
            match inherited {
                Value::Property { setter, getter, .. } => {
                    if let Some(s) = setter {
                        // Clone setter to drop any borrows into the property's storage
                        // before invoking the setter, which may mutate the receiver.
                        let s_clone = (*s).clone();
                        return call_setter(mc, obj, &s_clone, val);
                    }
                    if getter.is_some() {
                        return Err(raise_type_error!("Cannot set property which has only a getter").into());
                    }
                    // Inherited data property on prototype: respect prototype's writability
                    if !proto_obj.borrow().is_writable(key) {
                        if crate::core::env_get_strictness(_env) {
                            return Err(raise_type_error!("Cannot assign to read-only property").into());
                        } else {
                            return Ok(());
                        }
                    }
                    // Writable on prototype -> create own property on receiver
                    object_set_key_value(mc, obj, key, val)?;
                    Ok(())
                }
                Value::Setter(params, body, captured_env, home_opt) => {
                    call_setter_raw(mc, obj, &params, &body, &captured_env, home_opt.clone(), val)
                }
                Value::Getter(..) => Err(raise_type_error!("Cannot set property which has only a getter").into()),
                _ => {
                    // Plain inherited value: treat as data property; use prototype writability
                    if !proto_obj.borrow().is_writable(key) {
                        if crate::core::env_get_strictness(_env) {
                            return Err(raise_type_error!("Cannot assign to read-only property").into());
                        } else {
                            return Ok(());
                        }
                    }
                    object_set_key_value(mc, obj, key, val)?;
                    Ok(())
                }
            }
        }
    } else {
        // No owner found in chain: create own property
        if !obj.borrow().is_extensible() {
            if crate::core::env_get_strictness(_env) {
                return Err(raise_type_error!("Cannot add property to non-extensible object").into());
            }
            return Ok(());
        }
        object_set_key_value(mc, obj, key, val)?;
        Ok(())
    }
}

pub fn call_native_function<'gc>(
    mc: &MutationContext<'gc>,
    name: &str,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    if name == "Object.create" {
        let proto = args.first().unwrap_or(&Value::Undefined);
        let new_obj = crate::core::new_js_object_data(mc);
        match proto {
            Value::Object(obj) => {
                new_obj.borrow_mut(mc).prototype = Some(*obj);
            }
            Value::Null => {
                new_obj.borrow_mut(mc).prototype = None;
            }
            _ => return Err(raise_type_error!("Object prototype may only be an Object or null").into()),
        }
        if let Some(props) = args.get(1)
            && !matches!(props, Value::Undefined)
        {
            crate::js_object::define_properties(mc, &new_obj, props)?;
        }
        return Ok(Some(Value::Object(new_obj)));
    }

    // Special-case built-in iterator `next()` so iterator internal throws (EvalError::Throw)
    // propagate as JS-level throws without being converted to JSError.
    if name == "ArrayIterator.prototype.next" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            match crate::js_array::handle_array_iterator_next(mc, obj, env) {
                Ok(v) => return Ok(Some(v)),
                Err(e) => return Err(e),
            }
        } else {
            return Err(raise_eval_error!("ArrayIterator.prototype.next called on non-object").into());
        }
    }

    // Restricted accessor used to implement the throwing 'caller'/'arguments' accessors on Function.prototype
    if name == "Function.prototype.restrictedThrow" {
        return Err(raise_type_error!("Access to 'caller' or 'arguments' is restricted").into());
    }

    if name == "AsyncGenerator.prototype.next" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            match crate::js_async_generator::handle_async_generator_prototype_next(mc, Some(Value::Object(*obj)), args, env) {
                Ok(Some(v)) => return Ok(Some(v)),
                Ok(None) => return Ok(Some(Value::Undefined)),
                Err(e) => return Err(e.into()),
            }
        } else {
            return Err(raise_eval_error!("AsyncGenerator.prototype.next called on non-object").into());
        }
    }

    if name == "AsyncGenerator.prototype.throw" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            match crate::js_async_generator::handle_async_generator_prototype_throw(mc, Some(Value::Object(*obj)), args, env) {
                Ok(Some(v)) => return Ok(Some(v)),
                Ok(None) => return Ok(Some(Value::Undefined)),
                Err(e) => return Err(e.into()),
            }
        } else {
            return Err(raise_eval_error!("AsyncGenerator.prototype.throw called on non-object").into());
        }
    }

    if name == "AsyncGenerator.prototype.return" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            match crate::js_async_generator::handle_async_generator_prototype_return(mc, Some(Value::Object(*obj)), args, env) {
                Ok(Some(v)) => return Ok(Some(v)),
                Ok(None) => return Ok(Some(Value::Undefined)),
                Err(e) => return Err(e.into()),
            }
        } else {
            return Err(raise_eval_error!("AsyncGenerator.prototype.return called on non-object").into());
        }
    }

    if name == "__internal_async_gen_yield_star_resolve" {
        return Ok(Some(crate::js_async_generator::__internal_async_gen_yield_star_resolve(
            mc, args, env,
        )?));
    }

    if name == "__internal_async_gen_yield_star_reject" {
        return Ok(Some(crate::js_async_generator::__internal_async_gen_yield_star_reject(
            mc, args, env,
        )?));
    }

    if name == "__internal_async_gen_yield_resolve" {
        return Ok(Some(crate::js_async_generator::__internal_async_gen_yield_resolve(mc, args, env)?));
    }

    if name == "__internal_async_gen_yield_reject" {
        return Ok(Some(crate::js_async_generator::__internal_async_gen_yield_reject(mc, args, env)?));
    }

    if name == "__internal_async_gen_await_resolve" {
        return Ok(Some(crate::js_async_generator::__internal_async_gen_await_resolve(mc, args, env)?));
    }

    if name == "__internal_async_gen_await_reject" {
        return Ok(Some(crate::js_async_generator::__internal_async_gen_await_reject(mc, args, env)?));
    }

    if name == "AsyncGenerator.prototype.asyncIterator" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(Value::Object(*obj)));
        } else {
            return Err(raise_eval_error!("AsyncGenerator.prototype.asyncIterator called on non-object").into());
        }
    }

    if name == "call" || name == "Function.prototype.call" {
        let this = this_val.ok_or_else(|| EvalError::Js(raise_eval_error!("Cannot call call without this")))?;
        let new_this = args.first().unwrap_or(&Value::Undefined);
        let rest_args = if args.is_empty() { &[] } else { &args[1..] };
        return match this {
            Value::Closure(cl) => Ok(Some(call_closure(mc, cl, Some(new_this), rest_args, env, None)?)),
            Value::AsyncClosure(cl) => Ok(Some(handle_async_closure_call(mc, cl, Some(new_this), rest_args, env, None)?)),
            Value::GeneratorFunction(_, cl) => Ok(Some(handle_generator_function_call(mc, cl, rest_args, Some(new_this), None, None)?)),
            Value::AsyncGeneratorFunction(_, cl) => Ok(Some(handle_async_generator_function_call(mc, cl, rest_args, None)?)),

            Value::Function(func_name) => {
                if let Some(res) = call_native_function(mc, func_name, Some(new_this), rest_args, env)? {
                    Ok(Some(res))
                } else {
                    let call_env = crate::core::new_js_object_data(mc);
                    call_env.borrow_mut(mc).prototype = Some(*env);
                    call_env.borrow_mut(mc).is_function_scope = true;
                    object_set_key_value(mc, &call_env, "this", new_this)?;
                    // If this is a call/apply that targets the builtin "eval", evaluate in the global environment
                    let target_env_for_call = if func_name == "eval" {
                        let mut root_env = *env;
                        while let Some(proto) = root_env.borrow().prototype {
                            root_env = proto;
                        }
                        root_env
                    } else {
                        call_env
                    };
                    Ok(Some(handle_global_function(mc, func_name, rest_args, &target_env_for_call)?))
                }
            }
            Value::Object(obj) => {
                if let Some(cl_ptr) = obj.borrow().get_closure() {
                    match &*cl_ptr.borrow() {
                        Value::Closure(cl) => Ok(Some(call_closure(mc, cl, Some(new_this), rest_args, env, Some(*obj))?)),
                        Value::AsyncClosure(cl) => Ok(Some(handle_async_closure_call(mc, cl, Some(new_this), rest_args, env, Some(*obj))?)),
                        Value::GeneratorFunction(_, cl) => Ok(Some(handle_generator_function_call(
                            mc,
                            cl,
                            rest_args,
                            Some(new_this),
                            None,
                            Some(*obj),
                        )?)),
                        Value::AsyncGeneratorFunction(_, cl) => Ok(Some(handle_async_generator_function_call(mc, cl, rest_args, None)?)),
                        _ => Err(raise_type_error!("Not a function").into()),
                    }
                } else {
                    Err(raise_type_error!("Not a function").into())
                }
            }
            _ => Err(raise_type_error!("Not a function").into()),
        };
    }

    if name == "apply" || name == "Function.prototype.apply" {
        let this = this_val.ok_or_else(|| EvalError::Js(raise_eval_error!("Cannot call apply without this")))?;
        log::trace!("call_native_function: apply called on this={:?}", this);
        let new_this = args.first().cloned().unwrap_or(Value::Undefined);
        let arg_array = args.get(1).cloned().unwrap_or(Value::Undefined);

        let mut rest_args = Vec::new();
        if let Value::Object(obj) = arg_array
            && is_array(mc, &obj)
        {
            let len_val = object_get_key_value(&obj, "length").unwrap_or(new_gc_cell_ptr(mc, Value::Undefined));
            let len = if let Value::Number(n) = *len_val.borrow() { n as usize } else { 0 };
            for k in 0..len {
                let item = object_get_key_value(&obj, k).unwrap_or(new_gc_cell_ptr(mc, Value::Undefined));
                rest_args.push(item.borrow().clone());
            }
        }

        return match this {
            Value::Closure(cl) => Ok(Some(call_closure(mc, cl, Some(&new_this), &rest_args, env, None)?)),
            Value::AsyncClosure(cl) => Ok(Some(handle_async_closure_call(mc, cl, Some(&new_this), &rest_args, env, None)?)),
            Value::GeneratorFunction(_, cl) => Ok(Some(handle_generator_function_call(
                mc,
                cl,
                &rest_args,
                Some(&new_this),
                None,
                None,
            )?)),
            Value::AsyncGeneratorFunction(_, cl) => Ok(Some(handle_async_generator_function_call(mc, cl, &rest_args, None)?)),
            Value::Function(func_name) => {
                if let Some(res) = call_native_function(mc, func_name, Some(&new_this), &rest_args, env)? {
                    Ok(Some(res))
                } else {
                    let call_env = crate::core::new_js_object_data(mc);
                    call_env.borrow_mut(mc).prototype = Some(*env);
                    call_env.borrow_mut(mc).is_function_scope = true;
                    object_set_key_value(mc, &call_env, "this", &new_this)?;
                    // If this is a call/apply that targets the builtin "eval", evaluate in the global environment
                    let target_env_for_call = if func_name == "eval" {
                        let mut root_env = *env;
                        while let Some(proto) = root_env.borrow().prototype {
                            root_env = proto;
                        }
                        root_env
                    } else {
                        call_env
                    };
                    Ok(Some(crate::js_function::handle_global_function(
                        mc,
                        func_name,
                        &rest_args,
                        &target_env_for_call,
                    )?))
                }
            }
            Value::Object(obj) => {
                if let Some(cl_ptr) = obj.borrow().get_closure() {
                    match &*cl_ptr.borrow() {
                        Value::Closure(cl) => Ok(Some(call_closure(mc, cl, Some(&new_this), &rest_args, env, Some(*obj))?)),
                        Value::AsyncClosure(cl) => Ok(Some(handle_async_closure_call(
                            mc,
                            cl,
                            Some(&new_this),
                            &rest_args,
                            env,
                            Some(*obj),
                        )?)),
                        Value::GeneratorFunction(_, cl) => Ok(Some(handle_generator_function_call(
                            mc,
                            cl,
                            &rest_args,
                            Some(&new_this),
                            None,
                            Some(*obj),
                        )?)),
                        Value::AsyncGeneratorFunction(_, cl) => Ok(Some(handle_async_generator_function_call(mc, cl, &rest_args, None)?)),
                        _ => Err(raise_type_error!("Not a function").into()),
                    }
                } else {
                    Err(raise_type_error!("Not a function").into())
                }
            }
            _ => Err(raise_type_error!("Not a function").into()),
        };
    }

    if name == "toString" {
        let this = this_val.unwrap_or(&Value::Undefined);
        let tag = match &this {
            Value::Number(_) => "Number".to_string(),
            Value::String(_) => "String".to_string(),
            Value::Boolean(_) => "Boolean".to_string(),
            Value::BigInt(_) => "BigInt".to_string(),
            Value::Symbol(_) => "Symbol".to_string(),
            Value::Undefined => "Undefined".to_string(),
            Value::Null => "Null".to_string(),
            Value::Object(obj) => {
                let mut t = "Object".to_string();
                if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                    && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                    && let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
                    && let Value::Symbol(s) = &*tag_sym.borrow()
                    && let Some(val) = object_get_key_value(obj, s)
                    && let Value::String(s_val) = &*val.borrow()
                {
                    t = crate::unicode::utf16_to_utf8(s_val);
                }
                t
            }
            Value::Closure(_) | Value::Function(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..) => "Function".to_string(),
            _ => "Object".to_string(),
        };
        return Ok(Some(Value::String(crate::unicode::utf8_to_utf16(&format!("[object {tag}]")))));
    }
    if name == "IteratorSelf" {
        return Ok(Some(this_val.unwrap_or(&Value::Undefined).clone()));
    }
    if name == "StringIterator.prototype.next" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_string::handle_string_iterator_next(mc, obj)?));
        } else {
            return Err(raise_eval_error!("TypeError: StringIterator.prototype.next called on non-object").into());
        }
    }
    if name == "MapIterator.prototype.next" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_map::handle_map_iterator_next(mc, obj, env)?));
        } else {
            return Err(raise_eval_error!("TypeError: MapIterator.prototype.next called on non-object").into());
        }
    }

    if name == "ArrayIterator.prototype.next" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_array::handle_array_iterator_next(mc, obj, env)?));
        } else {
            return Err(raise_eval_error!("TypeError: ArrayIterator.prototype.next called on non-object").into());
        }
    }

    if name == "SetIterator.prototype.next" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_set::handle_set_iterator_next(mc, obj, env)?));
        } else {
            return Err(raise_eval_error!("TypeError: SetIterator.prototype.next called on non-object").into());
        }
    }

    if name == "Symbol" {
        return Ok(Some(crate::js_symbol::handle_symbol_call(mc, args, env)?));
    }

    if name == "Symbol.for" {
        return Ok(Some(crate::js_symbol::handle_symbol_for(mc, args, env)?));
    }

    if name == "Symbol.keyFor" {
        return Ok(Some(crate::js_symbol::handle_symbol_keyfor(mc, args, env)?));
    }

    if name == "Symbol.prototype.toString" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        return Ok(Some(crate::js_symbol::handle_symbol_tostring(mc, this_v)?));
    }

    if name == "Symbol.prototype.valueOf" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        return Ok(Some(crate::js_symbol::handle_symbol_valueof(mc, this_v)?));
    }

    if name.starts_with("Map.")
        && let Some(method) = name.strip_prefix("Map.prototype.")
    {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            if let Some(map_val) = object_get_key_value(obj, "__map__") {
                if let Value::Map(map_ptr) = &*map_val.borrow() {
                    return Ok(Some(crate::js_map::handle_map_instance_method(mc, map_ptr, method, args, env)?));
                } else {
                    return Err(raise_eval_error!("TypeError: Map.prototype method called on incompatible receiver").into());
                }
            } else {
                return Err(raise_eval_error!("TypeError: Map.prototype method called on incompatible receiver").into());
            }
        } else if let Value::Map(map_ptr) = this_v {
            return Ok(Some(crate::js_map::handle_map_instance_method(mc, map_ptr, method, args, env)?));
        } else {
            return Err(raise_eval_error!("TypeError: Map.prototype method called on non-object receiver").into());
        }
    }

    if name.starts_with("Set.")
        && let Some(method) = name.strip_prefix("Set.prototype.")
    {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            if let Some(set_val) = object_get_key_value(obj, "__set__") {
                if let Value::Set(set_ptr) = &*set_val.borrow() {
                    return Ok(Some(handle_set_instance_method(mc, set_ptr, this_v, method, args, env)?));
                } else {
                    return Err(raise_eval_error!("TypeError: Set.prototype method called on incompatible receiver").into());
                }
            } else {
                return Err(raise_eval_error!("TypeError: Set.prototype method called on incompatible receiver").into());
            }
        } else if let Value::Set(set_ptr) = this_v {
            return Ok(Some(handle_set_instance_method(
                mc,
                set_ptr,
                &Value::Set(*set_ptr),
                method,
                args,
                env,
            )?));
        } else {
            return Err(raise_eval_error!("TypeError: Set.prototype method called on non-object receiver").into());
        }
    }

    if name.starts_with("DataView.prototype.")
        && let Some(method) = name.strip_prefix("DataView.prototype.")
    {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_typedarray::handle_dataview_method(mc, obj, method, args, env)?));
        } else {
            return Err(raise_eval_error!("TypeError: DataView method called on non-object").into());
        }
    }

    if name.starts_with("Atomics.")
        && let Some(method) = name.strip_prefix("Atomics.")
    {
        return Ok(Some(crate::js_typedarray::handle_atomics_method(mc, method, args, env)?));
    }

    if name == "ArrayBuffer.prototype.byteLength" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_typedarray::handle_arraybuffer_accessor(mc, obj, "byteLength")?));
        } else {
            return Err(raise_eval_error!("TypeError: ArrayBuffer.prototype.byteLength called on non-object").into());
        }
    }

    if name == "ArrayBuffer.prototype.resize" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_typedarray::handle_arraybuffer_method(mc, obj, "resize", args)?));
        } else {
            return Err(raise_eval_error!("TypeError: ArrayBuffer.prototype.resize called on non-object").into());
        }
    }

    if name == "SharedArrayBuffer.prototype.byteLength" {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_typedarray::handle_arraybuffer_accessor(mc, obj, "byteLength")?));
        } else {
            return Err(raise_eval_error!("TypeError: SharedArrayBuffer.prototype.byteLength called on non-object").into());
        }
    }

    if name.starts_with("TypedArray.prototype.")
        && let Some(method) = name.strip_prefix("TypedArray.prototype.")
        && (method == "values" || method == "set" || method == "subarray")
    {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        return Ok(Some(crate::js_typedarray::handle_typedarray_method(mc, this_v, method, args, env)?));
    }

    if let Some(prop) = name.strip_prefix("TypedArray.prototype.") {
        let this_v = this_val.unwrap_or(&Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_typedarray::handle_typedarray_accessor(mc, obj, prop)?));
        } else {
            return Err(raise_eval_error!("TypeError: TypedArray accessor called on non-object").into());
        }
    }
    Ok(None)
}

pub(crate) fn call_accessor<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    accessor: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match accessor {
        Value::Function(name) => {
            // Special-case restricted accessor thrower
            if name == "Function.prototype.restrictedThrow" {
                return Err(raise_type_error!("Access to 'caller' or 'arguments' is restricted").into());
            }
            if let Some(res) = call_native_function(mc, name, Some(&Value::Object(*receiver)), &[], env)? {
                Ok(res)
            } else {
                // For certain well-known internal accessors, surface a TypeError (e.g., restricted 'caller'/'arguments')
                if name.contains("restrictedThrow") {
                    Err(raise_type_error!("Access to 'caller' or 'arguments' is restricted").into())
                } else {
                    Err(raise_type_error!(format!("Accessor function {name} not supported")).into())
                }
            }
        }
        Value::Getter(body, captured_env, home_opt) => {
            let call_env = crate::core::new_js_object_data(mc);
            call_env.borrow_mut(mc).prototype = Some(*captured_env);
            call_env.borrow_mut(mc).is_function_scope = true;
            object_set_key_value(mc, &call_env, "this", &Value::Object(*receiver))?;
            // If the getter carried a home object, propagate it into call env so `super` resolves
            call_env.borrow_mut(mc).set_home_object(home_opt.clone());
            let body_clone = body.clone();
            match evaluate_statements_with_labels(mc, &call_env, &body_clone, &[], &[])? {
                ControlFlow::Return(val) => Ok(val),
                ControlFlow::Normal(_) => Ok(Value::Undefined),
                ControlFlow::Throw(v, line, column) => Err(EvalError::Throw(v, line, column)),
                ControlFlow::Break(_) => Err(raise_syntax_error!("break statement not in loop or switch").into()),
                ControlFlow::Continue(_) => Err(raise_syntax_error!("continue statement not in loop").into()),
            }
        }
        Value::Closure(cl) => crate::core::call_closure(mc, cl, Some(&Value::Object(*receiver)), &[], env, None),
        Value::Object(obj) => {
            // Check for internal closure
            let cl_val_opt = obj.borrow().get_closure();
            if let Some(cl_val) = cl_val_opt
                && let Value::Closure(cl) = &*cl_val.borrow()
            {
                return crate::core::call_closure(mc, cl, Some(&Value::Object(*receiver)), &[], env, Some(*obj));
            }
            Err(raise_type_error!("Accessor is not a function").into())
        }
        _ => Err(raise_type_error!("Accessor is not a function").into()),
    }
}

fn call_setter<'gc>(
    mc: &MutationContext<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    setter: &Value<'gc>,
    val: &Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    match setter {
        Value::Setter(params, body, captured_env, home_opt) => {
            if let Some(hb) = &home_opt {
                log::trace!("DBG call_setter: stored Setter has home_obj={:p}", Gc::as_ptr(*hb.borrow()));
            } else {
                log::trace!("DBG call_setter: stored Setter has no home_obj");
            }
            call_setter_raw(mc, receiver, params, body, captured_env, home_opt.clone(), val)
        }
        Value::Closure(cl) => {
            let cl_data = cl;
            let call_env = crate::core::new_js_object_data(mc);
            call_env.borrow_mut(mc).prototype = cl_data.env;
            call_env.borrow_mut(mc).is_function_scope = true;
            object_set_key_value(mc, &call_env, "this", &Value::Object(*receiver))?;

            if let Some(first_param) = cl_data.params.first()
                && let DestructuringElement::Variable(name, _) = first_param
            {
                crate::core::env_set(mc, &call_env, name, val)?;
            }

            // If the closure has a stored home object, propagate it into the call environment
            call_env.borrow_mut(mc).set_home_object(cl_data.home_object.clone());

            let body_clone = cl_data.body.clone();
            evaluate_statements(mc, &call_env, &body_clone).map(|_| ())
        }
        Value::Object(obj) => {
            // Check for internal closure
            let cl_val_opt = obj.borrow().get_closure();
            if let Some(cl_val) = cl_val_opt
                && let Value::Closure(cl) = &*cl_val.borrow()
            {
                // If the function object wrapper holds a home object, propagate it
                let home_opt = obj.borrow().get_home_object();
                let env_ptr = cl.env.expect("Closure must have an env for setter call");
                return call_setter_raw(mc, receiver, &cl.params, &cl.body, &env_ptr, home_opt, val);
            }
            Err(raise_type_error!("Setter is not a function").into())
        }
        _ => Err(raise_type_error!("Setter is not a function").into()),
    }
}

fn call_setter_raw<'gc>(
    mc: &MutationContext<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    params: &[DestructuringElement],
    body: &[Statement],
    env: &JSObjectDataPtr<'gc>,
    home_opt: Option<GcCell<JSObjectDataPtr<'gc>>>,
    val: &Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    let params_env = crate::core::new_js_object_data(mc);
    params_env.borrow_mut(mc).prototype = Some(*env);
    params_env.borrow_mut(mc).is_function_scope = true;
    object_set_key_value(mc, &params_env, "this", &Value::Object(*receiver))?;

    // If the setter carried a home object, propagate it into call env so `super` resolves
    if let Some(home_obj) = &home_opt {
        log::debug!(
            "DBG call_setter_raw: propagating home_obj={:p} into call_env",
            Gc::as_ptr(*home_obj.borrow())
        );
    } else {
        log::debug!("DBG call_setter_raw: no home_obj provided");
    }
    params_env.borrow_mut(mc).set_home_object(home_opt.clone());

    if let Some(param) = params.first()
        && let DestructuringElement::Variable(name, _) = param
    {
        crate::core::env_set(mc, &params_env, name, val)?;
    }

    // If this setter has a parameter default initializer expression, evaluate it
    // in the parameter environment to ensure it cannot see body declarations.
    if let Some(param) = params.first()
        && let DestructuringElement::Variable(_name, Some(default_expr)) = param
    {
        let _ = evaluate_expr(mc, &params_env, default_expr)?;
    }

    let call_env = crate::core::new_js_object_data(mc);
    call_env.borrow_mut(mc).prototype = Some(params_env);
    call_env.borrow_mut(mc).is_function_scope = true;
    object_set_key_value(mc, &call_env, "this", &Value::Object(*receiver))?;
    call_env.borrow_mut(mc).set_home_object(home_opt);

    let body_clone = body.to_vec();
    evaluate_statements(mc, &call_env, &body_clone).map(|_| ())
}

pub(crate) fn js_error_to_value<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, js_err: &JSError) -> Value<'gc> {
    let full_msg = js_err.message();

    let (name, raw_msg) = match js_err.kind() {
        JSErrorKind::ReferenceError { message } => ("ReferenceError", message.clone()),
        JSErrorKind::SyntaxError { message } => ("SyntaxError", message.clone()),
        JSErrorKind::TypeError { message } => ("TypeError", message.clone()),
        JSErrorKind::RangeError { message } => ("RangeError", message.clone()),
        JSErrorKind::VariableNotFound { name } => ("ReferenceError", format!("{} is not defined", name)),
        JSErrorKind::TokenizationError { message } => ("SyntaxError", message.clone()),
        JSErrorKind::ParseError { message } => ("SyntaxError", message.clone()),
        JSErrorKind::EvaluationError { message } => ("Error", message.clone()),
        JSErrorKind::RuntimeError { message } => ("Error", message.clone()),
        JSErrorKind::Throw(_) => ("Error", full_msg.clone()),
        _ => ("Error", full_msg.clone()),
    };

    // Prefer global constructors by searching the lexical environment chain
    // upwards for a constructor with the given name. Avoid climbing the object
    // prototype chain which may bypass the lexical/global environment.
    let mut found_proto: Option<JSObjectDataPtr<'gc>> = None;
    let mut found_ctor: Option<JSObjectDataPtr<'gc>> = None;
    let mut search_env: Option<JSObjectDataPtr<'gc>> = Some(*env);
    while let Some(cur) = search_env {
        if let Some(err_ctor_val) = env_get_own(&cur, name)
            && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(err_ctor, "prototype")
            && let Value::Object(proto) = &*proto_val.borrow()
        {
            found_proto = Some(*proto);
            found_ctor = Some(*err_ctor);
            break;
        }
        search_env = cur.borrow().prototype;
    }

    // If not found in lexical chain, fallback to Error constructor's prototype via env or root
    if found_proto.is_none() {
        // try env's Error
        if let Some(err_ctor_val) = env_get(env, "Error")
            && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(err_ctor, "prototype")
            && let Value::Object(proto) = &*proto_val.borrow()
        {
            found_proto = Some(*proto);
            found_ctor = Some(*err_ctor);
        } else {
            found_proto = None;
        }
    }

    let error_proto = found_proto;

    let err_val = create_error(mc, error_proto, (&raw_msg).into()).unwrap_or(Value::Undefined);

    if let Value::Object(obj) = &err_val {
        obj.borrow_mut(mc).set_property(mc, "name", name.into());
        // Mark name non-enumerable (match built-in Error behavior)
        obj.borrow_mut(mc).set_non_enumerable("name");
        obj.borrow_mut(mc).set_property(mc, "message", (&raw_msg).into());
        // Mark message non-enumerable
        obj.borrow_mut(mc).set_non_enumerable("message");

        let stack = js_err.stack();
        let stack_str = if stack.is_empty() {
            format!("{name}: {raw_msg}")
        } else {
            format!("{name}: {raw_msg}\n    {}", stack.join("\n    "))
        };
        obj.borrow_mut(mc).set_property(mc, "stack", stack_str.into());
        // Mark stack non-enumerable
        obj.borrow_mut(mc).set_non_enumerable("stack");

        // Ensure constructor property on the thrown instance points to the native constructor
        // if available in the global environment chain. This makes `e.constructor === TypeError` behave as expected.
        let mut root_env = *env;
        while let Some(proto) = root_env.borrow().prototype {
            root_env = proto;
        }
        // Prefer the constructor referenced by the error prototype if available
        // This ensures the instance's constructor matches the prototype's constructor
        // and avoids mismatches when the same named constructor is bound differently
        // higher in the global root chain.
        if let Some(ctor_obj) = found_ctor {
            obj.borrow_mut(mc).set_property(mc, "constructor", Value::Object(ctor_obj));
        } else if let Some(proto) = error_proto {
            if let Some(ctor_val) = object_get_key_value(&proto, "constructor") {
                obj.borrow_mut(mc).set_property(mc, "constructor", ctor_val.borrow().clone());
            }
        } else if let Some(ctor_val) = object_get_key_value(&root_env, name)
            && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        {
            // Fallback: set to whatever constructor is present on the root environment
            obj.borrow_mut(mc).set_property(mc, "constructor", Value::Object(*ctor_obj));
        }
    }
    err_val
}

pub fn call_closure<'gc>(
    mc: &MutationContext<'gc>,
    cl: &crate::core::ClosureData<'gc>,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    fn_obj: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if let Some(target_name) = &cl.native_target {
        let effective_this = if let Some(bound) = &cl.bound_this { Some(bound) } else { this_val };
        if let Some(res) = crate::core::call_native_function(mc, target_name, effective_this, args, env)? {
            return Ok(res);
        }
        let msg = format!("Native function binding failed for {target_name}");
        return Err(raise_type_error!(msg).into());
    }

    // Create param/var environments per spec if parameter initializers exist.
    let _has_param_expressions = cl.params.iter().any(|p| matches!(p, DestructuringElement::Variable(_, Some(_))));
    let (param_env, var_env) = {
        // Always create both parameter and variable environments so we can
        // bind the function's own name into the parameter environment (so that
        // `var` declarations in the body can shadow it per spec expectations).
        let p = crate::core::new_js_object_data(mc);
        p.borrow_mut(mc).prototype = cl.env;
        p.borrow_mut(mc).is_function_scope = true;
        let v = crate::core::new_js_object_data(mc);
        v.borrow_mut(mc).prototype = Some(p);
        v.borrow_mut(mc).is_function_scope = true;
        // Record whether this call's function is an ArrowFunction so eval() can
        // determine `new.target` early-error applicability even when the
        // environment does not carry a `__function` binding.
        object_set_key_value(mc, &v, "__is_arrow_function", &Value::Boolean(cl.is_arrow))?;
        (p, v)
    };

    // Debug: log whether the closure's env chain contains __import_meta (helps diagnose missing import.meta)
    let closure_env_ptr = if let Some(e) = cl.env {
        Gc::as_ptr(e) as *const _
    } else {
        std::ptr::null()
    };
    log::trace!(
        "call_closure: closure_env_ptr={:p} has___import_meta={}",
        closure_env_ptr,
        crate::core::object_get_key_value(&cl.env.unwrap_or_else(|| crate::core::new_js_object_data(mc)), "__import_meta").is_some()
    );

    // Determine whether this function is strict (either via its own 'use strict'
    // directive at creation time or because its lexical environment is marked strict).
    // Determine whether any ancestor of the function's lexical environment is strict.
    let mut env_strict_ancestor = false;
    if cl.enforce_strictness_inheritance {
        let mut proto_iter = cl.env;
        while let Some(cur) = proto_iter {
            if env_get_strictness(&cur) {
                env_strict_ancestor = true;
                break;
            }
            proto_iter = cur.borrow().prototype;
        }
    }

    let fn_is_strict = cl.is_strict || env_strict_ancestor;
    // Explicitly set strictness on both environments
    env_set_strictness(mc, &param_env, fn_is_strict)?;
    env_set_strictness(mc, &var_env, fn_is_strict)?;

    // If this is a Named Function Expression and the function object has a
    // `name` property, bind that name in the function's parameter environment so
    // the function can reference itself by name (e.g., `fac` inside `function fac ...`).
    if let Some(fn_obj_ptr) = fn_obj {
        if let Some(name) = fn_obj_ptr.borrow().get_property("name") {
            // If the function object was created as a hoisted declaration on its
            // creation environment, do NOT create a separate per-call binding for
            // the name (that behavior is only for Named Function Expressions).
            let mut should_bind_name = true;
            if let Some(creation_env) = cl.env
                && let Some(existing_cell) = crate::core::env_get_own(&creation_env, &name)
            {
                // If the existing own binding is the same object as the function
                // object being called, assume this was a function declaration
                // and skip creating the per-call name binding.
                match &*existing_cell.borrow() {
                    Value::Object(existing_obj_ptr) => {
                        if Gc::as_ptr(*existing_obj_ptr) == Gc::as_ptr(fn_obj_ptr) {
                            should_bind_name = false;
                        }
                    }
                    Value::Property { value: Some(v), .. } => {
                        if let Value::Object(existing_obj_ptr) = &*v.borrow()
                            && Gc::as_ptr(*existing_obj_ptr) == Gc::as_ptr(fn_obj_ptr)
                        {
                            should_bind_name = false;
                        }
                    }
                    _ => {}
                }
            }
            if should_bind_name {
                crate::core::env_set(mc, &param_env, &name, &Value::Object(fn_obj_ptr))?;
                // If this function executes in strict mode, the name binding must be immutable
                // (assignment to it should throw a TypeError). Mark it as a const binding so
                // subsequent assignment attempts will produce the correct error.
                if fn_is_strict {
                    param_env.borrow_mut(mc).set_const(name.clone());
                }
            }
            // Always set a frame name on the variable environment so thrown errors can
            // indicate which function they occurred in (used in stack traces). Even when
            // the function was hoisted into its creation environment (and therefore we
            // skipped creating a per-call name binding), the frame name is useful for
            // generating meaningful stack traces.
            object_set_key_value(mc, &var_env, "__frame", &Value::String(utf8_to_utf16(&name)))?;
        }
        // Link caller environment so stacks can be assembled by walking __caller
        object_set_key_value(mc, &var_env, "__caller", &Value::Object(*env))?;
        // Expose the function object itself for runtime checks (e.g., `super` handling in eval)
        object_set_key_value(mc, &var_env, "__function", &Value::Object(fn_obj_ptr))?;

        // Debug: indicate whether the function object has a [[HomeObject]] internal slot
        if std::env::var("TEST262_LOG_LEVEL").map(|v| v == "debug").unwrap_or(false) {
            if let Some(home) = fn_obj_ptr.borrow().get_home_object() {
                log::debug!("DBG call_closure: fn_obj has home_object ptr={:p}", Gc::as_ptr(*home.borrow()));
            } else {
                log::debug!("DBG call_closure: fn_obj has NO home_object");
            }
        }
    }

    // Predeclare parameter bindings as Uninitialized so destructuring assignments
    // can use env_set_recursive without ReferenceError and TDZ can be enforced.
    if !cl.params.is_empty() {
        let mut param_names: Vec<String> = Vec::new();
        for param in cl.params.iter() {
            collect_names_from_destructuring_element(param, &mut param_names);
        }
        for name in param_names {
            if env_get_own(&param_env, &name).is_none() {
                env_set(mc, &param_env, &name, &Value::Uninitialized)?;
            }
        }
    }

    // Determine the [[This]] binding for the call.
    // If the closure has a bound_this (from bind()), use it.
    // Otherwise, if a caller supplied an explicit this_val, use it.
    // If no this_val was supplied (bare call), we must default according to the function's strictness:
    // - strict functions: undefined
    // - non-strict functions: global object
    let effective_this = if cl.is_arrow {
        let lexical_env = cl.env.as_ref().unwrap_or(env);
        Some(crate::js_class::evaluate_this_allow_uninitialized(mc, lexical_env)?)
    } else if let Some(bound) = &cl.bound_this {
        Some(bound.clone())
    } else if let Some(tv) = this_val {
        Some(tv.clone())
    } else {
        // No explicit this provided. Choose default based on function strictness.
        if fn_is_strict {
            Some(Value::Undefined)
        } else {
            // Non-strict: default to the global object (topmost env object)
            let mut root_env = *env;
            while let Some(proto) = root_env.borrow().prototype {
                root_env = proto;
            }
            Some(Value::Object(root_env))
        }
    };

    // Bind 'this' into the var_env (function body environment) so the body can access it.
    if let Some(tv) = effective_this {
        object_set_key_value(mc, &var_env, "this", &tv)?;
        log::trace!("DBG call_closure: bound this into var_env = {:?}", tv);
        if cl.is_arrow && matches!(tv, Value::Uninitialized) {
            // If it's an arrow function capturing an uninitialized 'this' (e.g. in derived constructor before super()),
            // we should also mark the call environment as uninitialized.
            object_set_key_value(mc, &var_env, "__this_initialized", &Value::Boolean(false))?;
        } else {
            object_set_key_value(mc, &var_env, "__this_initialized", &Value::Boolean(true))?;
        }
    }
    log::debug!("call_closure: is_arrow={} returning env_ptr={:p}", cl.is_arrow, var_env.as_ptr());

    // If the function was stored as an object with a home object, propagate
    // that into the function body and parameter environments (this covers methods defined in object-literals
    // where default parameter initializers must see [[HomeObject]] when they evaluate).
    if let Some(fn_obj_ptr) = fn_obj
        && let Some(home_obj) = fn_obj_ptr.borrow().get_home_object()
    {
        var_env.borrow_mut(mc).set_home_object(Some(home_obj.clone()));
        param_env.borrow_mut(mc).set_home_object(Some(home_obj));
    }

    // FIX: propagate [[HomeObject]] into var_env and param_env so `super` resolves parent prototype and avoids recursive lookup
    // Propagate home object into the function body and parameter environments so `super.*` can resolve
    // the proper parent prototype during method calls.
    if let Some(home_obj) = &cl.home_object {
        var_env.borrow_mut(mc).set_home_object(Some(home_obj.clone()));
        param_env.borrow_mut(mc).set_home_object(Some(home_obj.clone()));
    }

    if !cl.is_arrow {
        let args_obj = crate::core::new_js_object_data(mc);
        if let Some(obj_val) = crate::core::env_get(env, "Object")
            && let Value::Object(obj_ctor) = &*obj_val.borrow()
            && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
            && let Value::Object(proto) = &*proto_val.borrow()
        {
            args_obj.borrow_mut(mc).prototype = Some(*proto);
        }

        if let Some(obj_val) = crate::core::env_get(env, "Object")
            && let Value::Object(obj_ctor) = &*obj_val.borrow()
            && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
            && let Value::Object(proto) = &*proto_val.borrow()
        {
            args_obj.borrow_mut(mc).prototype = Some(*proto);
        }

        object_set_key_value(mc, &args_obj, "length", &Value::Number(args.len() as f64))?;

        // Define iterator to allow spread args...
        // arguments[Symbol.iterator] = Array.prototype.values
        // This is needed for `...arguments` to work.
        if let Some(sym_val) = crate::core::env_get(env, "Symbol")
            && let Value::Object(sym_ctor) = &*sym_val.borrow()
            && let Some(iter_sym_val) = object_get_key_value(sym_ctor, "iterator")
        {
            // Get Array.prototype.values
            if let Some(arr_val) = crate::core::env_get(env, "Array")
                && let Value::Object(arr_ctor) = &*arr_val.borrow()
                && let Some(arr_proto_val) = object_get_key_value(arr_ctor, "prototype")
                && let Value::Object(arr_proto) = &*arr_proto_val.borrow()
                && let Some(_values_fn) = object_get_key_value(arr_proto, "values")
            {
                let key_val = iter_sym_val.borrow().clone();
                let _key_str = match key_val {
                    Value::Symbol(s) => s.description().unwrap_or_default().to_owned(), // logic for symbol key?
                    _ => "iterator".to_string(),
                };
                // But object_set_key_value takes PropertyKey.
                // PropertyKey can be Symbol?
                // Let's check object_set_key_value.
                // If PropertyKey is string only, we are in trouble.
                // Value::Symbol IS supported in PropertyKey?
            }
        }

        for (i, val) in args.iter().enumerate() {
            object_set_key_value(mc, &args_obj, i, val)?;
        }

        // Minimal arguments object: expose numeric properties and length
        // We use create_arguments_object here so we get consistent behavior with strict mode callee/caller restrictions
        // object_set_length(mc, &args_obj, args.len())?;

        // if let Some(fn_ptr) = fn_obj {
        //     object_set_key_value(mc, &args_obj, "callee", &Value::Object(fn_ptr))?;
        // }

        let callee_val = fn_obj.map(Value::Object);
        // Place the arguments object into the var_env (function body env)
        crate::js_class::create_arguments_object(mc, &var_env, args, callee_val.as_ref())?;
        // Ensure parameter defaults can access `arguments` via the parameter env chain.
        if crate::core::get_own_property(&param_env, "arguments").is_none() {
            crate::js_class::create_arguments_object(mc, &param_env, args, callee_val.as_ref())?;
        }

        // env_set(mc, &call_env, "arguments", &Value::Object(args_obj))?;
    }

    for (i, param) in cl.params.iter().enumerate() {
        match param {
            DestructuringElement::Variable(name, default_expr_opt) => {
                let mut arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                if matches!(arg_val, Value::Undefined)
                    && let Some(default_expr) = default_expr_opt
                {
                    arg_val = evaluate_expr(mc, &param_env, default_expr)?;
                }
                crate::core::env_set(mc, &param_env, name, &arg_val)?;
            }
            DestructuringElement::Rest(name) => {
                let rest_args = if i < args.len() { args[i..].to_vec() } else { Vec::new() };
                let array_obj = crate::js_array::create_array(mc, env)?;
                for (j, val) in rest_args.iter().enumerate() {
                    object_set_key_value(mc, &array_obj, j, val)?;
                }
                crate::js_array::set_array_length(mc, &array_obj, rest_args.len())?;
                crate::core::env_set(mc, &param_env, name, &Value::Object(array_obj))?;
            }
            DestructuringElement::RestPattern(inner) => {
                let rest_args = if i < args.len() { args[i..].to_vec() } else { Vec::new() };
                let array_obj = crate::js_array::create_array(mc, env)?;
                for (j, val) in rest_args.iter().enumerate() {
                    object_set_key_value(mc, &array_obj, j, val)?;
                }
                crate::js_array::set_array_length(mc, &array_obj, rest_args.len())?;
                evaluate_destructuring_element_rec(mc, &param_env, inner, &Value::Object(array_obj), false, None, None)?;
            }
            DestructuringElement::NestedArray(inner_pattern, inner_default) => {
                let mut arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                // If arg is undefined and there is a parameter-level default, evaluate it
                if matches!(arg_val, Value::Undefined)
                    && let Some(def) = inner_default
                {
                    arg_val = evaluate_expr(mc, &param_env, def)?;
                }

                evaluate_destructuring_array_assignment(mc, &param_env, inner_pattern, &arg_val, false, None, None)?;
            }
            DestructuringElement::NestedObject(inner_pattern, inner_default) => {
                let mut arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                if matches!(arg_val, Value::Undefined) || matches!(arg_val, Value::Null) {
                    if let Some(def) = inner_default {
                        arg_val = evaluate_expr(mc, &param_env, def)?;
                    } else {
                        return Err(raise_type_error!("Cannot convert undefined or null to object").into());
                    }
                }
                if matches!(arg_val, Value::Undefined) || matches!(arg_val, Value::Null) {
                    return Err(raise_type_error!("Cannot convert undefined or null to object").into());
                }
                if let Value::Object(obj) = &arg_val {
                    bind_object_inner_for_letconst(mc, &param_env, inner_pattern, obj, false)?;
                }
            }
            _ => {}
        }
    }
    let body_clone = cl.body.clone();
    // Use the lower-level evaluator to distinguish an explicit `return`
    match evaluate_statements_with_labels(mc, &var_env, &body_clone, &[], &[])? {
        ControlFlow::Return(val) => Ok(val),
        ControlFlow::Normal(_) => Ok(Value::Undefined),
        ControlFlow::Throw(v, line, column) => Err(EvalError::Throw(v, line, column)),
        ControlFlow::Break(_) => Err(raise_syntax_error!("break statement not in loop or switch").into()),
        ControlFlow::Continue(_) => Err(raise_syntax_error!("continue statement not in loop").into()),
    }
}

fn evaluate_update_expression<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    delta: f64,
    is_post: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let (old_val, new_val) = match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            // Use ToNumeric/ToNumber semantics so objects are coerced via ToPrimitive
            let prim_numeric = to_numeric_with_env(mc, env, &current)?;
            match prim_numeric {
                Value::BigInt(b) => {
                    let mut nb = (*b).clone();
                    let delta_i = delta as i64;
                    nb += BigInt::from(delta_i);
                    let new_v = Value::BigInt(Box::new(nb));
                    crate::core::env_set_recursive(mc, env, name, &new_v)?;
                    (current, new_v)
                }
                Value::Number(n) => {
                    let new_num = n + delta;
                    let new_v = Value::Number(new_num);
                    crate::core::env_set_recursive(mc, env, name, &new_v)?;
                    (current, new_v)
                }
                _ => unreachable!("to_numeric_with_env returned non-numeric"),
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                // Use get_property_with_accessors so getters are invoked for reads
                let current = get_property_with_accessors(mc, env, &obj, key)?;

                // Coerce per ToNumeric/ToNumber (objects -> ToPrimitive with hint 'number')
                let prim_numeric = to_numeric_with_env(mc, env, &current)?;
                match prim_numeric {
                    Value::BigInt(b) => {
                        let mut nb = (*b).clone();
                        let delta_i = delta as i64;
                        nb += BigInt::from(delta_i);
                        let new_v = Value::BigInt(Box::new(nb));
                        set_property_with_accessors(mc, env, &obj, key, &new_v)?;
                        (current, new_v)
                    }
                    Value::Number(n) => {
                        let new_num = n + delta;
                        let new_v = Value::Number(new_num);
                        set_property_with_accessors(mc, env, &obj, key, &new_v)?;
                        (current, new_v)
                    }
                    _ => unreachable!("to_numeric_with_env returned non-numeric"),
                }
            } else {
                return Err(raise_type_error!("Cannot update property of non-object").into());
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let k_val = evaluate_expr(mc, env, key_expr)?;
            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot update property of non-object").into());
            }
            let key = to_property_key_for_assignment(mc, env, &k_val)?;

            if let Value::Object(obj) = obj_val {
                // Use get_property_with_accessors so getters are invoked for reads
                let current = get_property_with_accessors(mc, env, &obj, &key)?;

                let prim_numeric = to_numeric_with_env(mc, env, &current)?;
                match prim_numeric {
                    Value::BigInt(b) => {
                        let mut nb = (*b).clone();
                        let delta_i = delta as i64;
                        nb += BigInt::from(delta_i);
                        let new_v = Value::BigInt(Box::new(nb));
                        set_property_with_accessors(mc, env, &obj, &key, &new_v)?;
                        (current, new_v)
                    }
                    Value::Number(n) => {
                        let new_num = n + delta;
                        let new_v = Value::Number(new_num);
                        set_property_with_accessors(mc, env, &obj, &key, &new_v)?;
                        (current, new_v)
                    }
                    _ => unreachable!("to_numeric_with_env returned non-numeric"),
                }
            } else {
                return Err(raise_type_error!("Cannot update property of non-object").into());
            }
        }
        Expr::PrivateMember(obj_expr, name) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let pv = evaluate_var(mc, env, name)?;
            if let Value::Object(obj) = obj_val {
                if let Value::PrivateName(n, id) = pv {
                    let key = PropertyKey::Private(n, id);
                    let current = get_property_with_accessors(mc, env, &obj, &key)?;
                    let prim_numeric = to_numeric_with_env(mc, env, &current)?;
                    match prim_numeric {
                        Value::BigInt(b) => {
                            let mut nb = (*b).clone();
                            let delta_i = delta as i64;
                            nb += BigInt::from(delta_i);
                            let new_v = Value::BigInt(Box::new(nb));
                            set_property_with_accessors(mc, env, &obj, &key, &new_v)?;
                            (current, new_v)
                        }
                        Value::Number(n) => {
                            let new_num = n + delta;
                            let new_v = Value::Number(new_num);
                            set_property_with_accessors(mc, env, &obj, &key, &new_v)?;
                            (current, new_v)
                        }
                        _ => unreachable!("to_numeric_with_env returned non-numeric"),
                    }
                } else {
                    return Err(raise_syntax_error!(format!("Private field '{}' must be declared in an enclosing class", name)).into());
                }
            } else {
                return Err(raise_type_error!("Cannot access private member of non-object").into());
            }
        }
        _ => return Err(raise_eval_error!("Invalid L-value in update expression").into()),
    };

    if is_post {
        // For post-increment/decrement, return the original value.
        // If it's a BigInt, return it as BigInt; otherwise return ToNumber(oldValue).
        match old_val {
            Value::BigInt(_) => Ok(old_val),
            _ => {
                let num = to_number_with_env(mc, env, &old_val)?;
                Ok(Value::Number(num))
            }
        }
    } else {
        Ok(new_val)
    }
}

// Helpers for js_object and other modules

pub fn extract_closure_from_value<'gc>(val: &Value<'gc>) -> Option<(Vec<DestructuringElement>, Vec<Statement>, JSObjectDataPtr<'gc>)> {
    match val {
        Value::Closure(cl) => {
            let data = cl;
            Some((data.params.clone(), data.body.clone(), data.env?))
        }
        Value::AsyncClosure(cl) => {
            let data = cl;
            Some((data.params.clone(), data.body.clone(), data.env?))
        }
        Value::Object(obj) => {
            if let Some(closure_prop) = obj.borrow().get_closure() {
                let closure_val = closure_prop.borrow();
                match &*closure_val {
                    Value::Closure(cl) => {
                        let data = cl;
                        Some((data.params.clone(), data.body.clone(), data.env?))
                    }
                    Value::AsyncClosure(cl) => {
                        let data = cl;
                        Some((data.params.clone(), data.body.clone(), data.env?))
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

// Convert nested array parameter like ([x]) into an Expr::Array pattern and
// delegate to the assign-target helper so GetIterator semantics are used.
fn convert_array_pattern_inner(elms: &[DestructuringElement]) -> Vec<Option<Expr>> {
    let mut out: Vec<Option<Expr>> = Vec::new();
    for e in elms.iter() {
        match e {
            DestructuringElement::Empty => out.push(None),
            DestructuringElement::Variable(name, maybe_def) => {
                if let Some(def) = maybe_def {
                    out.push(Some(Expr::Assign(
                        Box::new(Expr::Var(name.clone(), None, None)),
                        Box::new((**def).clone()),
                    )));
                } else {
                    out.push(Some(Expr::Var(name.clone(), None, None)));
                }
            }
            DestructuringElement::Rest(name) => {
                out.push(Some(Expr::Spread(Box::new(Expr::Var(name.clone(), None, None)))));
            }
            DestructuringElement::RestPattern(inner) => match &**inner {
                DestructuringElement::Variable(name, _) => {
                    out.push(Some(Expr::Spread(Box::new(Expr::Var(name.clone(), None, None)))));
                }
                DestructuringElement::NestedArray(sub, _) => {
                    let inner_arr = convert_array_pattern_inner(sub);
                    out.push(Some(Expr::Spread(Box::new(Expr::Array(inner_arr)))));
                }
                DestructuringElement::NestedObject(sub, _) => {
                    let inner_obj = convert_object_pattern_inner(sub);
                    out.push(Some(Expr::Spread(Box::new(Expr::Object(inner_obj)))));
                }
                _ => {
                    out.push(None);
                }
            },
            DestructuringElement::NestedArray(sub, maybe_def) => {
                let inner = convert_array_pattern_inner(sub);
                let mut arr_expr = Expr::Array(inner);
                if let Some(d) = maybe_def {
                    arr_expr = Expr::Assign(Box::new(arr_expr), Box::new((**d).clone()));
                }
                out.push(Some(arr_expr));
            }
            DestructuringElement::NestedObject(sub, maybe_def) => {
                let inner = convert_object_pattern_inner(sub);
                let mut obj_expr = Expr::Object(inner);
                if let Some(d) = maybe_def {
                    obj_expr = Expr::Assign(Box::new(obj_expr), Box::new((**d).clone()));
                }
                out.push(Some(obj_expr));
            }
            _ => {
                // Fallback: treat as elision for now
                out.push(None);
            }
        }
    }
    out
}

fn convert_object_pattern_inner(elms: &[DestructuringElement]) -> Vec<(Expr, Expr, bool, bool)> {
    let mut out: Vec<(Expr, Expr, bool, bool)> = Vec::new();
    for e in elms.iter() {
        match e {
            DestructuringElement::Property(key, boxed) => {
                let key_expr = Expr::StringLit(utf8_to_utf16(key));
                let val_expr = match &**boxed {
                    DestructuringElement::Variable(name, maybe_def) => {
                        let base = Expr::Var(name.clone(), None, None);
                        if let Some(def) = maybe_def {
                            Expr::Assign(Box::new(base), Box::new((**def).clone()))
                        } else {
                            base
                        }
                    }
                    DestructuringElement::NestedArray(sub, maybe_def) => {
                        let inner = convert_array_pattern_inner(sub);
                        let mut arr_expr = Expr::Array(inner);
                        if let Some(def) = maybe_def {
                            arr_expr = Expr::Assign(Box::new(arr_expr), Box::new((**def).clone()));
                        }
                        arr_expr
                    }
                    DestructuringElement::NestedObject(sub, maybe_def) => {
                        let inner = convert_object_pattern_inner(sub);
                        let mut obj_expr = Expr::Object(inner);
                        if let Some(def) = maybe_def {
                            obj_expr = Expr::Assign(Box::new(obj_expr), Box::new((**def).clone()));
                        }
                        obj_expr
                    }
                    _ => Expr::Var(String::new(), None, None),
                };
                // plain-property flag not meaningful in pattern conversion
                out.push((key_expr, val_expr, false, false));
            }
            DestructuringElement::ComputedProperty(key_expr, boxed) => {
                let val_expr = match &**boxed {
                    DestructuringElement::Variable(name, maybe_def) => {
                        let base = Expr::Var(name.clone(), None, None);
                        if let Some(def) = maybe_def {
                            Expr::Assign(Box::new(base), Box::new((**def).clone()))
                        } else {
                            base
                        }
                    }
                    DestructuringElement::NestedArray(sub, maybe_def) => {
                        let inner = convert_array_pattern_inner(sub);
                        let mut arr_expr = Expr::Array(inner);
                        if let Some(def) = maybe_def {
                            arr_expr = Expr::Assign(Box::new(arr_expr), Box::new((**def).clone()));
                        }
                        arr_expr
                    }
                    DestructuringElement::NestedObject(sub, maybe_def) => {
                        let inner = convert_object_pattern_inner(sub);
                        let mut obj_expr = Expr::Object(inner);
                        if let Some(def) = maybe_def {
                            obj_expr = Expr::Assign(Box::new(obj_expr), Box::new((**def).clone()));
                        }
                        obj_expr
                    }
                    _ => Expr::Var(String::new(), None, None),
                };
                out.push((key_expr.clone(), val_expr, false, false));
            }
            DestructuringElement::Rest(name) => {
                let target_expr = Expr::Var(name.clone(), None, None);
                out.push((Expr::Var(name.clone(), None, None), target_expr, true, false));
            }
            _ => {}
        }
    }
    out
}

fn init_function_call_env<'gc>(
    mc: &MutationContext<'gc>,
    call_env: &JSObjectDataPtr<'gc>,
    params_opt: Option<&[DestructuringElement]>,
    args: &[Value<'gc>],
) -> Result<(), EvalError<'gc>> {
    // Create the arguments object before default parameter evaluation so
    // defaults can reference `arguments` correctly.
    if crate::core::get_own_property(call_env, "arguments").is_none() {
        crate::js_class::create_arguments_object(mc, call_env, args, None)?;
    }

    if let Some(params) = params_opt {
        // Predeclare parameter bindings as Uninitialized so TDZ is enforced
        // during evaluation of default initializers.
        let mut param_names: Vec<String> = Vec::new();
        for param in params.iter() {
            collect_names_from_destructuring_element(param, &mut param_names);
        }
        for name in param_names {
            if env_get_own(call_env, &name).is_none() {
                env_set(mc, call_env, &name, &Value::Uninitialized)?;
            }
        }

        for (i, param) in params.iter().enumerate() {
            match param {
                DestructuringElement::Variable(name, default_expr) => {
                    let mut arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                    if matches!(arg_val, Value::Undefined)
                        && let Some(def) = default_expr
                    {
                        arg_val = evaluate_expr(mc, call_env, def)?;
                        maybe_set_function_name_for_default(mc, name, def, &arg_val)?;
                    }
                    env_set(mc, call_env, name, &arg_val)?;
                }
                DestructuringElement::Rest(name) => {
                    let rest_args = if i < args.len() { args[i..].to_vec() } else { Vec::new() };
                    let array_obj = crate::js_array::create_array(mc, call_env)?;
                    for (j, val) in rest_args.iter().enumerate() {
                        object_set_key_value(mc, &array_obj, j, val)?;
                    }
                    crate::js_array::set_array_length(mc, &array_obj, rest_args.len())?;
                    env_set(mc, call_env, name, &Value::Object(array_obj))?;
                }
                DestructuringElement::Property(key, boxed) => {
                    // Handle simple object destructuring param like ({ type }) => type
                    if let DestructuringElement::Variable(name, default_expr) = &**boxed {
                        let arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                        if matches!(arg_val, Value::Undefined | Value::Null) {
                            return Err(raise_type_error!("Cannot convert undefined or null to object").into());
                        }
                        let mut prop_val = match &arg_val {
                            Value::Object(o) => get_property_with_accessors(mc, call_env, o, key)?,
                            _ => get_primitive_prototype_property(mc, call_env, &arg_val, key)?,
                        };
                        if matches!(prop_val, Value::Undefined)
                            && let Some(def) = default_expr
                        {
                            prop_val = evaluate_expr(mc, call_env, def)?;
                            maybe_set_function_name_for_default(mc, name, def, &prop_val)?;
                        }
                        env_set(mc, call_env, name, &prop_val)?;
                    }
                }
                DestructuringElement::NestedArray(inner, nested_default) => {
                    let mut arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);

                    if matches!(arg_val, Value::Undefined)
                        && let Some(def) = nested_default
                    {
                        arg_val = evaluate_expr(mc, call_env, def)?;
                    }

                    evaluate_destructuring_array_assignment(mc, call_env, inner, &arg_val, false, None, None)?;
                }
                DestructuringElement::NestedObject(inner, nested_default) => {
                    // Bind object destructuring parameters like ({ type }) directly
                    let mut arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                    // If arg is null/undefined and there is no parameter-level default, throw a TypeError
                    if matches!(arg_val, Value::Undefined | Value::Null) {
                        if let Some(def) = nested_default {
                            arg_val = evaluate_expr(mc, call_env, def)?;
                        }
                        if matches!(arg_val, Value::Undefined | Value::Null) {
                            return Err(raise_type_error!("Cannot convert undefined or null to object").into());
                        }
                    }
                    let pattern = Expr::Object(convert_object_pattern_inner(inner));
                    let mut pattern_with_default = pattern;
                    if let Some(d) = nested_default {
                        pattern_with_default = Expr::Assign(Box::new(pattern_with_default), Box::new((**d).clone()));
                    }
                    evaluate_binding_target_with_value(mc, call_env, &pattern_with_default, &arg_val)?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

pub fn prepare_function_call_env<'gc>(
    mc: &MutationContext<'gc>,
    captured_env: Option<&JSObjectDataPtr<'gc>>,
    this_val: Option<&Value<'gc>>,
    params_opt: Option<&[DestructuringElement]>,
    args: &[Value<'gc>],
    _new_target: Option<&Value<'gc>>,
    _caller_env: Option<&JSObjectDataPtr<'gc>>,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    let call_env = new_js_object_data(mc);

    if let Some(c_env) = captured_env {
        call_env.borrow_mut(mc).prototype = Some(*c_env);
    }
    call_env.borrow_mut(mc).is_function_scope = true;

    if let Some(tv) = this_val {
        object_set_key_value(mc, &call_env, "this", tv)?;
    }

    init_function_call_env(mc, &call_env, params_opt, args)?;
    Ok(call_env)
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_function_call_env_with_home<'gc>(
    mc: &MutationContext<'gc>,
    captured_env: Option<&JSObjectDataPtr<'gc>>,
    this_val: Option<&Value<'gc>>,
    params_opt: Option<&[DestructuringElement]>,
    args: &[Value<'gc>],
    _new_target: Option<&Value<'gc>>,
    _caller_env: Option<&JSObjectDataPtr<'gc>>,
    home_opt: Option<GcCell<JSObjectDataPtr<'gc>>>,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    let call_env = new_js_object_data(mc);

    if let Some(c_env) = captured_env {
        call_env.borrow_mut(mc).prototype = Some(*c_env);
    }
    call_env.borrow_mut(mc).is_function_scope = true;

    if let Some(home) = home_opt {
        call_env.borrow_mut(mc).set_home_object(Some(home));
    }

    if let Some(tv) = this_val {
        object_set_key_value(mc, &call_env, "this", tv)?;
    }

    init_function_call_env(mc, &call_env, params_opt, args)?;
    Ok(call_env)
}

pub fn prepare_closure_call_env<'gc>(
    mc: &MutationContext<'gc>,
    captured_env: Option<&JSObjectDataPtr<'gc>>,
    params_opt: Option<&[DestructuringElement]>,
    args: &[Value<'gc>],
    caller_env: Option<&JSObjectDataPtr<'gc>>,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    prepare_function_call_env(mc, captured_env, None, params_opt, args, None, caller_env)
}

#[allow(dead_code)]
enum CtorRef<'a, 'gc> {
    Var(&'a str),
    Property(Value<'gc>, crate::core::PropertyKey<'gc>), // base value and key
    Index(Value<'gc>, Value<'gc>),                       // base and computed key value
    Other(Value<'gc>),
}

fn evaluate_expr_new<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    ctor: &Expr,
    args: &[Expr],
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Per ECMAScript semantics for 'new', the constructExpr evaluation yields a Reference
    // (ref) and the actual GetValue(ref) must happen *after* argument evaluation so
    // side-effects in arguments can affect the constructor value. Implement this by
    // capturing a small reference-like descriptor for common cases (Var, Property, Index)
    // and resolving the actual constructor value after evaluating args.
    let mut ctor_ref: Option<CtorRef<'_, 'gc>> = match ctor {
        Expr::Var(name, _, _) => {
            // GetValue(ref) now (before arguments evaluation)
            let val = evaluate_var(mc, env, name)?;
            Some(CtorRef::Other(val))
        }
        Expr::Property(obj_expr, key) => {
            // Special-case `import.meta` to avoid evaluating `import` which would
            // throw a ReferenceError. Use the module import.meta object as the
            // constructor value so 'new import.meta()' throws a TypeError as
            // expected by tests.
            if let crate::core::Expr::Var(name, ..) = &**obj_expr
                && name == "import"
                && key == "meta"
            {
                let import_meta_val = lookup_or_create_import_meta(mc, env)?;
                Some(CtorRef::Other(import_meta_val))
            } else {
                // evaluate base and perform GetValue(ref) now (before args)
                let base = evaluate_expr(mc, env, obj_expr)?;
                let val = match &base {
                    Value::Object(obj) => get_property_with_accessors(mc, env, obj, key)?,
                    other => get_primitive_prototype_property(mc, env, other, key)?,
                };
                Some(CtorRef::Other(val))
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            // evaluate base and key now, then GetValue(ref) now
            let base = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key = match &key_val {
                Value::Symbol(s) => PropertyKey::Symbol(*s),
                Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(s)),
                Value::Number(n) => PropertyKey::from(n.to_string()),
                _ => PropertyKey::from(crate::core::value_to_string(&key_val)),
            };
            let val = match &base {
                Value::Object(obj) => get_property_with_accessors(mc, env, obj, &key)?,
                other => get_primitive_prototype_property(mc, env, other, &key)?,
            };
            Some(CtorRef::Other(val))
        }
        _ => {
            let val = evaluate_expr(mc, env, ctor)?;
            Some(CtorRef::Other(val))
        }
    };

    let mut eval_args: Vec<Value<'gc>> = Vec::new();
    for arg in args {
        if let Expr::Spread(target) = arg {
            let val = evaluate_expr(mc, env, target)?;
            if let Value::Object(obj) = val {
                if is_array(mc, &obj) {
                    let len = object_get_length(&obj).unwrap_or(0);
                    for k in 0..len {
                        let item = object_get_key_value(&obj, k).unwrap_or(new_gc_cell_ptr(mc, Value::Undefined));
                        eval_args.push(item.borrow().clone());
                    }
                } else {
                    // Support iterable spread for constructors: if object has Symbol.iterator, iterate and push
                    let mut iter_fn_opt: Option<crate::core::GcPtr<'gc, Value<'gc>>> = None;
                    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
                        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                    {
                        // Use accessor-aware property read to support getters that return the iterator method
                        let iter_fn_val = get_property_with_accessors(mc, env, &obj, iter_sym)?;
                        // If the accessor returned undefined, there's no iterator
                        if !matches!(iter_fn_val, crate::core::Value::Undefined) {
                            // Wrap the returned Value into a GC pointer so code below can inspect/call it uniformly
                            iter_fn_opt = Some(crate::core::new_gc_cell_ptr(mc, iter_fn_val));
                        }
                    }
                    if let Some(iter_fn_val) = iter_fn_opt {
                        // Call iterator method on the object to get an iterator
                        let iterator = match &*iter_fn_val.borrow() {
                            Value::Function(name) => {
                                let call_env =
                                    prepare_call_env_with_this(mc, Some(env), Some(&Value::Object(obj)), None, &[], None, Some(env), None)?;
                                evaluate_call_dispatch(mc, &call_env, &Value::Function(name.clone()), Some(&Value::Object(obj)), &[])?
                            }
                            Value::Closure(cl) => call_closure(mc, cl, Some(&Value::Object(obj)), &[], env, None)?,
                            Value::Object(o) => {
                                // Function objects are represented as objects with an internal closure; unwrap and call if possible
                                if let Some(cl_ptr) = o.borrow().get_closure() {
                                    match &*cl_ptr.borrow() {
                                        Value::Closure(cl) => call_closure(mc, cl, Some(&Value::Object(*o)), &[], env, None)?,
                                        Value::Function(name) => {
                                            let call_env = prepare_call_env_with_this(
                                                mc,
                                                Some(env),
                                                Some(&Value::Object(*o)),
                                                None,
                                                &[],
                                                None,
                                                Some(env),
                                                None,
                                            )?;
                                            evaluate_call_dispatch(
                                                mc,
                                                &call_env,
                                                &Value::Function(name.clone()),
                                                Some(&Value::Object(*o)),
                                                &[],
                                            )?
                                        }
                                        _ => return Err(raise_type_error!("Spread target is not iterable").into()),
                                    }
                                } else {
                                    log::trace!("Spread target is not iterable: iterator property is object but not callable");
                                    return Err(raise_type_error!("Spread target is not iterable").into());
                                }
                            }
                            _ => return Err(raise_type_error!("Spread target is not iterable").into()),
                        };

                        if let Value::Object(iter_obj) = iterator {
                            loop {
                                if let Some(next_val) = object_get_key_value(&iter_obj, "next") {
                                    let next_fn = next_val.borrow().clone();
                                    let res = match &next_fn {
                                        Value::Function(name) => {
                                            let call_env = prepare_call_env_with_this(
                                                mc,
                                                Some(env),
                                                Some(&Value::Object(iter_obj)),
                                                None,
                                                &[],
                                                None,
                                                Some(env),
                                                None,
                                            )?;
                                            evaluate_call_dispatch(
                                                mc,
                                                &call_env,
                                                &Value::Function(name.clone()),
                                                Some(&Value::Object(iter_obj)),
                                                &[],
                                            )?
                                        }
                                        Value::Closure(cl) => call_closure(mc, cl, Some(&Value::Object(iter_obj)), &[], env, None)?,
                                        Value::Object(o) => {
                                            if let Some(cl_ptr) = o.borrow().get_closure() {
                                                match &*cl_ptr.borrow() {
                                                    Value::Closure(cl) => call_closure(mc, cl, Some(&Value::Object(*o)), &[], env, None)?,
                                                    Value::Function(name) => {
                                                        let call_env = prepare_call_env_with_this(
                                                            mc,
                                                            Some(env),
                                                            Some(&Value::Object(*o)),
                                                            None,
                                                            &[],
                                                            None,
                                                            Some(env),
                                                            None,
                                                        )?;
                                                        evaluate_call_dispatch(
                                                            mc,
                                                            &call_env,
                                                            &Value::Function(name.clone()),
                                                            Some(&Value::Object(*o)),
                                                            &[],
                                                        )?
                                                    }
                                                    _ => return Err(raise_type_error!("Iterator.next is not callable").into()),
                                                }
                                            } else {
                                                return Err(raise_type_error!("Iterator.next is not callable").into());
                                            }
                                        }
                                        _ => return Err(raise_type_error!("Iterator.next is not callable").into()),
                                    };

                                    if let Value::Object(res_obj) = res {
                                        // Per spec, IteratorValue uses Get(resObj, "done") and Get(resObj, "value"),
                                        // which must observe accessors (getters) and therefore may throw. Use
                                        // accessor-aware reads here so getter side-effects/exceptions propagate.
                                        let done_val = get_property_with_accessors(mc, env, &res_obj, "done")?;
                                        let done = matches!(done_val, Value::Boolean(b) if b);

                                        if done {
                                            break;
                                        }

                                        let value = get_property_with_accessors(mc, env, &res_obj, "value")?;

                                        eval_args.push(value);

                                        continue;
                                    } else {
                                        return Err(raise_type_error!("Iterator.next did not return an object").into());
                                    }
                                } else {
                                    return Err(raise_type_error!("Iterator has no next method").into());
                                }
                            }
                        } else {
                            return Err(raise_type_error!("Iterator call did not return an object").into());
                        }
                    }
                }
            } else {
                return Err(raise_type_error!("Spread only implemented for Objects").into());
            }
        } else {
            eval_args.push(evaluate_expr(mc, env, arg)?);
        }
    }

    // Resolve constructor value now (GetValue(ref)) after arguments are evaluated
    // Diagnostic: show how constructor was referenced when parsed
    if let Some(ct_ref) = ctor_ref.as_ref() {
        match ct_ref {
            CtorRef::Var(n) => log::debug!("evaluate_expr_new: ctor_ref = Var({})", n),
            CtorRef::Property(_, k) => log::debug!("evaluate_expr_new: ctor_ref = Property({:?})", k),
            CtorRef::Index(_, kv) => log::debug!("evaluate_expr_new: ctor_ref = Index({:?})", kv),
            CtorRef::Other(v) => log::debug!("evaluate_expr_new: ctor_ref = Other({:?})", v),
        }
    }
    let func_val = match ctor_ref.take().expect("ctor_ref must be set") {
        CtorRef::Var(name) => evaluate_var(mc, env, name)?,
        CtorRef::Property(base, key) => match base {
            Value::Object(obj) => get_property_with_accessors(mc, env, &obj, &key)?,
            other => get_primitive_prototype_property(mc, env, &other, &key)?,
        },
        CtorRef::Index(base, key_v) => {
            let key = match &key_v {
                Value::Symbol(s) => PropertyKey::Symbol(*s),
                Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(s)),
                Value::Number(n) => PropertyKey::from(n.to_string()),
                _ => PropertyKey::from(crate::core::value_to_string(&key_v)),
            };
            match base {
                Value::Object(obj) => get_property_with_accessors(mc, env, &obj, &key)?,
                other => get_primitive_prototype_property(mc, env, &other, &key)?,
            }
        }
        CtorRef::Other(v) => v,
    };

    // Diagnostic: log resolved constructor value
    log::debug!("evaluate_expr_new - resolved constructor value = {:?}", func_val);

    // If property descriptor was returned (Value::Property), unwrap its value per GetValue semantics
    let func_val = match func_val {
        Value::Property { value: Some(v), .. } => {
            log::debug!(
                "evaluate_expr_new: unwrapping Property descriptor to inner value = {:?}",
                v.borrow()
            );
            v.borrow().clone()
        }
        other => other,
    };

    match func_val {
        Value::Closure(cl) => {
            // Closure used directly as constructor (e.g., `new f();` where `f` is a closure)
            if cl.is_arrow {
                return Err(raise_type_error!("Not a constructor").into());
            }

            // Create instance
            let instance = crate::core::new_js_object_data(mc);
            // Attempt to set prototype from an associated function object if present (none for raw closures), otherwise fallback to Object.prototype
            if let Some(obj_val) = env_get(env, "Object")
                && let Value::Object(obj_ctor) = &*obj_val.borrow()
                && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
                && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
            {
                instance.borrow_mut(mc).prototype = Some(*obj_proto);
            }

            let call_env = crate::core::new_js_object_data(mc);
            call_env.borrow_mut(mc).prototype = cl.env;
            call_env.borrow_mut(mc).is_function_scope = true;
            object_set_key_value(mc, &call_env, "this", &Value::Object(instance))?;

            // Mark constructor call
            crate::core::object_set_key_value(mc, &call_env, "__instance", &Value::Object(instance))?;
            crate::core::object_set_key_value(mc, &call_env, "__function", &Value::Closure(cl))?;

            for (i, param) in cl.params.iter().enumerate() {
                match param {
                    DestructuringElement::Variable(name, _) => {
                        let arg_val = eval_args.get(i).cloned().unwrap_or(Value::Undefined);
                        env_set(mc, &call_env, name, &arg_val)?;
                    }
                    DestructuringElement::Rest(name) => {
                        let rest_args = if i < eval_args.len() { eval_args[i..].to_vec() } else { Vec::new() };
                        let array_obj = crate::js_array::create_array(mc, env)?;
                        for (j, val) in rest_args.iter().enumerate() {
                            object_set_key_value(mc, &array_obj, j, val)?;
                        }
                        object_set_length(mc, &array_obj, rest_args.len())?;
                        env_set(mc, &call_env, name, &Value::Object(array_obj))?;
                    }
                    _ => {}
                }
            }

            crate::js_class::create_arguments_object(mc, &call_env, &eval_args, None)?;

            let body_clone = cl.body.clone();
            match evaluate_statements_with_labels(mc, &call_env, &body_clone, &[], &[])? {
                ControlFlow::Return(Value::Object(obj)) => Ok(Value::Object(obj)),
                ControlFlow::Throw(val, line, col) => Err(EvalError::Throw(val, line, col)),
                _ => Ok(Value::Object(instance)),
            }
        }
        Value::Object(obj) => {
            if let Some(cl_ptr) = obj.borrow().get_closure() {
                log::debug!("evaluate_expr_new: constructor object has closure ptr = {:p}", Gc::as_ptr(obj));
                match &*cl_ptr.borrow() {
                    Value::Closure(cl) => {
                        // Diagnostic: log both the function object's [[HomeObject]] and the
                        // closure's stored home object so we can see which is present.
                        log::debug!(
                            "evaluate_expr_new: constructor.fn_obj.home_object = {:?}",
                            obj.borrow().get_home_object().map(|h| Gc::as_ptr(*h.borrow()))
                        );
                        log::debug!(
                            "evaluate_expr_new: constructor.closure.home_object = {:?}",
                            cl.home_object.as_ref().map(|h| Gc::as_ptr(*h.borrow()))
                        );

                        // If this closure or its function object has a [[HomeObject]] it was
                        // created as a method and per ECMAScript it is not a constructor.
                        if obj.borrow().get_home_object().is_some() || cl.home_object.is_some() {
                            return Err(raise_type_error!("Not a constructor").into());
                        }
                        log::debug!("evaluate_expr_new: found closure; is_arrow={} params={:?}", cl.is_arrow, cl.params);
                        // Arrow functions are not constructors per ECMAScript; throw TypeError
                        if cl.is_arrow {
                            return Err(raise_type_error!("Not a constructor").into());
                        }

                        // 1. Create instance
                        let instance = crate::core::new_js_object_data(mc);

                        // 2. Set prototype
                        if let Some(proto_val) = object_get_key_value(&obj, "prototype") {
                            // Debug: log the raw 'prototype' slot value we found on the constructor
                            log::debug!("evaluate_expr_new: raw prototype slot = {:?}", proto_val.borrow());
                            // Also log the constructor object's function/proto pointers
                            log::debug!("evaluate_expr_new: constructor obj ptr = {:p}", Gc::as_ptr(obj));

                            // Handle the case where 'prototype' is stored as a property descriptor
                            // (Value::Property { value: Some(v), .. }) or as a direct Object value.
                            let proto_value = match &*proto_val.borrow() {
                                Value::Property { value: Some(v), .. } => v.borrow().clone(),
                                other => other.clone(),
                            };
                            log::debug!("evaluate_expr_new: resolved prototype value = {:?}", proto_value);

                            if let Value::Object(proto_obj) = proto_value {
                                instance.borrow_mut(mc).prototype = Some(proto_obj);
                                object_set_key_value(mc, &instance, "__proto__", &Value::Object(proto_obj))?;
                                log::debug!("evaluate_expr_new: instance prototype set to {:p}", Gc::as_ptr(proto_obj));
                            } else {
                                // Fallback to Object.prototype
                                if let Some(obj_val) = env_get(env, "Object")
                                    && let Value::Object(obj_ctor) = &*obj_val.borrow()
                                    && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
                                    && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
                                {
                                    instance.borrow_mut(mc).prototype = Some(*obj_proto);
                                    log::debug!(
                                        "evaluate_expr_new: instance prototype fallback to Object.prototype {:p}",
                                        Gc::as_ptr(*obj_proto)
                                    );
                                }
                            }
                        }

                        let call_env = crate::core::new_js_object_data(mc);
                        call_env.borrow_mut(mc).prototype = cl.env;
                        call_env.borrow_mut(mc).is_function_scope = true;
                        object_set_key_value(mc, &call_env, "this", &Value::Object(instance))?;

                        // Ensure constructor call environment is marked as a constructor call
                        // so runtime `new.target` can observe the instance and function.
                        object_set_key_value(mc, &call_env, "__instance", &Value::Object(instance))?;
                        // Store the function *object* (not just the closure data) so `new.target` === the original function object
                        object_set_key_value(mc, &call_env, "__function", &Value::Object(obj))?;

                        for (i, param) in cl.params.iter().enumerate() {
                            match param {
                                DestructuringElement::Variable(name, _) => {
                                    let arg_val = eval_args.get(i).cloned().unwrap_or(Value::Undefined);
                                    env_set(mc, &call_env, name, &arg_val)?;
                                }
                                DestructuringElement::Rest(name) => {
                                    let rest_args = if i < eval_args.len() { eval_args[i..].to_vec() } else { Vec::new() };
                                    let array_obj = crate::js_array::create_array(mc, env)?;
                                    for (j, val) in rest_args.iter().enumerate() {
                                        object_set_key_value(mc, &array_obj, j, val)?;
                                    }
                                    object_set_length(mc, &array_obj, rest_args.len())?;
                                    env_set(mc, &call_env, name, &Value::Object(array_obj))?;
                                }
                                _ => {}
                            }
                        }

                        crate::js_class::create_arguments_object(mc, &call_env, &eval_args, None)?;

                        let body_clone = cl.body.clone();
                        match evaluate_statements_with_labels(mc, &call_env, &body_clone, &[], &[])? {
                            ControlFlow::Return(Value::Object(obj)) => Ok(Value::Object(obj)),
                            ControlFlow::Throw(val, line, col) => Err(EvalError::Throw(val, line, col)),
                            _ => Ok(Value::Object(instance)),
                        }
                    }
                    _ => Err(raise_type_error!("Not a constructor").into()),
                }
            } else if obj.borrow().class_def.is_some() {
                // Delegate to js_class::evaluate_new
                let val = crate::js_class::evaluate_new(mc, env, &func_val, &eval_args, None)?;
                Ok(val)
            } else {
                if let Some(native_name) = object_get_key_value(&obj, "__native_ctor")
                    && let Value::String(name) = &*native_name.borrow()
                {
                    let name_str = crate::unicode::utf16_to_utf8(name);
                    if matches!(
                        name_str.as_str(),
                        "Error" | "ReferenceError" | "TypeError" | "RangeError" | "SyntaxError" | "EvalError" | "URIError"
                    ) {
                        let msg = eval_args.first().cloned().unwrap_or(Value::Undefined);
                        let prototype = if let Some(proto_val) = object_get_key_value(&obj, "prototype")
                            && let Value::Object(proto_obj) = &*proto_val.borrow()
                        {
                            Some(*proto_obj)
                        } else {
                            None
                        };

                        let err_val = crate::core::js_error::create_error(mc, prototype, msg)?;
                        if let Value::Object(err_obj) = &err_val {
                            object_set_key_value(mc, err_obj, "name", &Value::String(name.clone()))?;
                        }
                        return Ok(err_val);
                    } else if name_str == "Object" {
                        return crate::js_class::handle_object_constructor(mc, &eval_args, env);
                    } else if name_str == "Promise" {
                        return crate::js_promise::handle_promise_constructor_val(mc, &eval_args, env);
                    } else if name_str == "String" {
                        let val = match crate::js_string::string_constructor(mc, &eval_args, env)? {
                            Value::String(s) => s,
                            _ => Vec::new(),
                        };
                        let new_obj = crate::core::new_js_object_data(mc);

                        object_set_key_value(mc, &new_obj, "__value__", &Value::String(val.clone()))?;

                        if let Some(proto_val) = object_get_key_value(&obj, "prototype")
                            && let Value::Object(proto_obj) = &*proto_val.borrow()
                        {
                            new_obj.borrow_mut(mc).prototype = Some(*proto_obj);
                        }

                        let val = Value::Number(crate::unicode::utf16_len(&val) as f64);
                        object_set_key_value(mc, &new_obj, "length", &val)?;
                        return Ok(Value::Object(new_obj));
                    } else if name_str == "Boolean" {
                        let val = match crate::js_boolean::boolean_constructor(&eval_args)? {
                            Value::Boolean(b) => b,
                            _ => false,
                        };
                        let new_obj = crate::core::new_js_object_data(mc);
                        object_set_key_value(mc, &new_obj, "__value__", &Value::Boolean(val))?;

                        if let Some(proto_val) = object_get_key_value(&obj, "prototype")
                            && let Value::Object(proto_obj) = &*proto_val.borrow()
                        {
                            new_obj.borrow_mut(mc).prototype = Some(*proto_obj);
                        }

                        return Ok(Value::Object(new_obj));
                    } else if name_str == "Number" {
                        let val = match number_constructor(mc, &eval_args, env)? {
                            Value::Number(n) => n,
                            _ => f64::NAN,
                        };
                        let new_obj = crate::core::new_js_object_data(mc);
                        object_set_key_value(mc, &new_obj, "__value__", &Value::Number(val))?;

                        if let Some(proto_val) = object_get_key_value(&obj, "prototype")
                            && let Value::Object(proto_obj) = &*proto_val.borrow()
                        {
                            new_obj.borrow_mut(mc).prototype = Some(*proto_obj);
                        }

                        return Ok(Value::Object(new_obj));
                    } else if name_str == "Date" {
                        return crate::js_date::handle_date_constructor(mc, &eval_args, env);
                    } else if name_str == "Array" {
                        return crate::js_array::handle_array_constructor(mc, &eval_args, env);
                    } else if name_str == "RegExp" {
                        return crate::js_regexp::handle_regexp_constructor(mc, &eval_args);
                    } else if name_str == "Map" {
                        return Ok(crate::js_map::handle_map_constructor(mc, &eval_args, env)?);
                    } else if name_str == "Proxy" {
                        return crate::js_proxy::handle_proxy_constructor(mc, &eval_args, env);
                    } else if name_str == "WeakMap" {
                        return Ok(crate::js_weakmap::handle_weakmap_constructor(mc, &eval_args, env)?);
                    } else if name_str == "WeakSet" {
                        return Ok(crate::js_weakset::handle_weakset_constructor(mc, &eval_args, env)?);
                    } else if name_str == "Set" {
                        return Ok(crate::js_set::handle_set_constructor(mc, &eval_args, env)?);
                    } else if name_str == "ArrayBuffer" {
                        return Ok(crate::js_typedarray::handle_arraybuffer_constructor(mc, &eval_args, env)?);
                    } else if name_str == "SharedArrayBuffer" {
                        return Ok(crate::js_typedarray::handle_sharedarraybuffer_constructor(mc, &eval_args, env)?);
                    } else if name_str == "DataView" {
                        return Ok(crate::js_typedarray::handle_dataview_constructor(mc, &eval_args, env)?);
                    } else if name_str == "TypedArray" {
                        return Ok(crate::js_typedarray::handle_typedarray_constructor(mc, &obj, &eval_args, env)?);
                    } else if name_str == "Function" {
                        return crate::js_function::handle_global_function(mc, "Function", &eval_args, env);
                    } else if name_str == "Symbol" {
                        return Err(raise_type_error!("Symbol is not a constructor").into());
                    }
                }
                // If we've reached here, the target object is not a recognized constructor
                // (no internal closure, no __class_def__, and no native constructor handled above).
                // Per ECMAScript, attempting `new` with a non-constructor should throw a TypeError.
                Err(raise_type_error!("Not a constructor").into())
            }
        }
        _ => Err(raise_type_error!("Not a constructor").into()),
    }
}

fn evaluate_expr_object<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    properties: &[(Expr, Expr, bool, bool)],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = crate::core::new_js_object_data(mc);
    if let Some(obj_val) = env_get(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        obj.borrow_mut(mc).prototype = Some(*proto);
    }

    for (key_expr, val_expr, is_computed, is_plain_property) in properties {
        if let Expr::Spread(target) = val_expr {
            let val = evaluate_expr(mc, env, target)?;
            if let Value::Object(source_obj) = val {
                let ordered = crate::core::ordinary_own_property_keys_mc(mc, &source_obj)?;
                // If this object is a proxy wrapper, delegate descriptor/get to proxy traps
                // so that Proxy traps are observed (ownKeys is already delegated above).
                if let Some(proxy_cell) = source_obj.borrow().properties.get(&PropertyKey::String("__proxy__".to_string()))
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    for k in ordered {
                        if k == "__proto__".into() {
                            continue;
                        }
                        println!(
                            "TRACE: object-literal spread proxy: obj_ptr={:p} proxy_ptr={:p} key={:?}",
                            source_obj.as_ptr(),
                            Gc::as_ptr(*proxy),
                            k
                        );
                        // Ask proxy for own property descriptor and check [[Enumerable]]
                        let desc_enum_opt = crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &k)?;
                        println!(
                            "TRACE: object-literal spread proxy: desc_enum_opt={:?} for key={:?}",
                            desc_enum_opt, k
                        );
                        if desc_enum_opt.is_none() {
                            continue;
                        }
                        if !desc_enum_opt.unwrap() {
                            continue;
                        }
                        // Get property value via proxy get trap
                        let val_opt = crate::js_proxy::proxy_get_property(mc, proxy, &k)?;
                        println!("TRACE: object-literal spread proxy: got val_opt={:?} for key={:?}", val_opt, k);
                        let v = val_opt.unwrap_or(Value::Undefined);
                        object_set_key_value(mc, &obj, &k, &v)?;
                    }
                } else {
                    for k in ordered {
                        if k == "__proto__".into() {
                            continue;
                        }
                        // Only process enumerable own properties (both string and symbol)
                        if !source_obj.borrow().is_enumerable(&k) {
                            continue;
                        }
                        // Use accessor-aware property lookup so getters are executed
                        let v = get_property_with_accessors(mc, env, &source_obj, &k)?;
                        object_set_key_value(mc, &obj, &k, &v)?;
                    }
                }
            }
            continue;
        }

        let key_val = evaluate_expr(mc, env, key_expr)?;

        // Apply ToPropertyKey semantics (ToPrimitive(hint='string')) before
        // evaluating the value expression. This ordering is required by the
        // spec: ToPropertyKey is performed before evaluating the value.

        let key_prim = if let Value::Object(_) = &key_val {
            crate::core::to_primitive(mc, &key_val, "string", env)?
        } else {
            key_val.clone()
        };

        let mut val = evaluate_expr(mc, env, val_expr)?;

        // If this value is a Closure/AsyncClosure/GeneratorFunction that is a method
        // defined on an object literal, set its [[HomeObject]] so that `super` works.
        // If the evaluated value is a closure-like function, record its [[HomeObject]]
        // on the inner closure data so runtime checks (which may inspect the
        // closure directly) can detect method-created functions.
        // If this evaluated value is a closure-like function created as part of
        // an object literal method, create a new closure value that records the
        // `[[HomeObject]]` so later constructor checks can observe it.
        if let Value::Closure(cl) = &val {
            let new_cl = crate::core::ClosureData {
                params: cl.params.clone(),
                body: cl.body.clone(),
                env: cl.env,
                home_object: Some(GcCell::new(obj)),
                captured_envs: cl.captured_envs.clone(),
                bound_this: cl.bound_this.clone(),
                is_arrow: cl.is_arrow,
                is_strict: cl.is_strict,
                native_target: cl.native_target.clone(),
                enforce_strictness_inheritance: cl.enforce_strictness_inheritance,
            };
            val = Value::Closure(Gc::new(mc, new_cl));
        }
        if let Value::AsyncClosure(cl) = &val {
            let new_cl = crate::core::ClosureData {
                params: cl.params.clone(),
                body: cl.body.clone(),
                env: cl.env,
                home_object: Some(GcCell::new(obj)),
                captured_envs: cl.captured_envs.clone(),
                bound_this: cl.bound_this.clone(),
                is_arrow: cl.is_arrow,
                is_strict: cl.is_strict,
                native_target: cl.native_target.clone(),
                enforce_strictness_inheritance: cl.enforce_strictness_inheritance,
            };
            val = Value::AsyncClosure(Gc::new(mc, new_cl));
        }
        if let Value::GeneratorFunction(name_opt, cl) = &val {
            let new_cl = crate::core::ClosureData {
                params: cl.params.clone(),
                body: cl.body.clone(),
                env: cl.env,
                home_object: Some(GcCell::new(obj)),
                captured_envs: cl.captured_envs.clone(),
                bound_this: cl.bound_this.clone(),
                is_arrow: cl.is_arrow,
                is_strict: cl.is_strict,
                native_target: cl.native_target.clone(),
                enforce_strictness_inheritance: cl.enforce_strictness_inheritance,
            };
            val = Value::GeneratorFunction(name_opt.clone(), Gc::new(mc, new_cl));
        }
        if let Value::AsyncGeneratorFunction(name_opt, cl) = &val {
            let new_cl = crate::core::ClosureData {
                params: cl.params.clone(),
                body: cl.body.clone(),
                env: cl.env,
                home_object: Some(GcCell::new(obj)),
                captured_envs: cl.captured_envs.clone(),
                bound_this: cl.bound_this.clone(),
                is_arrow: cl.is_arrow,
                is_strict: cl.is_strict,
                native_target: cl.native_target.clone(),
                enforce_strictness_inheritance: cl.enforce_strictness_inheritance,
            };
            val = Value::AsyncGeneratorFunction(name_opt.clone(), Gc::new(mc, new_cl));
        }

        match &mut val {
            Value::Closure(_cl) => {
                // Wrap Closure in a function object and attach internal closure so we can set home object field
                // and have consistent runtime semantics with other function values.
                let func_obj = crate::core::new_js_object_data(mc);
                let closure_val = val.clone();
                func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
                // Attach the home object so `super` resolves to the object's prototype
                func_obj.borrow_mut(mc).set_home_object(Some(GcCell::new(obj)));
                // Replace the original value with the function object wrapper
                val = Value::Object(func_obj);
            }
            Value::AsyncClosure(_) => {
                // handled via function object home object setting
            }
            Value::GeneratorFunction(_, _) => {
                // Wrap generator function in an object wrapper so it has the usual
                // function-object semantics (including an own 'prototype' property
                // whose internal prototype points to Generator.prototype).
                let gen_val = val.clone();
                let func_obj = crate::core::new_js_object_data(mc);
                // Set internal prototype to Function.prototype when available
                if let Some(func_ctor_val) = env_get(env, "Function")
                    && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                    && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                    && let Value::Object(proto) = &*proto_val.borrow()
                {
                    func_obj.borrow_mut(mc).prototype = Some(*proto);
                }
                func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, gen_val)));
                // Attach the home object so `super` resolves to the object's prototype
                func_obj.borrow_mut(mc).set_home_object(Some(GcCell::new(obj)));

                // Create prototype object with [[Prototype]] -> Generator.prototype
                let proto_obj = crate::core::new_js_object_data(mc);
                if let Some(gen_ctor_val) = env_get(env, "Generator")
                    && let Value::Object(gen_ctor) = &*gen_ctor_val.borrow()
                    && let Some(gen_proto_val) = object_get_key_value(gen_ctor, "prototype")
                {
                    let proto_value = match &*gen_proto_val.borrow() {
                        Value::Property { value: Some(v), .. } => v.borrow().clone(),
                        other => other.clone(),
                    };
                    if let Value::Object(gen_proto) = proto_value {
                        proto_obj.borrow_mut(mc).prototype = Some(gen_proto);
                    }
                }
                let proto_desc = crate::core::create_descriptor_object(mc, &Value::Object(proto_obj), true, false, false)?;
                crate::js_object::define_property_internal(mc, &func_obj, "prototype", &proto_desc)?;

                // DEBUG: report pointers for generator prototype linkage
                if let Some(gen_ctor_val) = env_get(env, "Generator")
                    && let Value::Object(gen_ctor) = &*gen_ctor_val.borrow()
                    && let Some(gen_proto_val) = object_get_key_value(gen_ctor, "prototype")
                {
                    let proto_value = match &*gen_proto_val.borrow() {
                        Value::Property { value: Some(v), .. } => v.borrow().clone(),
                        other => other.clone(),
                    };
                    if let Value::Object(gen_proto) = proto_value {
                        log::debug!(
                            "DBG: created method.func_obj={:p} .prototype -> gen_proto={:p}",
                            Gc::as_ptr(func_obj),
                            Gc::as_ptr(gen_proto)
                        );
                    }
                }

                val = Value::Object(func_obj);
            }

            // For getter/setter variants, wrap them with home object so super resolves
            Value::Getter(body, captured_env, _) => {
                val = Value::Getter(body.clone(), *captured_env, Some(GcCell::new(obj)));
            }
            Value::Setter(params, body, captured_env, _) => {
                val = Value::Setter(params.clone(), body.clone(), *captured_env, Some(GcCell::new(obj)));
            }
            _ => {}
        }

        let key_v = match key_prim {
            Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
            Value::Number(n) => PropertyKey::String(value_to_string(&Value::Number(n))),
            Value::Boolean(b) => PropertyKey::String(b.to_string()),
            Value::BigInt(b) => PropertyKey::String(b.to_string()),
            Value::Undefined => PropertyKey::String("undefined".to_string()),
            Value::Null => PropertyKey::String("null".to_string()),
            Value::Symbol(s) => PropertyKey::Symbol(s),
            other => PropertyKey::String(value_to_string(&other)),
        };

        // Special-case: non-computed `__proto__` property in object initializers
        // sets the internal [[Prototype]] of the created object when the value
        // is an object or null. Computed `['__proto__']` should create an own
        // property instead. See Annex B.3.1 semantics.
        if let PropertyKey::String(ref ks) = key_v
            && ks == "__proto__"
            && !is_computed
            && *is_plain_property
        {
            match &val {
                Value::Object(o) => {
                    obj.borrow_mut(mc).prototype = Some(*o);
                }
                Value::Null => {
                    obj.borrow_mut(mc).prototype = None;
                }
                _ => {
                    // if not object or null, per spec do not set prototype
                }
            }
            // Do not create an own property for this special-case
            continue;
        }

        // If the value is a function object (holds a internal closure), set home object field
        // on the function object so it can propagate [[HomeObject]] during calls
        if let Value::Object(func_obj) = &val {
            // Determine whether this object is a function-like value (closure-based
            // functions) or a class constructor (has internal class_def). We must
            // support SetFunctionName for both function and class values created
            // as object literal property initializers.
            let is_closure = func_obj.borrow().get_closure().is_some();
            let is_class_ctor = func_obj.borrow().class_def.is_some();

            // If this is a closure-based function, propagate home object into the
            // wrapper and inner closure so that `super` resolves correctly.
            if is_closure {
                // Set [[HomeObject]] on the function object wrapper so `super` resolves
                // from methods defined on object literals.
                func_obj.borrow_mut(mc).set_home_object(Some(GcCell::new(obj)));

                // Also propagate the home object into the underlying Closure data so
                // calls that execute the closure directly (e.g., generator continuations)
                // can still resolve `super` by walking the environment chain.
                // Read closure value without holding a long-lived borrow on the function object
                let cl_val_opt = {
                    let f = func_obj.borrow();
                    f.get_closure().map(|p| p.borrow().clone())
                };
                if let Some(cl_val) = cl_val_opt {
                    // Replace the closure value with a copy that has the home_object set
                    // so that both the function wrapper and the inner closure agree.
                    let new_cl_val = match cl_val {
                        Value::Closure(data) => {
                            let mut new_data = (*data).clone();
                            new_data.home_object = Some(GcCell::new(obj));
                            Value::Closure(Gc::new(mc, new_data))
                        }
                        Value::AsyncClosure(data) => {
                            let mut new_data = (*data).clone();
                            new_data.home_object = Some(GcCell::new(obj));
                            Value::AsyncClosure(Gc::new(mc, new_data))
                        }
                        Value::GeneratorFunction(name, data) => {
                            let mut new_data = (*data).clone();
                            new_data.home_object = Some(GcCell::new(obj));
                            Value::GeneratorFunction(name.clone(), Gc::new(mc, new_data))
                        }
                        Value::AsyncGeneratorFunction(name, data) => {
                            let mut new_data = (*data).clone();
                            new_data.home_object = Some(GcCell::new(obj));
                            Value::AsyncGeneratorFunction(name.clone(), Gc::new(mc, new_data))
                        }
                        other => other,
                    };
                    func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, new_cl_val)));
                }
                if !is_plain_property {
                    // Methods defined as object literal properties typically do not
                    // create an own 'prototype' property. However, per the
                    // specification generator and async generator methods *do*
                    // require a 'prototype' so that instances' [[Prototype]] can
                    // inherit from the intrinsic Generator.prototype.
                    // Therefore only remove the 'prototype' property for ordinary
                    // closures (non-generator).
                    let mut keep_prototype = false;
                    {
                        let f = func_obj.borrow();
                        if let Some(cl_val) = f.get_closure() {
                            let inner = cl_val.borrow().clone();
                            match inner {
                                Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => {
                                    keep_prototype = true;
                                }
                                _ => {}
                            }
                        }
                    }
                    if !keep_prototype {
                        let key = PropertyKey::String("prototype".to_string());
                        let _ = func_obj.borrow_mut(mc).properties.shift_remove(&key);
                    }
                }
            }

            // Per spec: for anonymous function/class definitions produced by object literal
            // property initializers, if the function/class object does not have an own
            // 'name' property **or** that own property resolves to the empty string,
            // perform SetFunctionName so the name reflects the property key.
            if is_closure || is_class_ctor {
                // Inspect the existing name property (if any) without holding long-lived borrows
                let existing_name_opt = {
                    let f = func_obj.borrow();
                    f.properties
                        .get(&PropertyKey::String("name".to_string()))
                        .map(|p| p.borrow().clone())
                };

                let mut should_set_name = false;
                if let Some(existing) = existing_name_opt {
                    // Normalize stored value to a Value and convert to a string
                    let existing_val = match existing {
                        Value::Property { value: Some(v), .. } => v.borrow().clone(),
                        other => other.clone(),
                    };
                    let existing_str = crate::core::value_to_string(&existing_val);
                    if existing_str.is_empty() {
                        should_set_name = true;
                    }
                } else {
                    should_set_name = true;
                }

                // Only infer a name when the original value expression is itself a
                // function/class definition (i.e., not an arbitrary expression that
                // evaluates to a function, such as a comma expression `(0, function(){})`).
                let value_expr_is_anonymous_definition = matches!(
                    val_expr,
                    Expr::Function(..)
                        | Expr::ArrowFunction(..)
                        | Expr::GeneratorFunction(..)
                        | Expr::AsyncFunction(..)
                        | Expr::AsyncGeneratorFunction(..)
                        | Expr::AsyncArrowFunction(..)
                        | Expr::Class(..)
                );

                if should_set_name && value_expr_is_anonymous_definition {
                    let prop_name = match &key_v {
                        PropertyKey::String(s) => s.clone(),
                        PropertyKey::Symbol(sd) => {
                            if let Some(desc) = sd.description() {
                                format!("[{}]", desc)
                            } else {
                                String::new()
                            }
                        }
                        PropertyKey::Private(s, _) => s.clone(),
                    };
                    log::debug!(
                        "SetFunctionName: func_obj={:p} key={:?} should_set_name={}",
                        Gc::as_ptr(*func_obj),
                        &key_v,
                        should_set_name
                    );
                    let desc = create_descriptor_object(mc, &Value::String(utf8_to_utf16(&prop_name)), false, false, true)?;
                    let _ = crate::js_object::define_property_internal(mc, func_obj, "name", &desc);
                }
            }
        }

        // Merge accessors if existing property is a getter or setter; otherwise set normally
        if let Some(existing_ptr) = object_get_key_value(&obj, &key_v) {
            let existing = existing_ptr.borrow().clone();
            let mut new_val = val.clone();
            // If this is a concise method (not a plain property), propagate the object's
            // [[HomeObject]] into any closure value so `super` can be resolved at
            // runtime even for generator continuations.
            if !is_plain_property {
                new_val = match new_val {
                    Value::Closure(data) => {
                        let mut nd = (*data).clone();
                        nd.home_object = Some(GcCell::new(obj));
                        Value::Closure(Gc::new(mc, nd))
                    }
                    Value::AsyncClosure(data) => {
                        let mut nd = (*data).clone();
                        nd.home_object = Some(GcCell::new(obj));
                        Value::AsyncClosure(Gc::new(mc, nd))
                    }
                    Value::GeneratorFunction(name, data) => {
                        let mut nd = (*data).clone();
                        nd.home_object = Some(GcCell::new(obj));
                        Value::GeneratorFunction(name.clone(), Gc::new(mc, nd))
                    }
                    Value::AsyncGeneratorFunction(name, data) => {
                        let mut nd = (*data).clone();
                        nd.home_object = Some(GcCell::new(obj));
                        Value::AsyncGeneratorFunction(name.clone(), Gc::new(mc, nd))
                    }
                    Value::Object(func_obj) => {
                        // Ensure wrapper object also gets [[HomeObject]] and propagate into inner closure
                        let exists = func_obj.borrow().get_closure().is_some();
                        if exists {
                            func_obj.borrow_mut(mc).set_home_object(Some(GcCell::new(obj)));
                            // Replace inner closure with one that has home_object set
                            let cl_val_opt = {
                                let f = func_obj.borrow();
                                f.get_closure().map(|p| p.borrow().clone())
                            };
                            if let Some(cl_val) = cl_val_opt {
                                let new_cl = match cl_val {
                                    Value::Closure(d) => {
                                        let mut nd = (*d).clone();
                                        nd.home_object = Some(GcCell::new(obj));
                                        Value::Closure(Gc::new(mc, nd))
                                    }
                                    Value::AsyncClosure(d) => {
                                        let mut nd = (*d).clone();
                                        nd.home_object = Some(GcCell::new(obj));
                                        Value::AsyncClosure(Gc::new(mc, nd))
                                    }
                                    Value::GeneratorFunction(nm, d) => {
                                        let mut nd = (*d).clone();
                                        nd.home_object = Some(GcCell::new(obj));
                                        Value::GeneratorFunction(nm.clone(), Gc::new(mc, nd))
                                    }
                                    Value::AsyncGeneratorFunction(nm, d) => {
                                        let mut nd = (*d).clone();
                                        nd.home_object = Some(GcCell::new(obj));
                                        Value::AsyncGeneratorFunction(nm.clone(), Gc::new(mc, nd))
                                    }
                                    other => other,
                                };
                                func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, new_cl)));
                            }
                        }
                        Value::Object(func_obj)
                    }
                    other => other,
                };
            }

            match (existing, new_val) {
                // If existing is a Property descriptor, merge appropriately
                (
                    Value::Property {
                        value: existing_value,
                        getter: existing_getter,
                        setter: existing_setter,
                    },
                    new_val,
                ) => {
                    let (mut getter_opt, mut setter_opt, mut value_opt) = (existing_getter, existing_setter, existing_value);
                    match new_val {
                        Value::Getter(_, _, _) => getter_opt = Some(Box::new(new_val)),
                        Value::Setter(_, _, _, _) => setter_opt = Some(Box::new(new_val)),
                        other => {
                            // Replace data value
                            let new_ptr = new_gc_cell_ptr(mc, other);
                            value_opt = Some(new_ptr);
                        }
                    }
                    let prop_descriptor = Value::Property {
                        value: value_opt,
                        getter: getter_opt,
                        setter: setter_opt,
                    };
                    {
                        let stored_val = &prop_descriptor;
                        let obj_ptr = Gc::as_ptr(obj) as *const _;
                        let stored_obj_ptr = if let Value::Object(o) = stored_val {
                            Gc::as_ptr(*o) as *const _
                        } else {
                            std::ptr::null()
                        };
                        log::debug!(
                            "DBG object-literal insert: obj={:p} key={:?} storing_val={:?} stored_obj_ptr={:p}",
                            obj_ptr,
                            &key_v,
                            stored_val,
                            stored_obj_ptr
                        );
                        object_set_key_value(mc, &obj, &key_v, &prop_descriptor)?;
                    }
                }
                // If existing is a Getter/Setter, create a Property descriptor
                (Value::Getter(_, _, _), Value::Getter(_, _, _))
                | (Value::Getter(_, _, _), Value::Setter(_, _, _, _))
                | (Value::Setter(_, _, _, _), Value::Getter(_, _, _))
                | (Value::Setter(_, _, _, _), Value::Setter(_, _, _, _)) => {
                    // Extract existing components
                    let (mut getter_opt, mut setter_opt) = (None::<Box<Value<'gc>>>, None::<Box<Value<'gc>>>);
                    if let Value::Getter(_, _, _) = object_get_key_value(&obj, &key_v).unwrap().borrow().clone() {
                        getter_opt = Some(Box::new(object_get_key_value(&obj, &key_v).unwrap().borrow().clone()));
                    }
                    if let Value::Setter(_, _, _, _) = object_get_key_value(&obj, &key_v).unwrap().borrow().clone() {
                        setter_opt = Some(Box::new(object_get_key_value(&obj, &key_v).unwrap().borrow().clone()));
                    }
                    // Incorporate new value
                    match val {
                        Value::Getter(_, _, _) => getter_opt = Some(Box::new(val)),
                        Value::Setter(_, _, _, _) => setter_opt = Some(Box::new(val)),
                        _ => {}
                    }
                    let prop_descriptor = Value::Property {
                        value: None,
                        getter: getter_opt,
                        setter: setter_opt,
                    };
                    {
                        let stored_val = &prop_descriptor;
                        let obj_ptr = Gc::as_ptr(obj) as *const _;
                        let stored_obj_ptr = if let Value::Object(o) = stored_val {
                            Gc::as_ptr(*o) as *const _
                        } else {
                            std::ptr::null()
                        };
                        log::debug!(
                            "DBG object-literal insert: obj={:p} key={:?} storing_val={:?} stored_obj_ptr={:p}",
                            obj_ptr,
                            &key_v,
                            stored_val,
                            stored_obj_ptr
                        );
                        object_set_key_value(mc, &obj, &key_v, &prop_descriptor)?;
                    }
                }
                // Otherwise just overwrite
                (_other, new_val) => {
                    let stored_val = &new_val;
                    let obj_ptr = Gc::as_ptr(obj) as *const _;
                    let stored_obj_ptr = if let Value::Object(o) = stored_val {
                        Gc::as_ptr(*o) as *const _
                    } else {
                        std::ptr::null()
                    };
                    log::debug!(
                        "DBG object-literal insert: obj={:p} key={:?} storing_val={:?} stored_obj_ptr={:p}",
                        obj_ptr,
                        &key_v,
                        stored_val,
                        stored_obj_ptr
                    );
                    object_set_key_value(mc, &obj, &key_v, &new_val)?;
                }
            }
        } else {
            {
                let stored_val = &val;
                let obj_ptr = Gc::as_ptr(obj) as *const _;
                let stored_obj_ptr = if let Value::Object(o) = stored_val {
                    Gc::as_ptr(*o) as *const _
                } else {
                    std::ptr::null()
                };
                log::debug!(
                    "DBG object-literal insert: obj={:p} key={:?} storing_val={:?} stored_obj_ptr={:p}",
                    obj_ptr,
                    &key_v,
                    stored_val,
                    stored_obj_ptr
                );
                object_set_key_value(mc, &obj, &key_v, &val)?;
            }
        }
    }
    Ok(Value::Object(obj))
}

fn evaluate_expr_array<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    elements: &[Option<Expr>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let arr_obj = create_array(mc, env)?;
    let mut index = 0;

    for elem_opt in elements.iter() {
        if let Some(elem) = elem_opt {
            if let Expr::Spread(target) = elem {
                let val = evaluate_expr(mc, env, target)?;
                if let Value::Object(obj) = val {
                    if is_array(mc, &obj) {
                        let len = object_get_length(&obj).unwrap_or(0);
                        for k in 0..len {
                            let item = object_get_key_value(&obj, k).unwrap_or(new_gc_cell_ptr(mc, Value::Undefined));
                            object_set_key_value(mc, &arr_obj, index, &item.borrow())?;
                            index += 1;
                        }
                    } else {
                        // Support generic iterables via Symbol.iterator
                        if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                            && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                            && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
                            && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                        {
                            let iter_fn_val = get_property_with_accessors(mc, env, &obj, iter_sym)?;
                            if matches!(iter_fn_val, Value::Undefined | Value::Null) {
                                return Err(raise_type_error!("Spread target is not iterable").into());
                            }

                            // Call iterator method on the object to get an iterator
                            let iterator = match iter_fn_val {
                                Value::Function(name) => {
                                    let call_env = prepare_call_env_with_this(
                                        mc,
                                        Some(env),
                                        Some(&Value::Object(obj)),
                                        None,
                                        &[],
                                        None,
                                        Some(env),
                                        None,
                                    )?;
                                    evaluate_call_dispatch(mc, &call_env, &Value::Function(name), Some(&Value::Object(obj)), &[])?
                                }
                                Value::Closure(cl) => call_closure(mc, &cl, Some(&Value::Object(obj)), &[], env, None)?,
                                Value::Object(func_obj) => {
                                    evaluate_call_dispatch(mc, env, &Value::Object(func_obj), Some(&Value::Object(obj)), &[])?
                                }
                                _ => return Err(raise_type_error!("Spread target is not iterable").into()),
                            };

                            // Consume iterator by repeatedly calling its next() method
                            if let Value::Object(iter_obj) = iterator {
                                loop {
                                    let next_fn = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                                    if !matches!(next_fn, Value::Undefined | Value::Null) {
                                        let res = match next_fn {
                                            Value::Function(name) => {
                                                let call_env = prepare_call_env_with_this(
                                                    mc,
                                                    Some(env),
                                                    Some(&Value::Object(iter_obj)),
                                                    None,
                                                    &[],
                                                    None,
                                                    Some(env),
                                                    None,
                                                )?;
                                                evaluate_call_dispatch(
                                                    mc,
                                                    &call_env,
                                                    &Value::Function(name.clone()),
                                                    Some(&Value::Object(iter_obj)),
                                                    &[],
                                                )?
                                            }
                                            Value::Closure(cl) => call_closure(mc, &cl, Some(&Value::Object(iter_obj)), &[], env, None)?,
                                            Value::Object(func_obj) => evaluate_call_dispatch(
                                                mc,
                                                env,
                                                &Value::Object(func_obj),
                                                Some(&Value::Object(iter_obj)),
                                                &[],
                                            )?,
                                            _ => {
                                                return Err(raise_type_error!("Iterator.next is not callable").into());
                                            }
                                        };

                                        if let Value::Object(res_obj) = res {
                                            let done_val = get_property_with_accessors(mc, env, &res_obj, "done")?;
                                            let done = matches!(done_val, Value::Boolean(true));

                                            if done {
                                                break;
                                            }

                                            let value = get_property_with_accessors(mc, env, &res_obj, "value")?;

                                            object_set_key_value(mc, &arr_obj, index, &value)?;
                                            index += 1;

                                            continue;
                                        } else {
                                            return Err(raise_type_error!("Iterator.next did not return an object").into());
                                        }
                                    } else {
                                        return Err(raise_type_error!("Iterator has no next method").into());
                                    }
                                }
                            } else {
                                return Err(raise_type_error!("Iterator call did not return an object").into());
                            }
                        }
                    }
                } else {
                    return Err(raise_type_error!("Spread only implemented for Objects").into());
                }
            } else {
                let val = evaluate_expr(mc, env, elem)?;
                object_set_key_value(mc, &arr_obj, index, &val)?;
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    set_array_length(mc, &arr_obj, index)?;
    Ok(Value::Object(arr_obj))
}

fn body_has_lexical(stmts: &[Statement]) -> bool {
    for s in stmts {
        match &*s.kind {
            StatementKind::Let(_)
            | StatementKind::Const(_)
            | StatementKind::LetDestructuringArray(..)
            | StatementKind::ConstDestructuringArray(..)
            | StatementKind::LetDestructuringObject(..)
            | StatementKind::ConstDestructuringObject(..)
            | StatementKind::Class(_) => {
                return true;
            }
            StatementKind::Block(inner) => {
                if body_has_lexical(inner) {
                    return true;
                }
            }
            StatementKind::If(if_stmt) => {
                if body_has_lexical(&if_stmt.then_body) {
                    return true;
                }
                if let Some(else_body) = &if_stmt.else_body
                    && body_has_lexical(else_body)
                {
                    return true;
                }
            }
            StatementKind::For(for_stmt) => {
                if body_has_lexical(&for_stmt.body) {
                    return true;
                }
            }
            StatementKind::While(_, body) | StatementKind::DoWhile(body, _) => {
                if body_has_lexical(body) {
                    return true;
                }
            }
            StatementKind::ForOf(_, _, _, body)
            | StatementKind::ForAwaitOf(_, _, _, body)
            | StatementKind::ForIn(_, _, _, body)
            | StatementKind::ForOfDestructuringObject(_, _, _, body)
            | StatementKind::ForOfDestructuringArray(_, _, _, body)
            | StatementKind::ForAwaitOfDestructuringObject(_, _, _, body)
            | StatementKind::ForAwaitOfDestructuringArray(_, _, _, body)
            | StatementKind::ForInDestructuringObject(_, _, _, body)
            | StatementKind::ForInDestructuringArray(_, _, _, body) => {
                if body_has_lexical(body) {
                    return true;
                }
            }
            StatementKind::TryCatch(tc) => {
                if body_has_lexical(&tc.try_body) {
                    return true;
                }
                if let Some(catch_body) = &tc.catch_body
                    && body_has_lexical(catch_body)
                {
                    return true;
                }
                if let Some(finally_body) = &tc.finally_body
                    && body_has_lexical(finally_body)
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

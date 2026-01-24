use crate::core::{ExportSpecifier, Gc, GcCell, MutationContext, object_get_length, object_set_length};
use crate::js_array::{create_array, handle_array_static_method, is_array, set_array_length};
use crate::js_bigint::bigint_constructor;
use crate::js_date::{handle_date_method, handle_date_static_method, is_date_object};
use crate::js_function::handle_function_prototype_method;
use crate::js_json::handle_json_method;
use crate::js_number::{handle_number_prototype_method, handle_number_static_method, number_constructor};
use crate::js_string::{handle_string_method, string_from_char_code, string_from_code_point, string_raw};
use crate::{
    JSError, JSErrorKind, PropertyKey, Value,
    core::{
        BinaryOp, ClosureData, DestructuringElement, EvalError, Expr, ImportSpecifier, JSObjectDataPtr, ObjectDestructuringElement,
        PromiseState, Statement, StatementKind, create_error, env_get, env_get_own, env_set, env_set_recursive, get_own_property, is_error,
        new_js_object_data, object_get_key_value, object_set_key_value, value_to_string,
    },
    js_math::handle_math_call,
    raise_eval_error, raise_reference_error,
    unicode::{utf8_to_utf16, utf16_to_utf8},
};
use crate::{Token, parse_statements, raise_syntax_error, raise_type_error, tokenize};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive, Zero};

#[derive(Clone, Debug)]
pub enum ControlFlow<'gc> {
    Normal(Value<'gc>),
    Return(Value<'gc>),
    Throw(Value<'gc>, Option<usize>, Option<usize>), // value, line, column
    Break(Option<String>),
    Continue(Option<String>),
}

fn to_number<'gc>(val: &Value<'gc>) -> Result<f64, EvalError<'gc>> {
    match val {
        Value::Number(n) => Ok(*n),
        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Null => Ok(0.0),
        Value::Undefined | Value::Uninitialized => Ok(f64::NAN),
        Value::String(s) => {
            let s = utf16_to_utf8(s);
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(0.0);
            }
            if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
                if hex.is_empty() {
                    return Ok(f64::NAN);
                }
                return Ok(i64::from_str_radix(hex, 16).map(|v| v as f64).unwrap_or(f64::NAN));
            }
            if let Some(bin) = trimmed.strip_prefix("0b").or_else(|| trimmed.strip_prefix("0B")) {
                if bin.is_empty() {
                    return Ok(f64::NAN);
                }
                return Ok(i64::from_str_radix(bin, 2).map(|v| v as f64).unwrap_or(f64::NAN));
            }
            if let Some(oct) = trimmed.strip_prefix("0o").or_else(|| trimmed.strip_prefix("0O")) {
                if oct.is_empty() {
                    return Ok(f64::NAN);
                }
                return Ok(i64::from_str_radix(oct, 8).map(|v| v as f64).unwrap_or(f64::NAN));
            }
            Ok(trimmed.parse::<f64>().unwrap_or(f64::NAN))
        }
        Value::BigInt(_) => Err(EvalError::Js(crate::raise_type_error!("Cannot convert a BigInt value to a number"))),
        Value::Symbol(_) => Err(EvalError::Js(crate::raise_type_error!("Cannot convert Symbol"))),
        _ => Ok(f64::NAN),
    }
}

fn to_int32_value<'gc>(val: &Value<'gc>) -> Result<i32, EvalError<'gc>> {
    let n = to_number(val)?;
    Ok(crate::core::number::to_int32(n))
}

fn to_uint32_value<'gc>(val: &Value<'gc>) -> Result<u32, EvalError<'gc>> {
    let n = to_number(val)?;
    Ok(crate::core::number::to_uint32(n))
}

fn value_to_concat_string<'gc>(val: &Value<'gc>) -> String {
    match val {
        Value::String(s) => utf16_to_utf8(s),
        Value::BigInt(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        _ => value_to_string(val),
    }
}

fn loose_equal<'gc>(
    mc: &MutationContext<'gc>,
    l_val: Value<'gc>,
    r_val: Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<bool, EvalError<'gc>> {
    match (l_val, r_val) {
        // Same type -> strict equal
        (Value::Number(l), Value::Number(r)) => Ok(l == r),
        (Value::BigInt(l), Value::BigInt(r)) => Ok(l == r),
        (Value::String(l), Value::String(r)) => Ok(l == r),
        (Value::Boolean(l), Value::Boolean(r)) => Ok(l == r),
        (Value::Null, Value::Null) => Ok(true),
        (Value::Undefined, Value::Undefined) => Ok(true),
        (Value::Object(l), Value::Object(r)) => Ok(Gc::ptr_eq(l, r)),
        (Value::Closure(l), Value::Closure(r)) => Ok(Gc::ptr_eq(l, r)),
        (Value::Symbol(l), Value::Symbol(r)) => Ok(Gc::ptr_eq(l, r)),

        (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null) => Ok(true),

        (Value::Number(l), Value::String(r)) => Ok(l == to_number(&Value::String(r))?),
        (Value::String(l), Value::Number(r)) => Ok(to_number(&Value::String(l))? == r),

        (Value::BigInt(l), Value::String(r)) => {
            let s = utf16_to_utf8(&r);
            match crate::js_bigint::parse_bigint_string(&s) {
                Ok(bn) => Ok(l == bn),
                Err(_) => Ok(false),
            }
        }
        (Value::String(l), Value::BigInt(r)) => {
            let s = utf16_to_utf8(&l);
            match crate::js_bigint::parse_bigint_string(&s) {
                Ok(bn) => Ok(bn == r),
                Err(_) => Ok(false),
            }
        }

        (Value::Boolean(l), r) => loose_equal(mc, Value::Number(if l { 1.0 } else { 0.0 }), r, env),
        (l, Value::Boolean(r)) => loose_equal(mc, l, Value::Number(if r { 1.0 } else { 0.0 }), env),

        (Value::Object(l), r @ (Value::String(_) | Value::Number(_) | Value::BigInt(_) | Value::Symbol(_))) => {
            let l_prim = crate::core::to_primitive(mc, &Value::Object(l), "default", env)?;
            loose_equal(mc, l_prim, r, env)
        }
        (l @ (Value::String(_) | Value::Number(_) | Value::BigInt(_) | Value::Symbol(_)), Value::Object(r)) => {
            let r_prim = crate::core::to_primitive(mc, &Value::Object(r), "default", env)?;
            loose_equal(mc, l, r_prim, env)
        }

        (Value::BigInt(l), Value::Number(r)) | (Value::Number(r), Value::BigInt(l)) => {
            if !r.is_finite() || r.is_nan() || r.fract() != 0.0 {
                Ok(false)
            } else {
                Ok(BigInt::from_f64(r).map(|rb| l == rb).unwrap_or(false))
            }
        }

        _ => Ok(false),
    }
}

fn bigint_shift_count<'gc>(count: &BigInt) -> Result<usize, EvalError<'gc>> {
    if count.sign() == num_bigint::Sign::Minus {
        return Err(EvalError::Js(crate::raise_eval_error!("invalid bigint shift")));
    }
    count
        .to_usize()
        .ok_or_else(|| EvalError::Js(crate::raise_eval_error!("invalid bigint shift")))
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
        DestructuringElement::Rest(name) => names.push(name.clone()),
        DestructuringElement::NestedArray(inner) => collect_names_from_destructuring(inner, names),
        DestructuringElement::NestedObject(inner) => collect_names_from_destructuring(inner, names),
        DestructuringElement::Empty => {}
    }
}

fn collect_names_from_object_destructuring(pattern: &[ObjectDestructuringElement], names: &mut Vec<String>) {
    for element in pattern {
        match element {
            ObjectDestructuringElement::Property { key: _, value } => collect_names_from_destructuring_element(value, names),
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
    for inner in pattern.iter() {
        match inner {
            DestructuringElement::Property(key, boxed) => match &**boxed {
                DestructuringElement::Variable(name, default_expr) => {
                    let mut prop_val = Value::Undefined;
                    if let Some(cell) = object_get_key_value(obj, key) {
                        prop_val = cell.borrow().clone();
                    }
                    if matches!(prop_val, Value::Undefined)
                        && let Some(def) = default_expr
                    {
                        prop_val = evaluate_expr(mc, env, def)?;
                    }
                    env_set(mc, env, name, prop_val.clone())?;
                    if is_const {
                        env.borrow_mut(mc).set_const(name.clone());
                    }
                }
                DestructuringElement::NestedObject(nested) => {
                    let mut prop_val = Value::Undefined;
                    if let Some(cell) = object_get_key_value(obj, key) {
                        prop_val = cell.borrow().clone();
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
                        return Err(EvalError::Js(raise_eval_error!(format!(
                            "Cannot destructure property '{}' of {}",
                            prop_name,
                            if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                        ))));
                    }
                    if let Value::Object(o3) = &prop_val {
                        bind_object_inner_for_letconst(mc, env, nested, o3, is_const)?;
                    } else {
                        return Err(EvalError::Js(raise_eval_error!("Expected object for nested destructuring")));
                    }
                }
                DestructuringElement::NestedArray(nested_arr) => {
                    let mut prop_val = Value::Undefined;
                    if let Some(cell) = object_get_key_value(obj, key) {
                        prop_val = cell.borrow().clone();
                    }
                    if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                        return Err(EvalError::Js(raise_eval_error!(format!(
                            "Cannot destructure property '{}' of {}",
                            key,
                            if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                        ))));
                    }
                    if let Value::Object(oarr) = &prop_val {
                        bind_array_inner_for_letconst(mc, env, nested_arr, oarr, is_const)?;
                    } else {
                        return Err(EvalError::Js(raise_eval_error!("Expected array for nested array destructuring")));
                    }
                }
                _ => {
                    return Err(EvalError::Js(crate::raise_syntax_error!(
                        "Nested object destructuring not implemented"
                    )));
                }
            },
            _ => {
                return Err(EvalError::Js(crate::raise_syntax_error!(
                    "Nested object destructuring not implemented"
                )));
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

    for inner in pattern.iter() {
        match inner {
            DestructuringElement::Property(key, boxed) => match &**boxed {
                DestructuringElement::Variable(name, default_expr) => {
                    let mut prop_val = Value::Undefined;
                    if let Some(cell) = object_get_key_value(obj, key) {
                        prop_val = cell.borrow().clone();
                    }
                    if matches!(prop_val, Value::Undefined)
                        && let Some(def) = default_expr
                    {
                        prop_val = evaluate_expr(mc, env, def)?;
                    }
                    env_set_recursive(mc, &target_env, name, prop_val)?;
                }
                DestructuringElement::NestedObject(nested) => {
                    let mut prop_val = Value::Undefined;
                    if let Some(cell) = object_get_key_value(obj, key) {
                        prop_val = cell.borrow().clone();
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
                        return Err(EvalError::Js(raise_eval_error!(format!(
                            "Cannot destructure property '{}' of {}",
                            prop_name,
                            if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                        ))));
                    }
                    if let Value::Object(o3) = &prop_val {
                        bind_object_inner_for_var(mc, env, nested, o3)?;
                    } else {
                        return Err(EvalError::Js(raise_eval_error!("Expected object for nested destructuring")));
                    }
                }
                DestructuringElement::NestedArray(nested_arr) => {
                    let mut prop_val = Value::Undefined;
                    if let Some(cell) = object_get_key_value(obj, key) {
                        prop_val = cell.borrow().clone();
                    }
                    if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                        return Err(EvalError::Js(raise_eval_error!(format!(
                            "Cannot destructure property '{}' of {}",
                            key,
                            if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                        ))));
                    }
                    if let Value::Object(oarr) = &prop_val {
                        bind_array_inner_for_var(mc, env, nested_arr, oarr)?;
                    } else {
                        return Err(EvalError::Js(raise_eval_error!("Expected array for nested array destructuring")));
                    }
                }
                _ => {
                    return Err(EvalError::Js(crate::raise_syntax_error!(
                        "Nested object destructuring not implemented"
                    )));
                }
            },
            _ => {
                return Err(EvalError::Js(crate::raise_syntax_error!(
                    "Nested object destructuring not implemented"
                )));
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
) -> Result<(), EvalError<'gc>> {
    let _current_len = crate::js_array::get_array_length(mc, arr_obj).unwrap_or(0);
    for (j, inner_elem) in pattern.iter().enumerate() {
        match inner_elem {
            DestructuringElement::Variable(name, default_expr) => {
                let mut elem_val = Value::Undefined;
                if let Some(cell) = object_get_key_value(arr_obj, j.to_string()) {
                    elem_val = cell.borrow().clone();
                }
                if matches!(elem_val, Value::Undefined)
                    && let Some(def) = default_expr
                {
                    elem_val = evaluate_expr(mc, env, def)?;
                }
                env_set(mc, env, name, elem_val.clone())?;
                if is_const {
                    env.borrow_mut(mc).set_const(name.clone());
                }
            }
            DestructuringElement::NestedObject(inner_pattern) => {
                // get element at index j
                let mut elem_val = Value::Undefined;
                if let Some(cell) = object_get_key_value(arr_obj, j.to_string()) {
                    elem_val = cell.borrow().clone();
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
                    return Err(EvalError::Js(raise_eval_error!(format!(
                        "Cannot destructure property '{}' of {}",
                        prop_name,
                        if matches!(elem_val, Value::Null) { "null" } else { "undefined" }
                    ))));
                }
                if let Value::Object(obj2) = &elem_val {
                    // bind inner properties in current env (let/const semantics)
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
                                        env_set(mc, env, name, prop_val.clone())?;
                                        if is_const {
                                            env.borrow_mut(mc).set_const(name.clone());
                                        }
                                    }
                                    DestructuringElement::NestedObject(nested) => {
                                        let mut prop_val = Value::Undefined;
                                        if let Some(cell) = object_get_key_value(obj2, key) {
                                            prop_val = cell.borrow().clone();
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
                                            return Err(EvalError::Js(raise_eval_error!(format!(
                                                "Cannot destructure property '{}' of {}",
                                                prop_name,
                                                if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                                            ))));
                                        }
                                        if let Value::Object(o3) = &prop_val {
                                            // recursively bind as let/const
                                            bind_object_inner_for_letconst(mc, env, nested, o3, is_const)?;
                                        } else {
                                            return Err(EvalError::Js(raise_eval_error!("Expected object for nested destructuring")));
                                        }
                                    }
                                    DestructuringElement::NestedArray(nested_arr) => {
                                        let mut prop_val = Value::Undefined;
                                        if let Some(cell) = object_get_key_value(obj2, key) {
                                            prop_val = cell.borrow().clone();
                                        }
                                        if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                            return Err(EvalError::Js(raise_eval_error!(format!(
                                                "Cannot destructure property '{}' of {}",
                                                key,
                                                if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                                            ))));
                                        }
                                        if let Value::Object(oarr) = &prop_val {
                                            bind_array_inner_for_letconst(mc, env, nested_arr, oarr, is_const)?;
                                        } else {
                                            return Err(EvalError::Js(raise_eval_error!("Expected array for nested array destructuring")));
                                        }
                                    }
                                    _ => {
                                        return Err(EvalError::Js(crate::raise_syntax_error!(
                                            "Nested object destructuring not implemented"
                                        )));
                                    }
                                }
                            }
                            _ => {
                                return Err(EvalError::Js(crate::raise_syntax_error!(
                                    "Nested object destructuring not implemented"
                                )));
                            }
                        }
                    }
                } else {
                    return Err(EvalError::Js(raise_eval_error!("Expected object for nested destructuring")));
                }
            }
            DestructuringElement::NestedArray(inner_array) => {
                let mut elem_val = Value::Undefined;
                if let Some(cell) = object_get_key_value(arr_obj, j.to_string()) {
                    elem_val = cell.borrow().clone();
                }
                if matches!(elem_val, Value::Undefined) || matches!(elem_val, Value::Null) {
                    return Err(EvalError::Js(raise_eval_error!("Cannot destructure array from undefined/null")));
                }
                if let Value::Object(oarr) = &elem_val {
                    bind_array_inner_for_letconst(mc, env, inner_array, oarr, is_const)?;
                } else {
                    return Err(EvalError::Js(raise_eval_error!("Expected array for nested array destructuring")));
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
                    if let Some(cell2) = object_get_key_value(arr_obj, jj.to_string()) {
                        object_set_key_value(mc, &arr_obj2, idx2, cell2.borrow().clone())?;
                        idx2 += 1;
                    }
                }
                object_set_key_value(mc, &arr_obj2, "length", Value::Number(idx2 as f64))?;
                env_set(mc, env, name, Value::Object(arr_obj2))?;
                if is_const {
                    env.borrow_mut(mc).set_const(name.clone());
                }
                break;
            }
            DestructuringElement::Empty => {}
            _ => {
                return Err(EvalError::Js(crate::raise_syntax_error!(
                    "Nested array destructuring not implemented"
                )));
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

    let _current_len = crate::js_array::get_array_length(mc, arr_obj).unwrap_or(0);
    for (j, inner_elem) in pattern.iter().enumerate() {
        match inner_elem {
            DestructuringElement::Variable(name, default_expr) => {
                let mut elem_val = Value::Undefined;
                if let Some(cell) = object_get_key_value(arr_obj, j.to_string()) {
                    elem_val = cell.borrow().clone();
                }
                if matches!(elem_val, Value::Undefined)
                    && let Some(def) = default_expr
                {
                    elem_val = evaluate_expr(mc, env, def)?;
                }
                env_set_recursive(mc, &target_env, name, elem_val)?;
            }
            DestructuringElement::NestedObject(inner_pattern) => {
                // get element at index j
                let mut elem_val = Value::Undefined;
                if let Some(cell) = object_get_key_value(arr_obj, j.to_string()) {
                    elem_val = cell.borrow().clone();
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
                    return Err(EvalError::Js(raise_eval_error!(format!(
                        "Cannot destructure property '{}' of {}",
                        prop_name,
                        if matches!(elem_val, Value::Null) { "null" } else { "undefined" }
                    ))));
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
                                        env_set_recursive(mc, &target_env, name, prop_val)?;
                                    }
                                    DestructuringElement::NestedObject(nested) => {
                                        let mut prop_val = Value::Undefined;
                                        if let Some(cell) = object_get_key_value(obj2, key) {
                                            prop_val = cell.borrow().clone();
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
                                            return Err(EvalError::Js(raise_eval_error!(format!(
                                                "Cannot destructure property '{}' of {}",
                                                prop_name,
                                                if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                                            ))));
                                        }
                                        if let Value::Object(o3) = &prop_val {
                                            // recursively bind as var
                                            bind_object_inner_for_var(mc, env, nested, o3)?;
                                        } else {
                                            return Err(EvalError::Js(raise_eval_error!("Expected object for nested destructuring")));
                                        }
                                    }
                                    DestructuringElement::NestedArray(nested_arr) => {
                                        let mut prop_val = Value::Undefined;
                                        if let Some(cell) = object_get_key_value(obj2, key) {
                                            prop_val = cell.borrow().clone();
                                        }
                                        if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                            return Err(EvalError::Js(raise_eval_error!(format!(
                                                "Cannot destructure property '{}' of {}",
                                                key,
                                                if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                                            ))));
                                        }
                                        if let Value::Object(oarr) = &prop_val {
                                            bind_array_inner_for_var(mc, env, nested_arr, oarr)?;
                                        } else {
                                            return Err(EvalError::Js(raise_eval_error!("Expected array for nested array destructuring")));
                                        }
                                    }
                                    _ => {
                                        return Err(EvalError::Js(crate::raise_syntax_error!(
                                            "Nested object destructuring not implemented"
                                        )));
                                    }
                                }
                            }
                            _ => {
                                return Err(EvalError::Js(crate::raise_syntax_error!(
                                    "Nested object destructuring not implemented"
                                )));
                            }
                        }
                    }
                } else {
                    return Err(EvalError::Js(raise_eval_error!("Expected object for nested destructuring")));
                }
            }
            DestructuringElement::NestedArray(inner_array) => {
                let mut elem_val = Value::Undefined;
                if let Some(cell) = object_get_key_value(arr_obj, j.to_string()) {
                    elem_val = cell.borrow().clone();
                }
                if matches!(elem_val, Value::Undefined) || matches!(elem_val, Value::Null) {
                    return Err(EvalError::Js(raise_eval_error!("Cannot destructure array from undefined/null")));
                }
                if let Value::Object(oarr) = &elem_val {
                    bind_array_inner_for_var(mc, env, inner_array, oarr)?;
                } else {
                    return Err(EvalError::Js(raise_eval_error!("Expected array for nested array destructuring")));
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
                    if let Some(cell) = object_get_key_value(arr_obj, jj.to_string()) {
                        object_set_key_value(mc, &arr_obj2, idx2, cell.borrow().clone())?;
                        idx2 += 1;
                    }
                }
                object_set_key_value(mc, &arr_obj2, "length", Value::Number(idx2 as f64))?;
                // bind var in function scope
                env_set_recursive(mc, &target_env, name, Value::Object(arr_obj2))?;
            }
            DestructuringElement::Empty => {}
            _ => {
                return Err(EvalError::Js(crate::raise_syntax_error!(
                    "Nested array destructuring not implemented"
                )));
            }
        }
    }
    Ok(())
}

fn hoist_name<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str) -> Result<(), EvalError<'gc>> {
    let mut target_env = *env;
    while !target_env.borrow().is_function_scope {
        if let Some(proto) = target_env.borrow().prototype {
            target_env = proto;
        } else {
            break;
        }
    }
    if env_get_own(&target_env, name).is_none() {
        env_set(mc, &target_env, name, Value::Undefined)?;
    }
    Ok(())
}

fn hoist_var_declarations<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
) -> Result<(), EvalError<'gc>> {
    for stmt in statements {
        match &*stmt.kind {
            StatementKind::Var(decls) => {
                for (name, _) in decls {
                    hoist_name(mc, env, name)?;
                }
            }
            StatementKind::VarDestructuringArray(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_destructuring(pattern, &mut names);
                for name in names {
                    hoist_name(mc, env, &name)?;
                }
            }
            StatementKind::VarDestructuringObject(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_object_destructuring(pattern, &mut names);
                for name in names {
                    hoist_name(mc, env, &name)?;
                }
            }
            StatementKind::Block(stmts) => hoist_var_declarations(mc, env, stmts)?,
            StatementKind::If(if_stmt) => {
                let if_stmt = if_stmt.as_ref();
                hoist_var_declarations(mc, env, &if_stmt.then_body)?;
                if let Some(else_stmts) = &if_stmt.else_body {
                    hoist_var_declarations(mc, env, else_stmts)?;
                }
            }
            StatementKind::For(for_stmt) => hoist_var_declarations(mc, env, &for_stmt.body)?,
            StatementKind::ForIn(_, _, _, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::ForOf(_, _, _, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::ForOfDestructuringObject(_, _, _, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::ForOfDestructuringArray(_, _, _, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::While(_, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::DoWhile(body, _) => hoist_var_declarations(mc, env, body)?,
            StatementKind::TryCatch(tc_stmt) => {
                let tc_stmt = tc_stmt.as_ref();
                hoist_var_declarations(mc, env, &tc_stmt.try_body)?;
                if let Some(catch_stmts) = &tc_stmt.catch_body {
                    hoist_var_declarations(mc, env, catch_stmts)?;
                }
                if let Some(finally_stmts) = &tc_stmt.finally_body {
                    hoist_var_declarations(mc, env, finally_stmts)?;
                }
            }
            StatementKind::Switch(sw_stmt) => {
                for case in &sw_stmt.cases {
                    match case {
                        crate::core::SwitchCase::Case(_, stmts) => hoist_var_declarations(mc, env, stmts)?,
                        crate::core::SwitchCase::Default(stmts) => hoist_var_declarations(mc, env, stmts)?,
                    }
                }
            }
            StatementKind::Label(_, stmt) => {
                // Label contains a single statement, but it might be a block or loop
                // We need to wrap it in a slice to recurse
                hoist_var_declarations(mc, env, std::slice::from_ref(stmt))?;
            }
            StatementKind::Export(_, Some(decl)) => {
                hoist_var_declarations(mc, env, std::slice::from_ref(decl))?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn hoist_declarations<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, statements: &[Statement]) -> Result<(), EvalError<'gc>> {
    // 1. Hoist FunctionDeclarations (only top-level in this list of statements)
    for stmt in statements {
        if let StatementKind::FunctionDeclaration(name, params, body, is_generator, is_async) = &*stmt.kind {
            let mut body_clone = body.clone();
            if *is_generator {
                // Create a generator function object (hoisted)
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
                    env: *env,
                    home_object: GcCell::new(None),
                    captured_envs: Vec::new(),
                    bound_this: None,
                    is_arrow: false,
                    is_strict,
                };
                let closure_val = Value::GeneratorFunction(Some(name.clone()), Gc::new(mc, closure_data));
                object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                object_set_key_value(mc, &func_obj, "name", Value::String(utf8_to_utf16(name)))?;

                // Create prototype object
                let proto_obj = crate::core::new_js_object_data(mc);
                if let Some(obj_val) = env_get(env, "Object")
                    && let Value::Object(obj_ctor) = &*obj_val.borrow()
                    && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
                    && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
                {
                    proto_obj.borrow_mut(mc).prototype = Some(*obj_proto);
                }

                object_set_key_value(mc, &func_obj, "prototype", Value::Object(proto_obj))?;
                env_set(mc, env, name, Value::Object(func_obj))?;
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
                    env: *env,
                    home_object: GcCell::new(None),
                    captured_envs: Vec::new(),
                    bound_this: None,
                    is_arrow: false,
                    is_strict,
                };
                let closure_val = Value::AsyncClosure(Gc::new(mc, closure_data));

                object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                object_set_key_value(mc, &func_obj, "name", Value::String(utf8_to_utf16(name)))?;
                env_set(mc, env, name, Value::Object(func_obj))?;
            } else {
                let func = evaluate_function_expression(mc, env, None, params, &mut body_clone)?;
                if let Value::Object(func_obj) = &func {
                    object_set_key_value(mc, func_obj, "name", Value::String(utf8_to_utf16(name)))?;
                }
                env_set(mc, env, name, func)?;
            }
        }
    }

    // 2. Hoist Var declarations (recursively)
    hoist_var_declarations(mc, env, statements)?;

    // 3. Hoist Lexical declarations (let, const, class) - top-level only, initialize to Uninitialized (TDZ)
    for stmt in statements {
        match &*stmt.kind {
            StatementKind::Let(decls) => {
                for (name, _) in decls {
                    env_set(mc, env, name, Value::Uninitialized)?;
                }
            }
            StatementKind::Const(decls) => {
                for (name, _) in decls {
                    env_set(mc, env, name, Value::Uninitialized)?;
                }
            }
            StatementKind::Class(class_def) => {
                env_set(mc, env, &class_def.name, Value::Uninitialized)?;
            }
            StatementKind::Import(specifiers, _) => {
                for spec in specifiers {
                    match spec {
                        ImportSpecifier::Default(name) => {
                            env_set(mc, env, name, Value::Uninitialized)?;
                        }
                        ImportSpecifier::Named(name, alias) => {
                            let binding_name = alias.as_ref().unwrap_or(name);
                            env_set(mc, env, binding_name, Value::Uninitialized)?;
                        }
                        ImportSpecifier::Namespace(name) => {
                            env_set(mc, env, name, Value::Uninitialized)?;
                        }
                    }
                }
            }
            StatementKind::LetDestructuringArray(pattern, _) | StatementKind::ConstDestructuringArray(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_destructuring(pattern, &mut names);
                for name in names {
                    env_set(mc, env, &name, Value::Uninitialized)?;
                }
            }
            StatementKind::LetDestructuringObject(pattern, _) | StatementKind::ConstDestructuringObject(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_object_destructuring(pattern, &mut names);
                for name in names {
                    env_set(mc, env, &name, Value::Uninitialized)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn evaluate_statements<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
) -> Result<Value<'gc>, EvalError<'gc>> {
    match evaluate_statements_with_labels(mc, env, statements, &[], &[])? {
        ControlFlow::Normal(val) => Ok(val),
        ControlFlow::Return(val) => Ok(val),
        ControlFlow::Throw(val, line, column) => Err(EvalError::Throw(val, line, column)),
        ControlFlow::Break(_) => Err(EvalError::Js(raise_syntax_error!("break statement not in loop or switch"))),
        ControlFlow::Continue(_) => Err(EvalError::Js(raise_syntax_error!("continue statement not in loop"))),
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

fn check_expr_for_arguments_assignment(e: &Expr) -> bool {
    match e {
        Expr::Assign(lhs, _rhs) => {
            if let Expr::Var(name, ..) = &**lhs {
                return name == "arguments";
            }
            false
        }
        Expr::Property(obj, _)
        | Expr::Call(obj, _)
        | Expr::New(obj, _)
        | Expr::Index(obj, _)
        | Expr::OptionalProperty(obj, _)
        | Expr::OptionalCall(obj, _)
        | Expr::OptionalIndex(obj, _) => check_expr_for_arguments_assignment(obj),
        Expr::Binary(l, _, r) | Expr::Comma(l, r) | Expr::Conditional(l, r, _) => {
            check_expr_for_arguments_assignment(l) || check_expr_for_arguments_assignment(r)
        }
        Expr::LogicalAnd(l, r) | Expr::LogicalOr(l, r) | Expr::NullishCoalescing(l, r) => {
            check_expr_for_arguments_assignment(l) || check_expr_for_arguments_assignment(r)
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
        | Expr::NullishAssign(l, r) => check_expr_for_arguments_assignment(l) || check_expr_for_arguments_assignment(r),
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
        | Expr::Decrement(inner) => check_expr_for_arguments_assignment(inner),
        _ => false,
    }
}

fn check_stmt_for_arguments_assignment(stmt: &Statement) -> bool {
    match &*stmt.kind {
        StatementKind::Expr(e) => check_expr_for_arguments_assignment(e),
        StatementKind::If(if_stmt) => {
            check_expr_for_arguments_assignment(&if_stmt.condition)
                || if_stmt.then_body.iter().any(check_stmt_for_arguments_assignment)
                || if_stmt
                    .else_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(check_stmt_for_arguments_assignment))
        }
        StatementKind::Block(stmts) => stmts.iter().any(check_stmt_for_arguments_assignment),
        StatementKind::TryCatch(try_stmt) => {
            try_stmt.try_body.iter().any(check_stmt_for_arguments_assignment)
                || try_stmt
                    .catch_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(check_stmt_for_arguments_assignment))
                || try_stmt
                    .finally_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(check_stmt_for_arguments_assignment))
        }
        StatementKind::FunctionDeclaration(_, _, body, _, _) => body.iter().any(check_stmt_for_arguments_assignment),
        _ => false,
    }
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
    if let Some(stmt0) = statements.first()
        && let StatementKind::Expr(expr) = &*stmt0.kind
        && let Expr::StringLit(s) = expr
        && utf16_to_utf8(s).as_str() == "use strict"
    {
        log::trace!("evaluate_statements: detected 'use strict' directive; marking env as strict");
        // If this env (or its prototype chain) indicates it is an indirect eval
        // invocation, create a fresh declarative environment whose prototype is
        // the current env so hoisting occurs on the new env and does not mutate
        // the global bindings.
        if let Some(flag_rc) = object_get_key_value(env, "__is_indirect_eval")
            && matches!(*flag_rc.borrow(), Value::Boolean(true))
        {
            log::trace!("evaluate_statements: indirect strict eval - creating declarative env");
            let new_env = crate::core::new_js_object_data(mc);
            new_env.borrow_mut(mc).prototype = Some(*env);
            // Prevent hoisting into the global env: treat this declarative env as a function scope
            new_env.borrow_mut(mc).is_function_scope = true;
            exec_env = new_env;
        }

        object_set_key_value(mc, &exec_env, "__is_strict", Value::Boolean(true))?;
    }

    hoist_declarations(mc, &exec_env, statements)?;

    // If the execution environment is marked strict, scan for certain forbidden
    // patterns such as assignment to the Identifier 'arguments' in function
    // bodies which should be a SyntaxError under strict mode (matching Test262
    // expectations for eval'd code in strict contexts).
    if let Some(is_strict_cell) = object_get_key_value(&exec_env, "__is_strict")
        && let Value::Boolean(true) = *is_strict_cell.borrow()
    {
        for stmt in statements {
            if check_stmt_for_arguments_assignment(stmt) {
                log::debug!("evaluate_statements: detected assignment to 'arguments' in function body under strict mode");
                // Construct a SyntaxError object and throw it so it behaves like a JS exception
                if let Some(syn_ctor_val) = object_get_key_value(&exec_env, "SyntaxError")
                    && let Value::Object(syn_ctor) = &*syn_ctor_val.borrow()
                    && let Some(proto_val_rc) = object_get_key_value(syn_ctor, "prototype")
                    && let Value::Object(proto_ptr) = &*proto_val_rc.borrow()
                {
                    let msg = Value::String(utf8_to_utf16("Strict mode violation: assignment to 'arguments'"));
                    let err_obj = crate::core::create_error(mc, Some(*proto_ptr), msg)?;
                    return Err(EvalError::Throw(err_obj, None, None));
                }
                // If we couldn't construct a SyntaxError instance for some reason, fall back
                return Err(EvalError::Js(raise_syntax_error!(
                    "Strict mode violation: assignment to 'arguments'"
                )));
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
    if let Some(stmt0) = statements.first()
        && let StatementKind::Expr(expr) = &*stmt0.kind
        && let Expr::StringLit(s) = expr
        && utf16_to_utf8(s).as_str() == "use strict"
    {
        if let Some(flag_rc) = object_get_key_value(env, "__is_indirect_eval")
            && matches!(*flag_rc.borrow(), Value::Boolean(true))
        {
            let new_env = crate::core::new_js_object_data(mc);
            new_env.borrow_mut(mc).prototype = Some(*env);
            new_env.borrow_mut(mc).is_function_scope = true;
            exec_env = new_env;
        }

        object_set_key_value(mc, &exec_env, "__is_strict", Value::Boolean(true))?;
    }

    hoist_declarations(mc, &exec_env, statements)?;

    if let Some(is_strict_cell) = object_get_key_value(&exec_env, "__is_strict")
        && let Value::Boolean(true) = *is_strict_cell.borrow()
    {
        for stmt in statements {
            if check_stmt_for_arguments_assignment(stmt) {
                if let Some(syn_ctor_val) = object_get_key_value(&exec_env, "SyntaxError")
                    && let Value::Object(syn_ctor) = &*syn_ctor_val.borrow()
                    && let Some(proto_val_rc) = object_get_key_value(syn_ctor, "prototype")
                    && let Value::Object(proto_ptr) = &*proto_val_rc.borrow()
                {
                    let msg = Value::String(utf8_to_utf16("Strict mode violation: assignment to 'arguments'"));
                    let err_obj = crate::core::create_error(mc, Some(*proto_ptr), msg)?;
                    return Err(EvalError::Throw(err_obj, Some(stmt.line), Some(stmt.column)));
                }
                return Err(EvalError::Js(raise_syntax_error!(
                    "Strict mode violation: assignment to 'arguments'"
                )));
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
                    *last_value = val;
                    Ok(None)
                }
                Err(e) => Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
            }
        }
        StatementKind::Let(decls) => {
            let mut last_init = Value::Undefined;
            for (name, expr_opt) in decls {
                let val = if let Some(expr) = expr_opt {
                    match evaluate_expr(mc, env, expr) {
                        Ok(v) => v,
                        Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                    }
                } else {
                    Value::Undefined
                };
                last_init = val.clone();
                env_set(mc, env, name, val)?;
            }
            *last_value = last_init;
            Ok(None)
        }
        StatementKind::Var(decls) => {
            for (name, expr_opt) in decls {
                if let Some(expr) = expr_opt {
                    let val = match evaluate_expr(mc, env, expr) {
                        Ok(v) => v,
                        Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                    };

                    let mut target_env = *env;
                    while !target_env.borrow().is_function_scope {
                        if let Some(proto) = target_env.borrow().prototype {
                            target_env = proto;
                        } else {
                            break;
                        }
                    }
                    env_set(mc, &target_env, name, val)?;
                }
            }
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Const(decls) => {
            let mut last_init = Value::Undefined;
            for (name, expr) in decls {
                let val = match evaluate_expr(mc, env, expr) {
                    Ok(v) => v,
                    Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                };
                last_init = val.clone();
                // Bind value and mark the binding as const so subsequent assignments fail
                env_set(mc, env, name, val)?;
                env.borrow_mut(mc).set_const(name.clone());
            }
            *last_value = last_init;
            Ok(None)
        }
        StatementKind::Class(class_def) => {
            // Evaluate class definition and bind to environment
            // This initializes the class binding which was hoisted as Uninitialized
            if let Err(e) = crate::js_class::create_class_object(mc, &class_def.name, &class_def.extends, &class_def.members, env, true) {
                return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e.into()));
            }
            *last_value = Value::Undefined;
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

            let exports = crate::js_module::load_module(mc, source, base_path.as_deref())
                .map_err(|e| EvalError::Throw(Value::String(utf8_to_utf16(&e.message())), Some(stmt.line), Some(stmt.column)))?;

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
                            env_set(mc, env, binding_name, val)?;
                        }
                        ImportSpecifier::Default(name) => {
                            let val_ptr_res = object_get_key_value(&exports_obj, "default");
                            let val = if let Some(cell) = val_ptr_res {
                                cell.borrow().clone()
                            } else {
                                Value::Undefined
                            };
                            env_set(mc, env, name, val)?;
                        }
                        ImportSpecifier::Namespace(name) => {
                            env_set(mc, env, name, Value::Object(exports_obj))?;
                        }
                    }
                }
            }
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Export(specifiers, inner_stmt) => {
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
                            if let Some(cell) = env_get(env, name) {
                                let val = cell.borrow().clone();
                                export_value(mc, env, name, val)?;
                            }
                        }
                    }
                    StatementKind::Let(decls) => {
                        for (name, _) in decls {
                            if let Some(cell) = env_get(env, name) {
                                let val = cell.borrow().clone();
                                export_value(mc, env, name, val)?;
                            }
                        }
                    }
                    StatementKind::Const(decls) => {
                        for (name, _) in decls {
                            if let Some(cell) = env_get(env, name) {
                                let val = cell.borrow().clone();
                                export_value(mc, env, name, val)?;
                            }
                        }
                    }
                    StatementKind::FunctionDeclaration(name, ..) => {
                        if let Some(cell) = env_get(env, name) {
                            let val = cell.borrow().clone();
                            export_value(mc, env, name, val)?;
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
                        // value should be in env
                        if let Some(cell) = env_get(env, name) {
                            let val = cell.borrow().clone();
                            let export_name = alias.as_ref().unwrap_or(name);
                            export_value(mc, env, export_name, val)?;
                        } else {
                            return Err(EvalError::Js(raise_reference_error!(format!("{} is not defined", name))));
                        }
                    }
                    ExportSpecifier::Default(expr) => {
                        // export default expr
                        let val = evaluate_expr(mc, env, expr)?;
                        export_value(mc, env, "default", val)?;
                    }
                }
            }

            *last_value = Value::Undefined;
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

            // If there are any nested patterns in the array, delegate to helper that handles all cases
            let has_nested = pattern.iter().any(|e| !matches!(e, DestructuringElement::Variable(_, _)));
            if has_nested {
                if let Value::Object(obj) = &val {
                    if is_array(mc, obj) {
                        let is_const = matches!(*stmt.kind, StatementKind::ConstDestructuringArray(_, _));
                        bind_array_inner_for_letconst(mc, env, pattern, obj, is_const)?;
                        *last_value = Value::Undefined;
                        return Ok(None);
                    } else {
                        return Err(EvalError::Js(raise_eval_error!("Cannot destructure non-array value")));
                    }
                } else {
                    return Err(EvalError::Js(raise_eval_error!("Cannot destructure non-array value")));
                }
            }

            // Simple variable-only destructuring
            for (i, elem) in pattern.iter().enumerate() {
                if let DestructuringElement::Variable(name, default_expr) = elem {
                    // Get element at index i if array
                    let mut elem_val = Value::Undefined;
                    if let Value::Object(obj) = &val
                        && is_array(mc, obj)
                        && let Some(cell) = object_get_key_value(obj, i)
                    {
                        elem_val = cell.borrow().clone();
                    }
                    // Apply default if undefined and default_expr present
                    if matches!(elem_val, Value::Undefined)
                        && let Some(def) = default_expr
                    {
                        elem_val = evaluate_expr(mc, env, def)?;
                    }
                    // Bind to environment (let/const bind to current env)
                    env_set(mc, env, name, elem_val.clone())?;
                    if matches!(*stmt.kind, StatementKind::ConstDestructuringArray(_, _)) {
                        env.borrow_mut(mc).set_const(name.clone());
                    }
                }
            }
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::VarDestructuringArray(pattern, expr) => {
            let val = match evaluate_expr(mc, env, expr) {
                Ok(v) => v,
                Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
            };

            // If there are any nested patterns in the array, delegate to helper that handles var semantics
            let has_nested = pattern.iter().any(|e| !matches!(e, DestructuringElement::Variable(_, _)));
            if has_nested {
                if let Value::Object(obj) = &val {
                    if is_array(mc, obj) {
                        bind_array_inner_for_var(mc, env, pattern, obj)?;
                        *last_value = Value::Undefined;
                        return Ok(None);
                    } else {
                        return Err(EvalError::Js(raise_eval_error!("Cannot destructure non-array value")));
                    }
                } else {
                    return Err(EvalError::Js(raise_eval_error!("Cannot destructure non-array value")));
                }
            }

            for (i, elem) in pattern.iter().enumerate() {
                match elem {
                    DestructuringElement::Variable(name, default_expr) => {
                        let mut elem_val = Value::Undefined;
                        if let Value::Object(obj) = &val
                            && is_array(mc, obj)
                            && let Some(cell) = object_get_key_value(obj, i)
                        {
                            elem_val = cell.borrow().clone();
                        }
                        if matches!(elem_val, Value::Undefined)
                            && let Some(def) = default_expr
                        {
                            elem_val = evaluate_expr(mc, env, def)?;
                        }
                        // For var, bind in function scope
                        let mut target_env = *env;
                        while !target_env.borrow().is_function_scope {
                            if let Some(proto) = target_env.borrow().prototype {
                                target_env = proto;
                            } else {
                                break;
                            }
                        }
                        env_set_recursive(mc, &target_env, name, elem_val)?;
                    }

                    DestructuringElement::NestedObject(inner_pattern) => {
                        // Nested object pattern (var binding)
                        let mut elem_val = Value::Undefined;
                        if let Value::Object(obj) = &val
                            && is_array(mc, obj)
                            && let Some(cell) = object_get_key_value(obj, i)
                        {
                            elem_val = cell.borrow().clone();
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
                            return Err(EvalError::Js(raise_eval_error!(format!(
                                "Cannot destructure property '{}' of {}",
                                prop_name,
                                if matches!(elem_val, Value::Null) { "null" } else { "undefined" }
                            ))));
                        }

                        if let Value::Object(obj) = &elem_val {
                            for inner in inner_pattern.iter() {
                                match inner {
                                    DestructuringElement::Property(key, boxed) => {
                                        match &**boxed {
                                            DestructuringElement::Variable(name, default_expr) => {
                                                let mut prop_val = Value::Undefined;
                                                if let Some(cell) = object_get_key_value(obj, key) {
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
                                                env_set_recursive(mc, &target_env, name, prop_val)?;
                                            }
                                            _ => {
                                                return Err(EvalError::Js(crate::raise_syntax_error!(
                                                    "Nested object destructuring not implemented"
                                                )));
                                            }
                                        }
                                    }
                                    _ => {
                                        return Err(EvalError::Js(crate::raise_syntax_error!(
                                            "Nested object destructuring not implemented"
                                        )));
                                    }
                                }
                            }
                        } else {
                            return Err(EvalError::Js(raise_eval_error!("Expected object for nested destructuring")));
                        }
                    }
                    DestructuringElement::Rest(name) => {
                        let arr_obj = crate::js_array::create_array(mc, env)?;
                        if let Value::Object(obj) = &val
                            && is_array(mc, obj)
                        {
                            let len = if let Some(len_cell) = object_get_key_value(obj, "length") {
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
                                if let Some(cell) = object_get_key_value(obj, j) {
                                    object_set_key_value(mc, &arr_obj, idx2, cell.borrow().clone())?;
                                    idx2 += 1;
                                }
                            }
                            object_set_key_value(mc, &arr_obj, "length", Value::Number(idx2 as f64))?;
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
                        env_set_recursive(mc, &target_env, name, Value::Object(arr_obj))?;
                    }
                    DestructuringElement::Empty => {}
                    DestructuringElement::NestedArray(inner) => {
                        let mut elem_val = Value::Undefined;
                        if let Value::Object(obj) = &val
                            && is_array(mc, obj)
                            && let Some(cell) = object_get_key_value(obj, i)
                        {
                            elem_val = cell.borrow().clone();
                        }
                        if let Value::Object(oarr) = &elem_val {
                            bind_array_inner_for_var(mc, env, inner, oarr)?;
                        }
                    }
                    _ => {
                        return Err(EvalError::Js(crate::raise_syntax_error!(
                            "Nested array destructuring not implemented"
                        )));
                    }
                }
            }

            *last_value = Value::Undefined;
            Ok(None)
        }
        // Object destructuring: let/var/const {a, b} = expr
        StatementKind::LetDestructuringObject(pattern, expr) | StatementKind::ConstDestructuringObject(pattern, expr) => {
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
                return Err(EvalError::Js(raise_eval_error!(format!(
                    "Cannot destructure property '{}' of {}",
                    prop_name,
                    if matches!(val, Value::Null) { "null" } else { "undefined" }
                ))));
            }

            for prop in pattern.iter() {
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
                                env_set(mc, env, name, prop_val.clone())?;
                                if matches!(*stmt.kind, StatementKind::ConstDestructuringObject(_, _)) {
                                    env.borrow_mut(mc).set_const(name.clone());
                                }
                            }
                            DestructuringElement::NestedObject(inner_pattern) => {
                                // fetch property
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
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
                                    return Err(EvalError::Js(raise_eval_error!(format!(
                                        "Cannot destructure property '{}' of {}",
                                        prop_name,
                                        if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                                    ))));
                                }
                                if let Value::Object(obj2) = &prop_val {
                                    let is_const = matches!(*stmt.kind, StatementKind::ConstDestructuringObject(_, _));
                                    bind_object_inner_for_letconst(mc, env, inner_pattern, obj2, is_const)?;
                                } else {
                                    return Err(EvalError::Js(raise_eval_error!("Expected object for nested destructuring")));
                                }
                            }
                            DestructuringElement::NestedArray(inner_array) => {
                                // fetch property
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                    return Err(EvalError::Js(raise_eval_error!(format!(
                                        "Cannot destructure property '{}' of {}",
                                        key,
                                        if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                                    ))));
                                }
                                if let Value::Object(oarr) = &prop_val {
                                    let is_const = matches!(*stmt.kind, StatementKind::ConstDestructuringObject(_, _));
                                    bind_array_inner_for_letconst(mc, env, inner_array, oarr, is_const)?;
                                } else {
                                    return Err(EvalError::Js(raise_eval_error!("Expected array for nested array destructuring")));
                                }
                            }
                            _ => {
                                return Err(EvalError::Js(crate::raise_syntax_error!(
                                    "Nested object destructuring not implemented"
                                )));
                            }
                        }
                    }
                    ObjectDestructuringElement::Rest(name) => {
                        // Create a new object with remaining properties
                        let obj = new_js_object_data(mc);
                        if let Value::Object(orig) = &val {
                            // copy all own properties except those in pattern keys
                            let ordered = crate::core::ordinary_own_property_keys(orig);
                            for k in ordered {
                                if let PropertyKey::String(s) = &k {
                                    // check if s is in pattern
                                    let mut skip = false;
                                    for p in pattern.iter() {
                                        if let ObjectDestructuringElement::Property { key: k2, .. } = p
                                            && s == k2
                                        {
                                            skip = true;
                                            break;
                                        }
                                    }
                                    if !skip && let Some(cell) = object_get_key_value(orig, &k) {
                                        obj.borrow_mut(mc).insert(k.clone(), cell);
                                    }
                                }
                            }
                        }
                        env_set(mc, env, name, Value::Object(obj))?;
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
                return Err(EvalError::Js(raise_eval_error!(format!(
                    "Cannot destructure property '{}' of {}",
                    prop_name,
                    if matches!(val, Value::Null) { "null" } else { "undefined" }
                ))));
            }

            for prop in pattern.iter() {
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
                                env_set_recursive(mc, &target_env, name, prop_val)?;
                            }
                            DestructuringElement::NestedObject(inner_pattern) => {
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
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
                                    return Err(EvalError::Js(raise_eval_error!(format!(
                                        "Cannot destructure property '{}' of {}",
                                        prop_name,
                                        if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                                    ))));
                                }
                                if let Value::Object(obj2) = &prop_val {
                                    bind_object_inner_for_var(mc, env, inner_pattern, obj2)?;
                                } else {
                                    return Err(EvalError::Js(raise_eval_error!("Expected object for nested destructuring")));
                                }
                            }
                            DestructuringElement::NestedArray(inner_array) => {
                                let mut prop_val = Value::Undefined;
                                if let Value::Object(obj) = &val
                                    && let Some(cell) = object_get_key_value(obj, key)
                                {
                                    prop_val = cell.borrow().clone();
                                }
                                if matches!(prop_val, Value::Undefined) || matches!(prop_val, Value::Null) {
                                    return Err(EvalError::Js(raise_eval_error!(format!(
                                        "Cannot destructure property '{}' of {}",
                                        key,
                                        if matches!(prop_val, Value::Null) { "null" } else { "undefined" }
                                    ))));
                                }
                                if let Value::Object(oarr) = &prop_val {
                                    bind_array_inner_for_var(mc, env, inner_array, oarr)?;
                                } else {
                                    return Err(EvalError::Js(raise_eval_error!("Expected array for nested array destructuring")));
                                }
                            }
                            _ => {
                                return Err(EvalError::Js(crate::raise_syntax_error!(
                                    "Nested object destructuring not implemented"
                                )));
                            }
                        }
                    }
                    ObjectDestructuringElement::Rest(name) => {
                        let obj = new_js_object_data(mc);
                        if let Value::Object(orig) = &val {
                            for (k, cell) in orig.borrow().properties.iter() {
                                if let PropertyKey::String(s) = k.clone() {
                                    let mut skip = false;
                                    for p in pattern.iter() {
                                        if let ObjectDestructuringElement::Property { key: k2, .. } = p
                                            && &s == k2
                                        {
                                            skip = true;
                                            break;
                                        }
                                    }
                                    if !skip {
                                        obj.borrow_mut(mc).insert(k.clone(), *cell);
                                    }
                                }
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
                        env_set_recursive(mc, &target_env, name, Value::Object(obj))?;
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
                if let Some(val) = get_own_property(&cur, &PropertyKey::String("__is_strict".to_string()))
                    && matches!(*val.borrow(), Value::Boolean(true))
                {
                    object_set_key_value(mc, &block_env, "__is_strict", Value::Boolean(true))?;
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
            let is_true = match cond_val {
                Value::Boolean(b) => b,
                Value::Number(n) => n != 0.0 && !n.is_nan(),
                Value::String(s) => !s.is_empty(),
                Value::Null | Value::Undefined => false,
                Value::Object(_) | Value::Symbol(_) => true,
                Value::BigInt(b) => !b.is_zero(),
                _ => false,
            };

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
            // Evaluate try block in its own block environment so block-scoped lexicals
            // (let/const/class) do not leak into the surrounding scope or into catch.
            let try_env = crate::core::new_js_object_data(mc);
            try_env.borrow_mut(mc).prototype = Some(*env);
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
                // Create new scope for catch
                let catch_env = crate::core::new_js_object_data(mc);
                catch_env.borrow_mut(mc).prototype = Some(*env);

                if let Some(param_name) = &tc_stmt.catch_param {
                    env_set(mc, &catch_env, param_name, val.clone())?;
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
        StatementKind::For(for_stmt) => {
            let for_stmt = for_stmt.as_ref();
            let loop_env = new_js_object_data(mc);
            loop_env.borrow_mut(mc).prototype = Some(*env);
            if let Some(init_stmt) = &for_stmt.init {
                evaluate_statements_with_context(mc, &loop_env, std::slice::from_ref(init_stmt), labels)?;
            }
            loop {
                if let Some(test_expr) = &for_stmt.test {
                    let cond_val = evaluate_expr(mc, &loop_env, test_expr)?;
                    let is_true = match cond_val {
                        Value::Boolean(b) => b,
                        Value::Number(n) => n != 0.0 && !n.is_nan(),
                        Value::String(s) => !s.is_empty(),
                        Value::Null | Value::Undefined => false,
                        Value::Object(_) | Value::Symbol(_) => true,
                        Value::BigInt(b) => !b.is_zero(),
                        _ => false,
                    };
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
            loop {
                let cond_val = evaluate_expr(mc, &loop_env, cond)?;
                let is_true = match cond_val {
                    Value::Boolean(b) => b,
                    Value::Number(n) => n != 0.0 && !n.is_nan(),
                    Value::String(s) => !s.is_empty(),
                    Value::Null | Value::Undefined => false,
                    Value::Object(_) | Value::Symbol(_) => true,
                    Value::BigInt(b) => !b.is_zero(),
                    _ => false,
                };
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
                let is_true = match cond_val {
                    Value::Boolean(b) => b,
                    Value::Number(n) => n != 0.0 && !n.is_nan(),
                    Value::String(s) => !s.is_empty(),
                    Value::Null | Value::Undefined => false,
                    Value::Object(_) | Value::Symbol(_) => true,
                    Value::BigInt(b) => !b.is_zero(),
                    _ => false,
                };
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
                        object_set_key_value(mc, &with_env, s, v.borrow().clone())?;
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
                hoist_name(mc, env, var_name)?;
            }

            // If this is a lexical (let/const) declaration, create a head lexical environment
            // which contains an uninitialized binding (TDZ) for the loop variable. The
            // iterable expression is evaluated with this head env to ensure TDZ accesses
            // throw ReferenceError as per spec.
            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind_opt {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                env_set(mc, &he, var_name, Value::Uninitialized)?;
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
                    get_primitive_prototype_property(mc, env, &iter_val, &PropertyKey::Symbol(*iter_sym_data))?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, method, Some(iter_val.clone()), vec![])?;

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

                    let next_res_val = evaluate_call_dispatch(mc, env, next_method, Some(Value::Object(iter_obj)), vec![])?;

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
                                crate::core::env_set_recursive(mc, env, var_name, value)?;
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
                                env_set(mc, &iter_env, var_name, value)?;
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
                        return Err(EvalError::Js(raise_type_error!("Iterator result is not an object")));
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
                    let val = get_property_with_accessors(mc, env, &obj, &PropertyKey::from(i))?;
                    match decl_kind_opt {
                        Some(crate::core::VarDeclKind::Var) | None => {
                            crate::core::env_set_recursive(mc, env, var_name, val)?;
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
                            env_set(mc, &iter_env, var_name, val)?;
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

            Err(EvalError::Js(raise_type_error!("Value is not iterable")))
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
                    get_primitive_prototype_property(mc, env, &iter_val, &PropertyKey::Symbol(*iter_sym_data))?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, method, Some(iter_val.clone()), vec![])?;

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

                    let next_res_val = evaluate_call_dispatch(mc, env, next_method, Some(Value::Object(iter_obj)), vec![])?;

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
                        evaluate_assign_target_with_value(mc, env, lhs, value.clone())?;
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
                        return Err(EvalError::Js(raise_type_error!("Iterator result is not an object")));
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
                    let val = get_property_with_accessors(mc, env, &obj, &PropertyKey::from(i))?;
                    // Assignment form: assign to lhs expression
                    evaluate_assign_target_with_value(mc, env, lhs, val)?;
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

            Err(EvalError::Js(raise_type_error!("Value is not iterable")))
        }
        StatementKind::ForIn(decl_kind, var_name, iterable, body) => {
            // If this is a lexical (let/const) declaration, create a head env with
            // TDZ binding for the loop variable so that iterable evaluation will see the
            // binding as uninitialized and throw ReferenceError on access.
            let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
            if let Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) = decl_kind {
                let he = new_js_object_data(mc);
                he.borrow_mut(mc).prototype = Some(*env);
                env_set(mc, &he, var_name, Value::Uninitialized)?;
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
                hoist_name(mc, env, var_name)?;
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
                    let own_keys = crate::core::value::ordinary_own_property_keys(&o);
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
                                        if let Value::TypedArray(ta) = &*ta_cell.borrow()
                                            && idx < ta.length
                                        {
                                            match ta.get(idx) {
                                                Ok(num) => log::trace!("for-in property {k} -> {}", num),
                                                Err(_) => log::trace!("for-in property {k} -> <typedarray element error>"),
                                            }
                                            key_present = true;
                                        }
                                    }
                                }
                            }
                            if !key_present {
                                continue;
                            }

                            env_set_recursive(mc, env, var_name, Value::String(utf8_to_utf16(k)))?;
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
                                    && idx < ta.length
                                {
                                    match ta.get(idx) {
                                        Ok(num) => log::trace!("for-in property {k} -> {}", num),
                                        Err(_) => log::trace!("for-in property {k} -> <typedarray element error>"),
                                    }
                                    key_present = true;
                                }
                            }
                            if !key_present {
                                continue;
                            }

                            let iter_env = new_js_object_data(mc);
                            iter_env.borrow_mut(mc).prototype = Some(*env);
                            env_set(mc, &iter_env, var_name, Value::String(utf8_to_utf16(k)))?;
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
                            && idx < ta.length
                        {
                            match ta.get(idx) {
                                Ok(num) => log::trace!("for-in property {k} -> {}", num),
                                Err(_) => log::trace!("for-in property {k} -> <typedarray element error>"),
                            }
                            key_present = true;
                        }
                    }
                    if !key_present {
                        continue;
                    }

                    evaluate_assign_target_with_value(mc, env, lhs, Value::String(utf8_to_utf16(k)))?;
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
                    hoist_name(mc, env, name)?;
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
                    env_set(mc, &he, name, Value::Uninitialized)?;
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
                        object_set_key_value(mc, &boxed, i, Value::String(utf8_to_utf16(&ch.to_string())))?;
                        char_indices.push(ch);
                    }
                    object_set_key_value(mc, &boxed, "length", Value::Number(char_indices.len() as f64))?;

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
                                            env_set(mc, &iter_env, name, prop_val.clone())?;
                                        }
                                        _ => {
                                            crate::core::env_set_recursive(mc, env, name, prop_val.clone())?;
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
            Err(EvalError::Js(raise_type_error!(
                "ForInDestructuringObject only supports Objects currently"
            )))
        }
        StatementKind::ForInDestructuringArray(decl_kind_opt, pattern, iterable, body) => {
            // Hoist var declarations from destructuring pattern (var case)
            let mut names = Vec::new();
            collect_names_from_destructuring(pattern, &mut names);
            for name in names.iter() {
                if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                    hoist_name(mc, env, name)?;
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
                    env_set(mc, &he, name, Value::Uninitialized)?;
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
                        object_set_key_value(mc, &boxed, i, Value::String(utf8_to_utf16(&ch.to_string())))?;
                        char_indices.push(ch);
                    }
                    object_set_key_value(mc, &boxed, "length", Value::Number(char_indices.len() as f64))?;

                    // Perform array destructuring: bind elements from boxed
                    match decl_kind_opt {
                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                            bind_array_inner_for_letconst(
                                mc,
                                &iter_env,
                                pattern,
                                &boxed,
                                matches!(decl_kind_opt, Some(crate::core::VarDeclKind::Const)),
                            )?;
                        }
                        _ => {
                            bind_array_inner_for_var(mc, env, pattern, &boxed)?;
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
            Err(EvalError::Js(raise_type_error!(
                "ForInDestructuringArray only supports Objects currently"
            )))
        }
        StatementKind::ForOfDestructuringObject(decl_kind_opt, pattern, iterable, body) => {
            // Hoist var declarations from destructuring pattern (var case)
            let mut names = Vec::new();
            collect_names_from_object_destructuring(pattern, &mut names);
            for name in names.iter() {
                if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                    hoist_name(mc, env, name)?;
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
                    env_set(mc, &he, name, Value::Uninitialized)?;
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
                    let val = get_property_with_accessors(mc, env, &obj, &PropertyKey::from(i))?;
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
                                    get_property_with_accessors(mc, env, o, &PropertyKey::from(key.clone()))?
                                } else {
                                    Value::Undefined
                                };
                                if let DestructuringElement::Variable(name, _) = value {
                                    match decl_kind_opt {
                                        Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const) => {
                                            env_set(mc, &iter_env, name, prop_val)?;
                                        }
                                        _ => {
                                            crate::core::env_set_recursive(mc, env, name, prop_val)?;
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
            Err(EvalError::Js(raise_type_error!(
                "ForOfDestructuringObject only supports Arrays currently"
            )))
        }
        StatementKind::ForOfDestructuringArray(decl_kind_opt, pattern, iterable, body) => {
            // Hoist var declarations from destructuring pattern (var case)
            let mut names = Vec::new();
            collect_names_from_destructuring(pattern, &mut names);
            for name in names.iter() {
                if let Some(crate::core::VarDeclKind::Var) = decl_kind_opt {
                    hoist_name(mc, env, name)?;
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
                    env_set(mc, &he, name, Value::Uninitialized)?;
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
                    get_primitive_prototype_property(mc, env, &iter_val, &PropertyKey::Symbol(*iter_sym_data))?
                };

                if !matches!(method, Value::Undefined | Value::Null) {
                    let res = evaluate_call_dispatch(mc, env, method, Some(iter_val.clone()), vec![])?;

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

                    let next_res_val = evaluate_call_dispatch(mc, env, next_method, Some(Value::Object(iter_obj)), vec![])?;

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
                                        env_set(mc, &iter_env, name, elem_val)?;
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
                                        crate::core::env_set_recursive(mc, env, name, elem_val)?;
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
                        return Err(EvalError::Js(raise_type_error!("Iterator result is not an object")));
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
                    let val = get_property_with_accessors(mc, env, &obj, &PropertyKey::from(i))?;
                    // Perform array destructuring
                    for (j, elem) in pattern.iter().enumerate() {
                        if let DestructuringElement::Variable(name, _) = elem {
                            let elem_val = if let Value::Object(o) = &val {
                                if is_array(mc, o) {
                                    get_property_with_accessors(mc, env, o, &PropertyKey::from(j))?
                                } else {
                                    Value::Undefined
                                }
                            } else {
                                Value::Undefined
                            };
                            crate::core::env_set_recursive(mc, env, name, elem_val)?;
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
            Err(EvalError::Js(raise_type_error!(
                "ForOfDestructuringArray only supports Arrays currently"
            )))
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

pub fn export_value<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str, val: Value<'gc>) -> Result<(), EvalError<'gc>> {
    if let Some(exports_cell) = env_get(env, "exports") {
        let exports = exports_cell.borrow().clone();
        if let Value::Object(exports_obj) = exports {
            object_set_key_value(mc, &exports_obj, name, val)?;
            return Ok(());
        }
    }

    if let Some(module_cell) = env_get(env, "module") {
        let module = module_cell.borrow().clone();
        if let Value::Object(module_obj) = module
            && let Some(exports_val) = object_get_key_value(&module_obj, "exports")
            && let Value::Object(exports_obj) = &*exports_val.borrow()
        {
            object_set_key_value(mc, exports_obj, name, val)?;
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
    key: &PropertyKey<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let proto_name = match obj_val {
        Value::BigInt(_) => "BigInt",
        Value::Number(_) => "Number",
        Value::String(_) => "String",
        Value::Boolean(_) => "Boolean",
        Value::Symbol(_) => "Symbol",
        Value::Closure(_) | Value::Function(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..) => "Function",
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
            if let Some(desc) = &sd.description {
                return Ok(Value::String(crate::unicode::utf8_to_utf16(desc)));
            }
            return Ok(Value::Undefined);
        }

        if let Some(val) = object_get_key_value(proto, key) {
            return Ok(val.borrow().clone());
        }
    }
    Ok(Value::Undefined)
}

fn evaluate_expr_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    match target {
        Expr::Var(name, _, _) => {
            env_set_recursive(mc, env, name, val.clone())?;
            Ok(val)
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                set_property_with_accessors(mc, env, &obj, &key_val, val.clone())?;
                Ok(val)
            } else {
                Err(EvalError::Js(raise_eval_error!("Cannot assign to property of non-object")))
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
                set_property_with_accessors(mc, env, &obj, &key, val.clone())?;
                Ok(val)
            } else {
                Err(EvalError::Js(raise_eval_error!("Cannot assign to property of non-object")))
            }
        }
        _ => {
            // Diagnostic: report the specific Expr variant of the unsupported assignment target
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
                Expr::OptionalIndex(_, _) => "OptionalIndex",
                Expr::OptionalCall(_, _) => "OptionalCall",
                Expr::TaggedTemplate(_, _, _) => "TaggedTemplate",
                Expr::TemplateString(_) => "TemplateString",
                Expr::GeneratorFunction(_, _, _) => "GeneratorFunction",
                Expr::AsyncFunction(_, _, _) => "AsyncFunction",
                Expr::AsyncArrowFunction(_, _) => "AsyncArrowFunction",
                _ => "Other",
            };
            log::error!("Unsupported assignment target reached in evaluate_expr_assign: {}", variant);
            Err(EvalError::Js(raise_eval_error!("Assignment target not supported")))
        }
    }
}

// Helper: assign a precomputed runtime value to an assignment target expression
fn evaluate_assign_target_with_value<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    val: Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match target {
        Expr::Var(name, _, _) => {
            env_set_recursive(mc, env, name, val.clone())?;
            Ok(val)
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                set_property_with_accessors(mc, env, &obj, &key_val, val.clone())?;
                Ok(val)
            } else {
                Err(EvalError::Js(raise_eval_error!("Cannot assign to property of non-object")))
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
                set_property_with_accessors(mc, env, &obj, &key, val.clone())?;
                Ok(val)
            } else {
                Err(EvalError::Js(raise_eval_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!("Assignment target not supported"))),
    }
}

fn evaluate_expr_add_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            let new_val = match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(ln + rn),
                (Value::BigInt(_), other) | (other, Value::BigInt(_)) => {
                    if matches!(other, Value::String(_)) {
                        return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types in +=")));
                    }
                    return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")));
                }
                (Value::Number(ln), Value::Number(rn)) => Value::Number(ln + rn),
                (Value::String(ls), Value::String(rs)) => {
                    let mut res = ls.clone();
                    res.extend(rs);
                    Value::String(res)
                }
                (Value::String(ls), other) => {
                    let mut res = ls.clone();
                    res.extend(utf8_to_utf16(&value_to_concat_string(&other)));
                    Value::String(res)
                }
                (other, Value::String(rs)) => {
                    let mut res = utf8_to_utf16(&value_to_concat_string(&other));
                    res.extend(rs);
                    Value::String(res)
                }
                _ => return Err(EvalError::Js(raise_eval_error!("AddAssign types invalid"))),
            };
            env_set_recursive(mc, env, name, new_val.clone())?;
            Ok(new_val)
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                let current = get_property_with_accessors(mc, env, &obj, &key_val)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(ln + rn),
                    (Value::BigInt(_), other) | (other, Value::BigInt(_)) => {
                        if matches!(other, Value::String(_)) {
                            return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types in +=")));
                        }
                        return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")));
                    }
                    (Value::Number(ln), Value::Number(rn)) => Value::Number(ln + rn),
                    (Value::String(ls), Value::String(rs)) => {
                        let mut res = ls.clone();
                        res.extend(rs);
                        Value::String(res)
                    }
                    (Value::String(ls), other) => {
                        let mut res = ls.clone();
                        res.extend(utf8_to_utf16(&value_to_concat_string(&other)));
                        Value::String(res)
                    }
                    (other, Value::String(rs)) => {
                        let mut res = utf8_to_utf16(&value_to_concat_string(&other));
                        res.extend(rs);
                        Value::String(res)
                    }
                    _ => return Err(EvalError::Js(raise_eval_error!("AddAssign types invalid"))),
                };
                set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                Ok(new_val)
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key_str = value_to_string(&key_val);
            let key = PropertyKey::from(key_str);
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(ln + rn),
                    (Value::BigInt(_), other) | (other, Value::BigInt(_)) => {
                        if matches!(other, Value::String(_)) {
                            return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types in +=")));
                        }
                        return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")));
                    }
                    (Value::Number(ln), Value::Number(rn)) => Value::Number(ln + rn),
                    (Value::String(ls), Value::String(rs)) => {
                        let mut res = ls.clone();
                        res.extend(rs);
                        Value::String(res)
                    }
                    (Value::String(ls), other) => {
                        let mut res = ls.clone();
                        res.extend(utf8_to_utf16(&value_to_concat_string(&other)));
                        Value::String(res)
                    }
                    (other, Value::String(rs)) => {
                        let mut res = utf8_to_utf16(&value_to_concat_string(&other));
                        res.extend(rs);
                        Value::String(res)
                    }
                    _ => return Err(EvalError::Js(raise_eval_error!("AddAssign types invalid"))),
                };
                set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                Ok(new_val)
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!(
            "AddAssign only for variables, properties or indexes"
        ))),
    }
}

fn evaluate_expr_bitand_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(ln & rn);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                }
                (l, r) => {
                    let l = to_int32_value(&l)?;
                    let r = to_int32_value(&r)?;
                    let new_val = Value::Number((l & r) as f64);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                let current = get_property_with_accessors(mc, env, &obj, &key_val)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(ln & rn);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = to_int32_value(&r)?;
                        let new_val = Value::Number((l & r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key_str = value_to_string(&key_val);
            let key = PropertyKey::from(key_str);
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(ln & rn);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = to_int32_value(&r)?;
                        let new_val = Value::Number((l & r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!(
            "BitAndAssign only for variables, properties or indexes"
        ))),
    }
}

fn evaluate_expr_bitor_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(ln | rn);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                }
                (l, r) => {
                    let l = to_int32_value(&l)?;
                    let r = to_int32_value(&r)?;
                    let new_val = Value::Number((l | r) as f64);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                let current = get_property_with_accessors(mc, env, &obj, &key_val)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(ln | rn);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = to_int32_value(&r)?;
                        let new_val = Value::Number((l | r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key_str = value_to_string(&key_val);
            let key = PropertyKey::from(key_str);
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(ln | rn);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = to_int32_value(&r)?;
                        let new_val = Value::Number((l | r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!(
            "BitOrAssign only for variables, properties or indexes"
        ))),
    }
}

fn evaluate_expr_bitxor_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(ln ^ rn);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                }
                (l, r) => {
                    let l = to_int32_value(&l)?;
                    let r = to_int32_value(&r)?;
                    let new_val = Value::Number((l ^ r) as f64);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                let current = get_property_with_accessors(mc, env, &obj, &key_val)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(ln ^ rn);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = to_int32_value(&r)?;
                        let new_val = Value::Number((l ^ r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key_str = value_to_string(&key_val);
            let key = PropertyKey::from(key_str);
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let new_val = Value::BigInt(ln ^ rn);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = to_int32_value(&r)?;
                        let new_val = Value::Number((l ^ r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!(
            "BitXorAssign only for variables, properties or indexes"
        ))),
    }
}

fn evaluate_expr_leftshift_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let shift = bigint_shift_count(&rn)?;
                    let new_val = Value::BigInt(ln << shift);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                }
                (l, r) => {
                    let l = to_int32_value(&l)?;
                    let r = (to_uint32_value(&r)? & 0x1F) as u32;
                    let new_val = Value::Number(((l << r) as i32) as f64);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                let current = get_property_with_accessors(mc, env, &obj, &key_val)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let shift = bigint_shift_count(&rn)?;
                        let new_val = Value::BigInt(ln << shift);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = (to_uint32_value(&r)? & 0x1F) as u32;
                        let new_val = Value::Number(((l << r) as i32) as f64);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key_str = value_to_string(&key_val);
            let key = PropertyKey::from(key_str);
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let shift = bigint_shift_count(&rn)?;
                        let new_val = Value::BigInt(ln << shift);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = (to_uint32_value(&r)? & 0x1F) as u32;
                        let new_val = Value::Number(((l << r) as i32) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!(
            "LeftShiftAssign only for variables, properties or indexes"
        ))),
    }
}

fn evaluate_expr_rightshift_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let shift = bigint_shift_count(&rn)?;
                    let new_val = Value::BigInt(ln >> shift);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                }
                (l, r) => {
                    let l = to_int32_value(&l)?;
                    let r = (to_uint32_value(&r)? & 0x1F) as u32;
                    let new_val = Value::Number((l >> r) as f64);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                let current = get_property_with_accessors(mc, env, &obj, &key_val)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let shift = bigint_shift_count(&rn)?;
                        let new_val = Value::BigInt(ln >> shift);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = (to_uint32_value(&r)? & 0x1F) as u32;
                        let new_val = Value::Number((l >> r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key_str = value_to_string(&key_val);
            let key = PropertyKey::from(key_str);
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => {
                        let shift = bigint_shift_count(&rn)?;
                        let new_val = Value::BigInt(ln >> shift);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                    }
                    (l, r) => {
                        let l = to_int32_value(&l)?;
                        let r = (to_uint32_value(&r)? & 0x1F) as u32;
                        let new_val = Value::Number((l >> r) as f64);
                        set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                        Ok(new_val)
                    }
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!(
            "RightShiftAssign only for variables, properties or indexes"
        ))),
    }
}

fn evaluate_expr_urightshift_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            match (current, val) {
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(EvalError::Js(crate::raise_type_error!("Unsigned right shift"))),
                (l, r) => {
                    let l = to_uint32_value(&l)?;
                    let r = (to_uint32_value(&r)? & 0x1F) as u32;
                    let new_val = Value::Number((l >> r) as f64);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                let current = get_property_with_accessors(mc, env, &obj, &key_val)?;
                if let Value::BigInt(_) = current {
                    return Err(EvalError::Js(crate::raise_type_error!("Unsigned right shift")));
                }
                let (l, r) = (current, val);
                {
                    let l = to_uint32_value(&l)?;
                    let r = (to_uint32_value(&r)? & 0x1F) as u32;
                    let new_val = Value::Number((l >> r) as f64);
                    set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                    Ok(new_val)
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key_str = value_to_string(&key_val);
            let key = PropertyKey::from(key_str);
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                if let Value::BigInt(_) = current {
                    return Err(EvalError::Js(crate::raise_type_error!("Unsigned right shift")));
                }
                let (l, r) = (current, val);
                {
                    let l = to_uint32_value(&l)?;
                    let r = (to_uint32_value(&r)? & 0x1F) as u32;
                    let new_val = Value::Number((l >> r) as f64);
                    set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                    Ok(new_val)
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!(
            "UnsignedRightShiftAssign only for variables, properties or indexes"
        ))),
    }
}

fn evaluate_expr_sub_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    if let Expr::Var(name, _, _) = target {
        let current = evaluate_var(mc, env, name)?;
        match (current, val) {
            (Value::BigInt(ln), Value::BigInt(rn)) => {
                let new_val = Value::BigInt(ln - rn);
                env_set_recursive(mc, env, name, new_val.clone())?;
                Ok(new_val)
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => {
                // Coerce to numbers and validate not NaN
                let ln = to_number(&l)?;
                let rn = to_number(&r)?;
                if ln.is_nan() || rn.is_nan() {
                    return Err(EvalError::Js(raise_eval_error!("Invalid operands for subtraction")));
                }
                let new_val = Value::Number(ln - rn);
                env_set_recursive(mc, env, name, new_val.clone())?;
                Ok(new_val)
            }
        }
    } else {
        Err(EvalError::Js(raise_eval_error!("SubAssign only for variables")))
    }
}

fn evaluate_expr_mul_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    match target {
        Expr::Var(name, _, _) => {
            let current = evaluate_var(mc, env, name)?;
            match (current, val) {
                (Value::BigInt(ln), Value::BigInt(rn)) => {
                    let new_val = Value::BigInt(ln * rn);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
                }
                (l, r) => {
                    let ln = to_number(&l)?;
                    let rn = to_number(&r)?;
                    if ln.is_nan() || rn.is_nan() {
                        return Err(EvalError::Js(raise_eval_error!("Invalid operands for multiplication")));
                    }
                    let new_val = Value::Number(ln * rn);
                    env_set_recursive(mc, env, name, new_val.clone())?;
                    Ok(new_val)
                }
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                let current = get_property_with_accessors(mc, env, &obj, &key_val)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(ln * rn),
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")));
                    }
                    (l, r) => {
                        let ln = to_number(&l)?;
                        let rn = to_number(&r)?;
                        if ln.is_nan() || rn.is_nan() {
                            return Err(EvalError::Js(raise_eval_error!("Invalid operands for multiplication")));
                        }
                        Value::Number(ln * rn)
                    }
                };
                set_property_with_accessors(mc, env, &obj, &key_val, new_val.clone())?;
                Ok(new_val)
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key_str = value_to_string(&key_val);
            let key = PropertyKey::from(key_str);
            if let Value::Object(obj) = obj_val {
                let current = get_property_with_accessors(mc, env, &obj, &key)?;
                let new_val = match (current, val) {
                    (Value::BigInt(ln), Value::BigInt(rn)) => Value::BigInt(ln * rn),
                    (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                        return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")));
                    }
                    (l, r) => {
                        let ln = to_number(&l)?;
                        let rn = to_number(&r)?;
                        if ln.is_nan() || rn.is_nan() {
                            return Err(EvalError::Js(raise_eval_error!("Invalid operands for multiplication")));
                        }
                        Value::Number(ln * rn)
                    }
                };
                set_property_with_accessors(mc, env, &obj, &key, new_val.clone())?;
                Ok(new_val)
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!(
            "MulAssign only for variables, properties or indexes"
        ))),
    }
}

fn evaluate_expr_div_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    if let Expr::Var(name, _, _) = target {
        let current = evaluate_var(mc, env, name)?;
        let new_val = match (current, val) {
            (Value::BigInt(ln), Value::BigInt(rn)) => {
                if rn.is_zero() {
                    return Err(EvalError::Js(raise_eval_error!("Division by zero")));
                }
                Value::BigInt(ln / rn)
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")));
            }
            (l, r) => {
                let denom = to_number(&r)?;
                if denom == 0.0 {
                    return Err(EvalError::Js(raise_eval_error!("Division by zero")));
                }
                let ln = to_number(&l)?;
                let new = ln / denom;
                if new.is_nan() {
                    return Err(EvalError::Js(raise_eval_error!("Invalid operands for division")));
                }
                Value::Number(new)
            }
        };
        env_set_recursive(mc, env, name, new_val.clone())?;
        Ok(new_val)
    } else {
        Err(EvalError::Js(raise_eval_error!("DivAssign only for variables")))
    }
}

fn evaluate_expr_mod_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    if let Expr::Var(name, _, _) = target {
        let current = evaluate_var(mc, env, name)?;
        let new_val = match (current, val) {
            (Value::BigInt(ln), Value::BigInt(rn)) => {
                if rn.is_zero() {
                    return Err(EvalError::Js(raise_eval_error!("Division by zero")));
                }
                Value::BigInt(ln % rn)
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")));
            }
            (l, r) => {
                let denom = to_number(&r)?;
                if denom == 0.0 {
                    return Err(EvalError::Js(raise_eval_error!("Division by zero")));
                }
                let ln = to_number(&l)?;
                let res = ln % denom;
                if res.is_nan() {
                    return Err(EvalError::Js(raise_eval_error!("Invalid operands for modulo")));
                }
                Value::Number(res)
            }
        };
        env_set_recursive(mc, env, name, new_val.clone())?;
        Ok(new_val)
    } else {
        Err(EvalError::Js(raise_eval_error!("ModAssign only for variables")))
    }
}

fn evaluate_expr_pow_assign<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    target: &Expr,
    value_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let val = evaluate_expr(mc, env, value_expr)?;
    if let Expr::Var(name, _, _) = target {
        let current = evaluate_var(mc, env, name)?;
        let new_val = match (current, val) {
            (Value::BigInt(base), Value::BigInt(exp)) => {
                if exp.sign() == num_bigint::Sign::Minus {
                    return Err(EvalError::Js(crate::raise_range_error!("Exponent must be non-negative")));
                }
                let e = exp
                    .to_u32()
                    .ok_or_else(|| EvalError::Js(crate::raise_range_error!("Exponent too large")))?;
                Value::BigInt(base.pow(e))
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                return Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")));
            }
            (l, r) => Value::Number(to_number(&l)?.powf(to_number(&r)?)),
        };
        env_set_recursive(mc, env, name, new_val.clone())?;
        Ok(new_val)
    } else {
        Err(EvalError::Js(raise_eval_error!("PowAssign only for variables")))
    }
}

pub fn check_top_level_return<'gc>(stmts: &[Statement]) -> Result<(), EvalError<'gc>> {
    for stmt in stmts {
        match &*stmt.kind {
            StatementKind::Return(_) => {
                return Err(EvalError::Js(raise_syntax_error!("Illegal return statement")));
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
            StatementKind::ForIn(_, _, _, body) => check_top_level_return(body)?,
            StatementKind::ForOfDestructuringObject(_, _, _, body) => check_top_level_return(body)?,
            StatementKind::ForOfDestructuringArray(_, _, _, body) => check_top_level_return(body)?,
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
            StatementKind::Export(_, Some(s)) => check_top_level_return(std::slice::from_ref(s))?,
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
        if crate::core::get_own_property(env, &key).is_some() && !env.borrow().is_configurable(&key) {
            return Err(EvalError::Js(crate::raise_type_error!(format!(
                "Cannot declare global function '{}'",
                name
            ))));
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
    let original_strict = object_get_key_value(env, "__is_strict").map(|c| c.borrow().clone());

    // Remove the __is_strict own property temporarily
    let _ = env
        .borrow_mut(mc)
        .properties
        .shift_remove(&PropertyKey::String("__is_strict".to_string()));

    let res = f();

    if let Some(orig) = original_strict {
        object_set_key_value(mc, env, "__is_strict", orig)?;
    } else {
        // No original value -- ensure it remains removed (in case executed code set it)
        let _ = env
            .borrow_mut(mc)
            .properties
            .shift_remove(&PropertyKey::String("__is_strict".to_string()));
    }

    res
}

fn handle_eval_function<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    eval_args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let first_arg = eval_args.first().cloned().unwrap_or(Value::Undefined);
    if let Value::String(script_str) = first_arg {
        let script = utf16_to_utf8(&script_str);
        let mut tokens = tokenize(&script)?;
        if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
            tokens.pop();
        }
        let mut index = 0;
        let statements = parse_statements(&tokens, &mut index)?;

        // If executing in the global environment, perform EvalDeclarationInstantiation
        // checks for FunctionDeclarations per spec: if any function cannot be declared
        // as a global (e.g., conflicts with non-configurable existing property such
        // as 'NaN'), throw a TypeError and do not create any global functions.
        if env.borrow().prototype.is_none() {
            check_global_declarations(env, &statements)?;
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
        let caller_is_strict = object_get_key_value(env, "__is_strict")
            .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
            .unwrap_or(false);

        let is_indirect_eval = object_get_key_value(env, "__is_indirect_eval")
            .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
            .unwrap_or(false);

        let is_strict_eval = starts_with_use_strict || (caller_is_strict && !is_indirect_eval);

        // Prepare execution environment
        let exec_env = if is_strict_eval {
            let new_env = crate::core::new_js_object_data(mc);
            new_env.borrow_mut(mc).prototype = Some(*env);
            new_env.borrow_mut(mc).is_function_scope = true;
            object_set_key_value(mc, &new_env, "__is_strict", Value::Boolean(true))?;
            new_env
        } else {
            *env
        };

        // Execution closure
        let run_stmts = || check_top_level_return(&statements).and_then(|_| evaluate_statements(mc, &exec_env, &statements));

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

pub fn evaluate_call_dispatch<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    func_val: Value<'gc>,
    this_val: Option<Value<'gc>>,
    eval_args: Vec<Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match func_val {
        Value::Closure(cl) => call_closure(mc, &cl, this_val.clone(), &eval_args, env, None),
        Value::GeneratorFunction(_, cl) => match crate::js_generator::handle_generator_function_call(mc, &cl, &eval_args) {
            Ok(v) => Ok(v),
            Err(e) => Err(EvalError::Js(e)),
        },
        Value::Function(name) => {
            if let Some(res) = call_native_function(mc, &name, this_val.clone(), &eval_args, env)? {
                return Ok(res);
            }
            if name == "eval" {
                handle_eval_function(mc, env, &eval_args)
            } else if let Some(method_name) = name.strip_prefix("console.") {
                crate::js_console::handle_console_method(mc, method_name, &eval_args, env)
            } else if let Some(_method) = name.strip_prefix("os.") {
                #[cfg(feature = "os")]
                {
                    let this_val = this_val.clone().unwrap_or(Value::Object(*env));
                    Ok(crate::js_os::handle_os_method(mc, this_val, _method, &eval_args, env)?)
                }
                #[cfg(not(feature = "os"))]
                {
                    Err(EvalError::Js(raise_eval_error!(
                        "os module not enabled. Recompile with --features os"
                    )))
                }
            } else if let Some(_method) = name.strip_prefix("std.") {
                #[cfg(feature = "std")]
                {
                    match _method {
                        "sprintf" => Ok(crate::js_std::sprintf::handle_sprintf_call(&eval_args)?),
                        "tmpfile" => Ok(crate::js_std::tmpfile::create_tmpfile(mc)?),
                        _ => Err(EvalError::Js(raise_eval_error!(format!(
                            "std method '{}' not implemented",
                            _method
                        )))),
                    }
                }
                #[cfg(not(feature = "std"))]
                {
                    Err(EvalError::Js(raise_eval_error!(
                        "std module not enabled. Recompile with --features std"
                    )))
                }
            } else if let Some(_method) = name.strip_prefix("tmp.") {
                #[cfg(feature = "std")]
                {
                    if let Some(Value::Object(this_obj)) = this_val {
                        Ok(crate::js_std::tmpfile::handle_file_method(&this_obj, _method, &eval_args)?)
                    } else {
                        Err(EvalError::Js(raise_eval_error!(
                            "TypeError: tmp method called on incompatible receiver"
                        )))
                    }
                }
                #[cfg(not(feature = "std"))]
                {
                    Err(EvalError::Js(raise_eval_error!(
                        "std module (tmpfile) not enabled. Recompile with --features std"
                    )))
                }
            } else if let Some(method) = name.strip_prefix("Boolean.prototype.") {
                let this_v = this_val.clone().unwrap_or(Value::Undefined);
                Ok(crate::js_boolean::handle_boolean_prototype_method(this_v, method)?)
            } else if let Some(method) = name.strip_prefix("BigInt.prototype.") {
                let this_v = this_val.clone().unwrap_or(Value::Undefined);
                Ok(crate::js_bigint::handle_bigint_object_method(this_v, method, &eval_args)?)
            } else if name == "Object.prototype.toString" {
                let this_v = this_val.clone().unwrap_or(Value::Undefined);
                Ok(handle_object_prototype_to_string(mc, &this_v, env))
            } else if let Some(method) = name.strip_prefix("BigInt.") {
                Ok(crate::js_bigint::handle_bigint_static_method(mc, method, &eval_args, env)?)
            } else if let Some(method) = name.strip_prefix("Number.prototype.") {
                Ok(handle_number_prototype_method(this_val.clone(), method, &eval_args)?)
            } else if let Some(method) = name.strip_prefix("Number.") {
                Ok(handle_number_static_method(method, &eval_args)?)
            } else if let Some(method) = name.strip_prefix("Math.") {
                Ok(handle_math_call(mc, method, &eval_args, env)?)
            } else if let Some(method) = name.strip_prefix("JSON.") {
                Ok(handle_json_method(mc, method, &eval_args, env)?)
            } else if let Some(method) = name.strip_prefix("Reflect.") {
                Ok(crate::js_reflect::handle_reflect_method(mc, method, &eval_args, env)?)
            } else if let Some(method) = name.strip_prefix("Date.prototype.") {
                if let Some(this_obj) = this_val {
                    Ok(handle_date_method(mc, &this_obj, method, &eval_args, env)?)
                } else {
                    Err(EvalError::Js(raise_eval_error!(
                        "TypeError: Date method called on incompatible receiver"
                    )))
                }
            } else if let Some(method) = name.strip_prefix("Date.") {
                Ok(handle_date_static_method(method, &eval_args)?)
            } else if name.starts_with("String.") {
                if name == "String.fromCharCode" {
                    Ok(string_from_char_code(&eval_args)?)
                } else if name == "String.fromCodePoint" {
                    Ok(string_from_code_point(&eval_args)?)
                } else if name == "String.raw" {
                    Ok(string_raw(&eval_args)?)
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
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    let s_vec = match this_v {
                        Value::String(s) => s.clone(),
                        Value::Object(ref obj) => {
                            if let Some(val_rc) = object_get_key_value(obj, "__value__") {
                                if let Value::String(s2) = &*val_rc.borrow() {
                                    s2.clone()
                                } else {
                                    utf8_to_utf16(&value_to_string(&this_v))
                                }
                            } else {
                                utf8_to_utf16(&value_to_string(&this_v))
                            }
                        }
                        _ => utf8_to_utf16(&value_to_string(&this_v)),
                    };
                    Ok(handle_string_method(mc, &s_vec, method, &eval_args, env)?)
                } else {
                    Err(EvalError::Js(raise_eval_error!(format!("Unknown String function: {}", name))))
                }
            } else if let Some(suffix) = name.strip_prefix("Object.") {
                if let Some(method) = suffix.strip_prefix("prototype.") {
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    match method {
                        "valueOf" => Ok(crate::js_object::handle_value_of_method(mc, &this_v, &eval_args, env)?),
                        "toString" => Ok(crate::js_object::handle_to_string_method(mc, &this_v, &eval_args, env)?),
                        "toLocaleString" => Ok(crate::js_object::handle_to_string_method(mc, &this_v, &eval_args, env)?),
                        "hasOwnProperty" | "isPrototypeOf" | "propertyIsEnumerable" => {
                            // Need object wrapper
                            if let Value::Object(o) = this_v {
                                let res_opt = crate::js_object::handle_object_prototype_builtin(mc, &name, &o, &eval_args, env)?;
                                Ok(res_opt.unwrap_or(Value::Undefined))
                            } else {
                                Err(EvalError::Js(raise_type_error!(
                                    "Object.prototype method called on non-object receiver"
                                )))
                            }
                        }
                        _ => Err(EvalError::Js(raise_eval_error!(format!("Unknown Object function: {}", name)))),
                    }
                } else {
                    Ok(crate::js_object::handle_object_method(mc, suffix, &eval_args, env)?)
                }
            } else if let Some(suffix) = name.strip_prefix("Array.") {
                if let Some(method) = suffix.strip_prefix("prototype.") {
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        Ok(crate::js_array::handle_array_instance_method(mc, &obj, method, &eval_args, env)?)
                    } else {
                        Err(EvalError::Js(raise_eval_error!(
                            "TypeError: Array method called on non-object receiver"
                        )))
                    }
                } else {
                    Ok(handle_array_static_method(mc, suffix, &eval_args, env)?)
                }
            } else if name.starts_with("RegExp.") {
                if let Some(method) = name.strip_prefix("RegExp.prototype.") {
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        Ok(crate::js_regexp::handle_regexp_method(mc, &obj, method, &eval_args, env)?)
                    } else {
                        Err(EvalError::Js(raise_type_error!(
                            "RegExp.prototype method called on non-object receiver"
                        )))
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!(format!("Unknown RegExp function: {}", name))))
                }
            } else if name.starts_with("Generator.") {
                if let Some(method) = name.strip_prefix("Generator.prototype.") {
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(gen_rc) = object_get_key_value(&obj, "__generator__") {
                            let gen_val = gen_rc.borrow().clone();
                            if let Value::Generator(gen_ptr) = gen_val {
                                return Ok(crate::js_generator::handle_generator_instance_method(
                                    mc, &gen_ptr, method, &eval_args, env,
                                )?);
                            }
                        }
                        Err(EvalError::Js(raise_eval_error!(
                            "TypeError: Generator.prototype method called on incompatible receiver"
                        )))
                    } else {
                        Err(EvalError::Js(raise_eval_error!(
                            "TypeError: Generator.prototype method called on incompatible receiver"
                        )))
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!(format!("Unknown Generator function: {}", name))))
                }
            } else if name.starts_with("Map.") {
                if let Some(method) = name.strip_prefix("Map.prototype.") {
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(map_val) = object_get_key_value(&obj, "__map__") {
                            if let Value::Map(map_ptr) = &*map_val.borrow() {
                                Ok(crate::js_map::handle_map_instance_method(mc, map_ptr, method, &eval_args, env)?)
                            } else {
                                Err(EvalError::Js(raise_eval_error!(
                                    "TypeError: Map.prototype method called on incompatible receiver"
                                )))
                            }
                        } else {
                            Err(EvalError::Js(raise_eval_error!(
                                "TypeError: Map.prototype method called on incompatible receiver"
                            )))
                        }
                    } else if let Value::Map(map_ptr) = this_v {
                        Ok(crate::js_map::handle_map_instance_method(mc, &map_ptr, method, &eval_args, env)?)
                    } else {
                        Err(EvalError::Js(raise_eval_error!(
                            "TypeError: Map.prototype method called on non-object receiver"
                        )))
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!(format!("Unknown Map function: {}", name))))
                }
            } else if name.starts_with("Map.") {
                if let Some(method) = name.strip_prefix("Map.prototype.") {
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(map_val) = object_get_key_value(&obj, "__map__") {
                            if let Value::Map(map_ptr) = &*map_val.borrow() {
                                Ok(crate::js_map::handle_map_instance_method(mc, map_ptr, method, &eval_args, env)?)
                            } else {
                                Err(EvalError::Js(raise_eval_error!(
                                    "TypeError: Map.prototype method called on incompatible receiver"
                                )))
                            }
                        } else {
                            Err(EvalError::Js(raise_eval_error!(
                                "TypeError: Map.prototype method called on incompatible receiver"
                            )))
                        }
                    } else if let Value::Map(map_ptr) = this_v {
                        Ok(crate::js_map::handle_map_instance_method(mc, &map_ptr, method, &eval_args, env)?)
                    } else {
                        Err(EvalError::Js(raise_eval_error!(
                            "TypeError: Map.prototype method called on non-object receiver"
                        )))
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!(format!("Unknown Map function: {}", name))))
                }
            } else if name.starts_with("WeakMap.") {
                if let Some(method) = name.strip_prefix("WeakMap.prototype.") {
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(wm_val) = object_get_key_value(&obj, "__weakmap__") {
                            if let Value::WeakMap(wm_ptr) = &*wm_val.borrow() {
                                Ok(crate::js_weakmap::handle_weakmap_instance_method(
                                    mc, wm_ptr, method, &eval_args, env,
                                )?)
                            } else {
                                Err(EvalError::Js(raise_eval_error!(
                                    "TypeError: WeakMap.prototype method called on incompatible receiver"
                                )))
                            }
                        } else {
                            Err(EvalError::Js(raise_eval_error!(
                                "TypeError: WeakMap.prototype method called on incompatible receiver"
                            )))
                        }
                    } else if let Value::WeakMap(wm_ptr) = this_v {
                        Ok(crate::js_weakmap::handle_weakmap_instance_method(
                            mc, &wm_ptr, method, &eval_args, env,
                        )?)
                    } else {
                        Err(EvalError::Js(raise_eval_error!(
                            "TypeError: WeakMap.prototype method called on non-object receiver"
                        )))
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!(format!("Unknown Map function: {}", name))))
                }
            } else if name.starts_with("WeakSet.") {
                if let Some(method) = name.strip_prefix("WeakSet.prototype.") {
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(ws_val) = object_get_key_value(&obj, "__weakset__") {
                            if let Value::WeakSet(ws_ptr) = &*ws_val.borrow() {
                                Ok(crate::js_weakset::handle_weakset_instance_method(mc, ws_ptr, method, &eval_args)?)
                            } else {
                                Err(EvalError::Js(raise_eval_error!(
                                    "TypeError: WeakSet.prototype method called on incompatible receiver"
                                )))
                            }
                        } else {
                            Err(EvalError::Js(raise_eval_error!(
                                "TypeError: WeakSet.prototype method called on incompatible receiver"
                            )))
                        }
                    } else if let Value::WeakSet(ws_ptr) = this_v {
                        Ok(crate::js_weakset::handle_weakset_instance_method(mc, &ws_ptr, method, &eval_args)?)
                    } else {
                        Err(EvalError::Js(raise_eval_error!(
                            "TypeError: WeakSet.prototype method called on non-object receiver"
                        )))
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!(format!("Unknown Map function: {}", name))))
                }
            } else if name.starts_with("Set.") {
                if let Some(method) = name.strip_prefix("Set.prototype.") {
                    let this_v = this_val.clone().unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = this_v {
                        if let Some(set_val) = object_get_key_value(&obj, "__set__") {
                            if let Value::Set(set_ptr) = &*set_val.borrow() {
                                Ok(crate::js_set::handle_set_instance_method(
                                    mc,
                                    set_ptr,
                                    this_v.clone(),
                                    method,
                                    &eval_args,
                                    env,
                                )?)
                            } else {
                                Err(EvalError::Js(raise_eval_error!(
                                    "TypeError: Set.prototype method called on incompatible receiver"
                                )))
                            }
                        } else {
                            Err(EvalError::Js(raise_eval_error!(
                                "TypeError: Set.prototype method called on incompatible receiver"
                            )))
                        }
                    } else if let Value::Set(set_ptr) = this_v.clone() {
                        Ok(crate::js_set::handle_set_instance_method(
                            mc,
                            &set_ptr,
                            this_v.clone(),
                            method,
                            &eval_args,
                            env,
                        )?)
                    } else {
                        // Fallback: if `this_v` is an object, check if it has the Set internal slot on the underlying object
                        // This happens when `this_v` is a JSObject wrapping the Set pointer? No, `Value::Set` is separate.
                        // Actually, `this_val` from `Expr::Call` might be just `obj_val`.
                        // If `this_val` is `Value::Object` (which it defaults to in `Expr::Call` matching logic),
                        // it might still fail the `__set__` check if it's not set up yet?

                        // Debug:
                        // println!("Set method call debug: method={}, this_val={:?}", method, this_v);

                        Err(EvalError::Js(raise_eval_error!(
                            "TypeError: Set.prototype method called on non-object receiver"
                        )))
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!(format!("Unknown Set function: {}", name))))
                }
            } else if name.starts_with("Function.") {
                if let Some(method) = name.strip_prefix("Function.prototype.") {
                    if method == "call" {
                        // function.call(thisArg, ...args)
                        let this_v = this_val.clone().unwrap_or(Value::Undefined);
                        let this_arg = eval_args.first().cloned().unwrap_or(Value::Undefined);
                        let call_args = if eval_args.len() > 1 { &eval_args[1..] } else { &[] };
                        // Call this_v with this_arg as this
                        match this_v {
                            Value::Closure(cl) => call_closure(mc, &cl, Some(this_arg), call_args, env, None),
                            Value::Function(n) => {
                                if let Some(res) = call_native_function(mc, &n, Some(this_arg), call_args, env)? {
                                    Ok(res)
                                } else {
                                    Err(EvalError::Js(raise_eval_error!("Native function call failed")))
                                }
                            }
                            Value::Object(obj) => {
                                if let Some(cl_ptr) = object_get_key_value(&obj, "__closure__") {
                                    if let Value::Closure(cl) = &*cl_ptr.borrow() {
                                        call_closure(mc, cl, Some(this_arg), call_args, env, None)
                                    } else {
                                        Err(EvalError::Js(raise_eval_error!("Not a function")))
                                    }
                                } else {
                                    Err(EvalError::Js(raise_eval_error!("Not a function")))
                                }
                            }
                            _ => Err(EvalError::Js(raise_eval_error!("Function.prototype.call called on non-function"))),
                        }
                    } else {
                        let this_v = this_val.clone().unwrap_or(Value::Undefined);
                        handle_function_prototype_method(mc, &this_v, method, &eval_args, env)
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!(format!("Unknown Function method: {}", name))))
                }
            } else {
                let call_env = crate::js_class::prepare_call_env_with_this(mc, Some(env), this_val.clone(), None, &[], None, Some(env))?;
                Ok(crate::js_function::handle_global_function(mc, &name, &eval_args, &call_env)?)
            }
        }
        Value::Object(obj) => {
            if let Some(cl_ptr) = object_get_key_value(&obj, "__closure__") {
                match &*cl_ptr.borrow() {
                    Value::Closure(cl) => {
                        let res = call_closure(mc, cl, this_val.clone(), &eval_args, env, Some(obj));
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
                    Value::AsyncClosure(cl) => {
                        match crate::js_async::handle_async_closure_call(mc, cl, this_val.clone(), &eval_args, env, Some(obj)) {
                            Ok(v) => Ok(v),
                            Err(e) => Err(EvalError::Js(e)),
                        }
                    }
                    Value::GeneratorFunction(_, cl) => match crate::js_generator::handle_generator_function_call(mc, cl, &eval_args) {
                        Ok(v) => Ok(v),
                        Err(e) => Err(EvalError::Js(e)),
                    },
                    _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
                }
            } else if (object_get_key_value(&obj, "__class_def__")).is_some() {
                Err(EvalError::Js(crate::raise_type_error!(
                    "Class constructor cannot be invoked without 'new'"
                )))
            } else if let Some(native_name) = object_get_key_value(&obj, "__native_ctor") {
                match &*native_name.borrow() {
                    Value::String(name) => {
                        if name == &crate::unicode::utf8_to_utf16("Object") {
                            Ok(crate::js_class::handle_object_constructor(mc, &eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("String") {
                            Ok(crate::js_string::string_constructor(mc, &eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Boolean") {
                            Ok(crate::js_boolean::boolean_constructor(&eval_args)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Number") {
                            Ok(number_constructor(mc, &eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("BigInt") {
                            Ok(bigint_constructor(mc, &eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Symbol") {
                            Ok(crate::js_symbol::handle_symbol_call(mc, &eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Array") {
                            Ok(crate::js_array::handle_array_constructor(mc, &eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Function") {
                            Ok(crate::js_function::handle_global_function(mc, "Function", &eval_args, env)?)
                        } else if name == &crate::unicode::utf8_to_utf16("Error")
                            || name == &crate::unicode::utf8_to_utf16("TypeError")
                            || name == &crate::unicode::utf8_to_utf16("ReferenceError")
                            || name == &crate::unicode::utf8_to_utf16("RangeError")
                            || name == &crate::unicode::utf8_to_utf16("SyntaxError")
                        {
                            // For native Error constructors, calling them as a function
                            // should produce a new Error object with the provided message.
                            let msg_val = eval_args.first().cloned().unwrap_or(Value::Undefined);
                            // The constructor's "prototype" property points to the error prototype
                            if let Some(prototype_rc) = object_get_key_value(&obj, "prototype")
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
                            Err(EvalError::Js(raise_eval_error!("Not a function")))
                        }
                    }
                    _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
                }
            } else {
                Err(EvalError::Js(raise_eval_error!("Not a function")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
    }
}

fn evaluate_expr_call<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    func_expr: &Expr,
    args: &[Expr],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let (func_val, this_val) = match func_expr {
        Expr::OptionalProperty(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if obj_val.is_null_or_undefined() {
                // Short-circuit optional chain for method call on null/undefined
                return Ok(Value::Undefined);
            }
            let f_val = if let Value::Object(obj) = &obj_val {
                if let Some(val) = object_get_key_value(obj, key) {
                    val.borrow().clone()
                } else if (key.as_str() == "call" || key.as_str() == "apply") && object_get_key_value(obj, "__closure__").is_some() {
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
            ) && (key == "call" || key == "apply")
            {
                Value::Function(key.to_string())
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, &key.as_str().into())?
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
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let f_val = if let Value::Object(obj) = &obj_val {
                if let Some(val) = object_get_key_value(obj, key) {
                    val.borrow().clone()
                } else if (key.as_str() == "call" || key.as_str() == "apply") && object_get_key_value(obj, "__closure__").is_some() {
                    Value::Function(key.to_string())
                } else {
                    Value::Undefined
                }
            } else if let Value::String(s) = &obj_val
                && key == "length"
            {
                Value::Number(s.len() as f64)
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(EvalError::Js(raise_eval_error!("Cannot read properties of null or undefined")));
            } else if matches!(
                obj_val,
                Value::Closure(_) | Value::Function(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..)
            ) && (key == "call" || key == "apply")
            {
                Value::Function(key.to_string())
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, &key.as_str().into())?
            };
            (f_val, Some(obj_val))
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
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
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(EvalError::Js(raise_eval_error!("Cannot read properties of null or undefined")));
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
                    let len_val = object_get_key_value(&obj, "length").unwrap_or(Gc::new(mc, GcCell::new(Value::Undefined)));
                    let len = if let Value::Number(n) = *len_val.borrow() { n as usize } else { 0 };
                    for k in 0..len {
                        let item = object_get_key_value(&obj, k).unwrap_or(Gc::new(mc, GcCell::new(Value::Undefined)));
                        eval_args.push(item.borrow().clone());
                    }
                } else {
                    // Support generic iterables via Symbol.iterator
                    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
                        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                        && let Some(iter_fn_val) = object_get_key_value(&obj, iter_sym)
                    {
                        // Call iterator method on the object to get an iterator
                        let iterator = match &*iter_fn_val.borrow() {
                            Value::Function(name) => {
                                let call_env = crate::js_class::prepare_call_env_with_this(
                                    mc,
                                    Some(env),
                                    Some(Value::Object(obj)),
                                    None,
                                    &[],
                                    None,
                                    Some(env),
                                )?;
                                evaluate_call_dispatch(mc, &call_env, Value::Function(name.clone()), Some(Value::Object(obj)), vec![])?
                            }
                            Value::Closure(cl) => crate::core::call_closure(mc, cl, Some(Value::Object(obj)), &[], env, None)?,
                            _ => return Err(EvalError::Js(raise_type_error!("Spread target is not iterable"))),
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
                                                Some(Value::Object(iter_obj)),
                                                None,
                                                &[],
                                                None,
                                                Some(env),
                                            )?;
                                            evaluate_call_dispatch(
                                                mc,
                                                &call_env,
                                                Value::Function(name.clone()),
                                                Some(Value::Object(iter_obj)),
                                                vec![],
                                            )?
                                        }
                                        Value::Closure(cl) => call_closure(mc, cl, Some(Value::Object(iter_obj)), &[], env, None)?,
                                        _ => {
                                            return Err(EvalError::Js(raise_type_error!("Iterator.next is not callable")));
                                        }
                                    };

                                    if let Value::Object(res_obj) = res {
                                        let done = if let Some(done_rc) = object_get_key_value(&res_obj, "done") {
                                            if let Value::Boolean(b) = &*done_rc.borrow() { *b } else { false }
                                        } else {
                                            false
                                        };

                                        if done {
                                            break;
                                        }

                                        let value = if let Some(val_rc) = object_get_key_value(&res_obj, "value") {
                                            val_rc.borrow().clone()
                                        } else {
                                            Value::Undefined
                                        };

                                        eval_args.push(value);

                                        continue;
                                    } else {
                                        return Err(EvalError::Js(raise_type_error!("Iterator.next did not return an object")));
                                    }
                                } else {
                                    return Err(EvalError::Js(raise_type_error!("Iterator has no next method")));
                                }
                            }
                        } else {
                            return Err(EvalError::Js(raise_type_error!("Iterator call did not return an object")));
                        }
                    }
                }
            } else {
                return Err(EvalError::Js(raise_type_error!("Spread only implemented for Objects")));
            }
        } else {
            let val = evaluate_expr(mc, env, arg_expr)?;
            eval_args.push(val);
        }
    }

    // If callee appears to be a non-callable primitive and the callee expression is a variable,
    // prefer a TypeError with the variable name (e.g., "a is not a function").
    if !matches!(func_val, Value::Closure(_) | Value::Function(_) | Value::Object(_)) {
        if let Expr::Var(name, ..) = func_expr {
            return Err(EvalError::Js(raise_type_error!(format!("{} is not a function", name))));
        } else {
            return Err(EvalError::Js(raise_eval_error!("Not a function")));
        }
    }

    // Is this a *direct* eval call? (IsDirectEvalCall: callee is an IdentifierReference named "eval")
    let is_direct_eval = matches!(func_expr, Expr::Var(name, ..) if name == "eval");

    // If this is an *indirect* call to the builtin "eval", execute it in the global environment
    let is_indirect_eval_call = matches!(func_val, Value::Function(ref name) if name == "eval") && !is_direct_eval;
    let env_for_call = if is_indirect_eval_call {
        // Walk up prototypes to find the top-level (global) environment
        let mut root_env = *env;
        while let Some(proto) = root_env.borrow().prototype {
            root_env = proto;
        }
        root_env
    } else {
        *env
    };

    if is_indirect_eval_call {
        // Temporarily mark the global env so eval can detect it was called indirectly.
        object_set_key_value(mc, &env_for_call, "__is_indirect_eval", Value::Boolean(true))?;
        let res = evaluate_call_dispatch(mc, &env_for_call, func_val, this_val, eval_args);
        // Remove temporary marker to avoid leaking into future calls.
        let _ = env_for_call
            .borrow_mut(mc)
            .properties
            .shift_remove(&PropertyKey::String("__is_indirect_eval".to_string()));
        res
    } else {
        evaluate_call_dispatch(mc, &env_for_call, func_val, this_val, eval_args)
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
        BinaryOp::Add => match (l_val.clone(), r_val.clone()) {
            (Value::BigInt(ln), Value::BigInt(rn)) => Ok(Value::BigInt(ln + rn)),
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln + rn)),
            (Value::String(ls), Value::String(rs)) => {
                let mut res = ls.clone();
                res.extend(rs);
                Ok(Value::String(res))
            }
            (Value::String(ls), other) => {
                // Ensure ToPrimitive('default') is applied to the non-string operand
                let r_prim = crate::core::to_primitive(mc, &other, "default", env)?;
                let mut res = ls.clone();
                match &r_prim {
                    Value::String(rs) => res.extend(rs.clone()),
                    _ => res.extend(utf8_to_utf16(&value_to_concat_string(&r_prim))),
                }
                Ok(Value::String(res))
            }
            (other, Value::String(rs)) => {
                // Ensure ToPrimitive('default') is applied to the non-string operand
                let l_prim = crate::core::to_primitive(mc, &other, "default", env)?;
                let mut res = match &l_prim {
                    Value::String(ls2) => ls2.clone(),
                    _ => utf8_to_utf16(&value_to_concat_string(&l_prim)),
                };
                res.extend(rs.clone());
                Ok(Value::String(res))
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            _ => {
                // Try ToPrimitive on both operands with 'default' hint and handle string concatenation or numeric addition
                let l_prim = crate::core::to_primitive(mc, &l_val, "default", env)?;
                let r_prim = crate::core::to_primitive(mc, &r_val, "default", env)?;
                match (l_prim, r_prim) {
                    (Value::String(ls), other) => {
                        let mut res = ls.clone();
                        res.extend(utf8_to_utf16(&value_to_concat_string(&other)));
                        Ok(Value::String(res))
                    }
                    (other, Value::String(rs)) => {
                        let mut res = utf8_to_utf16(&value_to_concat_string(&other));
                        res.extend(rs);
                        Ok(Value::String(res))
                    }
                    (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln + rn)),
                    (lprim, rprim) => Ok(Value::Number(to_number(&lprim)? + to_number(&rprim)?)),
                }
            }
        },
        BinaryOp::Sub => match (l_val, r_val) {
            (Value::BigInt(ln), Value::BigInt(rn)) => Ok(Value::BigInt(ln - rn)),
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => Ok(Value::Number(to_number(&l)? - to_number(&r)?)),
        },
        BinaryOp::Mul => match (l_val, r_val) {
            (Value::BigInt(ln), Value::BigInt(rn)) => Ok(Value::BigInt(ln * rn)),
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => Ok(Value::Number(to_number(&l)? * to_number(&r)?)),
        },
        BinaryOp::Div => match (l_val, r_val) {
            (Value::BigInt(ln), Value::BigInt(rn)) => {
                if rn.is_zero() {
                    return Err(EvalError::Js(crate::raise_range_error!("Division by zero")));
                }
                Ok(Value::BigInt(ln / rn))
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => Ok(Value::Number(to_number(&l)? / to_number(&r)?)),
        },
        BinaryOp::LeftShift => match (l_val, r_val) {
            (Value::BigInt(ln), Value::BigInt(rn)) => {
                let shift = bigint_shift_count(&rn)?;
                Ok(Value::BigInt(ln << shift))
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => {
                let l = to_int32_value(&l)?;
                let r = (to_uint32_value(&r)? & 0x1F) as u32;
                Ok(Value::Number(((l << r) as i32) as f64))
            }
        },
        BinaryOp::RightShift => match (l_val, r_val) {
            (Value::BigInt(ln), Value::BigInt(rn)) => {
                let shift = bigint_shift_count(&rn)?;
                Ok(Value::BigInt(ln >> shift))
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => {
                let l = to_int32_value(&l)?;
                let r = (to_uint32_value(&r)? & 0x1F) as u32;
                Ok(Value::Number((l >> r) as f64))
            }
        },
        BinaryOp::UnsignedRightShift => match (l_val, r_val) {
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(EvalError::Js(crate::raise_type_error!("BigInt does not support >>>"))),
            (l, r) => {
                let l = to_uint32_value(&l)?;
                let r = (to_uint32_value(&r)? & 0x1F) as u32;
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
            if let Value::Object(obj) = r_val {
                let key = match l_val {
                    Value::String(s) => utf16_to_utf8(&s),
                    Value::Number(n) => n.to_string(),
                    _ => value_to_string(&l_val),
                };
                // Handle Proxy's has trap if present
                if let Some(proxy_ptr) = crate::core::get_own_property(&obj, &"__proxy__".into())
                    && let Value::Proxy(p) = &*proxy_ptr.borrow()
                {
                    let present = crate::js_proxy::_proxy_has_property(mc, p, &PropertyKey::from(key.clone()))?;
                    return Ok(Value::Boolean(present));
                }
                let present = object_get_key_value(&obj, key).is_some();
                Ok(Value::Boolean(present))
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Right-hand side of 'in' must be an object")))
            }
        }
        BinaryOp::Equal => {
            let eq = loose_equal(mc, l_val, r_val, env)?;
            Ok(Value::Boolean(eq))
        }
        BinaryOp::NotEqual => {
            let eq = loose_equal(mc, l_val, r_val, env)?;
            Ok(Value::Boolean(!eq))
        }
        BinaryOp::GreaterThan => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::Boolean(l > r)),
            (Value::String(l), Value::String(r)) => {
                let ls = crate::unicode::utf16_to_utf8(&l);
                let rs = crate::unicode::utf16_to_utf8(&r);
                Ok(Value::Boolean(ls > rs))
            }
            (Value::BigInt(l), other) => {
                let ln = l.to_f64().unwrap_or(f64::NAN);
                let rn = to_number(&other)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln > rn))
            }
            (other, Value::BigInt(r)) => {
                let ln = to_number(&other)?;
                let rn = r.to_f64().unwrap_or(f64::NAN);
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln > rn))
            }
            (l, r) => {
                let ln = to_number(&l)?;
                let rn = to_number(&r)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln > rn))
            }
        },
        BinaryOp::LessThan => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::Boolean(l < r)),
            (Value::String(l), Value::String(r)) => {
                let ls = crate::unicode::utf16_to_utf8(&l);
                let rs = crate::unicode::utf16_to_utf8(&r);
                Ok(Value::Boolean(ls < rs))
            }
            (Value::BigInt(l), other) => {
                let ln = l.to_f64().unwrap_or(f64::NAN);
                let rn = to_number(&other)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln < rn))
            }
            (other, Value::BigInt(r)) => {
                let ln = to_number(&other)?;
                let rn = r.to_f64().unwrap_or(f64::NAN);
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln < rn))
            }
            (l, r) => {
                let ln = to_number(&l)?;
                let rn = to_number(&r)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln < rn))
            }
        },
        BinaryOp::GreaterEqual => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::Boolean(l >= r)),
            (Value::String(l), Value::String(r)) => {
                let ls = crate::unicode::utf16_to_utf8(&l);
                let rs = crate::unicode::utf16_to_utf8(&r);
                Ok(Value::Boolean(ls >= rs))
            }
            (Value::BigInt(l), other) => {
                let ln = l.to_f64().unwrap_or(f64::NAN);
                let rn = to_number(&other)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln >= rn))
            }
            (other, Value::BigInt(r)) => {
                let ln = to_number(&other)?;
                let rn = r.to_f64().unwrap_or(f64::NAN);
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln >= rn))
            }
            (l, r) => {
                let ln = to_number(&l)?;
                let rn = to_number(&r)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln >= rn))
            }
        },
        BinaryOp::LessEqual => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::Boolean(l <= r)),
            (Value::String(l), Value::String(r)) => {
                let ls = crate::unicode::utf16_to_utf8(&l);
                let rs = crate::unicode::utf16_to_utf8(&r);
                Ok(Value::Boolean(ls <= rs))
            }
            (Value::BigInt(l), other) => {
                let ln = l.to_f64().unwrap_or(f64::NAN);
                let rn = to_number(&other)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln <= rn))
            }
            (other, Value::BigInt(r)) => {
                let ln = to_number(&other)?;
                let rn = r.to_f64().unwrap_or(f64::NAN);
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln <= rn))
            }
            (l, r) => {
                let ln = to_number(&l)?;
                let rn = to_number(&r)?;
                Ok(Value::Boolean(!ln.is_nan() && !rn.is_nan() && ln <= rn))
            }
        },
        BinaryOp::Mod => match (l_val, r_val) {
            (Value::BigInt(ln), Value::BigInt(rn)) => {
                if rn.is_zero() {
                    return Err(EvalError::Js(crate::raise_range_error!("Division by zero")));
                }
                Ok(Value::BigInt(ln % rn))
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => Ok(Value::Number(to_number(&l)? % to_number(&r)?)),
        },
        BinaryOp::Pow => match (l_val, r_val) {
            (Value::BigInt(base), Value::BigInt(exp)) => {
                if exp.sign() == num_bigint::Sign::Minus {
                    return Err(EvalError::Js(crate::raise_range_error!("Exponent must be non-negative")));
                }
                let e = exp
                    .to_u32()
                    .ok_or_else(|| EvalError::Js(crate::raise_range_error!("Exponent too large")))?;
                Ok(Value::BigInt(base.pow(e)))
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => Ok(Value::Number(to_number(&l)?.powf(to_number(&r)?))),
        },
        BinaryOp::BitAnd => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::BigInt(l & r)),
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => {
                let l = to_int32_value(&l)?;
                let r = to_int32_value(&r)?;
                Ok(Value::Number((l & r) as f64))
            }
        },
        BinaryOp::BitOr => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::BigInt(l | r)),
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => {
                let l = to_int32_value(&l)?;
                let r = to_int32_value(&r)?;
                Ok(Value::Number((l | r) as f64))
            }
        },
        BinaryOp::BitXor => match (l_val, r_val) {
            (Value::BigInt(l), Value::BigInt(r)) => Ok(Value::BigInt(l ^ r)),
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(EvalError::Js(crate::raise_type_error!("Cannot mix BigInt and other types")))
            }
            (l, r) => {
                let l = to_int32_value(&l)?;
                let r = to_int32_value(&r)?;
                Ok(Value::Number((l ^ r) as f64))
            }
        },
        BinaryOp::NullishCoalescing => {
            if l_val.is_null_or_undefined() {
                Ok(r_val)
            } else {
                Ok(l_val)
            }
        }
        BinaryOp::InstanceOf => match r_val {
            Value::Object(ctor) => {
                if let Value::Object(obj) = l_val {
                    let res = crate::js_class::is_instance_of(&obj, &ctor)?;
                    Ok(Value::Boolean(res))
                } else {
                    Ok(Value::Boolean(false))
                }
            }
            _ => Err(EvalError::Js(crate::raise_type_error!(
                "Right-hand side of 'instanceof' is not an object"
            ))),
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
            let current = evaluate_var(mc, env, name)?;
            let should_assign = match op {
                LogicalAssignOp::And => is_truthy(&current),
                LogicalAssignOp::Or => !is_truthy(&current),
                LogicalAssignOp::Nullish => matches!(current, Value::Null | Value::Undefined),
            };

            if should_assign {
                let val = evaluate_expr(mc, env, value_expr)?;
                env_set_recursive(mc, env, name, val.clone())?;
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
                let key = PropertyKey::from(key_str.as_str());
                let current = get_property_with_accessors(mc, env, &obj, &key)?;

                let should_assign = match op {
                    LogicalAssignOp::And => is_truthy(&current),
                    LogicalAssignOp::Or => !is_truthy(&current),
                    LogicalAssignOp::Nullish => matches!(current, Value::Null | Value::Undefined),
                };

                if should_assign {
                    let val = evaluate_expr(mc, env, value_expr)?;
                    set_property_with_accessors(mc, env, &obj, &key, val.clone())?;
                    Ok(val)
                } else {
                    Ok(current)
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
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
                let current = get_property_with_accessors(mc, env, &obj, &key)?;

                let should_assign = match op {
                    LogicalAssignOp::And => is_truthy(&current),
                    LogicalAssignOp::Or => !is_truthy(&current),
                    LogicalAssignOp::Nullish => matches!(current, Value::Null | Value::Undefined),
                };

                if should_assign {
                    let val = evaluate_expr(mc, env, value_expr)?;
                    set_property_with_accessors(mc, env, &obj, &key, val.clone())?;
                    Ok(val)
                } else {
                    Ok(current)
                }
            } else {
                Err(EvalError::Js(crate::raise_type_error!("Cannot assign to property of non-object")))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!("Invalid assignment target"))),
    }
}

fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Boolean(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Null | Value::Undefined => false,
        Value::Object(_) | Value::Symbol(_) => true,
        Value::BigInt(b) => !b.is_zero(),
        _ => false,
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
            // Assuming the parser gives us a valid integer string.
            // But it might have 'n' at the end?
            // The parser probably stripped 'n' or it's just digits.
            // If it's from source code "123n", lexer handles it.
            // Let's assume it's parsable.
            let bi = s
                .parse::<BigInt>()
                .map_err(|e| EvalError::Js(raise_eval_error!(format!("Invalid BigInt literal: {e}"))))?;
            Ok(Value::BigInt(bi))
        }
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::Boolean(b) => Ok(Value::Boolean(*b)),
        Expr::Null => Ok(Value::Null),
        Expr::Undefined => Ok(Value::Undefined),
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
            Ok(Value::Boolean(!is_truthy(&val)))
        }
        Expr::Conditional(cond, then_expr, else_expr) => {
            let val = evaluate_expr(mc, env, cond)?;
            if is_truthy(&val) {
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
                env: *env,
                home_object: GcCell::new(None),
                captured_envs: Vec::new(),
                bound_this: None,
                is_arrow: false,
                is_strict,
            };
            let closure_val = Value::GeneratorFunction(name.clone(), Gc::new(mc, closure_data));
            object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
            if let Some(n) = name {
                object_set_key_value(mc, &func_obj, "name", Value::String(utf8_to_utf16(n)))?;
            }

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

            // Set 'constructor' on prototype
            object_set_key_value(mc, &proto_obj, "constructor", Value::Object(func_obj))?;
            // Set 'prototype' on function
            object_set_key_value(mc, &func_obj, "prototype", Value::Object(proto_obj))?;

            Ok(Value::Object(func_obj))
        }
        Expr::AsyncFunction(name, params, body) => {
            // Async functions are represented as objects with an AsyncClosure stored
            // under the hidden '__closure__' property. They inherit from Function.prototype.
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
                env: *env,
                home_object: GcCell::new(None),
                captured_envs: Vec::new(),
                bound_this: None,
                is_arrow: false,
                is_strict,
            };
            let closure_val = Value::AsyncClosure(Gc::new(mc, closure_data));
            object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
            if let Some(n) = name {
                object_set_key_value(mc, &func_obj, "name", Value::String(utf8_to_utf16(n)))?;
            }

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

            // Capture current `this` value for lexical this
            let captured_this = match crate::js_class::evaluate_this(mc, env) {
                Ok(v) => v,
                Err(e) => return Err(EvalError::Js(e)),
            };

            let is_strict = body.first()
                .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
                .unwrap_or(false);

            let closure_data = ClosureData {
                params: params.to_vec(),
                body: body.clone(),
                env: *env,
                home_object: GcCell::new(None),
                captured_envs: Vec::new(),
                bound_this: Some(captured_this),
                is_arrow: true,
                is_strict,
            };
            let closure_val = Value::Closure(Gc::new(mc, closure_data));
            object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;

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

            // Set 'constructor' on prototype and 'prototype' on function
            object_set_key_value(mc, &proto_obj, "constructor", Value::Object(func_obj))?;
            object_set_key_value(mc, &func_obj, "prototype", Value::Object(proto_obj))?;

            Ok(Value::Object(func_obj))
        }
        Expr::Call(func_expr, args) => evaluate_expr_call(mc, env, func_expr, args),
        Expr::New(ctor, args) => evaluate_expr_new(mc, env, ctor, args),

        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;

            if let Value::Object(obj) = &obj_val {
                let val = get_property_with_accessors(mc, env, obj, &key.as_str().into())?;
                // Special-case `__proto__` getter: if not present as an own property, return
                // the internal prototype pointer (or null).
                if let Value::Undefined = val
                    && key == "__proto__"
                {
                    if let Some(proto_ptr) = obj.borrow().prototype {
                        return Ok(Value::Object(proto_ptr));
                    } else {
                        return Ok(Value::Null);
                    }
                }
                Ok(val)
            } else if let Value::String(s) = &obj_val
                && key == "length"
            {
                Ok(Value::Number(s.len() as f64))
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                Err(EvalError::Js(raise_eval_error!("Cannot read properties of null or undefined")))
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, &key.as_str().into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;

            let key = match key_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(n.to_string()),
                Value::Symbol(s) => PropertyKey::Symbol(s),
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
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                Err(EvalError::Js(raise_eval_error!("Cannot read properties of null or undefined")))
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
                            Value::Number(n) => result.extend(crate::unicode::utf8_to_utf16(&n.to_string())),
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
                                            if let Ok(res) =
                                                crate::core::evaluate_call_dispatch(mc, env, method_val, Some(val.clone()), Vec::new())
                                            {
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
            let class_obj = crate::js_class::create_class_object(mc, &class_def.name, &class_def.extends, &class_def.members, env, true)?;
            Ok(class_obj)
        }
        Expr::UnaryNeg(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            match val {
                Value::BigInt(b) => Ok(Value::BigInt(-b)),
                other => Ok(Value::Number(-to_number(&other)?)),
            }
        }
        Expr::UnaryPlus(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            Ok(Value::Number(to_number(&val)?))
        }
        Expr::BitNot(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            match val {
                Value::BigInt(b) => Ok(Value::BigInt(!b)),
                other => {
                    let n = to_int32_value(&other)?;
                    Ok(Value::Number((!n) as f64))
                }
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
                    if object_get_key_value(&obj, "__closure__").is_some() {
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
            let is_truthy = match &lhs {
                Value::Boolean(b) => *b,
                Value::Number(n) => *n != 0.0 && !n.is_nan(),
                Value::String(s) => !s.is_empty(),
                Value::Null | Value::Undefined => false,
                Value::Object(_)
                | Value::Function(_)
                | Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::GeneratorFunction(..)
                | Value::ClassDefinition(_)
                | Value::Symbol(_) => true,
                Value::BigInt(b) => !b.is_zero(),
                _ => false,
            };
            if !is_truthy { Ok(lhs) } else { evaluate_expr(mc, env, right) }
        }
        Expr::LogicalOr(left, right) => {
            let lhs = evaluate_expr(mc, env, left)?;
            let is_truthy = match &lhs {
                Value::Boolean(b) => *b,
                Value::Number(n) => *n != 0.0 && !n.is_nan(),
                Value::String(s) => !s.is_empty(),
                Value::Null | Value::Undefined => false,
                Value::Object(_)
                | Value::Function(_)
                | Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::GeneratorFunction(..)
                | Value::ClassDefinition(_)
                | Value::Symbol(_) => true,
                Value::BigInt(b) => !b.is_zero(),
                _ => false,
            };
            if is_truthy { Ok(lhs) } else { evaluate_expr(mc, env, right) }
        }
        Expr::This => Ok(crate::js_class::evaluate_this(mc, env)?),
        Expr::SuperCall(args) => {
            let mut eval_args = Vec::new();
            for arg in args {
                eval_args.push(evaluate_expr(mc, env, arg)?);
            }
            Ok(crate::js_class::evaluate_super_call(mc, env, &eval_args)?)
        }
        Expr::SuperProperty(prop) => Ok(crate::js_class::evaluate_super_property(mc, env, prop)?),
        Expr::SuperMethod(prop, args) => {
            let mut eval_args = Vec::new();
            for arg in args {
                eval_args.push(evaluate_expr(mc, env, arg)?);
            }
            Ok(crate::js_class::evaluate_super_method(mc, env, prop, &eval_args)?)
        }
        Expr::Super => Ok(crate::js_class::evaluate_super(mc, env)?),
        Expr::OptionalProperty(lhs, prop) => {
            let left_val = evaluate_expr(mc, env, lhs)?;
            if left_val.is_null_or_undefined() {
                Ok(Value::Undefined)
            } else if let Value::Object(obj) = &left_val {
                if let Some(val_rc) = object_get_key_value(obj, prop) {
                    Ok(val_rc.borrow().clone())
                } else {
                    Ok(Value::Undefined)
                }
            } else {
                get_primitive_prototype_property(mc, env, &left_val, &prop.into())
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
                    if let Some(val_rc) = object_get_key_value(obj, &prop_key) {
                        Ok(val_rc.borrow().clone())
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
                    let obj_val = evaluate_expr(mc, env, obj_expr)?;
                    if obj_val.is_null_or_undefined() {
                        return Ok(Value::Undefined);
                    }
                    let mut eval_args = Vec::new();
                    for arg in args {
                        eval_args.push(evaluate_expr(mc, env, arg)?);
                    }

                    let f_val = if let Value::Object(obj) = &obj_val {
                        if let Some(val) = object_get_key_value(obj, key) {
                            val.borrow().clone()
                        } else if key.as_str() == "call" && object_get_key_value(obj, "__closure__").is_some() {
                            Value::Function("call".to_string())
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
                        get_primitive_prototype_property(mc, env, &obj_val, &key.as_str().into())?
                    };

                    match f_val {
                        Value::Function(name) => crate::js_function::handle_global_function(mc, &name, &eval_args, &env.clone()),
                        Value::Closure(c) => call_closure(mc, &c, Some(obj_val.clone()), &eval_args, env, None),
                        _ => Err(EvalError::Js(crate::raise_type_error!("OptionalCall target is not a function"))),
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

                    let mut eval_args = Vec::new();
                    for arg in args {
                        eval_args.push(evaluate_expr(mc, env, arg)?);
                    }

                    let f_val = if let Value::Object(obj) = &obj_val {
                        if let Some(val_rc) = object_get_key_value(obj, &prop_key) {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        }
                    } else {
                        get_primitive_prototype_property(mc, env, &obj_val, &prop_key)?
                    };

                    match f_val {
                        Value::Function(name) => Ok(crate::js_function::handle_global_function(mc, &name, &eval_args, &env.clone())?),
                        Value::Closure(c) => call_closure(mc, &c, Some(obj_val.clone()), &eval_args, env, None),
                        _ => Err(EvalError::Js(crate::raise_type_error!("OptionalCall target is not a function"))),
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
                                let mut eval_args = Vec::new();
                                for arg in args {
                                    eval_args.push(evaluate_expr(mc, env, arg)?);
                                }
                                match left_val {
                                    Value::Function(name) => {
                                        Ok(crate::js_function::handle_global_function(mc, &name, &eval_args, &env.clone())?)
                                    }
                                    Value::Closure(c) => call_closure(mc, &c, None, &eval_args, env, None),
                                    _ => Err(EvalError::Js(crate::raise_type_error!("OptionalCall target is not a function"))),
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
            Expr::Property(obj_expr, key) => {
                let obj_val = evaluate_expr(mc, env, obj_expr)?;
                if obj_val.is_null_or_undefined() {
                    return Err(EvalError::Js(crate::raise_type_error!(
                        "Cannot delete property of null or undefined"
                    )));
                }
                if let Value::Object(obj) = obj_val {
                    let key_val = PropertyKey::from(key.to_string());
                    // Proxy wrapper: delegate to deleteProperty trap
                    if let Some(proxy_ptr) = crate::core::get_own_property(&obj, &"__proxy__".into())
                        && let Value::Proxy(p) = &*proxy_ptr.borrow()
                    {
                        let deleted = crate::js_proxy::proxy_delete_property(mc, p, &key_val)?;
                        return Ok(Value::Boolean(deleted));
                    }

                    if obj.borrow().non_configurable.contains(&key_val) {
                        Err(EvalError::Js(crate::raise_type_error!(format!(
                            "Cannot delete non-configurable property '{key}'",
                        ))))
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
                    return Err(EvalError::Js(crate::raise_type_error!(
                        "Cannot delete property of null or undefined"
                    )));
                }
                let key_val_res = evaluate_expr(mc, env, key_expr)?;
                let key = match &key_val_res {
                    Value::String(s) => PropertyKey::String(utf16_to_utf8(s)),
                    Value::Number(n) => PropertyKey::String(n.to_string()),
                    Value::Symbol(s) => PropertyKey::Symbol(*s),
                    _ => PropertyKey::from(value_to_string(&key_val_res)),
                };
                if let Value::Object(obj) = obj_val {
                    if obj.borrow().non_configurable.contains(&key) {
                        Err(EvalError::Js(crate::raise_type_error!(format!(
                            "Cannot delete non-configurable property '{}'",
                            value_to_string(&key_val_res)
                        ))))
                    } else {
                        let _ = obj.borrow_mut(mc).properties.shift_remove(&key);
                        // Deleting a non-existent property returns true per JS semantics
                        Ok(Value::Boolean(true))
                    }
                } else {
                    Ok(Value::Boolean(true))
                }
            }
            Expr::Var(name, _, _) => Err(EvalError::Js(crate::raise_syntax_error!(format!(
                "Delete of an unqualified identifier '{name}' in strict mode",
            )))),
            _ => Ok(Value::Boolean(true)),
        },
        Expr::Getter(func_expr) => {
            let val = evaluate_expr(mc, env, func_expr)?;
            let closure = match &val {
                Value::Object(obj) => {
                    let c_val = object_get_key_value(obj, "__closure__");
                    if let Some(c_ptr) = c_val {
                        let c_ref = c_ptr.borrow();
                        if let Value::Closure(c) = &*c_ref {
                            *c
                        } else {
                            panic!("Getter function missing __closure__ (not a closure)");
                        }
                    } else {
                        panic!("Getter function missing __closure__");
                    }
                }
                Value::Closure(c) => *c,
                _ => panic!("Expr::Getter evaluated to invalid value: {:?}", val),
            };
            Ok(Value::Getter(closure.body.clone(), closure.env, None))
        }
        Expr::Setter(func_expr) => {
            let val = evaluate_expr(mc, env, func_expr)?;
            let closure = match &val {
                Value::Object(obj) => {
                    let c_val = object_get_key_value(obj, "__closure__");
                    if let Some(c_ptr) = c_val {
                        let c_ref = c_ptr.borrow();
                        if let Value::Closure(c) = &*c_ref {
                            *c
                        } else {
                            panic!("Setter function missing __closure__ (not a closure)");
                        }
                    } else {
                        panic!("Setter function missing __closure__");
                    }
                }
                Value::Closure(c) => *c,
                _ => panic!("Expr::Setter evaluated to invalid value: {:?}", val),
            };
            Ok(Value::Setter(closure.params.clone(), closure.body.clone(), closure.env, None))
        }
        Expr::TaggedTemplate(tag_expr, strings, exprs) => {
            // Evaluate the tag function
            let func_val = evaluate_expr(mc, env, tag_expr.as_ref())?;

            // Create the "segments" (cooked) array and populate it
            let segments_arr = crate::js_array::create_array(mc, env)?;
            for (i, s) in strings.iter().enumerate() {
                object_set_key_value(mc, &segments_arr, i, Value::String(s.clone()))?;
            }
            crate::js_array::set_array_length(mc, &segments_arr, strings.len())?;

            // Create the raw array (use same strings here; raw/cooked handling can be refined later)
            let raw_arr = crate::js_array::create_array(mc, env)?;
            for (i, s) in strings.iter().enumerate() {
                object_set_key_value(mc, &raw_arr, i, Value::String(s.clone()))?;
            }
            crate::js_array::set_array_length(mc, &raw_arr, strings.len())?;

            // Attach raw as a property on segments
            object_set_key_value(mc, &segments_arr, "raw", Value::Object(raw_arr))?;

            // Evaluate substitution expressions
            let mut call_args: Vec<Value<'gc>> = Vec::new();
            call_args.push(Value::Object(segments_arr));
            for e in exprs.iter() {
                let v = evaluate_expr(mc, env, e)?;
                call_args.push(v);
            }

            // Call the tag function with 'undefined' as this
            let res = crate::core::evaluate_call_dispatch(mc, env, func_val, Some(Value::Undefined), call_args)?;
            Ok(res)
        }
        Expr::Await(expr) => {
            log::trace!("DEBUG: Evaluating Await");
            // Evaluate the inner expression and normalize to a Promise using Promise.resolve
            let value = evaluate_expr(mc, env, expr)?;

            // Obtain Promise.resolve from the current environment
            let promise_resolve = if let Some(ctor) = crate::core::env_get(env, "Promise") {
                if let Some(resolve_method) = object_get_key_value(
                    &match ctor.borrow().clone() {
                        Value::Object(o) => o,
                        _ => return Err(EvalError::Js(crate::raise_eval_error!("Promise not object"))),
                    },
                    "resolve",
                ) {
                    resolve_method.borrow().clone()
                } else {
                    return Err(EvalError::Js(crate::raise_eval_error!("Promise.resolve missing")));
                }
            } else {
                return Err(EvalError::Js(crate::raise_eval_error!("Promise not found")));
            };

            // Call Promise.resolve(value)
            let p_val = crate::js_promise::call_function(mc, &promise_resolve, std::slice::from_ref(&value), env)?;

            // If we got a real Promise object, wait until it settles by running the event loop
            if let Value::Object(p_obj) = &p_val
                && let Some(promise_ref) = crate::js_promise::get_promise_from_js_object(p_obj)
            {
                loop {
                    // Check current state
                    let state = promise_ref.borrow().state.clone();
                    match state {
                        PromiseState::Pending => {
                            match crate::js_promise::run_event_loop(mc)? {
                                crate::js_promise::PollResult::Executed => continue,
                                // If event loop reports a timed wait, sleep briefly and then
                                // continue polling so we wait for the promise to settle.
                                crate::js_promise::PollResult::Wait(d) => {
                                    std::thread::sleep(d);
                                    continue;
                                }
                                // No tasks currently queued. Yield the thread briefly
                                // and continue polling instead of returning early. This
                                // ensures `await` blocks until the promise actually
                                // settles (matching Node.js behavior) rather than
                                // observing a still-pending promise when the loop is
                                // momentarily idle.
                                crate::js_promise::PollResult::Empty => {
                                    std::thread::yield_now();
                                    continue;
                                }
                            }
                        }
                        PromiseState::Fulfilled(v) => return Ok(v.clone()),
                        PromiseState::Rejected(r) => return Err(EvalError::Throw(r.clone(), None, None)),
                    }
                }
            }

            // Not a promise object; return the resolved value
            Ok(p_val)
        }
        Expr::Yield(_) | Expr::YieldStar(_) => Err(EvalError::Js(raise_eval_error!("`yield` is only valid inside generator functions"))),
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

            // Capture current `this` value for lexical this
            let captured_this = match crate::js_class::evaluate_this(mc, env) {
                Ok(v) => v,
                Err(e) => return Err(EvalError::Js(e)),
            };

            let is_strict = body.first()
                .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
                .unwrap_or(false);
            let closure_data = ClosureData {
                params: params.to_vec(),
                body: body.clone(),
                env: *env,
                home_object: GcCell::new(None),
                captured_envs: Vec::new(),
                bound_this: Some(captured_this),
                is_arrow: true,
                is_strict,
            };
            let closure_val = Value::AsyncClosure(Gc::new(mc, closure_data));
            object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;

            Ok(Value::Object(func_obj))
        }
        Expr::ValuePlaceholder => Ok(Value::Undefined),
        _ => todo!("{expr:?}"),
    }
}

fn evaluate_var<'gc>(_mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str) -> Result<Value<'gc>, JSError> {
    if let Some(val_ptr) = env_get(env, name) {
        let val = val_ptr.borrow().clone();
        if let Value::Uninitialized = val {
            return Err(raise_reference_error!(format!("Cannot access '{}' before initialization", name)));
        }
        return Ok(val);
    }
    Err(raise_reference_error!(format!("{} is not defined", name)))
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
        if let Some(val) = get_own_property(&cur, &PropertyKey::String("__is_strict".to_string()))
            && matches!(*val.borrow(), Value::Boolean(true))
        {
            env_strict_ancestor = true;
            break;
        }
        proto_iter = cur.borrow().prototype;
    }

    let is_strict = has_body_use_strict || env_strict_ancestor;

    let closure_data = ClosureData {
        params: params.to_vec(),
        body: body.to_vec(),
        env: *env,
        home_object: GcCell::new(None),
        captured_envs: Vec::new(),
        bound_this: None,
        is_arrow: false,
        is_strict,
    };
    let closure_val = Value::Closure(Gc::new(mc, closure_data));
    object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
    if let Some(n) = name {
        object_set_key_value(mc, &func_obj, "name", Value::String(utf8_to_utf16(&n)))?;
    }

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

    // Set 'constructor' on prototype
    object_set_key_value(mc, &proto_obj, "constructor", Value::Object(func_obj))?;
    // Set 'prototype' on function
    object_set_key_value(mc, &func_obj, "prototype", Value::Object(proto_obj))?;

    Ok(Value::Object(func_obj))
}

pub(crate) fn get_property_with_accessors<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // If this object is a Proxy wrapper, delegate to proxy hooks
    if let Some(proxy_ptr) = crate::core::get_own_property(obj, &"__proxy__".into())
        && let Value::Proxy(p) = &*proxy_ptr.borrow()
    {
        let res_opt = crate::js_proxy::proxy_get_property(mc, p, key)?;
        if let Some(v) = res_opt {
            return Ok(v);
        } else {
            return Ok(Value::Undefined);
        }
    }

    if let Some(val_ptr) = object_get_key_value(obj, key) {
        let val = val_ptr.borrow().clone();
        match val {
            Value::Property { getter, value, .. } => {
                if let Some(g) = getter {
                    return call_accessor(mc, env, obj, &g);
                }
                if let Some(v) = value {
                    return Ok(v.borrow().clone());
                }
                Ok(Value::Undefined)
            }
            Value::Getter(..) => call_accessor(mc, env, obj, &val),
            _ => Ok(val),
        }
    } else {
        Ok(Value::Undefined)
    }
}

fn set_property_with_accessors<'gc>(
    mc: &MutationContext<'gc>,
    _env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
    val: Value<'gc>,
) -> Result<(), EvalError<'gc>> {
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
                        return Err(EvalError::Js(raise_type_error!("Cannot change prototype of non-extensible object")));
                    }
                }
                // Update internal prototype pointer; do NOT create an own enumerable '__proto__' property
                obj.borrow_mut(mc).prototype = Some(*proto_obj);
                return Ok(());
            }
            Value::Null => {
                if !obj.borrow().is_extensible() && obj.borrow().prototype.is_some() {
                    return Err(EvalError::Js(raise_type_error!("Cannot change prototype of non-extensible object")));
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
                    return Err(EvalError::Js(crate::raise_range_error!("Invalid array length")));
                }
                if *n < 0.0 {
                    return Err(EvalError::Js(crate::raise_range_error!("Invalid array length")));
                }
                if *n != n.trunc() {
                    return Err(EvalError::Js(crate::raise_range_error!("Invalid array length")));
                }
                let new_len = *n as usize;
                crate::core::object_set_length(mc, obj, new_len)?;
                return Ok(());
            }
            _ => {
                // In JS, setting length to non-number triggers ToUint32 conversion; for now, reject
                return Err(EvalError::Js(crate::raise_range_error!("Invalid array length")));
            }
        }
    }

    // If this object is a Proxy wrapper, delegate to its set trap
    if let Some(proxy_ptr) = crate::core::get_own_property(obj, &"__proxy__".into())
        && let Value::Proxy(p) = &*proxy_ptr.borrow()
    {
        // Proxy#set returns boolean; ignore the boolean here but propagate errors
        let _ok = crate::js_proxy::proxy_set_property(mc, p, key, val)?;
        return Ok(());
    }

    if let Some(prop_ptr) = object_get_key_value(obj, key) {
        let prop = prop_ptr.borrow().clone();
        match prop {
            Value::Property { setter, getter, .. } => {
                if let Some(s) = setter {
                    return call_setter(mc, obj, &s, val);
                }
                if getter.is_some() {
                    return Err(EvalError::Js(crate::raise_type_error!(
                        "Cannot set property which has only a getter"
                    )));
                }
                // If the existing property is non-writable, TypeError should be thrown
                if !obj.borrow().is_writable(key) {
                    return Err(EvalError::Js(crate::raise_type_error!("Cannot assign to read-only property")));
                }
                object_set_key_value(mc, obj, key, val)?;
                Ok(())
            }
            Value::Setter(params, body, captured_env, _) => call_setter_raw(mc, obj, &params, &body, &captured_env, val),
            Value::Getter(..) => Err(EvalError::Js(crate::raise_type_error!(
                "Cannot set property which has only a getter"
            ))),
            _ => {
                // For plain existing properties, respect writability
                if !obj.borrow().is_writable(key) {
                    return Err(EvalError::Js(crate::raise_type_error!("Cannot assign to read-only property")));
                }
                object_set_key_value(mc, obj, key, val)?;
                Ok(())
            }
        }
    } else {
        object_set_key_value(mc, obj, key, val)?;
        Ok(())
    }
}

pub fn call_native_function<'gc>(
    mc: &MutationContext<'gc>,
    name: &str,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    // Special-case built-in iterator `next()` so iterator internal throws (EvalError::Throw)
    // propagate as JS-level throws without being converted to JSError.
    if name == "ArrayIterator.prototype.next" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            match crate::js_array::handle_array_iterator_next(mc, &obj, env) {
                Ok(v) => return Ok(Some(v)),
                Err(e) => return Err(e),
            }
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "ArrayIterator.prototype.next called on non-object"
            )));
        }
    }

    if name == "call" || name == "Function.prototype.call" {
        let this = this_val.ok_or_else(|| EvalError::Js(raise_eval_error!("Cannot call call without this")))?;
        let new_this = args.first().cloned().unwrap_or(Value::Undefined);
        let rest_args = if args.is_empty() { &[] } else { &args[1..] };
        return match this {
            Value::Closure(cl) => Ok(Some(call_closure(mc, &cl, Some(new_this), rest_args, env, None)?)),

            Value::Function(func_name) => {
                if let Some(res) = call_native_function(mc, &func_name, Some(new_this.clone()), rest_args, env)? {
                    Ok(Some(res))
                } else {
                    let call_env = crate::core::new_js_object_data(mc);
                    call_env.borrow_mut(mc).prototype = Some(*env);
                    call_env.borrow_mut(mc).is_function_scope = true;
                    object_set_key_value(mc, &call_env, "this", new_this.clone())?;
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
                        &func_name,
                        rest_args,
                        &target_env_for_call,
                    )?))
                }
            }
            Value::Object(obj) => {
                if let Some(cl_ptr) = object_get_key_value(&obj, "__closure__") {
                    match &*cl_ptr.borrow() {
                        Value::Closure(cl) => Ok(Some(call_closure(mc, cl, Some(new_this), rest_args, env, Some(obj))?)),

                        _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!("Not a function")))
                }
            }
            _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
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
            let len_val = object_get_key_value(&obj, "length").unwrap_or(Gc::new(mc, GcCell::new(Value::Undefined)));
            let len = if let Value::Number(n) = *len_val.borrow() { n as usize } else { 0 };
            for k in 0..len {
                let item = object_get_key_value(&obj, k).unwrap_or(Gc::new(mc, GcCell::new(Value::Undefined)));
                rest_args.push(item.borrow().clone());
            }
        }

        return match this {
            Value::Closure(cl) => Ok(Some(call_closure(mc, &cl, Some(new_this), &rest_args, env, None)?)),
            Value::Function(func_name) => {
                if let Some(res) = call_native_function(mc, &func_name, Some(new_this.clone()), &rest_args, env)? {
                    Ok(Some(res))
                } else {
                    let call_env = crate::core::new_js_object_data(mc);
                    call_env.borrow_mut(mc).prototype = Some(*env);
                    call_env.borrow_mut(mc).is_function_scope = true;
                    object_set_key_value(mc, &call_env, "this", new_this.clone())?;
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
                        &func_name,
                        &rest_args,
                        &target_env_for_call,
                    )?))
                }
            }
            Value::Object(obj) => {
                if let Some(cl_ptr) = object_get_key_value(&obj, "__closure__") {
                    match &*cl_ptr.borrow() {
                        Value::Closure(cl) => Ok(Some(call_closure(mc, cl, Some(new_this), &rest_args, env, Some(obj))?)),
                        _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
                    }
                } else {
                    Err(EvalError::Js(raise_eval_error!("Not a function")))
                }
            }
            _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
        };
    }

    if name == "toString" {
        let this = this_val.unwrap_or(Value::Undefined);
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
        return Ok(Some(this_val.unwrap_or(Value::Undefined)));
    }
    if name == "StringIterator.prototype.next" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_string::handle_string_iterator_next(mc, &obj)?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: StringIterator.prototype.next called on non-object"
            )));
        }
    }
    if name == "MapIterator.prototype.next" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_map::handle_map_iterator_next(mc, &obj, env)?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: MapIterator.prototype.next called on non-object"
            )));
        }
    }

    if name == "ArrayIterator.prototype.next" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_array::handle_array_iterator_next(mc, &obj, env)?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: ArrayIterator.prototype.next called on non-object"
            )));
        }
    }

    if name == "SetIterator.prototype.next" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_set::handle_set_iterator_next(mc, &obj, env)?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: SetIterator.prototype.next called on non-object"
            )));
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
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        return Ok(Some(crate::js_symbol::handle_symbol_tostring(mc, this_v)?));
    }

    if name == "Symbol.prototype.valueOf" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        return Ok(Some(crate::js_symbol::handle_symbol_valueof(mc, this_v)?));
    }

    if name.starts_with("Map.")
        && let Some(method) = name.strip_prefix("Map.prototype.")
    {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            if let Some(map_val) = object_get_key_value(&obj, "__map__") {
                if let Value::Map(map_ptr) = &*map_val.borrow() {
                    return Ok(Some(crate::js_map::handle_map_instance_method(mc, map_ptr, method, args, env)?));
                } else {
                    return Err(EvalError::Js(raise_eval_error!(
                        "TypeError: Map.prototype method called on incompatible receiver"
                    )));
                }
            } else {
                return Err(EvalError::Js(raise_eval_error!(
                    "TypeError: Map.prototype method called on incompatible receiver"
                )));
            }
        } else if let Value::Map(map_ptr) = this_v {
            return Ok(Some(crate::js_map::handle_map_instance_method(mc, &map_ptr, method, args, env)?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: Map.prototype method called on non-object receiver"
            )));
        }
    }

    if name.starts_with("Set.")
        && let Some(method) = name.strip_prefix("Set.prototype.")
    {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            if let Some(set_val) = object_get_key_value(&obj, "__set__") {
                if let Value::Set(set_ptr) = &*set_val.borrow() {
                    return Ok(Some(crate::js_set::handle_set_instance_method(
                        mc,
                        set_ptr,
                        this_v.clone(),
                        method,
                        args,
                        env,
                    )?));
                } else {
                    return Err(EvalError::Js(raise_eval_error!(
                        "TypeError: Set.prototype method called on incompatible receiver"
                    )));
                }
            } else {
                return Err(EvalError::Js(raise_eval_error!(
                    "TypeError: Set.prototype method called on incompatible receiver"
                )));
            }
        } else if let Value::Set(set_ptr) = this_v {
            return Ok(Some(crate::js_set::handle_set_instance_method(
                mc,
                &set_ptr,
                Value::Set(set_ptr),
                method,
                args,
                env,
            )?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: Set.prototype method called on non-object receiver"
            )));
        }
    }

    if name.starts_with("DataView.prototype.")
        && let Some(method) = name.strip_prefix("DataView.prototype.")
    {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_typedarray::handle_dataview_method(mc, &obj, method, args, env)?));
        } else {
            return Err(EvalError::Js(raise_eval_error!("TypeError: DataView method called on non-object")));
        }
    }

    if name.starts_with("Atomics.")
        && let Some(method) = name.strip_prefix("Atomics.")
    {
        return Ok(Some(crate::js_typedarray::handle_atomics_method(mc, method, args, env)?));
    }

    if name == "ArrayBuffer.prototype.byteLength" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_typedarray::handle_arraybuffer_accessor(mc, &obj, "byteLength")?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: ArrayBuffer.prototype.byteLength called on non-object"
            )));
        }
    }

    if name == "SharedArrayBuffer.prototype.byteLength" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_typedarray::handle_arraybuffer_accessor(mc, &obj, "byteLength")?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: SharedArrayBuffer.prototype.byteLength called on non-object"
            )));
        }
    }

    if let Some(prop) = name.strip_prefix("TypedArray.prototype.") {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_typedarray::handle_typedarray_accessor(mc, &obj, prop)?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: TypedArray accessor called on non-object"
            )));
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
            if let Some(res) = call_native_function(mc, name, Some(Value::Object(*receiver)), &[], env)? {
                Ok(res)
            } else {
                Err(EvalError::Js(crate::raise_type_error!(format!(
                    "Accessor function {} not supported",
                    name
                ))))
            }
        }
        Value::Getter(body, captured_env, home_opt) => {
            let call_env = crate::core::new_js_object_data(mc);
            call_env.borrow_mut(mc).prototype = Some(*captured_env);
            call_env.borrow_mut(mc).is_function_scope = true;
            object_set_key_value(mc, &call_env, "this", Value::Object(*receiver))?;
            // If the getter carried a home object, propagate it into call env so `super` resolves
            if let Some(home_box) = home_opt
                && let Value::Object(home_obj) = &**home_box
            {
                object_set_key_value(mc, &call_env, "__home_object__", Value::Object(*home_obj))?;
            }
            let body_clone = body.clone();
            evaluate_statements(mc, &call_env, &body_clone)
        }
        Value::Closure(cl) => {
            let cl_data = cl;
            let call_env = crate::core::new_js_object_data(mc);
            call_env.borrow_mut(mc).prototype = Some(cl_data.env);
            call_env.borrow_mut(mc).is_function_scope = true;
            object_set_key_value(mc, &call_env, "this", Value::Object(*receiver))?;
            // Propagate [[HomeObject]] if present on the closure
            if let Some(home_obj) = *cl_data.home_object.borrow() {
                object_set_key_value(mc, &call_env, "__home_object__", Value::Object(home_obj))?;
            }
            let body_clone = cl_data.body.clone();
            evaluate_statements(mc, &call_env, &body_clone)
        }
        Value::Object(obj) => {
            // Check for __closure__
            let cl_val_opt = object_get_key_value(obj, "__closure__");
            if let Some(cl_val) = cl_val_opt
                && let Value::Closure(cl) = &*cl_val.borrow()
            {
                // If the function object has a stored __home_object__, propagate that into the call env
                if let Some(home_val_rc) = object_get_key_value(obj, "__home_object__")
                    && let Value::Object(home_obj) = &*home_val_rc.borrow()
                {
                    let cl_data = cl;
                    let call_env = crate::core::new_js_object_data(mc);
                    call_env.borrow_mut(mc).prototype = Some(cl_data.env);
                    call_env.borrow_mut(mc).is_function_scope = true;
                    object_set_key_value(mc, &call_env, "this", Value::Object(*receiver))?;
                    object_set_key_value(mc, &call_env, "__home_object__", Value::Object(*home_obj))?;
                    let body_clone = cl_data.body.clone();
                    return evaluate_statements(mc, &call_env, &body_clone);
                }
                return call_accessor(mc, env, receiver, &Value::Closure(*cl));
            }
            Err(EvalError::Js(crate::raise_type_error!("Accessor is not a function")))
        }
        _ => Err(EvalError::Js(crate::raise_type_error!("Accessor is not a function"))),
    }
}

fn call_setter<'gc>(
    mc: &MutationContext<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    setter: &Value<'gc>,
    val: Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    match setter {
        Value::Setter(params, body, captured_env, _) => call_setter_raw(mc, receiver, params, body, captured_env, val),
        Value::Closure(cl) => {
            let cl_data = cl;
            let call_env = crate::core::new_js_object_data(mc);
            call_env.borrow_mut(mc).prototype = Some(cl_data.env);
            call_env.borrow_mut(mc).is_function_scope = true;
            object_set_key_value(mc, &call_env, "this", Value::Object(*receiver))?;

            if let Some(first_param) = cl_data.params.first()
                && let DestructuringElement::Variable(name, _) = first_param
            {
                crate::core::env_set(mc, &call_env, name, val)?;
            }
            let body_clone = cl_data.body.clone();
            evaluate_statements(mc, &call_env, &body_clone).map(|_| ())
        }
        Value::Object(obj) => {
            // Check for __closure__
            let cl_val_opt = object_get_key_value(obj, "__closure__");
            if let Some(cl_val) = cl_val_opt
                && let Value::Closure(cl) = &*cl_val.borrow()
            {
                return call_setter(mc, receiver, &Value::Closure(*cl), val);
            }
            Err(EvalError::Js(crate::raise_type_error!("Setter is not a function")))
        }
        _ => Err(EvalError::Js(crate::raise_type_error!("Setter is not a function"))),
    }
}

fn call_setter_raw<'gc>(
    mc: &MutationContext<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    params: &[DestructuringElement],
    body: &[Statement],
    env: &JSObjectDataPtr<'gc>,
    val: Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    let call_env = crate::core::new_js_object_data(mc);
    call_env.borrow_mut(mc).prototype = Some(*env);
    call_env.borrow_mut(mc).is_function_scope = true;
    object_set_key_value(mc, &call_env, "this", Value::Object(*receiver))?;

    if let Some(param) = params.first()
        && let DestructuringElement::Variable(name, _) = param
    {
        crate::core::env_set(mc, &call_env, name, val)?;
    }
    let body_clone = body.to_vec();
    evaluate_statements(mc, &call_env, &body_clone).map(|_| ())
}

fn js_error_to_value<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, js_err: &JSError) -> Value<'gc> {
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

    // Prefer global (root) constructors so created Error objects have the same
    // constructor identity as user-visible global constructors (avoids mismatches
    // when errors are created in nested or eval environments).
    let mut root_env = *env;
    while let Some(proto) = root_env.borrow().prototype {
        root_env = proto;
    }

    let error_proto = if let Some(err_ctor_val) = object_get_key_value(&root_env, name)
        && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(err_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else if let Some(err_ctor_val) = object_get_key_value(&root_env, "Error")
        && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(err_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else if let Some(err_ctor_val) = object_get_key_value(env, name)
        && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(err_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else if let Some(err_ctor_val) = object_get_key_value(env, "Error")
        && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(err_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let err_val = create_error(mc, error_proto, (&raw_msg).into()).unwrap_or(Value::Undefined);

    if let Value::Object(obj) = &err_val {
        obj.borrow_mut(mc).set_property(mc, "name", name.into());
        obj.borrow_mut(mc).set_property(mc, "message", (&raw_msg).into());

        let stack = js_err.stack();
        let stack_str = if stack.is_empty() {
            format!("{name}: {raw_msg}")
        } else {
            format!("{name}: {raw_msg}\n    {}", stack.join("\n    "))
        };
        obj.borrow_mut(mc).set_property(mc, "stack", stack_str.into());
    }
    err_val
}

pub fn call_closure<'gc>(
    mc: &MutationContext<'gc>,
    cl: &crate::core::ClosureData<'gc>,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    fn_obj: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let call_env = crate::core::new_js_object_data(mc);
    call_env.borrow_mut(mc).prototype = Some(cl.env);
    call_env.borrow_mut(mc).is_function_scope = true;

    // Determine whether this function is strict (either via its own 'use strict'
    // directive at creation time or because its lexical environment is marked strict).
    // Determine whether any ancestor of the function's lexical environment is strict.
    let mut proto_iter = Some(cl.env);
    let mut env_strict_ancestor = false;
    while let Some(cur) = proto_iter {
        if let Some(val) = get_own_property(&cur, &PropertyKey::String("__is_strict".to_string()))
            && matches!(*val.borrow(), Value::Boolean(true))
        {
            env_strict_ancestor = true;
            break;
        }
        proto_iter = cur.borrow().prototype;
    }

    let fn_is_strict = cl.is_strict || env_strict_ancestor;
    if fn_is_strict {
        object_set_key_value(mc, &call_env, "__is_strict", Value::Boolean(true))?;
    }

    // If this is a Named Function Expression and the function object has a
    // `name` property, bind that name in the function's call environment so the
    // function can reference itself by name (e.g., `fac` inside `function fac ...`).
    if let Some(fn_obj_ptr) = fn_obj {
        if let Some(name) = fn_obj_ptr.borrow().get_property("name") {
            crate::core::env_set(mc, &call_env, &name, Value::Object(fn_obj_ptr))?;
            // Also set a frame name on the call environment so thrown errors can
            // indicate which function they occurred in (used in stack traces).
            object_set_key_value(mc, &call_env, "__frame", Value::String(utf8_to_utf16(&name)))?;
        }
        // Link caller environment so stacks can be assembled by walking __caller
        object_set_key_value(mc, &call_env, "__caller", Value::Object(*env))?;
    }

    // Determine the [[This]] binding for the call.
    // If the closure has a bound_this (from bind() or arrow capture), use it.
    // Otherwise, if a caller supplied an explicit this_val, use it.
    // If no this_val was supplied (bare call), we must default according to the function's strictness:
    // - strict functions: undefined
    // - non-strict functions: global object
    let effective_this = if let Some(bound) = &cl.bound_this {
        Some(bound.clone())
    } else if let Some(tv) = this_val {
        Some(tv)
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

    // Always place a 'this' binding in the call environment (may be undefined)
    if let Some(tv) = effective_this {
        object_set_key_value(mc, &call_env, "this", tv.clone())?;
    }

    // If the function was stored as an object with a `__home_object__` property, propagate
    // that into the call environment (this covers methods defined in object-literals).
    if let Some(fn_obj_ptr) = fn_obj
        && let Some(home_val_rc) = object_get_key_value(&fn_obj_ptr, "__home_object__")
        && let Value::Object(home_obj) = &*home_val_rc.borrow()
    {
        object_set_key_value(mc, &call_env, "__home_object__", Value::Object(*home_obj))?;
    }

    // FIX: propagate [[HomeObject]] into call_env so `super` resolves parent prototype and avoids recursive lookup
    // Propagate home object into the call environment so `super.*` can resolve
    // the proper parent prototype during method calls.
    if let Some(home_obj) = *cl.home_object.borrow() {
        // Debug: indicate that we are propagating the home object into the call environment
        object_set_key_value(mc, &call_env, "__home_object__", Value::Object(home_obj))?;
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

        object_set_key_value(mc, &args_obj, "length", Value::Number(args.len() as f64))?;

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
                    Value::Symbol(s) => s.description.clone().unwrap_or_default(), // logic for symbol key?
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
            object_set_key_value(mc, &args_obj, i, val.clone())?;
        }

        // Minimal arguments object: expose numeric properties and length
        // We use create_arguments_object here so we get consistent behavior with strict mode callee/caller restrictions
        // object_set_length(mc, &args_obj, args.len())?;

        // if let Some(fn_ptr) = fn_obj {
        //     object_set_key_value(mc, &args_obj, "callee", Value::Object(fn_ptr))?;
        // }

        let callee_val = fn_obj.map(Value::Object);
        crate::js_class::create_arguments_object(mc, &call_env, args, callee_val)?;

        // env_set(mc, &call_env, "arguments", Value::Object(args_obj))?;
    }

    for (i, param) in cl.params.iter().enumerate() {
        match param {
            DestructuringElement::Variable(name, default_expr_opt) => {
                let mut arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                if matches!(arg_val, Value::Undefined)
                    && let Some(default_expr) = default_expr_opt
                {
                    arg_val = evaluate_expr(mc, &call_env, default_expr)?;
                }
                crate::core::env_set(mc, &call_env, name, arg_val)?;
            }
            DestructuringElement::Rest(name) => {
                let rest_args = if i < args.len() { args[i..].to_vec() } else { Vec::new() };
                let array_obj = crate::js_array::create_array(mc, env)?;
                for (j, val) in rest_args.iter().enumerate() {
                    object_set_key_value(mc, &array_obj, j, val.clone())?;
                }
                crate::js_array::set_array_length(mc, &array_obj, rest_args.len())?;
                crate::core::env_set(mc, &call_env, name, Value::Object(array_obj))?;
            }
            DestructuringElement::NestedArray(inner_pattern) => {
                let arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                if let Value::Object(obj) = &arg_val
                    && is_array(mc, obj)
                {
                    bind_array_inner_for_letconst(mc, &call_env, inner_pattern, obj, false)?;
                }
            }
            DestructuringElement::NestedObject(inner_pattern) => {
                let arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                if let Value::Object(obj) = &arg_val {
                    bind_object_inner_for_letconst(mc, &call_env, inner_pattern, obj, false)?;
                }
            }
            _ => {}
        }
    }
    let body_clone = cl.body.clone();
    // Use the lower-level evaluator to distinguish an explicit `return`
    match evaluate_statements_with_labels(mc, &call_env, &body_clone, &[], &[])? {
        ControlFlow::Return(val) => Ok(val),
        ControlFlow::Normal(_) => Ok(Value::Undefined),
        ControlFlow::Throw(v, line, column) => Err(EvalError::Throw(v, line, column)),
        ControlFlow::Break(_) => Err(EvalError::Js(raise_syntax_error!("break statement not in loop or switch"))),
        ControlFlow::Continue(_) => Err(EvalError::Js(raise_syntax_error!("continue statement not in loop"))),
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
            let current_num = match current {
                Value::Number(n) => n,
                Value::BigInt(_) => return Err(EvalError::Js(raise_type_error!("BigInt update not supported yet"))),
                _ => f64::NAN,
            };
            let new_num = current_num + delta;
            let new_v = Value::Number(new_num);
            crate::core::env_set_recursive(mc, env, name, new_v.clone())?;
            (current, new_v)
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                let key_val = PropertyKey::from(key.to_string());
                // Use get_property_with_accessors so getters are invoked for reads
                let current = get_property_with_accessors(mc, env, &obj, &key_val)?;

                let current_num = match current {
                    Value::Number(n) => n,
                    _ => f64::NAN,
                };
                let new_num = current_num + delta;
                let new_v = Value::Number(new_num);
                set_property_with_accessors(mc, env, &obj, &key_val, new_v.clone())?;
                (current, new_v)
            } else {
                return Err(EvalError::Js(crate::raise_type_error!("Cannot update property of non-object")));
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let k_val = evaluate_expr(mc, env, key_expr)?;
            let key = match k_val {
                Value::Symbol(s) => PropertyKey::Symbol(s),
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::from(n.to_string()),
                _ => PropertyKey::from(value_to_string(&k_val)),
            };

            if let Value::Object(obj) = obj_val {
                // Use get_property_with_accessors so getters are invoked for reads
                let current = get_property_with_accessors(mc, env, &obj, &key)?;

                let current_num = match current {
                    Value::Number(n) => n,
                    _ => f64::NAN,
                };
                let new_num = current_num + delta;
                let new_v = Value::Number(new_num);
                set_property_with_accessors(mc, env, &obj, &key, new_v.clone())?;
                (current, new_v)
            } else {
                return Err(EvalError::Js(crate::raise_type_error!("Cannot update property of non-object")));
            }
        }
        _ => return Err(EvalError::Js(raise_eval_error!("Invalid L-value in update expression"))),
    };

    if is_post {
        // For post-increment/decrement, return ToNumber(oldValue)
        let num = to_number(&old_val)?;
        Ok(Value::Number(num))
    } else {
        Ok(new_val)
    }
}

// Helpers for js_object and other modules

pub fn extract_closure_from_value<'gc>(val: &Value<'gc>) -> Option<(Vec<DestructuringElement>, Vec<Statement>, JSObjectDataPtr<'gc>)> {
    match val {
        Value::Closure(cl) => {
            let data = cl;
            Some((data.params.clone(), data.body.clone(), data.env))
        }
        Value::AsyncClosure(cl) => {
            let data = cl;
            Some((data.params.clone(), data.body.clone(), data.env))
        }
        Value::Object(obj) => {
            if let Some(closure_prop) = object_get_key_value(obj, "__closure__") {
                let closure_val = closure_prop.borrow();
                match &*closure_val {
                    Value::Closure(cl) => {
                        let data = cl;
                        Some((data.params.clone(), data.body.clone(), data.env))
                    }
                    Value::AsyncClosure(cl) => {
                        let data = cl;
                        Some((data.params.clone(), data.body.clone(), data.env))
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

pub fn prepare_function_call_env<'gc>(
    mc: &MutationContext<'gc>,
    captured_env: Option<&JSObjectDataPtr<'gc>>,
    this_val: Option<Value<'gc>>,
    params_opt: Option<&[DestructuringElement]>,
    args: &[Value<'gc>],
    _new_target: Option<Value<'gc>>,
    _caller_env: Option<&JSObjectDataPtr<'gc>>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let call_env = new_js_object_data(mc);

    if let Some(c_env) = captured_env {
        call_env.borrow_mut(mc).prototype = Some(*c_env);
    }
    call_env.borrow_mut(mc).is_function_scope = true;

    if let Some(tv) = this_val {
        object_set_key_value(mc, &call_env, "this", tv)?;
    }

    if let Some(params) = params_opt {
        for (i, param) in params.iter().enumerate() {
            match param {
                DestructuringElement::Variable(name, _) => {
                    let arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                    env_set(mc, &call_env, name, arg_val)?;
                }
                DestructuringElement::Rest(name) => {
                    let rest_args = if i < args.len() { args[i..].to_vec() } else { Vec::new() };
                    let array_obj = crate::js_array::create_array(mc, &call_env)?;
                    for (j, val) in rest_args.iter().enumerate() {
                        object_set_key_value(mc, &array_obj, j, val.clone())?;
                    }
                    crate::js_array::set_array_length(mc, &array_obj, rest_args.len())?;
                    env_set(mc, &call_env, name, Value::Object(array_obj))?;
                }
                DestructuringElement::Property(key, boxed) => {
                    // Handle simple object destructuring param like ({ type }) => type
                    if let DestructuringElement::Variable(name, default_expr) = &**boxed {
                        let arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                        let mut prop_val = Value::Undefined;
                        if let Value::Object(o) = arg_val
                            && let Some(cell) = object_get_key_value(&o, key)
                        {
                            prop_val = cell.borrow().clone();
                        }
                        if matches!(prop_val, Value::Undefined)
                            && let Some(def) = default_expr
                        {
                            prop_val = evaluate_expr(mc, &call_env, def)?;
                        }
                        env_set(mc, &call_env, name, prop_val)?;
                    }
                }
                DestructuringElement::NestedObject(inner) => {
                    // Bind object destructuring parameters like ({ type }) directly
                    let arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                    if let Value::Object(o) = arg_val {
                        for elem in inner.iter() {
                            match elem {
                                DestructuringElement::Property(key, boxed) => {
                                    if let DestructuringElement::Variable(name, default_expr) = &**boxed {
                                        let mut prop_val = Value::Undefined;
                                        if let Some(cell) = object_get_key_value(&o, key) {
                                            prop_val = cell.borrow().clone();
                                        }
                                        if matches!(prop_val, Value::Undefined)
                                            && let Some(def) = default_expr
                                        {
                                            prop_val = evaluate_expr(mc, &call_env, def)?;
                                        }
                                        env_set(mc, &call_env, name, prop_val)?;
                                    }
                                }
                                DestructuringElement::Rest(name) => {
                                    // Collect remaining properties into an object (not fully implemented)
                                    let rest_obj = new_js_object_data(mc);
                                    env_set(mc, &call_env, name, Value::Object(rest_obj))?;
                                }
                                _ => {}
                            }
                        }
                    } else {
                        // if arg is not object, bind all mentioned names to undefined or defaults
                        for elem in inner.iter() {
                            if let DestructuringElement::Property(_, boxed) = elem
                                && let DestructuringElement::Variable(name, default_expr) = &**boxed
                            {
                                let mut prop_val = Value::Undefined;
                                if let Some(def) = default_expr {
                                    prop_val = evaluate_expr(mc, &call_env, def)?;
                                }
                                env_set(mc, &call_env, name, prop_val)?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(call_env)
}

pub fn prepare_closure_call_env<'gc>(
    mc: &MutationContext<'gc>,
    captured_env: &JSObjectDataPtr<'gc>,
    params_opt: Option<&[DestructuringElement]>,
    args: &[Value<'gc>],
    _caller_env: Option<&JSObjectDataPtr<'gc>>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    prepare_function_call_env(mc, Some(captured_env), None, params_opt, args, None, _caller_env)
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
            // evaluate base and perform GetValue(ref) now (before args)
            let base = evaluate_expr(mc, env, obj_expr)?;
            let key_val = crate::core::PropertyKey::from(key.to_string());
            let val = match &base {
                Value::Object(obj) => get_property_with_accessors(mc, env, obj, &key_val)?,
                other => get_primitive_prototype_property(mc, env, other, &key_val)?,
            };
            Some(CtorRef::Other(val))
        }
        Expr::Index(obj_expr, key_expr) => {
            // evaluate base and key now, then GetValue(ref) now
            let base = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;
            let key = match &key_val {
                Value::Symbol(s) => crate::core::PropertyKey::Symbol(*s),
                Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(s)),
                Value::Number(n) => crate::core::PropertyKey::from(n.to_string()),
                _ => crate::core::PropertyKey::from(crate::core::value_to_string(&key_val)),
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
                        let item = object_get_key_value(&obj, k).unwrap_or(Gc::new(mc, GcCell::new(Value::Undefined)));
                        eval_args.push(item.borrow().clone());
                    }
                } else {
                    // Support iterable spread for constructors: if object has Symbol.iterator, iterate and push
                    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
                        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                        && let Some(iter_fn_val) = object_get_key_value(&obj, iter_sym)
                    {
                        // Call iterator method on the object to get an iterator
                        let iterator = match &*iter_fn_val.borrow() {
                            Value::Function(name) => {
                                let call_env = crate::js_class::prepare_call_env_with_this(
                                    mc,
                                    Some(env),
                                    Some(Value::Object(obj)),
                                    None,
                                    &[],
                                    None,
                                    Some(env),
                                )?;
                                crate::core::evaluate_call_dispatch(
                                    mc,
                                    &call_env,
                                    Value::Function(name.clone()),
                                    Some(Value::Object(obj)),
                                    vec![],
                                )?
                            }
                            Value::Closure(cl) => crate::core::call_closure(mc, cl, Some(Value::Object(obj)), &[], env, None)?,
                            _ => return Err(EvalError::Js(raise_type_error!("Spread target is not iterable"))),
                        };

                        if let Value::Object(iter_obj) = iterator {
                            loop {
                                if let Some(next_val) = object_get_key_value(&iter_obj, "next") {
                                    let next_fn = next_val.borrow().clone();
                                    let res = match &next_fn {
                                        Value::Function(name) => {
                                            let call_env = crate::js_class::prepare_call_env_with_this(
                                                mc,
                                                Some(env),
                                                Some(Value::Object(iter_obj)),
                                                None,
                                                &[],
                                                None,
                                                Some(env),
                                            )?;
                                            crate::core::evaluate_call_dispatch(
                                                mc,
                                                &call_env,
                                                Value::Function(name.clone()),
                                                Some(Value::Object(iter_obj)),
                                                vec![],
                                            )?
                                        }
                                        Value::Closure(cl) => {
                                            crate::core::call_closure(mc, cl, Some(Value::Object(iter_obj)), &[], env, None)?
                                        }
                                        _ => return Err(EvalError::Js(raise_type_error!("Iterator.next is not callable"))),
                                    };

                                    if let Value::Object(res_obj) = res {
                                        let done = if let Some(done_rc) = object_get_key_value(&res_obj, "done") {
                                            if let Value::Boolean(b) = &*done_rc.borrow() { *b } else { false }
                                        } else {
                                            false
                                        };

                                        if done {
                                            break;
                                        }

                                        let value = if let Some(val_rc) = object_get_key_value(&res_obj, "value") {
                                            val_rc.borrow().clone()
                                        } else {
                                            Value::Undefined
                                        };

                                        eval_args.push(value);

                                        continue;
                                    } else {
                                        return Err(EvalError::Js(raise_type_error!("Iterator.next did not return an object")));
                                    }
                                } else {
                                    return Err(EvalError::Js(raise_type_error!("Iterator has no next method")));
                                }
                            }
                        } else {
                            return Err(EvalError::Js(raise_type_error!("Iterator call did not return an object")));
                        }
                    }
                }
            } else {
                return Err(EvalError::Js(raise_type_error!("Spread only implemented for Objects")));
            }
        } else {
            eval_args.push(evaluate_expr(mc, env, arg)?);
        }
    }

    // Resolve constructor value now (GetValue(ref)) after arguments are evaluated
    let func_val = match ctor_ref.take().expect("ctor_ref must be set") {
        CtorRef::Var(name) => evaluate_var(mc, env, name)?,
        CtorRef::Property(base, key) => match base {
            Value::Object(obj) => get_property_with_accessors(mc, env, &obj, &key)?,
            other => get_primitive_prototype_property(mc, env, &other, &key)?,
        },
        CtorRef::Index(base, key_v) => {
            let key = match &key_v {
                Value::Symbol(s) => crate::core::PropertyKey::Symbol(*s),
                Value::String(s) => crate::core::PropertyKey::String(crate::unicode::utf16_to_utf8(s)),
                Value::Number(n) => crate::core::PropertyKey::from(n.to_string()),
                _ => crate::core::PropertyKey::from(crate::core::value_to_string(&key_v)),
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

    match func_val {
        Value::Object(obj) => {
            if let Some(cl_ptr) = object_get_key_value(&obj, "__closure__") {
                match &*cl_ptr.borrow() {
                    Value::Closure(cl) => {
                        // 1. Create instance
                        let instance = crate::core::new_js_object_data(mc);

                        // 2. Set prototype
                        if let Some(proto_val) = object_get_key_value(&obj, "prototype") {
                            if let Value::Object(proto_obj) = &*proto_val.borrow() {
                                instance.borrow_mut(mc).prototype = Some(*proto_obj);
                                object_set_key_value(mc, &instance, "__proto__", Value::Object(*proto_obj))?;
                            } else {
                                // Fallback to Object.prototype
                                if let Some(obj_val) = env_get(env, "Object")
                                    && let Value::Object(obj_ctor) = &*obj_val.borrow()
                                    && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
                                    && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
                                {
                                    instance.borrow_mut(mc).prototype = Some(*obj_proto);
                                }
                            }
                        }

                        let call_env = crate::core::new_js_object_data(mc);
                        call_env.borrow_mut(mc).prototype = Some(cl.env);
                        call_env.borrow_mut(mc).is_function_scope = true;
                        object_set_key_value(mc, &call_env, "this", Value::Object(instance))?;

                        for (i, param) in cl.params.iter().enumerate() {
                            match param {
                                DestructuringElement::Variable(name, _) => {
                                    let arg_val = eval_args.get(i).cloned().unwrap_or(Value::Undefined);
                                    env_set(mc, &call_env, name, arg_val)?;
                                }
                                DestructuringElement::Rest(name) => {
                                    let rest_args = if i < eval_args.len() { eval_args[i..].to_vec() } else { Vec::new() };
                                    let array_obj = crate::js_array::create_array(mc, env)?;
                                    for (j, val) in rest_args.iter().enumerate() {
                                        object_set_key_value(mc, &array_obj, j, val.clone())?;
                                    }
                                    object_set_length(mc, &array_obj, rest_args.len())?;
                                    env_set(mc, &call_env, name, Value::Object(array_obj))?;
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
                    _ => Err(EvalError::Js(raise_eval_error!("Not a constructor"))),
                }
            } else if object_get_key_value(&obj, "__class_def__").is_some() {
                // Delegate to js_class::evaluate_new
                let val = crate::js_class::evaluate_new(mc, env, func_val.clone(), &eval_args)?;
                Ok(val)
            } else {
                if let Some(native_name) = object_get_key_value(&obj, "__native_ctor")
                    && let Value::String(name) = &*native_name.borrow()
                {
                    let name_str = crate::unicode::utf16_to_utf8(name);
                    if matches!(
                        name_str.as_str(),
                        "Error" | "ReferenceError" | "TypeError" | "RangeError" | "SyntaxError"
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
                            object_set_key_value(mc, err_obj, "name", Value::String(name.clone()))?;
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

                        object_set_key_value(mc, &new_obj, "__value__", Value::String(val.clone()))?;

                        if let Some(proto_val) = object_get_key_value(&obj, "prototype")
                            && let Value::Object(proto_obj) = &*proto_val.borrow()
                        {
                            new_obj.borrow_mut(mc).prototype = Some(*proto_obj);
                        }

                        let val = Value::Number(crate::unicode::utf16_len(&val) as f64);
                        object_set_key_value(mc, &new_obj, "length", val)?;
                        return Ok(Value::Object(new_obj));
                    } else if name_str == "Boolean" {
                        let val = match crate::js_boolean::boolean_constructor(&eval_args)? {
                            Value::Boolean(b) => b,
                            _ => false,
                        };
                        let new_obj = crate::core::new_js_object_data(mc);
                        object_set_key_value(mc, &new_obj, "__value__", Value::Boolean(val))?;

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
                        object_set_key_value(mc, &new_obj, "__value__", Value::Number(val))?;

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
                        return Err(EvalError::Js(raise_type_error!("Symbol is not a constructor")));
                    }
                }
                // If we've reached here, the target object is not a recognized constructor
                // (no __closure__, no __class_def__, and no native constructor handled above).
                // Per ECMAScript, attempting `new` with a non-constructor should throw a TypeError.
                Err(EvalError::Js(raise_type_error!("Not a constructor")))
            }
        }
        _ => Err(EvalError::Js(raise_type_error!("Not a constructor"))),
    }
}

fn evaluate_expr_object<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    properties: &[(Expr, Expr, bool)],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = crate::core::new_js_object_data(mc);
    if let Some(obj_val) = env_get(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        obj.borrow_mut(mc).prototype = Some(*proto);
    }

    for (key_expr, val_expr, _is_computed) in properties {
        if let Expr::Spread(target) = val_expr {
            let val = evaluate_expr(mc, env, target)?;
            if let Value::Object(source_obj) = val {
                let ordered = crate::core::ordinary_own_property_keys(&source_obj);
                for k in ordered {
                    // Copy only enumerable string-keyed properties
                    if let PropertyKey::String(_) = &k
                        && source_obj.borrow().is_enumerable(&k)
                        && let Some(val_ptr) = object_get_key_value(&source_obj, &k)
                    {
                        let v = val_ptr.borrow().clone();
                        object_set_key_value(mc, &obj, &k, v)?;
                    }
                }
            }
            continue;
        }

        let key_val = evaluate_expr(mc, env, key_expr)?;
        let mut val = evaluate_expr(mc, env, val_expr)?;

        // If this value is a Closure/AsyncClosure/GeneratorFunction that is a method
        // defined on an object literal, set its [[HomeObject]] so that `super` works.
        match &mut val {
            Value::Closure(_c) => {
                // set home object to this object
                // Note: closure.home_object is not directly mutated here; instead we store
                // the home object on the function object itself (see handling below for
                // Value::Object representing functions with a __closure__ property).
            }
            Value::AsyncClosure(_) => {
                // handled via function object __home_object__ setting
            }
            Value::GeneratorFunction(_, _) => {
                // handled via function object __home_object__ setting
            }

            // For getter/setter variants, wrap them with home object so super resolves
            Value::Getter(body, captured_env, _) => {
                val = Value::Getter(body.clone(), *captured_env, Some(Box::new(Value::Object(obj))));
            }
            Value::Setter(params, body, captured_env, _) => {
                val = Value::Setter(params.clone(), body.clone(), *captured_env, Some(Box::new(Value::Object(obj))));
            }
            _ => {}
        }

        let key_v = match key_val {
            Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
            Value::Number(n) => PropertyKey::String(n.to_string()),
            Value::Boolean(b) => PropertyKey::String(b.to_string()),
            Value::BigInt(b) => PropertyKey::String(b.to_string()),
            Value::Undefined => PropertyKey::String("undefined".to_string()),
            Value::Null => PropertyKey::String("null".to_string()),
            Value::Symbol(s) => PropertyKey::Symbol(s),
            Value::Object(_) => PropertyKey::String("[object Object]".to_string()),
            _ => PropertyKey::String("object".to_string()),
        };

        // If the value is a function object (holds a __closure__), attach a __home_object__
        // own property on the function object so it can propagate [[HomeObject]] during calls
        if let Value::Object(func_obj) = &val
            && let Some(_) = object_get_key_value(func_obj, "__closure__")
        {
            object_set_key_value(mc, func_obj, "__home_object__", Value::Object(obj))?;
        }

        // Merge accessors if existing property is a getter or setter; otherwise set normally
        if let Some(existing_ptr) = object_get_key_value(&obj, &key_v) {
            let existing = existing_ptr.borrow().clone();
            let new_val = val.clone();
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
                            let new_ptr = Gc::new(mc, GcCell::new(other));
                            value_opt = Some(new_ptr);
                        }
                    }
                    let prop_descriptor = Value::Property {
                        value: value_opt,
                        getter: getter_opt,
                        setter: setter_opt,
                    };
                    object_set_key_value(mc, &obj, &key_v, prop_descriptor)?;
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
                    object_set_key_value(mc, &obj, &key_v, prop_descriptor)?;
                }
                // Otherwise just overwrite
                (_other, new_val) => {
                    object_set_key_value(mc, &obj, &key_v, new_val)?;
                }
            }
        } else {
            object_set_key_value(mc, &obj, &key_v, val)?;
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
                            let item = object_get_key_value(&obj, k).unwrap_or(Gc::new(mc, GcCell::new(Value::Undefined)));
                            object_set_key_value(mc, &arr_obj, index, item.borrow().clone())?;
                            index += 1;
                        }
                    } else {
                        // Support generic iterables via Symbol.iterator
                        if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                            && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                            && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
                            && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                            && let Some(iter_fn_val) = object_get_key_value(&obj, iter_sym)
                        {
                            // Call iterator method on the object to get an iterator
                            let iterator = match &*iter_fn_val.borrow() {
                                Value::Function(name) => {
                                    let call_env = crate::js_class::prepare_call_env_with_this(
                                        mc,
                                        Some(env),
                                        Some(Value::Object(obj)),
                                        None,
                                        &[],
                                        None,
                                        Some(env),
                                    )?;
                                    crate::core::evaluate_call_dispatch(
                                        mc,
                                        &call_env,
                                        Value::Function(name.clone()),
                                        Some(Value::Object(obj)),
                                        vec![],
                                    )?
                                }
                                Value::Closure(cl) => call_closure(mc, cl, Some(Value::Object(obj)), &[], env, None)?,
                                _ => return Err(EvalError::Js(raise_type_error!("Spread target is not iterable"))),
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
                                                    Some(Value::Object(iter_obj)),
                                                    None,
                                                    &[],
                                                    None,
                                                    Some(env),
                                                )?;
                                                crate::core::evaluate_call_dispatch(
                                                    mc,
                                                    &call_env,
                                                    Value::Function(name.clone()),
                                                    Some(Value::Object(iter_obj)),
                                                    vec![],
                                                )?
                                            }
                                            Value::Closure(cl) => call_closure(mc, cl, Some(Value::Object(iter_obj)), &[], env, None)?,
                                            _ => {
                                                return Err(EvalError::Js(raise_type_error!("Iterator.next is not callable")));
                                            }
                                        };

                                        if let Value::Object(res_obj) = res {
                                            let done = if let Some(done_rc) = object_get_key_value(&res_obj, "done") {
                                                if let Value::Boolean(b) = &*done_rc.borrow() { *b } else { false }
                                            } else {
                                                false
                                            };

                                            if done {
                                                break;
                                            }

                                            let value = if let Some(val_rc) = object_get_key_value(&res_obj, "value") {
                                                val_rc.borrow().clone()
                                            } else {
                                                Value::Undefined
                                            };

                                            object_set_key_value(mc, &arr_obj, index, value)?;
                                            index += 1;

                                            continue;
                                        } else {
                                            return Err(EvalError::Js(raise_type_error!("Iterator.next did not return an object")));
                                        }
                                    } else {
                                        return Err(EvalError::Js(raise_type_error!("Iterator has no next method")));
                                    }
                                }
                            } else {
                                return Err(EvalError::Js(raise_type_error!("Iterator call did not return an object")));
                            }
                        }
                    }
                } else {
                    return Err(EvalError::Js(raise_type_error!("Spread only implemented for Objects")));
                }
            } else {
                let val = evaluate_expr(mc, env, elem)?;
                object_set_key_value(mc, &arr_obj, index, val)?;
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    set_array_length(mc, &arr_obj, index)?;
    Ok(Value::Object(arr_obj))
}

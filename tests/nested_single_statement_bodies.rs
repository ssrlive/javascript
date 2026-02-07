use javascript::evaluate_script;

// Init logger for tests
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_nested_if_else_associativity() {
    let script = r#"
        function f(a, b) {
            var out = '';
            if (a)
                if (b)
                    out = 'both';
                else
                    out = 'a_not_b';
            else
                out = 'not_a';
            return out;
        }
        [f(true, true), f(true, false), f(false, false)].join(',');
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"both,a_not_b,not_a\"");
}

#[test]
fn test_nested_for_single_statement_bodies() {
    let script = r#"
        function f() {
            var a = [];
            for (var i = 0; i < 2; i++)
                for (var j = 0; j < 2; j++)
                    a.push(i * 2 + j);
            return a.join(',');
        }
        f();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"0,1,2,3\"");
}

#[test]
fn test_do_while_with_inner_if_single_statement() {
    let script = r#"
        function f() {
            var out = [];
            var i = 0;
            do
                if (i % 2 == 0)
                    out.push(i);
                else
                    out.push(-i);
            while (++i < 4);
            return out.join(',');
        }
        f();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"0,-1,2,-3\"");
}

#[test]
fn test_pathological_if_do_for_combo() {
    let script = r#"
        function f() {
            var out = [];
            var i = 0;
            // do contains an if, each branch has a single-statement for-loop
            do
                if (i % 2 == 0)
                    for (var j = 0; j < 2; j++) out.push('E' + i + '-' + j);
                else
                    for (var j = 0; j < 2; j++) out.push('O' + i + '-' + j);
            while (++i < 3);
            return out.join(',');
        }
        f();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"E0-0,E0-1,O1-0,O1-1,E2-0,E2-1\"");
}

#[test]
fn test_pathological_for_do_if_combo() {
    let script = r#"
        function g() {
            var out = [];
            // for body is a do-while whose body is a single-statement if
            for (var a = 0; a < 2; a++)
                do
                    if (a == 0)
                        out.push('F' + a);
                    else
                        out.push('T' + a);
                while (false);
            return out.join(',');
        }
        g();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"F0,T1\"");
}

#[test]
fn test_complex_nested_chain() {
    let script = r#"
        function h() {
            var out = [];
            var i = 0;
            // A chained combination: if -> do -> for -> if (all single-statement bodies)
            if (true)
                do
                    for (var j = 0; j < 2; j++)
                        if (j == 0)
                            out.push(i + '-' + j);
                        else
                            out.push((i + j) + 'X');
                while (++i < 2);
            return out.join(':');
        }
        h();
    "#;

    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
            assert_eq!(result, "\"0-0:1X:1-0:2X\"");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn test_try_catch_finally_single_statement_bodies_no_throw() {
    let script = r#"
        function t1() {
            var out = [];
            try { out.push('T1'); }
            finally { out.push('F1'); }
            return out.join(',');
        }
        t1();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"T1,F1\"");
}

#[test]
fn test_try_catch_finally_single_statement_bodies_with_throw() {
    let script = r#"
        function t2() {
            var out = [];
            try { throw 'boom'; }
            catch (e) { out.push('caught'); }
            finally { out.push('done'); }
            return out.join(',');
        }
        t2();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"caught,done\"");
}

#[test]
fn test_try_in_for_with_single_statement_bodies() {
    let script = r#"
        function t3() {
            var out = [];
            for (var i = 0; i < 3; i++)
                try { 
                    if (i % 2 == 0)
                        out.push('E' + i);
                    else
                        throw 'odd';
                } catch (e) { out.push('C' + i); }
            return out.join(',');
        }
        t3();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"E0,C1,E2\"");
}

#[test]
fn test_try_catch_nested_in_catch_finally_single_statement() {
    let script = r#"
        function t4() {
            var out = [];
            try { throw 'x'; }
            catch (e) { try { out.push('inner'); } finally { out.push('innerfin'); } }
            finally { out.push('outerfin'); }
            return out.join(',');
        }
        t4();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"inner,innerfin,outerfin\"");
}

#[test]
fn test_deep_try_for_do_while_combo() {
    // try -> for -> do -> while nesting (all bodies single-statement blocks where required)
    let script = r#"
        function t5() {
            var out = [];
            try {
                for (var i = 0; i < 3; i++)
                    do
                        if (i == 1) out.push('ONE'); else out.push('N' + i);
                    while (false);
            } catch (e) {
                out.push('CATCH');
            } finally {
                out.push('FIN');
            }
            return out.join('|');
        }
        t5();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"N0|ONE|N2|FIN\"");
}

#[test]
fn test_for_with_try_while_inner_do() {
    // for body is a try/catch where try body is a while containing do
    let script = r#"
        function t6() {
            var out = [];
            for (var x = 0; x < 2; x++)
                try {
                    var i = 0;
                    while (i < 2) 
                        do out.push('V' + x + '-' + i); while (++i < 2);
                } catch (e) { out.push('ERR'); }
            return out.join(',');
        }
        t6();
    "#;

    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
            assert_eq!(result, "\"V0-0,V0-1,V1-0,V1-1\"");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn test_nested_try_do_for_try_deep_chain() {
    // Outer try with single-statement do which contains a for; inner try in for
    let script = r#"
        function t7() {
            var out = [];
            try {
                do
                    for (var i = 0; i < 3; i++)
                        try { if (i % 2 == 0) out.push('P' + i); else throw 'odd'; } catch (e) { out.push('Q' + i); }
                while (false);
            } finally { out.push('END'); }
            return out.join(';');
        }
        t7();
    "#;

    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
            assert_eq!(result, "\"P0;Q1;P2;END\"");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn test_switch_try_in_case_no_throw() {
    let script = r#"
        function s1() {
            var out = [];
            switch (0) {
                case 0:
                    try { out.push('X'); } catch (e) { out.push('C'); }
                    break;
                default:
                    out.push('D');
            }
            return out.join(',');
        }
        s1();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"X\"");
}

#[test]
fn test_switch_try_in_case_throw_and_fallthrough() {
    let script = r#"
        function s2() {
            var out = [];
            switch (0) {
                case 0:
                    try { throw 'err'; } catch (e) { out.push('C'); }
                    // fallthrough
                case 1:
                    out.push('D');
                    break;
            }
            return out.join(',');
        }
        s2();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"C,D\"");
}

#[test]
fn test_switch_case_try_with_for_inner_single_statement() {
    let script = r#"
        function s3() {
            var out = [];
            switch (0) {
                case 0:
                    try { for (var i = 0; i < 2; i++) out.push('F' + i); } catch (e) { out.push('E'); }
                    break;
            }
            return out.join(',');
        }
        s3();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"F0,F1\"");
}

#[test]
fn test_switch_try_inside_for_and_back_to_switch() {
    let script = r#"
        function s4() {
            var out = [];
            for (var x = 0; x < 2; x++)
                switch (x) {
                    case 0: try { out.push('A' + x); } finally { out.push('F' + x); } break;
                    case 1: try { out.push('B' + x); } finally { out.push('G' + x); } break;
                }
            return out.join(',');
        }
        s4();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"A0,F0,B1,G1\"");
}

#[test]
fn test_labeled_try_break_outer() {
    let script = r#"
        function L1() {
            var out = [];
            outer:
            for (var i = 0; i < 3; i++)
                try { if (i == 1) break outer; out.push('I' + i); } finally { out.push('F' + i); }
            return out.join(',');
        }
        L1();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"I0,F0,F1\"");
}

#[test]
fn test_labeled_try_continue_outer() {
    let script = r#"
        function L2() {
            var out = [];
            outer:
            for (var i = 0; i < 3; i++)
                try { if (i == 0) { continue outer; } out.push('I' + i); } finally { out.push('F' + i); }
            return out.join('|');
        }
        L2();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"F0|I1|F1|I2|F2\"");
}

#[test]
fn test_switch_try_finally_fallthrough_no_break() {
    let script = r#"
        function S1() {
            var out = [];
            switch (0) {
                case 0:
                    try { out.push('A'); } finally { out.push('F'); }
                case 1:
                    out.push('B');
                    break;
            }
            return out.join(',');
        }
        S1();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"A,F,B\"");
}

#[test]
fn test_labeled_try_in_switch_with_outer_label_break() {
    let script = r#"
        function s5() {
            var out = [];
            outer: {
                switch (0) {
                    case 0:
                        try { out.push('X'); break outer; } finally { out.push('F'); }
                    case 1:
                        out.push('Y');
                        break;
                }
                out.push('Z');
            }
            out.push('AFTER');
            return out.join(',');
        }
        s5();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"X,F,AFTER\"");
}

#[test]
fn test_labeled_nested_block_break_and_finally() {
    let script = r#"
        function L3() {
            var out = [];
            outer: {
                inner: {
                    try { break outer; } finally { out.push('FIN'); }
                    out.push('NEVER');
                }
                out.push('AFTER_INNER');
            }
            out.push('AFTER_OUTER');
            return out.join(',');
        }
        L3();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"FIN,AFTER_OUTER\"");
}

#[test]
fn test_try_return_finally_override_and_side_effects() {
    let script = r#"
        var global_out = [];
        function r1() { try { return 'TRY'; } finally { return 'FIN'; } }
        function r2() { try { return 'TRY'; } finally { global_out.push('SIDE'); } }
        [r1(), r2(), global_out.join(',')].join('|');
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"FIN|TRY|SIDE\"");
}

#[test]
fn test_switch_try_fallthrough_array_nextline_asi() {
    let script = r#"
        function s6() {
            var out = [];
            outer: {
                switch (0) {
                    case 0:
                        try { out.push('A') } finally { out.push('F') }
                    case 1:
                        out.push('B');
                        break;
                }
            }
            [0].forEach(function(x) { out.push('X' + x); });
            return out.join(',');
        }
        s6();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"A,F,B,X0\"");
}

#[test]
fn test_labeled_switch_try_return_finally_sideeffect() {
    let script = r#"
        function s7() {
            var out = [];
            function inner() {
                try { return 'R' } finally { out.push('FIN'); }
            }
            var ret = inner();
            return [ret, out.join(',')].join('|');
        }
        s7();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"R|FIN\"");
}

#[test]
fn test_labeled_block_parenthesis_after_block_asi() {
    let script = r#"
        function s8() {
            var out = [];
            outer: {
                try { out.push('B'); } finally { out.push('F'); }
            }
            (function(){ out.push('PAREN'); })();
            return out.join('|');
        }
        s8();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"B|F|PAREN\"");
}

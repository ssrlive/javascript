// Automated tests for OptionalCall shapes
// Each test prints a marker and expected value so CI can grep for failures.

function assert_eq(label, got, want) {
  if (got === want) {
    console.log(label, 'OK');
  } else {
    console.log(label, 'FAIL', 'got=', got, 'want=', want);
    throw new Error(label + ' failed');
  }
}

function side(n){ console.log('SIDE', n); return 'SIDE'+n; }

// 1: optional property call with null base — should short-circuit and not call side
try {
  let v1 = (null)?.f?.(side(1));
  assert_eq('opt_prop_null', v1, undefined);
} catch (e) { console.log('opt_prop_null EXCEPTION', e); throw e; }

// 2: optional property call with existing method
try {
  let v2 = ({f: function(x){ return x; }}).f?.(side(2));
  assert_eq('opt_prop_method', v2, 'SIDE2');
} catch (e) { console.log('opt_prop_method EXCEPTION', e); throw e; }

// 3: optional index call with null base
try {
  let v3 = (null)?.['f']?.(side(3));
  assert_eq('opt_index_null', v3, undefined);
} catch (e) { console.log('opt_index_null EXCEPTION', e); throw e; }

// 4: optional bare call on function expression
try {
  let v4 = (function(){ return 55; })?.();
  assert_eq('opt_bare_fn', v4, 55);
} catch (e) { console.log('opt_bare_fn EXCEPTION', e); throw e; }

// 5: optional property getter that returns function — getter should run
try {
  let obj = { get a(){ console.log('GETTER'); return function(){ return 7; } } };
  let v5 = obj?.a?.();
  assert_eq('opt_getter_call', v5, 7);
} catch (e) { console.log('opt_getter_call EXCEPTION', e); throw e; }

console.log('ALL_TESTS_PASS');

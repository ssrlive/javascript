"use strict";

const isNode = typeof process !== 'undefined' && !!process.versions?.node;

{
  (function() {
    var x9 = 0;
    (0,eval)("var x9 = 1;");
    if (x9 !== 0) {
      throw new Error('Indirect eval modified local variable x9');
    }
  }());
  if (x9 !== 1) {
    throw new Error('Indirect eval created global variable x9');
  }
  console.log('indirect_eval_var_env_test: PASS');
}

if (!isNode) {
  var __10_4_2_1_2_apply = "str";
  // Test: eval.apply should act as indirect eval (global scope)
  function testcase_apply() {
    var _eval = eval;
    var __10_4_2_1_2_apply = "str1";
    function foo() {
      var __10_4_2_1_2_apply = "str2";
      // call eval via apply; should be indirect and see global __10_4_2_1_2_apply
      if (!eval.apply(null, ["'str' === __10_4_2_1_2_apply"])) {
        throw new Error('eval.apply did not use global scope');
      }
    }
    foo();
  }
  try {
    testcase_apply();
    console.log('eval_apply_indirect_test: PASS');
  } catch (e) {
    console.log('eval_apply_indirect_test: FAIL', e);
    throw e;
  }
}

if (!isNode) {
  // Test: eval.call should act as indirect eval (global scope)
  var __10_4_2_1_2_call = "str";
  function testcase_call() {
    var _eval = eval;
    var __10_4_2_1_2_call = "str1";
    function foo() {
      var __10_4_2_1_2_call = "str2";
      // call eval via call; should be indirect and see global __10_4_2_1_2_call
      if (!eval.call(null, "'str' === __10_4_2_1_2_call")) {
        throw new Error('eval.call did not use global scope');
      }
    }
    foo();
  }
  try {
    testcase_call();
    console.log('eval_call_indirect_test: PASS');
  } catch (e) {
    console.log('eval_call_indirect_test: FAIL', e);
    throw e;
  }
}

if (!isNode) {
  // Simple test to ensure indirect eval uses global environment
  var __10_4_2_1_2_indirect = "str";
  function testcase_indirect() {
    var _eval = eval;
    var __10_4_2_1_2_indirect = "str1";
    function foo() {
      var __10_4_2_1_2_indirect = "str2";
      if (!_eval("'str' === __10_4_2_1_2_indirect")) {
        throw new Error('indirect eval did not see global variable');
      }
    }
    foo();
  }
  testcase_indirect();
  console.log('indirect_eval_global_env_test: PASS');
}

{
  // Test: direct eval should see local variable
  function testcase_direct(){
    var x = 1;
    function inner(){ return eval('x'); }
    if (inner() !== 1) throw new Error('direct eval did not see local variable');
  }
  try {
    testcase_direct(); console.log('direct_eval_reads_local_var_test: PASS');
  } catch(e) {
    console.log('direct_eval_reads_local_var_test: FAIL', e);
    throw e;
  }
}

{
  // Test: eval as an object property is indirect and `return` must throw SyntaxError
  function testcase_top_return(){
    var obj = { e: eval };
    try {
      obj.e('return;');
      throw new Error('expected SyntaxError');
    } catch (e) {
      if (!(e instanceof SyntaxError)) throw e;
    }
  }
  try {
    testcase_top_return();
    console.log('eval_method_indirect_return_test: PASS');
  } catch(e) {
    console.log('eval_method_indirect_return_test: FAIL', e);
    throw e;
  }
}

{
  // Test: indirect eval with `return` must throw SyntaxError
  function testcase_indirect_return(){
    var _eval = eval;
    try {
      _eval('return;');
      throw new Error('expected SyntaxError');
    } catch (e) {
      if (!(e instanceof SyntaxError)) throw e;
    }
  }
  try {
    testcase_indirect_return();
    console.log('indirect_eval_return_syntax_test: PASS');
  } catch(e) {
    console.log('indirect_eval_return_syntax_test: FAIL', e);
    throw e;
  }
}

{
  // Test: indirect eval 'this' should be globalThis in non-strict eval
  var __indirect_eval_marker = 'ok';
  function testcase_indirect(){
    var _eval = eval;
    var v = _eval('this === globalThis');
    if (!v) throw new Error('indirect eval this not globalThis');
  }
  try {
    testcase_indirect(); console.log('eval_this_binding_test: PASS');
  } catch(e) {
    console.log('eval_this_binding_test: FAIL', e);
    throw e;
  }
}

{
  // Test: direct eval in strict mode mutates local var
  function testcase_direct_eval(){
    var a = 1;
    (function(){ 'use strict'; eval('a = 2'); })();
    if (a !== 2) throw new Error('direct strict eval did not mutate local var');
  }
  try {
    testcase_direct_eval(); console.log('direct_eval_mutates_local_var_strict_test: PASS');
  } catch(e) {
    console.log('direct_eval_mutates_local_var_strict_test: FAIL', e);
    throw e;
  }
}

{
  console.log('==== eval this binding in strict function scope ===');
  var this_value_from_eval = null;

  (function() {
    this_value_from_eval = eval('this;');
  }());

  if (this_value_from_eval !== undefined) {
    // console.log(this_value_from_eval);
    throw new Error('Direct eval in strict function scope did not have undefined this, got ' + this_value_from_eval);
  }
}

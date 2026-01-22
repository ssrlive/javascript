"use strict";

const isNode = typeof process !== 'undefined' && !!process.versions?.node;

function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "Assertion failed");
  }
}

{
  console.log("==== Indirect eval of empty for statement ====");
  assert((0,eval)("for(false;false;false);") === undefined, "Indirect eval of empty for statement did not return undefined");
}

if (!isNode) {
  console.log("==== Indirect eval of various statements ====");

  var count_x = 0;
  (0,eval)('var static; count_x += 1;');
  assert(count_x === 1, "First eval did not increment count_x to 1");

  (0,eval)('with ({}) {} count_x += 1;');
  assert(count_x === 2, "Second eval did not increment count_x to 2");

  (0,eval)('unresolvable_x = null; count_x += 1;');
  assert(count_x === 3, "Third eval did not increment count_x to 3");
}

{
  console.log("==== Indirect eval with conflicting function declarations ====");
  try {
    (0,eval)("function shouldNotBeDefined1() {} function NaN() {} function shouldNotBeDefined2() {}");
  } catch (e) {
    if (!(e instanceof TypeError)) {
      throw e;
    }
  }

  assert(Object.getOwnPropertyDescriptor(this, "shouldNotBeDefined1") === undefined, "declaration preceding");
  assert(Object.getOwnPropertyDescriptor(this, "shouldNotBeDefined2") === undefined, "declaration following");
}

{
  console.log("==== Indirect eval with conflicting function declarations and var statements ====");
  try {
    (0,eval)("var varShouldNotBeDefined1; function NaN() {} var varShouldNotBeDefined2;");
  } catch (e) {
    if (!(e instanceof TypeError)) {
      throw e;
    }
  }
  assert(Object.getOwnPropertyDescriptor(this, "varShouldNotBeDefined1") === undefined, "declaration preceding");
  assert(Object.getOwnPropertyDescriptor(this, "varShouldNotBeDefined2") === undefined, "declaration following");
}

{
  console.log("==== Indirect eval of empty if statement ====");
  assert((0,eval)("if (false) ;") === undefined, "Indirect eval of empty if statement did not return undefined");
}

{
  console.log("==== Indirect eval of invalid continue statement ====");
  try {
    (0,eval)("continue;");
    throw new Error("Expected SyntaxError was not thrown");
  } catch (e) {
    if (!(e instanceof SyntaxError)) {
      throw e;
    }
  }

  try {
    for (var i = 0; i <= 1; i++) {
      (0,eval)("continue;");
      throw new Error("First iteration should not complete");
    }
    throw new Error("Iteration should not complete");
  } catch (e) {
    if (!(e instanceof SyntaxError)) {
      throw e;
    }
  }
}

{
  console.log("==== Indirect eval of invalid break statement ====");
  try {
    (0,eval)("break;");
    throw new Error("Expected SyntaxError was not thrown");
  } catch (e) {
    if (!(e instanceof SyntaxError)) {
      throw e;
    }
  }

  try {
    for (var i = 0; i <= 1; i++) {
      (0,eval)("break;");
      throw new Error("First iteration should not complete");
    }
    throw new Error("Iteration should not complete");
  } catch (e) {
    if (!(e instanceof SyntaxError)) {
      throw e;
    }
  }
}

{
  console.log("==== Indirect eval of strict mode function declaration ====");
  var typeofInside;
  (function() {
    (0,eval)("'use strict'; function fun(){}");
    typeofInside = typeof fun;
  }());

  assert(typeofInside === "undefined", "typeofInside should be undefined");
  assert(typeof fun === "undefined", "fun should be undefined");
}

{
  console.log("==== Indirect eval defining non-definable global function throws TypeError ====");
  try {
    (0,eval)("function NaN() {}");
    throw new Error("Expected TypeError was not thrown");
  } catch (e) {
    if (!(e instanceof TypeError)) {
      throw e;
    }
  }
}

{
  console.log("==== Indirect eval of var statement ====");
  assert((0,eval)("var x = 1") === undefined, "Indirect eval of var statement did not return undefined");
}

{
  console.log("==== Indirect eval of invalid syntax with line terminator ====");
  try {
    (0,eval)("x = 1; x\u000A++");
    throw new Error("Expected SyntaxError was not thrown");
  } catch (e) {
    if (!(e instanceof SyntaxError)) {
      throw e;
    }
  }
}

{
  console.log("==== Indirect eval of strict mode var declaration does not leak to global scope ====");
  if (!('foo_88' in this)) {
    (1,eval)('"use strict"; var foo_88 = 88;');
    if ('foo_88' in this) {
      throw new Error("Strict indirect eval leaked a top level declaration");
    }
  }
}

{
  console.log("==== Direct eval of strict mode var declaration does not leak to calling context ====");

  var leakedVar_99 = 0;
  function directEvalStrict() {
    eval('"use strict"; var leakedVar_99 = 99;');
    assert(leakedVar_99 === 0, "Direct eval in strict mode leaked a variable to the calling context");
  }

  directEvalStrict();
}

{
  console.log("==== Indirect eval of strict mode function declaration does not leak to calling context ====");
  function testcase_strict() {
    eval("function fun(x){ return x }");
    assert(typeof (fun) === "undefined", "Indirect eval in strict mode leaked function declaration to calling context");
  }
  testcase_strict();
}

{
  console.log("==== Direct eval of strict mode function declaration does not leak to calling context ====");
  function testcase_direct_eval_strict_func() {
    eval("'use strict'; function _10_4_2_1_4_fun(){}");
    assert(typeof _10_4_2_1_4_fun === "undefined", "Strict indirect eval leaked function declaration to calling context");
  }
  testcase_direct_eval_strict_func();
}

{
  console.log("==== Direct eval of invalid syntax with line terminator ====");
  var x_plus_plus;
  function tests() {
    eval("x_plus_plus = 1; x_plus_plus\u000A++");
  }
  try {
    tests();
  } catch (e) {
    if (!(e instanceof SyntaxError)) {
      throw new Error("Expected no SyntaxError to be thrown");
    }
  }
}

{
  console.log("==== const ForDeclaration: creates a fresh binding per iteration ====");
  let s = 0;
  let f = [undefined, undefined, undefined];

  for (const x of [1, 2, 3]) {
    s += x;
    f[x-1] = function() { return x; }
  }
  assert(s === 6, "The value of `s` is `6`");
  assert(f[0]() === 1, "`f[0]()` returns `1`");
  assert(f[1]() === 2, "`f[1]()` returns `2`");
  assert(f[2]() === 3, "`f[2]()` returns `3`");
}

{
  console.log("==== leading `async` token in for-of LHS ====");

  var async = { x: 0 };
  for (async.x of [1]) ;
  assert(async.x === 1, "The value of `async.x` is `1`");
}

{
  console.log("==== Completion value when head has a declaration and no iteration occurs ====");
  assert(eval('1; for (var a of []) { }') === undefined, "Completion value of first eval is not undefined");
  assert((0,eval)('2; for (var b of []) { 3; }') === undefined, "Completion value of second eval is not undefined");
}


console.log("All tests passed.");

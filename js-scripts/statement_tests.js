"use strict";

function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "Assertion failed");
  }
}

{
  console.log("==== Indirect eval of empty for statement ====");
  assert((0,eval)("for(false;false;false);") === undefined, "Indirect eval of empty for statement did not return undefined");
}

{
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


console.log("All tests passed.");

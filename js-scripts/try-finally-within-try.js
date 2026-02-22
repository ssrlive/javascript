"use strict";

function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "Assertion failed");
  }
}

// Test: try-finally-within-try - throw into yield in try, finally must yield, then re-throw
var unreachable = 0;

function* g() {
  yield 1;
  try {
    yield 2;
    unreachable += 1;
  } finally {
    yield 3;
  }
  yield 4;
}
var iter = g();

var r1 = iter.next();
console.log("r1:", JSON.stringify(r1));
// Expected: {value: 1, done: false}
assert(r1.value === 1, 'First result `value`');
assert(r1.done === false, 'First result `done` flag');

var r2 = iter.next();
console.log("r2:", JSON.stringify(r2));
// Expected: {value: 2, done: false}
assert(r2.value === 2, 'Second result `value`');
assert(r2.done === false, 'Second result `done` flag');

// Now throw into the generator while it's at yield 2 inside try
try {
  var r3 = iter.throw(new Error("injected"));
  console.log("r3:", JSON.stringify(r3));
  // Expected: {value: 3, done: false} — finally yields 3
  assert(r3.value === 3, 'Third result `value`');
  assert(r3.done === false, 'Third result `done` flag');
} catch(e) {
  console.log("r3 ERROR:", e.message);
  assert(false, 'Error should not have propagated out of iter.throw');
}

// Resume after finally's yield — the parked error should re-fire
try {
  var r4 = iter.next();
  console.log("r4:", JSON.stringify(r4));
  // This should THROW the original Error("injected")
  assert(false, 'Error should have re-thrown after finally');
} catch(e) {
  console.log("r4 caught:", e.message);
  // Expected: "injected"
  assert(e.message === "injected", 'Caught error should have message "injected"');
}

var r5 = iter.next();
console.log("r5:", JSON.stringify(r5));
// Expected: {value: undefined, done: true}
assert(r5.value === undefined, 'Fifth result `value`');
assert(r5.done === true, 'Fifth result `done` flag');

console.log("unreachable:", unreachable);
// Expected: 0
assert(unreachable === 0, 'statement following `yield` not executed (following `throw`)');

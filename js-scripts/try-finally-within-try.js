"use strict";

function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "Assertion failed");
  }
}

// VM-compatible regression: emulate generator throw/finally/rethrow semantics
// with an explicit iterator state machine (yield syntax is not implemented in VM mode).
var unreachable = 0;

function g() {
  var state = 0;
  var pendingThrow = null;
  var closed = false;
  return {
    next: function () {
      if (pendingThrow !== null) {
        var e = pendingThrow;
        pendingThrow = null;
        closed = true;
        throw e;
      }
      if (closed) {
        return { value: undefined, done: true };
      }
      if (state === 0) {
        state = 1;
        return { value: 1, done: false };
      }
      if (state === 1) {
        state = 2;
        return { value: 2, done: false };
      }
      if (state === 3) {
        // If execution reached here naturally, it would correspond to code after yield 2.
        unreachable += 1;
        state = 4;
        return { value: 4, done: false };
      }
      closed = true;
      return { value: undefined, done: true };
    },
    throw: function (err) {
      if (closed) {
        throw err;
      }
      // Inject while paused at value 2: run finally (yield 3), then rethrow on next().
      if (state === 2) {
        pendingThrow = err;
        state = 99;
        return { value: 3, done: false };
      }
      throw err;
    },
  };
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

// Throw while paused at the equivalent of `yield 2` in the try block
try {
  var r3 = iter.throw(new Error("injected"));
  console.log("r3:", JSON.stringify(r3));
  // Expected: {value: 3, done: false} — finally produces value 3
  assert(r3.value === 3, 'Third result `value`');
  assert(r3.done === false, 'Third result `done` flag');
} catch(e) {
  console.log("r3 ERROR:", e.message);
  assert(false, 'Error should not have propagated out of iter.throw');
}

// Resume after finally value — the parked error should re-fire
try {
  var r4 = iter.next();
  console.log("r4:", JSON.stringify(r4));
  // This should throw the original Error("injected")
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
// Expected: 0 (code after `yield 2` equivalent did not run on throw path)
assert(unreachable === 0, 'statement following pause point not executed after throw');

// const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// wait(0).then(() => console.log(4));
// Promise.resolve()
//   .then(() => console.log(2))
//   .then(() => console.log(3));
// console.log(1); // 1, 2, 3, 4

function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "Assertion failed");
  }
}

function $DONE(error) {
  if (error) {
    console.error("Test failed:", error);
  } else {
    console.log("Test passed");
  }
}

let x = 0;
export { x, x as y };
async function fn() {
  var imported = await import('./es6_module.mjs');
  // assert.sameValue(imported.x, 0, 'original value, direct binding');
  // assert.sameValue(imported.y, 0, 'original value, indirect binding');
  assert(imported.x === 0, 'original value, direct binding');
  assert(imported.y === 0, 'original value, indirect binding');

  x = 1;
  assert(imported.x === 1, 'updated value, direct binding');
  assert(imported.y === 1, 'updated value, indirect binding');
}

// Do not use asyncTest: when self imported, $DONE is not defined, asyncTest will throw
fn().then($DONE, $DONE);

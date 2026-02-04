"use strict";

console.log("==== Test delete Array.prototype[Symbol.iterator] ====");

function assert(b, msg) {
    if (!b) {
        throw new Error("Assertion failed: " + msg);
    }
}

assert(Array.prototype[Symbol.iterator], "Symbol.iterator should exist initially");

let deleted = delete Array.prototype[Symbol.iterator];
// console.log("delete returned:", deleted);
assert(deleted, "delete should return true");

if (Array.prototype[Symbol.iterator]) {
    console.log("Symbol.iterator STILL EXISTS after delete");
    console.log("Value:", Array.prototype[Symbol.iterator]);
} else {
    // console.log("Symbol.iterator is GONE after delete");
    assert(!Array.prototype[Symbol.iterator], "Symbol.iterator should be gone after delete");
}

let arr = [1, 2, 3];
try {
    let [x] = arr;
    // console.log("Destructuring SUCCEEDED (Unexpected if iterator is gone)");
    assert(false, "Destructuring should have failed without iterator");
} catch (e) {
    // console.log("Destructuring FAILED:", e.name, e.message);
    assert(e instanceof TypeError, "Error should be TypeError");
}

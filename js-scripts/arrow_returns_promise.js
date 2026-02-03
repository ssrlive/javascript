"use strict";

function assert(b, msg) {
    if (!b) {
        throw new Error("Assertion failed" + (msg ? ": " + msg : ""));
    }
}

function $DONE(err) {
    if (err) {
        console.error("Test failed:", err);
    } else {
        console.log("Test passed");
    }
}

{
    console.log("Running async arrow function test...");

    var pp = (async () => await 19 + await 2)();
    assert(Object.getPrototypeOf(pp) === Promise.prototype);
    pp.then(function (v) {
        assert(v === 21, "Expected value to be 21 but got " + v);
        $DONE();
    }, $DONE);
}

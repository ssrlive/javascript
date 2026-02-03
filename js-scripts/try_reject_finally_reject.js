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
    var f = async() => {
        try {
            await new Promise(function(resolve, reject) {
                reject("early-reject");
            });
        } finally {
            await new Promise(function(resolve, reject) {
                reject("override");
            });
        }
    };

    f().then($DONE, function(value) {
        assert(value === "override", "Awaited rejection in finally block, got: " + value);
    }).then($DONE, $DONE);
}

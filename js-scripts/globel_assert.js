// Minimal Test262 bootstrap for host-provided helpers
// Define globalThis if missing
if (typeof globalThis === 'undefined') {
    try {
        this.globalThis = this;
    } catch (e) {
        this.globalThis = (function(){ return this; })();
    }
}

// $DONE helper for async tests; here it prints reason and throws if provided
if (typeof $DONE === 'undefined') {
    function $DONE(err) {
        if (err) {
            // If err is an Error-like object, throw it so harness sees it
            throw err;
        }
        // For completed tests, just return
    }
    console.log('block-scoped typeof $DONE:', typeof $DONE);
}

console.log('global typeof $DONE:', typeof $DONE);

// Minimal $262 host object stub
if (typeof $262 === 'undefined') {
    var $262 = {
        createRealm: function() { return {}; },
        detachArrayBuffer: function(buf) { /* no-op for now */ },
        evalScript: function(s) { return eval(s); },
        // Add other shims as needed by tests
    };
}

console.log('typeof $262:', typeof $262);

// Provide Test262Error constructor if not present (fallback)
if (typeof Test262Error === 'undefined') {
    function Test262Error(message) {
        this.message = message;
    }
    Test262Error.prototype = {};
    Test262Error.prototype.constructor = Test262Error;
    Test262Error.prototype.name = 'Test262Error';
    Test262Error.prototype.toString = function() { return (this.name || 'Test262Error') + (this.message ? ': ' + this.message : ''); };
    this.Test262Error = Test262Error;
}

// Ensure Error is defined as a function constructor if absent
if (typeof Error === 'undefined') {
    function Error(message) {
        this.message = message;
    }
    Error.prototype = {};
    Error.prototype.constructor = Error;
    this.Error = Error;
}


// --- harness files follow ---

// Provide a minimal `assert` helper if the harness doesn't supply one
if (typeof assert === 'undefined') {
    function assert(cond) {
        if (!cond) throw new Test262Error('assertion failed');
    }
    this.assert = assert;
}

console.log('typeof assert:', typeof assert);

// --- begin assert-false.js ---
// Copyright (C) 2015 the V8 project authors. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
description: >
    `false` does not satisfy the assertion.
---*/

var threw = false;

try {
    assert(false);
} catch(err) {
    threw = true;
    if (err.constructor !== Test262Error) {
        throw new Error(
            'Expected a Test262Error, but a "' + err.constructor.name +
            '" was thrown.'
        );
    }
}

if (threw === false) {
  throw new Error('Expected a Test262Error, but no error was thrown.');
}

console.log('Test262Error successfully thrown.');
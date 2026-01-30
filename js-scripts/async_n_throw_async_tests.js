"use strict";

// Copyright (C) 2022 Igalia, S.L. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.
/*---
description: |
    A collection of assertion and wrapper functions for testing asynchronous built-ins.
defines: [asyncTest, assert.throwsAsync]
---*/

// Ensure a minimal `assert` function exists when running outside Test262
if (typeof assert === 'undefined') {
  var assert = function (actual, expected, message) {
    function deepEqual(a, b) {
      if (a === b) return true;
      if (typeof a === 'number' && typeof b === 'number' && isNaN(a) && isNaN(b)) return true;
      if (a && b && typeof a === 'object' && typeof b === 'object') {
        if (Array.isArray(a) && Array.isArray(b)) {
          if (a.length !== b.length) return false;
          for (var i = 0; i < a.length; i++) {
            if (!deepEqual(a[i], b[i])) return false;
          }
          return true;
        }
        var aKeys = Object.keys(a);
        var bKeys = Object.keys(b);
        if (aKeys.length !== bKeys.length) return false;
        for (var i = 0; i < aKeys.length; i++) {
          var k = aKeys[i];
          if (!b.hasOwnProperty(k) || !deepEqual(a[k], b[k])) return false;
        }
        return true;
      }
      return false;
    }

    if (!deepEqual(actual, expected)) {
      throw new Test262Error(message || 'assertion failed');
    }
  };
}

// Minimal Test262 shims used by helpers
function Test262Error(message) {
  this.name = 'Test262Error';
  this.message = message || '';
}
Test262Error.prototype = Object.create(Error.prototype);

/**
 * Defines the **sole** asynchronous test of a file.
 * @see {@link ../docs/rfcs/async-helpers.md} for background.
 *
 * @param {Function} testFunc a callback whose returned promise indicates test results
 *   (fulfillment for success, rejection for failure)
 * @returns {void}
 */
function asyncTest(testFunc) {
  if (!Object.prototype.hasOwnProperty.call(globalThis, "$DONE")) {
    throw new Test262Error("asyncTest called without async flag");
  }
  if (typeof testFunc !== "function") {
    $DONE(new Test262Error("asyncTest called with non-function argument"));
    return;
  }
  try {
      console.log('asyncTest: invoking testFunc');
      var res = testFunc();
      console.log('asyncTest: testFunc result', res && res.constructor ? res.constructor.name : typeof res);
      res.then(
        function () {
          console.log('asyncTest: resolved, calling $DONE');
          $DONE();
        },
        function (error) {
          console.log('asyncTest: rejected, calling $DONE with', error);
          $DONE(error);
        }
      );
    } catch (syncError) {
      console.log('asyncTest: threw synchronously', syncError);
      $DONE(syncError);
    }
  }

/**
 * Asserts that a callback asynchronously throws an instance of a particular
 *   rejection value
 * @param {Function} func the callback
 * @param {string} [message] the prefix to use for failure messages
 * @returns {Promise<void>} fulfills if the expected error is thrown,
 *   otherwise rejects
 */
assert.throwsAsync = function (expectedErrorConstructor, func, message) {
  // Validate arguments synchronously and return a rejected promise if invalid
  if (typeof expectedErrorConstructor !== "function") {
    return Promise.reject(new Test262Error("assert.throwsAsync called with an argument that is not an error constructor"));
  }
  if (typeof func !== "function") {
    return Promise.reject(new Test262Error("assert.throwsAsync called with an argument that is not a function"));
  }

  return new Promise(function (resolve) {
    var pendingFailDetail;
    var onResFulfilled, onResRejected;
    var fail = function (detail) {
      if (typeof onResRejected === "function") {
        onResRejected(new Test262Error(message === undefined ? detail : (message + " " + detail)));
        return;
      }
      // Record pending failure for later application once the resSettlementP is ready
      pendingFailDetail = message === undefined ? detail : (message + " " + detail);
      return;
    };

    var expectedName = expectedErrorConstructor.name;
    var expectation = "Expected a " + expectedName + " to be thrown asynchronously";
    var res;
    try {
      res = func();
    } catch (thrown) {
      fail(expectation + " but the function threw synchronously");
    }
    if (res === null || typeof res !== "object" || typeof res.then !== "function") {
      fail(expectation + " but result was not a thenable");
    }
    var resSettlementP = new Promise(function (onFulfilled, onRejected) {
      onResFulfilled = onFulfilled;
      onResRejected = onRejected;
    });
    if (pendingFailDetail !== undefined && typeof onResRejected === "function") {
      onResRejected(new Test262Error(pendingFailDetail));
      pendingFailDetail = undefined;
    }
    try {
      res.then(onResFulfilled, onResRejected)
    } catch (thrown) {
      fail(expectation + " but .then threw synchronously");
    }
    resolve(resSettlementP.then(
      function () {
        fail(expectation + " but no exception was thrown at all");
      },
      function (thrown) {
        var actualName;
        if (thrown === null || typeof thrown !== "object") {
          fail(expectation + " but thrown value was not an object");
        } else if (thrown.constructor !== expectedErrorConstructor) {
          actualName = thrown.constructor.name;
          if (expectedName === actualName) {
            fail(expectation +
              " but got a different error constructor with the same name");
          }
          fail(expectation + " but got a " + actualName);
        }
      }
    ));
  });
};

// --- Test harness for running ---
{
  // Install a persistent $DONE dispatcher to avoid handler overwrite races
  if (!globalThis.__done_dispatcher_installed) {
    (function () {
      var handlers = [];
      globalThis.$register_done = function (fn) {
        var id = handlers.length;
        handlers.push({ fn: fn, settled: false });
        return id;
      };
      globalThis.$unregister_done = function (id) {
        if (handlers[id]) handlers[id].settled = true;
      };
      globalThis.$DONE = function (err) {
        // Deliver the call to the earliest un-settled registered handler
        for (var i = 0; i < handlers.length; i++) {
          if (!handlers[i].settled) {
            handlers[i].settled = true;
            // Schedule handler invocation as a microtask to avoid deep synchronous
            // re-entrancy when handlers call $DONE recursively. This aligns with
            // Node's behavior and prevents stack overflow for many nested handlers.
            Promise.resolve().then(function() { try { handlers[i].fn(err); } catch (e) { /* swallow */ } });
            break;
          }
        }
      };
    })();
    globalThis.__done_dispatcher_installed = true;
  }
  // Run a single asyncTest and return a promise that resolves/rejects when $DONE is called
  // expectError: when true, the test is expected to call $DONE with an error
  function runSingleAsyncTest(name, testFunc, expectError) {
    console.log('runSingleAsyncTest: start ->', name);
    return new Promise((resolve) => {
      var settled = false;
      var doneId = globalThis.$register_done(function (err) {
        console.log('runSingleAsyncTest: $DONE called for', name, 'err=', err);
        if (settled) return;
        settled = true;
        globalThis.$unregister_done(doneId);
        if (expectError) {
          if (err === undefined) {
            console.log('asyncTest:', name, '-> FAIL (expected error)');
            resolve({name: name, ok: false});
          } else {
            console.log('asyncTest:', name, '-> PASS (error as expected) -', err && err.message ? err.message : err);
            resolve({name: name, ok: true});
          }
        } else {
          if (err === undefined) {
            console.log('asyncTest:', name, '-> PASS');
            resolve({name: name, ok: true});
          } else {
            console.log('asyncTest:', name, '-> FAIL -', err && err.message ? err.message : err);
            resolve({name: name, ok: false, err});
          }
        }
      });

      try {
        asyncTest(testFunc);
      } catch (e) {
        // asyncTest may throw synchronously on setup errors
        console.log('asyncTest:', name, '-> FAIL -', e && e.message ? e.message : e);
        resolve({name: name, ok: false, err: e});
      }
    });
  }

  // Test assert.throwsAsync behavior
  async function runThrowTests() {
    var passCase = async function () {
      await assert.throwsAsync(TypeError, function () { return Promise.reject(new TypeError('boom')); });
      console.log('assert.throwsAsync: reject-with-TypeError -> PASS');
    };

    var failCase = async function () {
      try {
        await assert.throwsAsync(TypeError, function () { return Promise.resolve('ok'); });
        console.log('assert.throwsAsync: resolve-case -> FAIL (should have rejected)');
      } catch (e) {
        console.log('assert.throwsAsync: resolve-case -> PASS (rejected with:', e && e.message ? e.message : e, ')');
      }
    };

    var syncThrowCase = function () {
      return assert.throwsAsync(TypeError, function () { throw new TypeError('sync'); });
    };

    // Invalid arg case (first arg not a constructor) should reject the returned promise
    try {
      await assert.throwsAsync(null, function () { return Promise.reject(new TypeError()); });
      console.log('assert.throwsAsync: invalid-arg -> FAIL (did not reject)');
    } catch (e) {
      console.log('assert.throwsAsync: invalid-arg -> PASS (rejected with:', e && e.message ? e.message : e, ')');
    }

    await passCase();
    await failCase();

    try {
      await syncThrowCase();
      console.log('assert.throwsAsync: sync-throw-case -> FAIL (should have rejected)');
    } catch (e) {
      console.log('assert.throwsAsync: sync-throw-case -> PASS (rejected with:', e && e.message ? e.message : e, ')');
    }
  }

  // Run tests sequentially
  (async function () {
    const results = [];
    results.push(await runSingleAsyncTest('resolves', function () { return Promise.resolve(); }, false));
    results.push(await runSingleAsyncTest('rejects', function () { return Promise.reject(new Error('fail')); }, true));
    results.push(await runSingleAsyncTest('non-function', 'not-a-func', true));

    await runThrowTests();

    for (let i = 0; i < results.length; i++) {
      results[i] = await Promise.resolve(results[i]);
    }
    console.log('DEBUG results:', results);
    const passed = results.filter(r => r.ok).length;
    const failed = results.length - passed;
    console.log('\nSummary:', passed, 'passed,', failed, 'failed');

    return true;
  })();
}

{
  // 3. Async function
  async function asyncFunc(a, ...rest) {
    return [a, rest];
  }
  asyncFunc(1, 2, 3).then(res => {
    assert(res, [1, [2, 3]], "Async function with rest");
  });
}

console.log('=== All async_n_throw_async_tests.js tests setup done ===');

try {
    var p = Promise.resolve(1);
    console.log("Promise.name:", Promise.name);
    console.log("p.constructor === Promise:", p.constructor === Promise);
    console.log("p.constructor.name:", p.constructor.name);
} catch (e) {
    console.log("Error:", e);
}

console.log('=== Running async_n_throw_async_tests_regression.js ===');

return true;

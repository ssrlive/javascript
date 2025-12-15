// Copyright (C) 2022 Igalia, S.L. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.
/*---
description: |
    A collection of assertion and wrapper functions for testing asynchronous built-ins.
defines: [asyncTest, assert.throwsAsync]
---*/

// Ensure a minimal `assert` object exists when running outside Test262
if (typeof assert === 'undefined') var assert = {};

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
    testFunc().then(
      function () {
        $DONE();
      },
      function (error) {
        $DONE(error);
      }
    );
  } catch (syncError) {
    $DONE(syncError);
  }
}

/**
 * Asserts that a callback asynchronously throws an instance of a particular
 * error (i.e., returns a promise whose rejection value is an object referencing
 * the constructor).
 *
 * @param {Function} expectedErrorConstructor the expected constructor of the
 *   rejection value
 * @param {Function} func the callback
 * @param {string} [message] the prefix to use for failure messages
 * @returns {Promise<void>} fulfills if the expected error is thrown,
 *   otherwise rejects
 */
assert.throwsAsync = function (expectedErrorConstructor, func, message) {
  return new Promise(function (resolve) {
    var fail = function (detail) {
      if (message === undefined) {
        throw new Test262Error(detail);
      }
      throw new Test262Error(message + " " + detail);
    };
    if (typeof expectedErrorConstructor !== "function") {
      fail("assert.throwsAsync called with an argument that is not an error constructor");
    }
    if (typeof func !== "function") {
      fail("assert.throwsAsync called with an argument that is not a function");
    }
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
    var onResFulfilled, onResRejected;
    var resSettlementP = new Promise(function (onFulfilled, onRejected) {
      onResFulfilled = onFulfilled;
      onResRejected = onRejected;
    });
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
  // Run a single asyncTest and return a promise that resolves/rejects when $DONE is called
  // expectError: when true, the test is expected to call $DONE with an error
  function runSingleAsyncTest(name, testFunc, expectError) {
    return new Promise((resolve) => {
      var settled = false;
      globalThis.$DONE = function (err) {
        if (settled) return;
        settled = true;
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
      };

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
    async function passCase() {
      await assert.throwsAsync(TypeError, function () { return Promise.reject(new TypeError('boom')); });
      console.log('assert.throwsAsync: reject-with-TypeError -> PASS');
    }

    async function failCase() {
      try {
        await assert.throwsAsync(TypeError, function () { return Promise.resolve('ok'); });
        console.log('assert.throwsAsync: resolve-case -> FAIL (should have rejected)');
      } catch (e) {
        console.log('assert.throwsAsync: resolve-case -> PASS (rejected with:', e && e.message ? e.message : e, ')');
      }
    }

    function syncThrowCase() {
      return assert.throwsAsync(TypeError, function () { throw new TypeError('sync'); });
    }

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

    const passed = results.filter(r => r.ok).length;
    const failed = results.length - passed;
    console.log('\nSummary:', passed, 'passed,', failed, 'failed');

    return true;
  })();
}

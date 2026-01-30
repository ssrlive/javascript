"use strict";

// Regression harness for async_n_throw_async_tests.js that exposes a
// summary object on globalThis for test assertions.

// (This file is a near copy of async_n_throw_async_tests.js but it
// records `passed` / `failed` and `results` to
// `globalThis.__async_regression_summary` before returning.)

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

function Test262Error(message) {
  this.name = 'Test262Error';
  this.message = message || '';
}
Test262Error.prototype = Object.create(Error.prototype);

// (Copy of async test harness with dispatcher)
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
      for (var i = 0; i < handlers.length; i++) {
        if (!handlers[i].settled) {
          handlers[i].settled = true;
          Promise.resolve().then(function() { try { handlers[i].fn(err); } catch (e) { /* swallow */ } });
          break;
        }
      }
    };
  })();
  globalThis.__done_dispatcher_installed = true;
}

function asyncTest(testFunc) {
  if (!Object.prototype.hasOwnProperty.call(globalThis, "$DONE")) {
    throw new Test262Error("asyncTest called without async flag");
  }
  if (typeof testFunc !== "function") {
    $DONE(new Test262Error("asyncTest called with non-function argument"));
    return;
  }
  try {
    var res = testFunc();
    res.then(
      function () { $DONE(); },
      function (error) { $DONE(error); }
    );
  } catch (syncError) {
    $DONE(syncError);
  }
}

function runSingleAsyncTest(name, testFunc, expectError) {
  return new Promise((resolve) => {
    var settled = false;
    var doneId = globalThis.$register_done(function (err) {
      if (settled) return;
      settled = true;
      globalThis.$unregister_done(doneId);
      if (expectError) {
        if (err === undefined) { console.log('RESOLVE CALL:', name, {name: name, ok: false}); resolve({name: name, ok: false}); }
        else { console.log('RESOLVE CALL:', name, {name: name, ok: true}); resolve({name: name, ok: true}); }
      } else {
        if (err === undefined) { console.log('RESOLVE CALL:', name, {name: name, ok: true}); resolve({name: name, ok: true}); }
        else { console.log('RESOLVE CALL:', name, {name: name, ok: false, err}); resolve({name: name, ok: false, err}); }
      }
    });

    try {
      asyncTest(testFunc);
    } catch (e) {
      resolve({name: name, ok: false, err: e});
    }
  });
}

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
      // If the resSettlementP's rejection handler is available, use it to reject
      if (typeof onResRejected === "function") {
        onResRejected(new Test262Error(message === undefined ? detail : (message + " " + detail)));
        return;
      }
      // Otherwise, record the pending failure so it can be applied once resSettlementP is available
      pendingFailDetail = message === undefined ? detail : (message + " " + detail);
      // Do not throw synchronously here; the pending failure will be applied to the promise
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
    // If a fail was requested before resSettlementP existed, apply it now via onResRejected
    if (pendingFailDetail !== undefined && typeof onResRejected === "function") {
      onResRejected(new Test262Error(pendingFailDetail));
      // clear pending detail after reporting
      pendingFailDetail = undefined;
    }
    try {
      res.then(onResFulfilled, onResRejected)
    } catch (thrown) {
      fail(expectation + " but .then threw synchronously");
    }
    resolve(resSettlementP.then(
      function () { fail(expectation + " but no exception was thrown at all"); },
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

async function runThrowTests() {
  var passCase = async function () {
    await assert.throwsAsync(TypeError, function () { return Promise.reject(new TypeError('boom')); });
  };
  var failCase = async function () {
    try {
      await assert.throwsAsync(TypeError, function () { return Promise.resolve('ok'); });
    } catch (e) {}
  };
  var syncThrowCase = function () {
    return assert.throwsAsync(TypeError, function () { throw new TypeError('sync'); });
  };

  try {
    console.log('RUN: before invalid-arg assert');
    await assert.throwsAsync(null, function () { return Promise.reject(new TypeError()); });
    console.log('RUN: after invalid-arg assert');
  } catch (e) { console.log('RUN: caught invalid-arg'); }

  console.log('RUN: before passCase');
  try {
    await assert.throwsAsync(TypeError, function () { return Promise.reject(new TypeError('boom')); });
    console.log('RUN: after inline passCase');
  } catch (e) { console.log('RUN: inline passCase threw', e && e.message ? e.message : e); }

  console.log('RUN: before failCase');
  try {
    try {
      await assert.throwsAsync(TypeError, function () { return Promise.resolve('ok'); });
      console.log('RUN: inline failCase -> FAIL (should have rejected)');
    } catch (e) {
      console.log('RUN: inline failCase -> PASS (rejected with:', e && e.message ? e.message : e, ')');
    }
  } catch (e) { console.log('RUN: inline failCase outer error', e && e.message ? e.message : e); }

  console.log('RUN: before syncThrowCase');
  try { await assert.throwsAsync(TypeError, function () { throw new TypeError('sync'); }); } catch (e) { console.log('RUN: caught inline syncThrowCase'); }
  console.log('RUN: after syncThrowCase');
}

(async function () {
  const results = [];
  results.push(await runSingleAsyncTest('resolves', function () { return Promise.resolve(); }, false));
  results.push(await runSingleAsyncTest('rejects', function () { return Promise.reject(new Error('fail')); }, true));
  results.push(await runSingleAsyncTest('non-function', 'not-a-func', true));

  await runThrowTests();

  for (let i = 0; i < results.length; i++) {
    results[i] = await Promise.resolve(results[i]);
  }

  // Debug: print raw results to help diagnose ordering/err values
  try {
    console.log('RESULTS JSON:', JSON.stringify(results));
  } catch (e) {
    console.log('RESULTS (toString):', results.map(r => String(r)));
  }

  const passed = results.filter(r => r.ok).length;
  const failed = results.length - passed;

  // Expose summary for regression testing
  globalThis.__async_regression_summary = { passed: passed, failed: failed, results: results };

  console.log('\nSummary:', passed, 'passed,', failed, 'failed');

  return true;
})();
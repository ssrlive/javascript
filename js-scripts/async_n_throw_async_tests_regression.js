"use strict";

// Regression harness for async_n_throw_async_tests.js

// Ensure a minimal `assert` function exists
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

// Global handlers
globalThis._async_regression_error = undefined;
globalThis.onerror = function(msg, url, line, col, err) {
  try { console.log('DBG GLOBAL onerror:', msg, err && err.message ? err.message : err); } catch (e) {}
  globalThis._async_regression_error = err || msg;
};
globalThis.onunhandledrejection = function(ev) {
  try { console.log('DBG GLOBAL onunhandledrejection:', ev && ev.reason && ev.reason.message ? ev.reason.message : ev.reason); } catch (e) {}
  globalThis._async_regression_error = ev && ev.reason;
};

// Dispatcher
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
          Promise.resolve().then(function() {
            try { handlers[i].fn(err); } catch (handler_err) { console.log('DBG: $DONE handler threw'); throw handler_err; }
          });
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
  console.log('ENTER runSingleAsyncTest:', name);
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
      res.then(function (v) {
        onResFulfilled(v);
      }, function (thrown) {
        onResRejected(thrown);
      });
    } catch (thrown) {
      fail(expectation + " but .then threw synchronously");
    }
    
    var finalP = resSettlementP.then(
      function () { console.log('DBG throwsAsync: resSettlementP fulfilled -> creating rejection'); throw new Test262Error(expectation + " but no exception was thrown at all"); },
      function (thrown) {
        console.log('DBG throwsAsync: resSettlementP rejected -> thrown ->', thrown && thrown.message ? thrown.message : thrown);
        var actualName;
        if (thrown === null || typeof thrown !== "object") {
          throw new Test262Error(expectation + " but thrown value was not an object");
        } else if (thrown.constructor !== expectedErrorConstructor) {
          actualName = thrown.constructor.name;
          if (expectedName === actualName) {
            throw new Test262Error(expectation +
              " but got a different error constructor with the same name");
          }
          throw new Test262Error(expectation + " but got a " + actualName);
        }
      }
    );
    resolve(finalP);
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

var regression_results = [];

async function runRegressionTest() {
  var p1;
    p1 = runSingleAsyncTest('resolves', function () { return Promise.resolve(); }, false);
    console.log('DBG: p1 created. p1 is:', JSON.stringify(p1), 'typeof:', typeof p1, 'isPromise:', p1 instanceof Promise);
    var v1 = await p1;
    console.log('DBG: p1 awaited. v1:', JSON.stringify(v1));
    regression_results.push(v1);
    
    regression_results.push(await runSingleAsyncTest('rejects', function () { return Promise.reject(new Error('fail')); }, true));
    regression_results.push(await runSingleAsyncTest('non-function', 'not-a-func', true));

    await runThrowTests();

    console.log('DBG: before resolution loop. results:', JSON.stringify(regression_results));

    // Workaround: Manual unrolling to avoid potential engine bugs with loops/variables across await in async functions
    if (regression_results.length > 0) regression_results[0] = await Promise.resolve(regression_results[0]);
    if (regression_results.length > 1) regression_results[1] = await Promise.resolve(regression_results[1]);
    if (regression_results.length > 2) regression_results[2] = await Promise.resolve(regression_results[2]);

    console.log('DBG: after resolution loop. results:', JSON.stringify(regression_results));

    try {
      console.log('RESULTS JSON:', JSON.stringify(regression_results));
    } catch (e) {
      console.log('RESULTS (toString):', regression_results.map(r => String(r)));
    }

    const passed = regression_results.filter(r => r.ok).length;
    const failed = regression_results.length - passed;

    globalThis.__async_regression_summary = { passed: passed, failed: failed, results: regression_results };

    console.log('\nSummary:', passed, 'passed,', failed, 'failed');

    if (globalThis._async_regression_error !== undefined) {
      try { console.log('DBG GLOBAL error at end:', globalThis._async_regression_error && globalThis._async_regression_error.message ? globalThis._async_regression_error.message : globalThis._async_regression_error); } catch (e) {}
    }

    return true;

}

(async function () {
  await runRegressionTest();
})();

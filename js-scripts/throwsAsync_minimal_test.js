"use strict";

if (typeof assert === 'undefined') {
  var assert = {};
}

// Copy assert.throwsAsync from harness (a simplified version)
assert.throwsAsync = function (expectedErrorConstructor, func, message) {
  if (typeof expectedErrorConstructor !== "function") {
    return Promise.reject(new Error("invalid expected constructor"));
  }
  if (typeof func !== "function") {
    return Promise.reject(new Error("func not a function"));
  }

  return new Promise(function (resolve) {
    var pendingFailDetail;
    var onResFulfilled, onResRejected;
    var fail = function (detail) {
      if (typeof onResRejected === "function") {
        onResRejected(new Error(detail));
        return;
      }
      pendingFailDetail = detail;
      return;
    };
    var expectedName = expectedErrorConstructor.name;
    var expectation = "Expected a " + expectedName + " to be thrown asynchronously";
    var res;
    try {
      res = func();
      console.log('DBG throwsAsync: res after func call ->', res === null ? 'null' : typeof res, res);
    } catch (thrown) {
      console.log('DBG throwsAsync: func threw synchronously ->', thrown && thrown.message ? thrown.message : thrown);
      fail(expectation + " but the function threw synchronously");
    }
    if (res === null || typeof res !== "object" || typeof res.then !== "function") {
      console.log('DBG throwsAsync: res is not thenable ->', res);
      fail(expectation + " but result was not a thenable");
    }
    var resSettlementP = new Promise(function (onFulfilled, onRejected) {
      onResFulfilled = onFulfilled;
      onResRejected = onRejected;
    });
    console.log('DBG throwsAsync: resSettlementP created; pendingFailDetail ->', pendingFailDetail, 'onResRejected defined? ->', typeof onResRejected);
    if (pendingFailDetail !== undefined && typeof onResRejected === "function") {
      console.log('DBG throwsAsync: applying pendingFailDetail via onResRejected ->', pendingFailDetail);
      onResRejected(new Error(pendingFailDetail));
      pendingFailDetail = undefined;
    }
    try {
      console.log('DBG throwsAsync: attaching then handlers');
      res.then(function (v) {
        console.log('DBG throwsAsync: res.then fulfilled with', v);
        try { onResFulfilled(v); } catch (e) { console.log('DBG throwsAsync: onResFulfilled threw ->', e && e.message ? e.message : e); throw e; }
      }, function (thrown) {
        console.log('DBG throwsAsync: res.then rejected with', thrown && thrown.message ? thrown.message : thrown);
        try { onResRejected(thrown); } catch (e) { console.log('DBG throwsAsync: onResRejected threw ->', e && e.message ? e.message : e); throw e; }
      });
    } catch (thrown) {
      console.log('DBG throwsAsync: .then threw synchronously ->', thrown && thrown.message ? thrown.message : thrown);
      fail(expectation + " but .then threw synchronously");
    }
    var finalP = resSettlementP.then(
      function () { console.log('DBG throwsAsync: resSettlementP fulfilled -> creating rejection'); throw new Error(expectation + " but no exception was thrown at all"); },
      function (thrown) {
        console.log('DBG throwsAsync: resSettlementP rejected -> thrown ->', thrown && thrown.message ? thrown.message : thrown);
        var actualName;
        if (thrown === null || typeof thrown !== "object") {
          throw new Error(expectation + " but thrown value was not an object");
        } else if (thrown.constructor !== expectedErrorConstructor) {
          actualName = thrown.constructor.name;
          if (expectedName === actualName) {
            throw new Error(expectation +
              " but got a different error constructor with the same name");
          }
          throw new Error(expectation + " but got a " + actualName);
        }
      }
    );
    finalP.then(function() { console.log('DBG throwsAsync: finalP fulfilled'); }, function() { console.log('DBG throwsAsync: finalP rejected'); });
    resolve(finalP);
  });
};

(async function () {
  try {
    console.log('START minimal throwsAsync test');
    try {
      await assert.throwsAsync(TypeError, function () { return Promise.resolve('ok'); });
      console.log('FAILED: throwsAsync did not reject');
    } catch (e) {
      console.log('PASS: throwsAsync rejected with', e && e.message ? e.message : e);
    }
  } catch (e) {
    console.log('IIFE CATCH', e && e.message ? e.message : e);
  }
})();
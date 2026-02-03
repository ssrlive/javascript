"use strict";

if (typeof assert === 'undefined') {
  var assert = {};
}

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
    if (pendingFailDetail !== undefined && typeof onResRejected === "function") {
      console.log('DBG throwsAsync: applying pendingFailDetail via onResRejected ->', pendingFailDetail);
      onResRejected(new Error(pendingFailDetail));
      pendingFailDetail = undefined;
    }
    try {
      console.log('DBG throwsAsync: attaching then handlers');
      res.then(function (v) {
        console.log('DBG throwsAsync: res.then fulfilled with', v);
        onResFulfilled(v);
      }, function (thrown) {
        console.log('DBG throwsAsync: res.then rejected with', thrown && thrown.message ? thrown.message : thrown);
        onResRejected(thrown);
      });
    } catch (thrown) {
      console.log('DBG throwsAsync: .then threw synchronously ->', thrown && thrown.message ? thrown.message : thrown);
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

(async function () {
  try {
    console.log('RUN: before inline failCase');
    try {
      try {
        await assert.throwsAsync(TypeError, function () { return Promise.resolve('ok'); });
        console.log('RUN: inline failCase -> FAIL (should have rejected)');
      } catch (e) {
        console.log('RUN: inline failCase -> PASS (rejected with:', e && e.message ? e.message : e, ')');
      }
    } catch (e) { console.log('RUN: inline failCase outer error', e && e.message ? e.message : e); }
  } catch (e) {
    console.log('IIFE CATCH', e && e.message ? e.message : e);
  }
})();
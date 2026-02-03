"use strict";

async function run() {
  var a = 1;
  try {
    await Promise.resolve();
  } catch (e) {}
  // Expose result for test harness
  globalThis.__var_scope_result = typeof a;
}

(async function () { await run(); return true; })();

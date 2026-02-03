"use strict";

async function run() {
  var a = 1;
  try {
    await Promise.reject(new Error('boom'));
  } catch (e) {}
  console.log('after await, typeof a ->', typeof a);
}

(async function () { await run(); })();

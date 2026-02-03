"use strict";

async function run() {
  var passCase = async function () { return 42; };
  console.log('passCase declared:', typeof passCase);
  var res = await passCase();
  console.log('res', res);
}

(async function () { await run(); })();

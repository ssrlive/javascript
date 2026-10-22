"use strict";

try {
  let calls = 0;
  (function loop(n) {
    if (n === 0) {
      calls += 1;
      return;
    }
    return loop(n - 1);
  })(10000);
  if (calls === 1) {
    console.log("OK");
  } else {
    console.log("NO");
  }
} catch (_e) {
  console.log("NO");
}

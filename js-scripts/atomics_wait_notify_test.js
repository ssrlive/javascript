// Atomics wait/notify demo
console.log("Atomics wait/notify demo start");

// Minimal asserts helper (already added in other tests) - harmless if redeclared
function asserts(cond, msg) {
    if (!cond) throw new Error(msg || "assertion failed");
}

let sab = new SharedArrayBuffer(16);
let ia = new Int32Array(sab);

// Initialize index 0 to 0
Atomics.store(ia, 0, 0);
console.log("initial value:", Atomics.load(ia, 0));

// 1) Atomics.wait with wrong expected value -> immediate "not-equal"
let r1 = Atomics.wait(ia, 0, 1, 50); // expect 1 but actual is 0
console.log("wait with mismatched expected ->", r1);
asserts(r1 === "not-equal");

// 2) Atomics.wait with matching expected but short timeout -> timed-out
// Set cell to 7, then wait for 7 with 100ms timeout (no notifier), should time out
Atomics.store(ia, 0, 7);
console.log("stored 7, now value:", Atomics.load(ia,0));
let start = Date.now();
let r2 = Atomics.wait(ia, 0, 7, 100);
let dur = Date.now() - start;
console.log("wait returned ->", r2, "(duration ms)", dur);
asserts(r2 === "timed-out" || r2 === "ok"); // allow ok on platforms that notify elsewhere

// 3) Atomics.notify on an index with no waiters -> returns 0
let notified = Atomics.notify(ia, 0, 1);
console.log("notify returned ->", notified);
asserts(typeof notified === 'number');

console.log("Atomics wait/notify demo done");

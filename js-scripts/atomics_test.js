// Atomics basic tests
console.log("Atomics basic tests");

// Minimal assertion helper used by the test harness
function asserts(cond, msg) {
	if (!cond) {
		throw new Error(msg || "assertion failed");
	}
}

let sab = new SharedArrayBuffer(32);
let ia = new Int32Array(sab);

console.log("isLockFree(4):", Atomics.isLockFree(4));

Atomics.store(ia, 0, 100);
console.log("store ->", Atomics.load(ia, 0));
asserts(Atomics.load(ia, 0) === 100);

let old = Atomics.compareExchange(ia, 0, 100, 200);
console.log("compareExchange returned", old, "new value", Atomics.load(ia,0));
asserts(Atomics.load(ia, 0) === 200);

let prev = Atomics.add(ia, 0, 5);
console.log("add returned", prev, "now", Atomics.load(ia,0));
asserts(Atomics.load(ia, 0) === 205);

let exchanged = Atomics.exchange(ia, 0, 0);
console.log("exchange returned", exchanged, "now", Atomics.load(ia,0));
asserts(Atomics.load(ia, 0) === 0);

console.log("Atomics basic tests done");

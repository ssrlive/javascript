// SharedArrayBuffer basic test
console.log("SharedArrayBuffer basic test");

let sab = new SharedArrayBuffer(16);
console.log("SharedArrayBuffer created, byteLength:", sab.byteLength);

let ta = new Int8Array(sab);
console.log("Int8Array length:", ta.length);

ta[0] = 42;
ta[15] = -1;
console.log("ta[0] =", ta[0], "ta[15] =", ta[15]);

console.log("SharedArrayBuffer basic test done");

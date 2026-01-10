"use strict";

function assert(condition, message) {
    if (!condition) {
        throw new Error(message);
    }
}

// Test TypedArray constructors from JavaScript
console.log("Testing TypedArray constructors...");

// Test ArrayBuffer
let buffer = new ArrayBuffer(16);
console.log("ArrayBuffer created with length:", buffer.byteLength);
assert(buffer.byteLength === 16, "ArrayBuffer length should be 16");

// Test DataView
let view = new DataView(buffer);
console.log("DataView created");

// Test TypedArrays
let int8Array = new Int8Array(10);
console.log("Int8Array created with length:", int8Array.length);
assert(int8Array.length === 10, "Int8Array length should be 10");

let uint8Array = new Uint8Array(10);
console.log("Uint8Array created with length:", uint8Array.length);
assert(uint8Array.length === 10, "Uint8Array length should be 10");

let int16Array = new Int16Array(5);
console.log("Int16Array created with length:", int16Array.length);
assert(int16Array.length === 5, "Int16Array length should be 5");

let uint16Array = new Uint16Array(5);
console.log("Uint16Array created with length:", uint16Array.length);
assert(uint16Array.length === 5, "Uint16Array length should be 5");

let int32Array = new Int32Array(3);
console.log("Int32Array created with length:", int32Array.length);
assert(int32Array.length === 3, "Int32Array length should be 3");

let uint32Array = new Uint32Array(3);
console.log("Uint32Array created with length:", uint32Array.length);
assert(uint32Array.length === 3, "Uint32Array length should be 3");

let float32Array = new Float32Array(4);
console.log("Float32Array created with length:", float32Array.length);
assert(float32Array.length === 4, "Float32Array length should be 4");

let float64Array = new Float64Array(2);
console.log("Float64Array created with length:", float64Array.length);
assert(float64Array.length === 2, "Float64Array length should be 2");

console.log("All TypedArray constructors work!");
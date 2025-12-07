// Test TypedArray constructors from JavaScript
console.log("Testing TypedArray constructors...");

// Test ArrayBuffer
let buffer = new ArrayBuffer(16);
console.log("ArrayBuffer created with length:", buffer.byteLength);

// Test DataView
let view = new DataView(buffer);
console.log("DataView created");

// Test TypedArrays
let int8Array = new Int8Array(10);
console.log("Int8Array created with length:", int8Array.length);

let uint8Array = new Uint8Array(10);
console.log("Uint8Array created with length:", uint8Array.length);

let int16Array = new Int16Array(5);
console.log("Int16Array created with length:", int16Array.length);

let uint16Array = new Uint16Array(5);
console.log("Uint16Array created with length:", uint16Array.length);

let int32Array = new Int32Array(3);
console.log("Int32Array created with length:", int32Array.length);

let uint32Array = new Uint32Array(3);
console.log("Uint32Array created with length:", uint32Array.length);

let float32Array = new Float32Array(4);
console.log("Float32Array created with length:", float32Array.length);

let float64Array = new Float64Array(2);
console.log("Float64Array created with length:", float64Array.length);

console.log("All TypedArray constructors work!");
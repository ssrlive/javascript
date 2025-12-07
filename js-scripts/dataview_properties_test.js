// DataView properties test
let buffer = new ArrayBuffer(16);
let view = new DataView(buffer, 4, 8);

console.log("buffer:", typeof view.buffer);
console.log("byteLength:", view.byteLength);
console.log("byteOffset:", view.byteOffset);

"Properties test completed";

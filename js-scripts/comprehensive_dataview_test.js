// Comprehensive DataView test
let buffer = new ArrayBuffer(16);
let view = new DataView(buffer);

// Test set operations at different offsets
view.setInt8(0, -10);
view.setUint8(1, 255);
view.setInt16(2, -1234, true);  // little endian
view.setUint16(6, 56789, false); // big endian
view.setInt32(10, -123456, true);
view.setFloat32(0, 3.14, true);
view.setFloat64(8, 2.71828, false);

// Test get operations
console.log("getInt8(0):", view.getInt8(0));  // Should be part of the float32
console.log("getUint8(1):", view.getUint8(1));  // Should be 255
console.log("getInt16(2, true):", view.getInt16(2, true));  // Should be part of the float32
console.log("getUint16(6, false):", view.getUint16(6, false));  // Should be 56789
console.log("getInt32(10, true):", view.getInt32(10, true));  // Should be -123456
console.log("getFloat32(0, true):", view.getFloat32(0, true));  // Should be 3.14
console.log("getFloat64(8, false):", view.getFloat64(8, false));  // Should be 2.71828

// Test properties
console.log("buffer type:", typeof view.buffer);
console.log("byteLength:", view.byteLength);
console.log("byteOffset:", view.byteOffset);

"Comprehensive test completed";

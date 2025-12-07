// Proper DataView test with non-overlapping regions
let buffer = new ArrayBuffer(32);
let view = new DataView(buffer);

// Set values in non-overlapping regions
view.setInt8(0, -10);
view.setUint8(4, 255);
view.setInt16(8, -1234, true);  // little endian
view.setUint16(12, 56789, false); // big endian
view.setInt32(16, -123456, true);
view.setFloat32(20, 3.14, true);
view.setFloat64(24, 2.71828, false);

// Read back the values
console.log("getInt8(0):", view.getInt8(0));  // Should be -10
console.log("getUint8(4):", view.getUint8(4));  // Should be 255
console.log("getInt16(8, true):", view.getInt16(8, true));  // Should be -1234
console.log("getUint16(12, false):", view.getUint16(12, false));  // Should be 56789
console.log("getInt32(16, true):", view.getInt32(16, true));  // Should be -123456
console.log("getFloat32(20, true):", view.getFloat32(20, true));  // Should be ~3.14
console.log("getFloat64(24, false):", view.getFloat64(24, false));  // Should be ~2.71828

// Test properties
console.log("buffer type:", typeof view.buffer);
console.log("byteLength:", view.byteLength);
console.log("byteOffset:", view.byteOffset);

"Proper test completed";

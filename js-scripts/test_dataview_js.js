// Test DataView JavaScript API
let buffer = new ArrayBuffer(32);
let view = new DataView(buffer);

// Test set operations at non-overlapping positions
view.setInt8(0, -10);
view.setUint8(4, 255);
view.setInt16(8, -1234, true);  // little endian
view.setUint16(12, 56789, false); // big endian
view.setInt32(16, -123456, true);
view.setUint32(20, 987654, false);
view.setFloat32(24, 3.14, true);
view.setFloat64(0, 2.71828, false);  // This will overwrite bytes 0-7

// Test get operations
console.log("getInt8(0):", view.getInt8(0));  // Part of float64
console.log("getUint8(4):", view.getUint8(4));  // Part of float64
console.log("getInt16(8, true):", view.getInt16(8, true));  // Should be -1234
console.log("getUint16(12, false):", view.getUint16(12, false));  // Should be 56789
console.log("getInt32(16, true):", view.getInt32(16, true));  // Should be -123456
console.log("getUint32(20, false):", view.getUint32(20, false));  // Should be 987654
console.log("getFloat32(24, true):", view.getFloat32(24, true));  // Should be ~3.14
console.log("getFloat64(0, false):", view.getFloat64(0, false));  // Should be ~2.71828

// Test properties
console.log("buffer:", view.buffer);
console.log("byteLength:", view.byteLength);
console.log("byteOffset:", view.byteOffset);

"DataView JavaScript API test completed";

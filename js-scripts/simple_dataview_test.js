// Simple DataView test
let buffer = new ArrayBuffer(4);
let view = new DataView(buffer);

console.log("Initial buffer content (should be 0s)");
console.log("getUint8(0):", view.getUint8(0));
console.log("getUint8(1):", view.getUint8(1));
console.log("getUint8(2):", view.getUint8(2));
console.log("getUint8(3):", view.getUint8(3));

console.log("Setting view.setUint8(0, 255)");
view.setUint8(0, 255);

console.log("After setting:");
console.log("getUint8(0):", view.getUint8(0));

"Simple test completed";

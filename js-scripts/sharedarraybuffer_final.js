
try {
    console.log("Testing SharedArrayBuffer...");
    let sab = new SharedArrayBuffer(1024);
    
    // Check byteLength
    if (sab.byteLength !== 1024) {
         throw new Error("byteLength is incorrect: " + sab.byteLength);
    }
    console.log("byteLength OK");

    // Check prototype
    if (!(sab instanceof SharedArrayBuffer)) {
         throw new Error("instanceof SharedArrayBuffer failed");
    }
    console.log("instanceof OK");

    if (sab instanceof ArrayBuffer) {
        // SharedArrayBuffer is NOT an ArrayBuffer
         throw new Error("SharedArrayBuffer should not be instanceof ArrayBuffer");
    }
    console.log("Not instanceof ArrayBuffer OK");

    // Check usage with TypedArray
    let i32 = new Int32Array(sab);
    if (i32.byteLength !== 1024) {
        throw new Error("TypedArray byteLength incorrect: " + i32.byteLength);
    }
    console.log("Int32Array on SAB OK");

    // Check Atomics
    Atomics.store(i32, 0, 42);
    let val = Atomics.load(i32, 0);
    if (val !== 42) {
        throw new Error("Atomics store/load failed: " + val);
    }
    console.log("Atomics basic OK");

    // Check Atomics.wait (timeout)
    // Value at index 0 is 42. verification:
    if (Atomics.load(i32, 0) !== 42) {
       throw new Error("Value check failed before wait");
    }
    console.log("Testing wait (timeout expected)...");
    let initialTime = Date.now();
    let waitResult = Atomics.wait(i32, 0, 42, 100); // 100ms timeout
    let duration = Date.now() - initialTime;
    
    // Note: implementation might not be high precision, but should be > 0.
    console.log("Wait result:", waitResult, "Duration:", duration);
    
    if (waitResult !== "timed-out") {
        throw new Error("Atomics.wait expected 'timed-out', got: " + waitResult);
    }
    
    // Check Atomics.notify (0 waiters)
    let notifyResult = Atomics.notify(i32, 0, 1);
    if (notifyResult !== 0) {
        throw new Error("Atomics.notify expected 0, got: " + notifyResult);
    }
    console.log("Atomics wait/notify OK");

    console.log("SharedArrayBuffer Final Test Passed!");

} catch (e) {
    console.log("Test Failed:");
    console.log(e);
}

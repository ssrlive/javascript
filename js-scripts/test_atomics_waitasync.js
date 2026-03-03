// Inline assert helper
function assert(cond, msg) { if (!cond) throw new Error("Assertion failed: " + msg); }

// Test Atomics.waitAsync existence
(function testWaitAsyncExists() {
    assert(typeof Atomics.waitAsync === 'function', "Atomics.waitAsync should be a function");
    assert(Atomics.waitAsync.length === 4, "Atomics.waitAsync.length should be 4");
    console.log("PASS: Atomics.waitAsync exists with correct length");
})();

// Test Atomics.waitAsync with non-equal value (synchronous not-equal result)
(function testWaitAsyncNotEqual() {
    const sab = new SharedArrayBuffer(16);
    const ia = new Int32Array(sab);
    ia[0] = 42;

    const result = Atomics.waitAsync(ia, 0, 0); // expected=0, actual=42
    assert(result.async === false, "Should be synchronous for not-equal, got async=" + result.async);
    assert(result.value === "not-equal", "Should be 'not-equal', got: " + result.value);
    console.log("PASS: Atomics.waitAsync returns not-equal synchronously");
})();

// Test Atomics.waitAsync with equal value and immediate timeout
(function testWaitAsyncImmediateTimeout() {
    const sab = new SharedArrayBuffer(16);
    const ia = new Int32Array(sab);
    ia[0] = 0;

    const result = Atomics.waitAsync(ia, 0, 0, 0); // timeout=0
    assert(result.async === false, "Should be synchronous for timeout=0, got async=" + result.async);
    assert(result.value === "timed-out", "Should be 'timed-out', got: " + result.value);
    console.log("PASS: Atomics.waitAsync returns timed-out for zero timeout");
})();

// Test Atomics.waitAsync returns promise with matching value
(function testWaitAsyncPromise() {
    const sab = new SharedArrayBuffer(16);
    const ia = new Int32Array(sab);
    ia[0] = 0;

    const result = Atomics.waitAsync(ia, 0, 0, 100); // timeout=100ms
    assert(result.async === true, "Should be async for matching value with timeout, got: " + result.async);
    assert(typeof result.value === 'object', "value should be a Promise object");
    console.log("PASS: Atomics.waitAsync returns {async: true, value: Promise}");
})();

// Test Atomics.waitAsync validation
(function testWaitAsyncValidation() {
    // Must be Int32Array or BigInt64Array
    try {
        const sab = new SharedArrayBuffer(16);
        const ua = new Uint8Array(sab);
        Atomics.waitAsync(ua, 0, 0);
        assert(false, "Should throw for Uint8Array");
    } catch (e) {
        assert(e instanceof TypeError, "Should be TypeError");
        console.log("PASS: Atomics.waitAsync rejects non-Int32Array");
    }

    // Must be SharedArrayBuffer
    try {
        const ab = new ArrayBuffer(16);
        const ia = new Int32Array(ab);
        Atomics.waitAsync(ia, 0, 0);
        assert(false, "Should throw for non-shared buffer");
    } catch (e) {
        assert(e instanceof TypeError, "Should be TypeError");
        console.log("PASS: Atomics.waitAsync rejects non-SharedArrayBuffer");
    }
})();

console.log("All Atomics.waitAsync tests passed!");

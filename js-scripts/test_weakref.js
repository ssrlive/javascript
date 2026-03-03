// Inline assert helper
function assert(cond, msg) { if (!cond) throw new Error("Assertion failed: " + msg); }

// Test WeakRef basic functionality
(function testWeakRefBasic() {
    let target = { name: "test" };
    const wr = new WeakRef(target);

    // deref() should return the target while it's alive
    const derefed = wr.deref();
    assert(derefed === target, "WeakRef.deref() should return target");
    assert(derefed.name === "test", "deref().name should be 'test'");
    console.log("PASS: WeakRef basic deref");
})();

// Test WeakRef with symbol
(function testWeakRefSymbol() {
    const sym = Symbol("test");
    const wr = new WeakRef(sym);
    const derefed = wr.deref();
    assert(derefed === sym, "WeakRef.deref() should return symbol");
    console.log("PASS: WeakRef symbol deref");
})();

// Test WeakRef constructor validation
(function testWeakRefValidation() {
    // Cannot create WeakRef with primitive
    try {
        new WeakRef(42);
        assert(false, "Should have thrown");
    } catch (e) {
        assert(e instanceof TypeError, "Should be TypeError for primitive target");
        console.log("PASS: WeakRef rejects primitive target");
    }

    try {
        new WeakRef("string");
        assert(false, "Should have thrown");
    } catch (e) {
        assert(e instanceof TypeError, "Should be TypeError for string target");
        console.log("PASS: WeakRef rejects string target");
    }

    // Cannot create WeakRef with registered symbol
    try {
        new WeakRef(Symbol.for("registered"));
        assert(false, "Should have thrown");
    } catch (e) {
        assert(e instanceof TypeError, "Should be TypeError for registered symbol");
        console.log("PASS: WeakRef rejects registered symbol");
    }
})();

// Test WeakRef toString tag
(function testWeakRefToStringTag() {
    const wr = new WeakRef({});
    const tag = Object.prototype.toString.call(wr);
    assert(tag === "[object WeakRef]", "toStringTag should be WeakRef, got: " + tag);
    console.log("PASS: WeakRef toStringTag");
})();

// Test WeakRef instanceof
(function testWeakRefInstanceof() {
    const wr = new WeakRef({});
    assert(wr instanceof WeakRef, "should be instanceof WeakRef");
    console.log("PASS: WeakRef instanceof");
})();

// Test WeakRef constructor properties
(function testWeakRefCtorProps() {
    assert(WeakRef.length === 1, "WeakRef.length should be 1");
    assert(WeakRef.name === "WeakRef", "WeakRef.name should be 'WeakRef'");
    console.log("PASS: WeakRef constructor properties");
})();

console.log("All WeakRef tests passed!");

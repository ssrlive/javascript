// Inline assert helper
function assert(cond, msg) { if (!cond) throw new Error("Assertion failed: " + msg); }

// Test FinalizationRegistry basic functionality
(function testFRBasic() {
    let called = false;
    const fr = new FinalizationRegistry(function(heldValue) {
        called = true;
    });

    assert(fr instanceof FinalizationRegistry, "should be instanceof FinalizationRegistry");
    console.log("PASS: FinalizationRegistry basic creation");
})();

// Test FinalizationRegistry.register
(function testFRRegister() {
    const fr = new FinalizationRegistry(function(heldValue) {});

    const target = {};
    const result = fr.register(target, "held value");
    assert(result === undefined, "register should return undefined");
    console.log("PASS: FinalizationRegistry.register returns undefined");
})();

// Test FinalizationRegistry.register with unregister token
(function testFRRegisterWithToken() {
    const fr = new FinalizationRegistry(function(heldValue) {});

    const target = {};
    const token = {};
    fr.register(target, "held value", token);
    console.log("PASS: FinalizationRegistry.register with unregister token");
})();

// Test FinalizationRegistry.register validation
(function testFRRegisterValidation() {
    const fr = new FinalizationRegistry(function() {});

    // target must be an object
    try {
        fr.register(42, "held");
        assert(false, "Should throw for primitive target");
    } catch (e) {
        assert(e instanceof TypeError, "Should be TypeError");
        console.log("PASS: FR rejects primitive target");
    }

    // target and heldValue cannot be the same
    try {
        const obj = {};
        fr.register(obj, obj);
        assert(false, "Should throw when target===heldValue");
    } catch (e) {
        assert(e instanceof TypeError, "Should be TypeError");
        console.log("PASS: FR rejects target===heldValue");
    }

    // unregisterToken must be object/symbol/undefined
    try {
        fr.register({}, "held", 42);
        assert(false, "Should throw for primitive unregisterToken");
    } catch (e) {
        assert(e instanceof TypeError, "Should be TypeError");
        console.log("PASS: FR rejects primitive unregisterToken");
    }
})();

// Test FinalizationRegistry.unregister
(function testFRUnregister() {
    const fr = new FinalizationRegistry(function() {});

    const target1 = {};
    const target2 = {};
    const token = {};

    fr.register(target1, "held1", token);
    fr.register(target2, "held2", token);

    const removed = fr.unregister(token);
    assert(removed === true, "unregister should return true when entries were removed");
    console.log("PASS: FinalizationRegistry.unregister removes entries");
})();

// Test FinalizationRegistry.unregister with no matching token
(function testFRUnregisterNoMatch() {
    const fr = new FinalizationRegistry(function() {});

    const target = {};
    const token1 = {};
    const token2 = {};

    fr.register(target, "held", token1);

    const removed = fr.unregister(token2);
    assert(removed === false, "unregister should return false when no entries matched");
    console.log("PASS: FinalizationRegistry.unregister returns false for no match");
})();

// Test FinalizationRegistry constructor validation
(function testFRCtorValidation() {
    try {
        new FinalizationRegistry(42);
        assert(false, "Should throw for non-callable cleanup");
    } catch (e) {
        assert(e instanceof TypeError, "Should be TypeError");
        console.log("PASS: FR constructor rejects non-callable");
    }
})();

// Test FinalizationRegistry toString tag
(function testFRToStringTag() {
    const fr = new FinalizationRegistry(function() {});
    const tag = Object.prototype.toString.call(fr);
    assert(tag === "[object FinalizationRegistry]", "toStringTag should be FinalizationRegistry, got: " + tag);
    console.log("PASS: FinalizationRegistry toStringTag");
})();

// Test FinalizationRegistry constructor properties
(function testFRCtorProps() {
    assert(FinalizationRegistry.length === 1, "FinalizationRegistry.length should be 1");
    assert(FinalizationRegistry.name === "FinalizationRegistry", "FinalizationRegistry.name should be 'FinalizationRegistry'");
    console.log("PASS: FinalizationRegistry constructor properties");
})();

console.log("All FinalizationRegistry tests passed!");

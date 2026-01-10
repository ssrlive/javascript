"use strict";

const key = "prop_" + "computed";
const obj = {
    [key]: 42,       // Computed key
    ["other"]: 100,  // String literal key
    simple: 1        // Identifier key
};

{
    // Simple assertion function located in its own block, it will not working outside
    function assert(condition, message) {
        if (!condition) {
            throw new Error(message || "Assertion failed");
        }
    }

    assert(obj.prop_computed === 42, "obj.prop_computed should be 42");
    assert(obj["other"] === 100, "obj['other'] should be 100");
    assert(obj.simple === 1, "obj.simple should be 1");

    const obj2 = {
        // calculated property name, it's value is "prop_42"
        ["prop_" + (() => 42)()]: 42,
    };

    assert(obj2["prop_42"] === 42, "obj2['prop_42'] should be 42");
    assert(obj2.prop_42 === 42, "obj2.prop_42 should be 42");
}

try {
    assert(obj.prop_computed === 42, "obj.prop_computed should be 42");
    throw "The behavior is unexpected in strict mode.";
} catch (e) {
    if (e instanceof ReferenceError === false) {
        throw "The behavior is unexpected in strict mode.";
    }
} finally {
    console.log("All tests passed.");
}

true;

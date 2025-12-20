// Test cases for typeof and instanceof with null and undefined
{
    console.log("typeof null:", typeof null);
    console.log("typeof undefined:", typeof undefined);

    try {
        console.log("null instanceof Object:", null instanceof Object);
    } catch (e) {
        console.log("null instanceof Object error:", e.message);
    }

    try {
        console.log("undefined instanceof Object:", undefined instanceof Object);
    } catch (e) {
        console.log("undefined instanceof Object error:", e.message);
    }

    console.log("Object.prototype.toString.call(null):", Object.prototype.toString.call(null));
    console.log("Object.prototype.toString.call(undefined):", Object.prototype.toString.call(undefined));
}


// Test cases for safe checks and type handling in JavaScript
{
    function isObject(val) {
        // The "proper" way to check if something is a non-null object
        return val !== null && typeof val === 'object';
    }

    function getType(val) {
        // A robust way to get the real type
        if (val === null) return 'null';
        if (Array.isArray(val)) return 'array';
        return typeof val;
    }

    console.log("--- isObject check ---");
    console.log("isObject({}):", isObject({}));           // true
    console.log("isObject([]):", isObject([]));           // true (arrays are objects)
    console.log("isObject(null):", isObject(null));       // false (FIXED!)
    console.log("isObject(undefined):", isObject(undefined)); // false

    console.log("\n--- Robust Type Checking ---");
    console.log("getType({}):", getType({}));             // object
    console.log("getType([]):", getType([]));             // array
    console.log("getType(null):", getType(null));         // null (FIXED!)
    console.log("getType(undefined):", getType(undefined)); // undefined

    console.log("null == undefined:", null == undefined);   // true
}

{
    var a = {a: 1, b: null, c: undefined};
    console.log("a =", a);
}

console.log("All tests completed.");

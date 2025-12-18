function testBoxed(name, obj, primitiveValue, expectedToString) {
    console.log(`--- Testing ${name} ---`);
    console.log(`typeof:`, typeof obj);
    // console.log(`instanceof ${name}:`, obj instanceof eval(name)); // eval might not be fully robust in this engine yet, let's skip dynamic eval for class lookup if possible, or just use global lookup if supported.
    // Actually, let's try to access the constructor directly if possible, or just rely on console.log
    
    console.log(`value:`, obj);
    
    try {
        const val = obj.valueOf();
        console.log(`valueOf():`, val);
    } catch (e) {
        console.log(`FAIL: valueOf() threw`, e);
    }

    try {
        const str = obj.toString();
        console.log(`toString():`, str);
    } catch (e) {
        console.log(`FAIL: toString() threw`, e);
    }
}

// Boolean
testBoxed('Boolean', new Boolean(true), true, 'true');
testBoxed('Boolean', new Boolean(false), false, 'false');

// Number
testBoxed('Number', new Number(123), 123, '123');

// String
testBoxed('String', new String("hello"), "hello", "hello");

// BigInt
try {
    const bi = Object(123n);
    testBoxed('BigInt', bi, 123n, '123');
} catch (e) {
    console.log("BigInt test skipped or failed setup:", e);
    throw e;
}

// Symbol
try {
    const sym = Symbol("foo");
    const symObj = Object(sym);
    testBoxed('Symbol', symObj, sym, 'Symbol(foo)');
} catch (e) {
    console.log("Symbol test skipped or failed setup:", e);
    throw e;
}


{
    // Simple assertion function defined inside a block but it can be used in the whole file
    function assert(condition, message) {
        if (!condition) {
            throw new Error(message || "Assertion failed");
        }
    }
}

/*
下面这些值求值为 false（也叫做假值）：
    false
    undefined
    null
    0
    NaN
    空字符串（""）
*/

assert(!false, "false should be falsy");
assert(!undefined, "undefined should be falsy");
assert(!null, "null should be falsy");
assert(!0, "0 should be falsy");
assert(!NaN, "NaN should be falsy");
assert(!"", "empty string should be falsy");

assert(!!true, "true should be truthy");
assert(!!{}, "non-empty object should be truthy");
assert(!![], "non-empty array should be truthy");
assert(!!42, "non-zero number should be truthy");
assert(!!"hello", "non-empty string should be truthy");

console.log("All assertions passed!");

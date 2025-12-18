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

console.log("typeof TypeError = " + typeof TypeError);
console.log("typeof Error = " + typeof Error);
console.log("typeof Function = " + typeof Function);
console.log("typeof Object = " + typeof Object);

try {
    throw new TypeError("msg");
} catch (e) {
    if (!(e instanceof TypeError)) {
        throw e;
    }
}

try {
    throw TypeError("msg");
} catch (e) {
    if (!(e instanceof TypeError)) {
        throw e;
    }
}

if (!(TypeError instanceof Function)) {
    throw new Error("TypeError is not a Function");
}

console.log("typeof TypeError = " + typeof TypeError);
console.log("typeof Error = " + typeof Error);
console.log("typeof Function = " + typeof Function);
console.log("typeof Object = " + typeof Object);

try {
    new TypeError("msg");
    console.log("new TypeError works");
} catch (e) {
    console.log("new TypeError failed: " + e);
}

try {
    TypeError("msg");
    console.log("TypeError() works");
} catch (e) {
    console.log("TypeError() failed: " + e);
}

console.log("Is TypeError instance of Function? " + (TypeError instanceof Function));

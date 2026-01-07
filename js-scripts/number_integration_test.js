// Test Number constants
console.log("=== Testing Number constants... ===");
console.log("MAX_VALUE:", Number.MAX_VALUE);
console.log("MIN_VALUE:", Number.MIN_VALUE);
console.log("NaN:", Number.NaN);
console.log("POSITIVE_INFINITY:", Number.POSITIVE_INFINITY);
console.log("NEGATIVE_INFINITY:", Number.NEGATIVE_INFINITY);
console.log("EPSILON:", Number.EPSILON);
console.log("MAX_SAFE_INTEGER:", Number.MAX_SAFE_INTEGER);
console.log("MIN_SAFE_INTEGER:", Number.MIN_SAFE_INTEGER);

// Test Number constructor as function
console.log("=== Testing Number() constructor as function... ===");
console.log("Number(123): " + Number(123));
console.log("Number('123'): " + Number('123'));
console.log("Number('  123  '): " + Number('  123  '));
console.log("Number(true): " + Number(true));
console.log("Number(false): " + Number(false));
// Note: In standard JS, Number(null) is 0, Number(undefined) is NaN. 
// Checking current implementation behavior:
console.log("Number(null): " + Number(null)); 
console.log("Number(undefined): " + Number(undefined));

// Test new Number()
console.log("=== Testing new Number()... ===");
var n = new Number(123);
console.log("typeof new Number(123): " + typeof n);
console.log("valueOf: " + n.valueOf());
console.log("toString: " + n.toString());

var n2 = new Number("456");
console.log("new Number('456') valueOf: " + n2.valueOf());

// Test static methods
console.log("=== Testing Static methods... ===");
console.log("Number.isNaN(NaN): " + Number.isNaN(NaN));
console.log("Number.isNaN(123): " + Number.isNaN(123));
console.log("Number.isFinite(123): " + Number.isFinite(123));
console.log("Number.isFinite(Infinity): " + Number.isFinite(Infinity));
console.log("Number.isInteger(123): " + Number.isInteger(123));
console.log("Number.isInteger(123.45): " + Number.isInteger(123.45));
console.log("Number.isSafeInteger(9007199254740991): " + Number.isSafeInteger(9007199254740991));
console.log("Number.isSafeInteger(9007199254740992): " + Number.isSafeInteger(9007199254740992));

console.log("Number.parseFloat('123.45'): " + Number.parseFloat('123.45'));
console.log("Number.parseInt('123', 10): " + Number.parseInt('123', 10));
console.log("Number.parseInt('101', 2): " + Number.parseInt('101', 2));

// Test instance methods
console.log("=== Testing Instance methods... ===");
var num = 123.456;
console.log("num = 123.456");
console.log("num.toFixed(2): " + num.toFixed(2));
console.log("num.toExponential(1): " + num.toExponential(1));
console.log("num.toPrecision(4): " + num.toPrecision(4));

console.log("(123).toString(): " + (123).toString());

try {
    console.log("Number.prototype.toFixed.call('not a number')");
    // Function.prototype.call might not be implemented yet.
    if (Number.prototype.toFixed.call) {
        Number.prototype.toFixed.call('not a number');
    } else {
        console.log("Skipping call() test as Function.prototype.call is not implemented.");
    }
} catch (e) {
    console.log("Caught expected error: " + e.name + ": " + e.message);
}

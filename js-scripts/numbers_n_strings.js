"use strict";

0e-5   // 0
0e+5   // 0
5e1    // 50
175e-2 // 1.75
1e3    // 1000
1e-3   // 0.001
1E3    // 1000

1_000_000_000_000
1_050.95
0b1010_0001_1000_0101
0o2_2_5_6
0xA0_B0_C0
1_000_000_000_000_000_000_000n

const biggestNum = Number.MAX_VALUE;
const smallestNum = Number.MIN_VALUE;
const infiniteNum = Number.POSITIVE_INFINITY;
const negInfiniteNum = Number.NEGATIVE_INFINITY;
const notANum = Number.NaN;

Math.sin(1.56);

const b1 = 123n;
// Can be arbitrarily large.
const b2 = -1234567890987654321n;

const b1a = BigInt(123);
// Using a string prevents loss of precision, since long number
// literals don't represent what they seem like.
const b2a = BigInt("-1234567890987654321");

const integer = 12 ** 34; // 4.9222352429520264e+36; only has limited precision
const bigint = 12n ** 34n; // 4922235242952026704037113243122008064n

const bigintDiv = 5n / 2n; // 2n, because there's no 2.5 in BigInt

'foo'
"bar"

"\xA9" // "©"

console.log("\u00A9"); // "©"

"\u{2F804}"

// the same with simple Unicode escapes
"\uD87E\uDC04"

console.log("hello".toUpperCase()); // HELLO

console.log(
  "string text line 1\n\
string text line 2",
);
// "string text line 1
// string text line 2"

console.log(`string text line 1
string text line 2`);
// "string text line 1
// string text line 2"

{
const five = 5;
const ten = 10;
console.log(
  "Fifteen is " + (five + ten) + " and not " + (2 * five + ten) + ".",
);
// "Fifteen is 15 and not 20."
}

{
const five = 5;
const ten = 10;
console.log(`Fifteen is ${five + ten} and not ${2 * five + ten}.`);
// "Fifteen is 15 and not 20."
}


try { console.log("at: " + "abc".at(0)); } catch (e) { console.log("at missing"); }
try { console.log("codePointAt: " + "abc".codePointAt(0)); } catch (e) { console.log("codePointAt missing"); }
try { console.log("matchAll: " + "abc".matchAll(/a/g)); } catch (e) { console.log("matchAll missing"); }
try { console.log("search: " + "abc".search(/a/)); } catch (e) { console.log("search missing"); }
try { console.log("toLocaleLowerCase: " + "ABC".toLocaleLowerCase()); } catch (e) { console.log("toLocaleLowerCase missing"); }
try { console.log("toLocaleUpperCase: " + "abc".toLocaleUpperCase()); } catch (e) { console.log("toLocaleUpperCase missing"); }
try { console.log("normalize: " + "\u00C4".normalize("NFD")); } catch (e) { console.log("normalize missing"); }
try { console.log("toWellFormed: " + "abc".toWellFormed()); } catch (e) { console.log("toWellFormed missing"); }



function assert(condition, message) {
    if (!condition) {
        throw new Error(message || "Assertion failed");
    }
}

function assertEq(actual, expected, message) {
    if (actual !== expected) {
        // Handle NaN comparison
        if (typeof actual === 'number' && isNaN(actual) && typeof expected === 'number' && isNaN(expected)) {
            return;
        }
        // Handle signed zero
        if (actual === 0 && expected === 0) {
            if (1 / actual !== 1 / expected) {
                throw new Error((message || "") + " Expected " + expected + " but got " + actual + " (signed zero mismatch)");
            }
            return;
        }
        throw new Error((message || "") + " Expected " + expected + " but got " + actual);
    }
}

function assertClose(actual, expected, epsilon, message) {
    if (Math.abs(actual - expected) > (epsilon || 1e-9)) {
        throw new Error((message || "") + " Expected " + expected + " but got " + actual);
    }
}

console.log("Testing Math methods...");

// asin
assertClose(Math.asin(0), 0);
assertClose(Math.asin(1), Math.PI / 2);
assertClose(Math.asin(-1), -Math.PI / 2);
assertEq(Math.asin(2), NaN);

// acos
assertClose(Math.acos(1), 0);
assertClose(Math.acos(0), Math.PI / 2);
assertClose(Math.acos(-1), Math.PI);
assertEq(Math.acos(2), NaN);

// atan
assertClose(Math.atan(0), 0);
assertClose(Math.atan(1), Math.PI / 4);
assertClose(Math.atan(-1), -Math.PI / 4);

// atan2
assertClose(Math.atan2(0, 0), 0);
assertClose(Math.atan2(1, 0), Math.PI / 2);
assertClose(Math.atan2(0, 1), 0);
assertClose(Math.atan2(0, -1), Math.PI);

// sinh
assertClose(Math.sinh(0), 0);
assertClose(Math.sinh(1), (Math.E - 1/Math.E) / 2);

// cosh
assertClose(Math.cosh(0), 1);
assertClose(Math.cosh(1), (Math.E + 1/Math.E) / 2);

// tanh
assertClose(Math.tanh(0), 0);
assertClose(Math.tanh(Infinity), 1);
assertClose(Math.tanh(-Infinity), -1);

// asinh
assertClose(Math.asinh(0), 0);

// acosh
assertClose(Math.acosh(1), 0);
assertEq(Math.acosh(0), NaN);

// atanh
assertClose(Math.atanh(0), 0);
assertEq(Math.atanh(2), NaN);

// exp
assertClose(Math.exp(0), 1);
assertClose(Math.exp(1), Math.E);

// expm1
assertClose(Math.expm1(0), 0);
assertClose(Math.expm1(1e-10), 1e-10); // Should be precise for small numbers

// log
assertClose(Math.log(1), 0);
assertClose(Math.log(Math.E), 1);

// log10
assertClose(Math.log10(1), 0);
assertClose(Math.log10(10), 1);
assertClose(Math.log10(100), 2);

// log1p
assertClose(Math.log1p(0), 0);
assertClose(Math.log1p(1e-10), 1e-10); // Should be precise for small numbers

// log2
assertClose(Math.log2(1), 0);
assertClose(Math.log2(2), 1);
assertClose(Math.log2(8), 3);

// fround
assertEq(Math.fround(0), 0);
assertEq(Math.fround(1.5), 1.5);
assertEq(Math.fround(1.337), 1.3370000123977661); // 32-bit float precision

// trunc
assertEq(Math.trunc(13.37), 13);
assertEq(Math.trunc(42.84), 42);
assertEq(Math.trunc(0.123), 0);
assertEq(Math.trunc(-0.123), -0);

// cbrt
assertClose(Math.cbrt(1), 1);
assertClose(Math.cbrt(8), 2);
assertClose(Math.cbrt(-8), -2);

// hypot
assertClose(Math.hypot(3, 4), 5);
assertClose(Math.hypot(3, 4, 5), Math.sqrt(50));

// sign
assertEq(Math.sign(3), 1);
assertEq(Math.sign(-3), -1);
assertEq(Math.sign(0), 0);
assertEq(Math.sign(-0), -0);
assertEq(Math.sign(NaN), NaN);

console.log("All Math tests passed!");


{
function assert(condition, message) {
    if (!condition) {
        throw new Error(message || "Assertion failed");
    }
}

function assertEq(actual, expected, message) {
    if (actual !== expected) {
        throw new Error((message || "") + " Expected '" + expected + "' but got '" + actual + "'");
    }
}

console.log("Testing Number methods...");

// toExponential
var num = 77.1234;
assertEq(num.toExponential(), "7.71234e+1");
assertEq(num.toExponential(4), "7.7123e+1");
assertEq(num.toExponential(2), "7.71e+1");
assertEq((77.1234).toExponential(), "7.71234e+1");

// toFixed
var num2 = 12345.6789;
assertEq(num2.toFixed(), "12346");
assertEq(num2.toFixed(1), "12345.7");
assertEq(num2.toFixed(6), "12345.678900");
assertEq((1.23e+20).toFixed(2), "123000000000000000000.00");
assertEq((1.23e-10).toFixed(2), "0.00");
assertEq((2.34).toFixed(1), "2.3");

// toPrecision
var num3 = 5.123456;
assertEq(num3.toPrecision(), "5.123456");
assertEq(num3.toPrecision(5), "5.1235");
assertEq(num3.toPrecision(2), "5.1");
assertEq(num3.toPrecision(1), "5");

var num4 = 0.000123;
assertEq(num4.toPrecision(), "0.000123"); // JS might output 0.000123 or 1.23e-4 depending on implementation details
assertEq(num4.toPrecision(5), "0.00012300"); // or 1.2300e-4
// Rust {:g} might behave slightly differently than JS toPrecision in edge cases, but let's test basic ones.

assertEq((1234.5).toPrecision(2), "1.2e+3"); // Rust {:g} does this

}

try {
    Number.MAX_VALUE = 42;
    throw new Error("Expected TypeError when assigning to Number.MAX_VALUE");
} catch (e) {
    if (!(e instanceof TypeError)) {
        throw new Error("Expected TypeError when assigning to Number.MAX_VALUE but got " + e);
    }
}

console.log("All Number tests passed!");

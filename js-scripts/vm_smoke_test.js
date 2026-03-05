// VM Smoke Test: exercises all features the bytecode VM currently supports
// Run with: cargo run -p js -- --use-vm js-scripts/vm_smoke_test.js

function assert(cond, msg) {
  if (!cond) {
    throw "FAIL: " + msg;
  }
  console.log("PASS: " + msg);
}

// === 1. Basic arithmetic ===
assert(2 + 3 === 5, "2 + 3 === 5");
assert(10 - 4 === 6, "10 - 4 === 6");
assert(3 * 7 === 21, "3 * 7 === 21");
assert(15 / 3 === 5, "15 / 3 === 5");
assert(17 % 5 === 2, "17 % 5 === 2");

// === 2. Comparison operators ===
assert(1 < 2, "1 < 2");
assert(3 > 2, "3 > 2");
assert(5 <= 5, "5 <= 5");
assert(5 >= 5, "5 >= 5");
assert(1 != 2, "1 != 2");
assert(1 !== "1", '1 !== "1"');

// === 3. Unary operators ===
assert(-(-5) === 5, "-(-5) === 5");
assert(!false === true, "!false === true");
assert(typeof 42 === "number", 'typeof 42 === "number"');
assert(typeof "hi" === "string", 'typeof "hi" === "string"');
assert(typeof true === "boolean", 'typeof true === "boolean"');
assert(typeof undefined === "undefined", 'typeof undefined === "undefined"');

// === 4. Logical operators ===
assert((true && true) === true, "true && true");
assert((true && false) === false, "true && false");
assert((false || true) === true, "false || true");
assert((false || false) === false, "false || false");

// === 5. String concatenation ===
assert("hello" + " " + "world" === "hello world", "string concat");
assert("val: " + 42 === "val: 42", "string + number");

// === 6. Variables and scoping ===
var x = 10;
var y = 20;
assert(x + y === 30, "var x + y");

// === 7. If/else ===
function absVal(n) {
  if (n < 0) {
    return -n;
  } else {
    return n;
  }
}
assert(absVal(-7) === 7, "absVal(-7)");
assert(absVal(3) === 3, "absVal(3)");

// === 8. While loop ===
var sum = 0;
var i = 1;
while (i <= 100) {
  sum = sum + i;
  i++;
}
assert(sum === 5050, "sum 1..100 = 5050");

// === 9. For loop ===
var product = 1;
for (var i = 1; i <= 5; i++) {
  product = product * i;
}
assert(product === 120, "5! = 120");

// === 10. Functions and recursion ===
function factorial(n) {
  if (n <= 1) return 1;
  return n * factorial(n - 1);
}
assert(factorial(10) === 3628800, "factorial(10) = 3628800");

function fib(n) {
  if (n <= 1) return n;
  return fib(n - 1) + fib(n - 2);
}
assert(fib(10) === 55, "fib(10) = 55");

// === 11. Arrays ===
var arr = [10, 20, 30, 40, 50];
assert(arr[0] === 10, "arr[0] === 10");
assert(arr[4] === 50, "arr[4] === 50");
assert(arr.length === 5, "arr.length === 5");

arr[2] = 99;
assert(arr[2] === 99, "arr[2] = 99 (mutation)");

// Sum array with for loop
var arrSum = 0;
for (var i = 0; i < arr.length; i++) {
  arrSum = arrSum + arr[i];
}
assert(arrSum === 219, "array sum = 219");

// === 12. Objects ===
var person = {name: "Alice", age: 30};
assert(person.name === "Alice", 'person.name === "Alice"');
assert(person.age === 30, "person.age === 30");

person.age = 31;
assert(person.age === 31, "person.age = 31 (mutation)");

// === 13. Nested objects and arrays ===
var data = {
  items: [1, 2, 3],
  meta: {count: 3}
};
assert(data.items[1] === 2, "data.items[1] === 2");
assert(data.meta.count === 3, "data.meta.count === 3");

// === 14. Increment/Decrement ===
var a = 5;
var b = a++;
assert(a === 6, "a++ -> a === 6");
assert(b === 5, "a++ -> b === 5 (old value)");

var c = 5;
var d = ++c;
assert(c === 6, "++c -> c === 6");
assert(d === 6, "++c -> d === 6 (new value)");

// === 15. console.log (built-in) ===
console.log("Testing console.log:", 1, true, "abc");

// === 16. Math built-ins ===
assert(Math.floor(3.7) === 3, "Math.floor(3.7) === 3");
assert(Math.ceil(2.1) === 3, "Math.ceil(2.1) === 3");
assert(Math.round(2.5) === 3, "Math.round(2.5) === 3");
assert(Math.abs(-42) === 42, "Math.abs(-42) === 42");
assert(Math.sqrt(144) === 12, "Math.sqrt(144) === 12");
assert(Math.max(3, 7, 1) === 7, "Math.max(3,7,1) === 7");
assert(Math.min(3, 7, 1) === 1, "Math.min(3,7,1) === 1");

// === 17. isNaN / parseInt / parseFloat ===
assert(isNaN(0 / 0) === true, "isNaN(NaN) === true");
assert(isNaN(42) === false, "isNaN(42) === false");

// === 18. try/catch/throw ===
var caught = "none";
try {
  throw "test error";
} catch (e) {
  caught = e;
}
assert(caught === "test error", "try/catch basic");

// Throw from function
function mayFail(n) {
  if (n < 0) throw "negative";
  return n * 10;
}
var result;
try {
  result = mayFail(3);
  result = mayFail(-1);
} catch (e) {
  result = "caught: " + e;
}
assert(result === "caught: negative", "throw from function caught");

// Throw object
var errObj;
try {
  throw {code: 404, msg: "not found"};
} catch (e) {
  errObj = "Error " + e.code + ": " + e.msg;
}
assert(errObj === "Error 404: not found", "throw object");

// === 19. typeof this (removed: global this differs between VM/Node/browser) ===
// assert(typeof this === "undefined", "global this is undefined");

// === 20. Complex integration: build array in loop and sum ===
function buildAndSum(n) {
  var arr = [];
  for (var i = 0; i < n; i++) {
    arr[i] = i * i;
  }
  var total = 0;
  for (var i = 0; i < n; i++) {
    total = total + arr[i];
  }
  return total;
}
// sum of squares 0..9 = 0+1+4+9+16+25+36+49+64+81 = 285
assert(buildAndSum(10) === 285, "sum of squares 0..9 = 285");

// ============================================================
// TIER 4: const, break/continue, do-while, for-in,
//         arrow functions, string/array methods, JSON
// ============================================================

// === 21. const ===
const PI = 3.14;
assert(PI === 3.14, "const PI");
const GREETING = "hello";
assert(GREETING === "hello", "const string");

// === 22. break ===
var breakSum = 0;
for (var i = 0; i < 100; i++) {
  if (i >= 5) break;
  breakSum = breakSum + i;
}
assert(breakSum === 10, "break exits loop at i=5");

// === 23. continue ===
var contSum = 0;
for (var i = 0; i < 10; i++) {
  if (i % 2 === 0) continue;
  contSum = contSum + i;
}
assert(contSum === 25, "continue skips even, sum odd 1..9=25");

// === 24. do-while ===
var dw = 0;
var dwCount = 0;
do {
  dw = dw + dwCount;
  dwCount = dwCount + 1;
} while (dwCount < 5);
assert(dw === 10, "do-while sum 0..4=10");

// body executes at least once even when condition is false
var dwOnce = 0;
do {
  dwOnce = 42;
} while (false);
assert(dwOnce === 42, "do-while runs at least once");

// === 25. for-in ===
var obj = { a: 1, b: 2, c: 3 };
var keys = "";
for (var k in obj) {
  keys = keys + k;
}
// key order is insertion order for string keys
assert(keys === "abc", "for-in iterates object keys");

var forInSum = 0;
for (var k in obj) {
  forInSum = forInSum + obj[k];
}
assert(forInSum === 6, "for-in access values");

// === 26. arrow functions ===
var double = (x) => x * 2;
assert(double(5) === 10, "arrow fn expression body");

var add = (a, b) => { return a + b; };
assert(add(3, 4) === 7, "arrow fn block body");

var zero = () => 0;
assert(zero() === 0, "arrow fn no args");

// === 27. Array.push / Array.pop ===
var arr2 = [10, 20];
arr2.push(30);
assert(arr2.length === 3, "push increases length");
assert(arr2[2] === 30, "push adds element");

var popped = arr2.pop();
assert(popped === 30, "pop returns last");
assert(arr2.length === 2, "pop decreases length");

// === 28. Array.join ===
var joined = [1, 2, 3].join("-");
assert(joined === "1-2-3", "array join with separator");

var joined2 = ["a", "b", "c"].join("");
assert(joined2 === "abc", "array join empty sep");

// === 29. Array.indexOf ===
var idx = [10, 20, 30, 40].indexOf(30);
assert(idx === 2, "array indexOf found");

var idx2 = [10, 20, 30].indexOf(99);
assert(idx2 === -1, "array indexOf not found");

// === 30. Array.slice ===
var sl = [1, 2, 3, 4, 5].slice(1, 3);
assert(sl.length === 2, "slice length");
assert(sl[0] === 2, "slice[0]");
assert(sl[1] === 3, "slice[1]");

// === 31. Array.concat ===
var c = [1, 2].concat([3, 4]);
assert(c.length === 4, "concat length");
assert(c[2] === 3, "concat[2]");
assert(c[3] === 4, "concat[3]");

// === 32. Array.map ===
var mapped = [1, 2, 3].map((x) => x * 10);
assert(mapped.length === 3, "map length");
assert(mapped[0] === 10, "map[0]");
assert(mapped[1] === 20, "map[1]");
assert(mapped[2] === 30, "map[2]");

// === 33. Array.filter ===
var filtered = [1, 2, 3, 4, 5, 6].filter((x) => x % 2 === 0);
assert(filtered.length === 3, "filter length");
assert(filtered[0] === 2, "filter[0]");
assert(filtered[1] === 4, "filter[1]");

// === 34. Array.forEach ===
var feSum = 0;
[10, 20, 30].forEach((x) => { feSum = feSum + x; });
assert(feSum === 60, "forEach sum");

// === 35. Array.reduce ===
var reduced = [1, 2, 3, 4].reduce((acc, x) => acc + x, 0);
assert(reduced === 10, "reduce sum");

// === 36. String.toUpperCase / toLowerCase ===
assert("hello".toUpperCase() === "HELLO", "toUpperCase");
assert("WORLD".toLowerCase() === "world", "toLowerCase");

// === 37. String.trim ===
assert("  hi  ".trim() === "hi", "trim");

// === 38. String.includes ===
assert("hello world".includes("world") === true, "includes found");
assert("hello world".includes("xyz") === false, "includes not found");

// === 39. String.indexOf ===
assert("abcdef".indexOf("cd") === 2, "string indexOf found");
assert("abcdef".indexOf("zz") === -1, "string indexOf not found");

// === 40. String.startsWith / endsWith ===
assert("hello".startsWith("hel") === true, "startsWith true");
assert("hello".startsWith("xyz") === false, "startsWith false");
assert("hello".endsWith("llo") === true, "endsWith true");
assert("hello".endsWith("xyz") === false, "endsWith false");

// === 41. String.slice ===
assert("abcdef".slice(1, 4) === "bcd", "string slice");

// === 42. String.split ===
var parts = "a,b,c".split(",");
assert(parts.length === 3, "split length");
assert(parts[0] === "a", "split[0]");
assert(parts[1] === "b", "split[1]");
assert(parts[2] === "c", "split[2]");

// === 43. String.charAt ===
assert("hello".charAt(1) === "e", "charAt");

// === 44. String.replace ===
assert("hello world".replace("world", "JS") === "hello JS", "replace");

// === 45. String.substring ===
assert("abcdef".substring(2, 5) === "cde", "substring");

// === 46. JSON.stringify ===
assert(JSON.stringify(42) === "42", "JSON.stringify number");
assert(JSON.stringify("hi") === "\"hi\"", "JSON.stringify string");
assert(JSON.stringify(true) === "true", "JSON.stringify bool");
assert(JSON.stringify(null) === "null", "JSON.stringify null");

// === 47. JSON.parse ===
assert(JSON.parse("42") === 42, "JSON.parse number");
assert(JSON.parse("true") === true, "JSON.parse bool");
assert(JSON.parse("null") === null, "JSON.parse null");
assert(JSON.parse("\"hi\"") === "hi", "JSON.parse string");

// === 48. Array.isArray ===
assert(Array.isArray([1, 2]) === true, "isArray true");
assert(Array.isArray(42) === false, "isArray false");

// === 49. Nested method calls ===
var nested = [1, 2, 3].map((x) => x + 1).filter((x) => x > 2);
assert(nested.length === 2, "chained map+filter length");
assert(nested[0] === 3, "chained[0]");
assert(nested[1] === 4, "chained[1]");

// === 50. break in while ===
var wBreak = 0;
while (true) {
  wBreak = wBreak + 1;
  if (wBreak === 10) break;
}
assert(wBreak === 10, "break in while");

// === 51. continue in while ===
var wCont = 0;
var wContI = 0;
while (wContI < 10) {
  wContI = wContI + 1;
  if (wContI % 3 === 0) continue;
  wCont = wCont + 1;
}
assert(wCont === 7, "continue in while skips multiples of 3");

console.log("=== All VM smoke tests (Tier 1-4) passed! ===");

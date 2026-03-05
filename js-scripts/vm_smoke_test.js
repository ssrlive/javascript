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

console.log("=== All VM smoke tests passed! ===");

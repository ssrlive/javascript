{
const re = /ab+c/;
console.log("test_string_methods.js running", re);
}

{
  const re = new RegExp("ab+c");
console.log("test_string_methods.js running", re);
}

{
  const myRe = /d(b+)d/g;
const myArray = myRe.exec("cdbbdbsbz");
console.log("myArray:", myArray);
}

{
  const myArray = /d(b+)d/g.exec("cdbbdbsbz");
// similar to 'cdbbdbsbz'.match(/d(b+)d/g); however,
// 'cdbbdbsbz'.match(/d(b+)d/g) outputs [ "dbbd" ]
// while /d(b+)d/g.exec('cdbbdbsbz') outputs [ 'dbbd', 'bb', index: 1, input: 'cdbbdbsbz' ]
console.log("myArray:", myArray);
}

{
  const myRe = new RegExp("d(b+)d", "g");
const myArray = myRe.exec("cdbbdbsbz");
console.log("myArray:", myArray);
}

{
  const myRe = /d(b+)d/g;
const myArray = myRe.exec("cdbbdbsbz");
console.log(`The value of lastIndex is ${myRe.lastIndex}`);

// "The value of lastIndex is 5"
}

{
  const myArray = /d(b+)d/g.exec("cdbbdbsbz");
console.log(`The value of lastIndex is ${/d(b+)d/g.lastIndex}`);

// "The value of lastIndex is 0"
}

{
// const re = /\w+\s/g;
const re = new RegExp("\\w+\\s", "g");
const str = "fee fi fo fum";
const myArray = str.match(re);
console.log(myArray);

// ["fee ", "fi ", "fo "]
}

{
const str = "fee fi fo fum";
const re = /\w+\s/g;

console.log(re.exec(str)); // ["fee ", index: 0, input: "fee fi fo fum"]
console.log(re.exec(str)); // ["fi ", index: 4, input: "fee fi fo fum"]
console.log(re.exec(str)); // ["fo ", index: 7, input: "fee fi fo fum"]
console.log(re.exec(str)); // null

console.log(str.match(re)); // ["fee ", "fi ", "fo "]
}

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

console.log("Testing RegExp flags...");

// Test 'd' flag (hasIndices)
var re = new RegExp("foo", "d");
assertEq(re.hasIndices, true, "hasIndices should be true");
assertEq(re.flags.includes("d"), true, "flags should include d");

var match = re.exec("barfoobaz");
assert(match !== null, "Should match");
assert(match.indices !== undefined, "indices should be present");
assertEq(match.indices[0][0], 3, "Start index");
assertEq(match.indices[0][1], 6, "End index");

// Test 'v' flag (unicodeSets)
var reV = new RegExp("foo", "v");
assertEq(reV.unicodeSets, true, "unicodeSets should be true");
assertEq(reV.flags.includes("v"), true, "flags should include v");

// Test 'v' flag implies 'u' behavior (basic check)
// Note: In our implementation we map 'v' to 'u' for regress, so it should work for basic unicode stuff.
var reV2 = new RegExp("😊", "v");
assert(reV2.test("😊"), "Should match unicode character with v flag");

console.log("All RegExp flag tests passed!");
}

{
// Test replaceAll
var s = "aabbccaa";
var r1 = s.replaceAll("a", "x");
if (r1 !== "xxbbccxx") throw "replaceAll string failed: " + r1;

var r2 = s.replaceAll(/a/g, "y");
if (r2 !== "yybbccyy") throw "replaceAll global regex failed: " + r2;

try {
    s.replaceAll(/a/, "z");
    throw "replaceAll non-global regex should throw";
} catch (e) {
    // Expected TypeError
}

// Test split
var s2 = "a,b,c";
var sp1 = s2.split(",");
if (sp1.length !== 3 || sp1[0] !== "a" || sp1[1] !== "b" || sp1[2] !== "c") throw "split string failed";

var sp2 = s2.split(/,/);
if (sp2.length !== 3 || sp2[0] !== "a" || sp2[1] !== "b" || sp2[2] !== "c") throw "split regex failed";

var s3 = "a1b2c";
var sp3 = s3.split(/(\d)/);
// Should be ["a", "1", "b", "2", "c"]
if (sp3.length !== 5 || sp3[1] !== "1" || sp3[3] !== "2") throw "split regex capturing failed: " + sp3;

var sp4 = s2.split(",", 2);
if (sp4.length !== 2 || sp4[1] !== "b") throw "split limit failed";

console.log("All String RegExp method tests passed!");

var s5 = "a,b,c";
var sp5 = s5.split(/,/, 2);
if (sp5.length !== 2 || sp5[1] !== "b") throw "split regex limit failed";
console.log("Extra regex limit test passed");
}


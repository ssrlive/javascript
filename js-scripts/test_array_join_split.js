// Test script for array join/split behavior with null and undefined
// Defines an Assert function for testing

function assert(condition, message) {
    if (!condition) {
        throw new Error(message || "Assertion failed");
    }
}

console.log("Testing array join/split behavior with null and undefined");

// Create test array
var arr = [10, 20, 30, 40, undefined, null, true, "sadf"];
console.log("Original array:", arr);

// Test join() - null and undefined become empty strings
var joined = arr.join();
console.log("Joined string:", joined);
assert(joined === "10,20,30,40,,,true,sadf", "Join should produce correct string with empty slots for null/undefined");

// Test split()
var splitArr = joined.split(",");
console.log("Split array:", splitArr);
assert(splitArr.length === 8, "Split should produce 8 elements");

// Compare elements
assert(arr[0] == splitArr[0], "First element should match");
assert(arr[1] == splitArr[1], "Second element should match");
assert(arr[2] == splitArr[2], "Third element should match");
assert(arr[3] == splitArr[3], "Fourth element should match");

// null and undefined become empty strings in join, so split gives ""
assert(splitArr[4] === "", "Fifth element should be empty string (from undefined)");
assert(splitArr[5] === "", "Sixth element should be empty string (from null)");

// Check that original null/undefined don't equal the empty strings
assert(arr[4] != splitArr[4], "Original undefined should not equal empty string");
assert(arr[5] != splitArr[5], "Original null should not equal empty string");

// Continue checking other elements
assert(arr[6] != splitArr[6], "Boolean true should not equal string 'true'");
assert(arr[7] == splitArr[7], "String element should match");

let c = arr + splitArr;
console.log("Concatenated result:", c);
assert(c === "10,20,30,40,,,true,sadf10,20,30,40,,,true,sadf", "Concatenation should produce correct string");

let d = arr.concat(splitArr);
console.log("Concatenated array:", d);
assert(d.length === 16, "Concatenated array should have 16 elements");

console.log("All tests passed!");
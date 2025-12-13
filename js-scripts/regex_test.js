function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "Assertion failed");
  }
}

// Escape user input so it can safely be used as a literal in a regular expression.
function escapeRegExp(string) {
  return string.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  // $& is the entire matched substring
}

var a = "hello. how are you?";
var escapedA = escapeRegExp(a);
console.log(escapedA); // Output: hello\. how are you\?

// You can now safely use escapedA as part of a RegExp without worrying about special characters.
var regex = new RegExp(escapedA);
var result = regex.test(a);
console.log(result); // Output: true

assert(result, "The regex should match the original string");

// Verify console.log prints RegExp objects as /pattern/flags
var re = /ab+c/;
console.log(re);

result

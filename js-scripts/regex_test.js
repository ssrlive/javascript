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

var myRe = new RegExp("d(b+)d", "g");
var myArray = myRe.exec("cdbbdbsbz");
console.log(myArray); // ["dbbd","bb"]

var myRe = /d(b+)d/g;
var myArray = myRe.exec("cdbbdbsbz");
console.log(myArray); // ["dbbd","bb"]

var myArray = /d(b+)d/g.exec("cdbbdbsbz");
console.log(myArray); // ["dbbd","bb"]

var expectedArray = "cdbbdbsbz".match(/d(b+)d/g);
console.log(expectedArray); // ["dbbd"]

var myRe = /d(b+)d/g;
var myArray = myRe.exec("cdbbdbsbz");
console.log(myRe.lastIndex); // 5
console.log("The value of lastIndex is " + myRe.lastIndex);

var myArray = /d(b+)d/g.exec("cdbbdbsbz");
console.log("The value of lastIndex is " + /d(b+)d/g.lastIndex); // 0

var re = /(\w+)\s(\w+)/;
var str = "John Smith";
var newstr = str.replace(re, "$2, $1");
console.log(newstr); // "Smith, John"

result

// The following name string contains multiple spaces and tabs,
// and there may be multiple spaces and tabs between the surname and first name.
var names = "Orange Trump ;Fred Barney; Helen Rigby ; Bill Abel ; Chris Hand ";

var output = ["---------- Original String\n", names + "\n"];

// Prepare two pattern regular expressions and put them into an array.
// Split the string into an array.

// Matching pattern: Match a semicolon and all possible consecutive invisible characters immediately before and after it.
var pattern = /\s*;\s*/;

// Put the strings split by the above matching pattern into an array called nameList.
var nameList = names.split(pattern);

// Create a new matching pattern: Match one or more consecutive invisible characters followed by a string consisting of
// one or more consecutive letters, numbers, and underscores from the basic Latin alphabet,
// Use a pair of parentheses to capture part of the match in this pattern.
// The captured results will be used later.
pattern = /(\w+)\s+(\w+)/;

// Create a new array bySurnameList to temporarily store the names being processed.
var bySurnameList = [];

// Output the elements of nameList and split the names in nameList
// using a comma followed by a space pattern to separate surname and first name, then store in array bySurnameList.
//
// The following replace method replaces the elements in nameList with the pattern $2, $1
// (the second captured match result followed by a comma and a space, then the first captured match result)
// Variables $1 and $2 are the captured match results from above.

output.push("---------- After Split by Regular Expression");

var i, len;
for (i = 0, len = nameList.length; i < len; i++) {
  output.push(nameList[i]);
  bySurnameList[i] = nameList[i].replace(pattern, "$2, $1");
}

// Output the new array
output.push("---------- Names Reversed");
for (i = 0, len = bySurnameList.length; i < len; i++) {
  output.push(bySurnameList[i]);
}

// Sort by surname, then output the sorted array.
bySurnameList.sort();
output.push("---------- Sorted");
for (i = 0, len = bySurnameList.length; i < len; i++) {
  output.push(bySurnameList[i]);
}

output.push("---------- End");

console.log(output.join("\n"));

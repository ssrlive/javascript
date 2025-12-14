var re = /(?:\d{3}|\(\d{3}\))([-\/\.])\d{3}\1\d{4}/;

function testInfo(phoneInput) {
    var OK = re.exec(phoneInput);
}

let phoneNumbers = [
    "123-456-7890",
    "(123) 456-7890",
    "123.456.7890",
    "123/456/7890",
    "123.456-7890",
    "1234567890",
    "123-45-6789",
    "12-3456-7890"
];

for (let number of phoneNumbers) {
   let result = testInfo(number);
    console.log(`Testing "${number}": ${result ? "Matched" : "Not Matched"}`);
}

console.log("Regex test completed.");

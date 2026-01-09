
var obj = {a: 1, b: "hello", c: [1, 2, 3]};
var json = JSON.stringify(obj);
console.log(json);

var parsed = JSON.parse(json);

console.log(parsed.a);
console.log(parsed.b);
console.log(parsed.c.length);
console.log(parsed.c);

true
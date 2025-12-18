var foo = ["one", "two", "three"];

// 不使用解构
var one = foo[0];
var two = foo[1];
var three = foo[2];
console.log(one, two, three); // one two three

// 使用解构
var [one, two, three] = foo;
console.log(one, two, three); // one two three

// 忽略某些值
var [,,three] = foo;
console.log(three); // three

// 使用默认值
var [one, two, three, four = "four"] = foo;
console.log(four); // four

// 与其他语法结合使用
var [one, ...rest] = foo;
console.log(rest); // [ 'two', 'three' ]

var [first, , third] = foo;
console.log(first, third); // one three

var [first, second, ...others] = foo;
console.log(others); // [ 'three' ]


const obj = { a: 1, b: 2, c: 3 };

// 不使用解构
var a = obj.a;
var b = obj.b;
var c = obj.c;
console.log(a, b, c); // 1 2 3

// 使用解构
var { a, b, c } = obj;
console.log(a, b, c); // 1 2 3

// 使用不同的变量名
var { a: alpha, b: beta, c: gamma } = obj;
console.log(alpha, beta, gamma); // 1 2 3

// 使用默认值
var { a, b, c, d = 4 } = obj;
console.log(d); // 4

// 与其他语法结合使用
var { a, ...rest } = obj;
console.log(rest); // { b: 2, c: 3 }

var { a, c } = obj;
console.log(a, c); // 1 3

var { a, b, ...others } = obj;
console.log(others); // { c: 3 }

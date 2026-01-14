"use strict";

function assert(condition, message) {
    if (!condition) {
        throw new Error(message || "Assertion failed");
    }
}

// 创建基本的字符串字面量
var a = `在 JavaScript 中，“\n” 是换行符。`;
console.log(a);

// 多行字符串
var b = `在 JavaScript 中，模板字符串可以
 跨越行，但是由双引号和单引号
 包裹的字符串不行。`;
console.log(b);

// 字符串插值
const _name = 'Lev', time = 'today';
var c = `你好 ${_name}, ${time} 过得怎么样？`;
console.log(c);
assert(c === "你好 Lev, today 过得怎么样？", "Template string interpolation failed");

// 表达式插值
var d = `1 + 6 = ${1 + 6}`;
console.log(d);
assert(d === "1 + 6 = 7", "Template string expression interpolation failed");

var a = 1;
var b = 2;
var s = `${a}${b}`;
console.log("Result:", s);
assert(s === "12", "Template string simple interpolation failed");

var c = {
    valueOf: function() { return 10; },
    toString: function() { return "20"; }
};
var s2 = `${c}`;
console.log("Object Result:", s2, "Typeof s2:", typeof s2);
assert(s2 === "20", "Template string object interpolation failed");

{
    let ct = { valueOf: function() { return 10; }, toString: function() { return "20"; } };
    console.log("ct.toString():", ct.toString());
    console.log("String(ct):", String(ct));
    console.log("`${ct}`:", `${ct}`);
    console.log("valueOf ct:", ct.valueOf(), "toString ct:", ct.toString());
    console.log("Primitive ct:", ct);
}

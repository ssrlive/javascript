function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "断言失败");
  }
}

var num = 0;
outPoint: for (var i = 0; i < 10; i++) {
  for (var j = 0; j < 10; j++) {
    if (i == 5 && j == 5) {
      break outPoint; // 在 i = 5，j = 5 时，跳出所有循环，
      // 返回到整个 outPoint 下方，继续执行
    }
    num++;
  }
}
console.log(num); // 输出 55
assert(num === 55, "break 标签测试失败");

var num = 0;
outPoint: for (var i = 0; i < 10; i++) {
  for (var j = 0; j < 10; j++) {
    if (i == 5 && j == 5) {
      continue outPoint;
    }
    num++;
  }
}
console.log(num); // 95
assert(num === 95, "continue 标签测试失败");

var a = [1, 2, 3, 4, 5];
var theValue = 3;
var sum = 0;
var i;
for (i = 0; i < a.length; i++) {
  if (a[i] == theValue) {
    break;
  }
  sum += a[i];
}
console.log(sum); // 输出 3 (1 + 2)
assert(sum === 3, "break 测试失败");

var x = 0;
var z = 0;
labelCancelLoops: while (true) {
  // console.log("外部循环：" + x);
  x += 1;
  z = 1;
  while (true) {
    // console.log("内部循环：" + z);
    z += 1;
    if (z === 10 && x === 10) {
      break labelCancelLoops;
    } else if (z === 10) {
      break;
    }
  }
}
console.log("x 的最终值是：" + x); // 输出 10
console.log("z 的最终值是：" + z); // 输出 10
assert(x === 10, "break 标签测试失败");
assert(z === 10, "break 标签测试失败");

var i = 0;
var n = 0;
var out = "";
while (i < 5) {
  i++;
  if (i == 3) {
    continue;
  }
  n += i;
  console.log(n);
  out += n + ",";
}
//1,3,7,12
console.log("输出结果为：" + out);
assert(out === "1,3,7,12,", "continue 测试失败");

var i = 0;
var n = 0;
var out = "";
while (i < 5) {
  i++;
  if (i == 3) {
    // continue;
  }
  n += i;
  console.log(n);
  out += n + ",";
}
// 1,3,6,10,15
console.log("输出结果为：" + out);
assert(out === "1,3,6,10,15,", "continue 测试失败");

var i = 0;
var j = 10;
checkiandj: while (i < 4) {
  console.log("in checkiandj, i =", i);
  i += 1;
  checkj: while (j > 4) {
    console.log("in checkj, j =", j);
    j -= 1;
    if (j % 2 == 0) {
      continue checkj;
    }
    console.log(j + " 是奇数。");
  }
  console.log("i = " + i);
  console.log("j = " + j);
}

function dump_props(obj, obj_name) {
  var result = "";
  for (var i in obj) {
    result += obj_name + "." + i + " = " + obj[i] + "<br>";
  }
  result += "<hr>";
  return result;
}
var person = { name: "Nicholas", age: 29, job: "Software Engineer", city: "Seattle" };
var result = dump_props(person, "person");
console.log(result);
assert(
  result.includes("person.name = Nicholas") && result.includes("person.age = 29"),
  "for...in 测试失败"
);

let arr = [3, 5, 7];
arr.foo = "hello";
var out = "";
for (let i in arr) {
  console.log(i); // 输出 "0", "1", "2", "foo"
  out += i + ",";
}
console.log("输出结果为：" + out);
assert(out.includes("foo"), "for...in 测试失败");

var out = "";
for (let i of arr) {
  console.log(i); // 输出 "3", "5", "7"
  out += i + ",";
}
// 注意 for...of 的输出没有出现 "hello"
console.log("输出结果为：" + out);
assert(out === "3,5,7,", "for...of 测试失败");

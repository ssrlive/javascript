function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "断言失败");
  }
}

{
  console.log("数组的创建方式:");

  const element0 = 78;
  const element1 = 79;
  const elementN = 80;

const arr1 = new Array(element0, element1, /* …, */ elementN);
const arr2 = Array(element0, element1, /* …, */ elementN);
const arr3 = [element0, element1, /* …, */ elementN];

  assert(arr1.length === 3, "arr1 长度错误");
  assert(arr2.length === 3, "arr2 长度错误");
  assert(arr3.length === 3, "arr3 长度错误");

  // 数组是对象，=== 比较的是引用。要比较内容，可以转换为字符串比较。
  assert(JSON.stringify(arr1) === JSON.stringify(arr2), "arr1 和 arr2 内容不相等");
  assert(JSON.stringify(arr1) === JSON.stringify(arr3), "arr1 和 arr3 内容不相等");
  assert(JSON.stringify(arr2) === JSON.stringify(arr3), "arr2 和 arr3 内容不相等");
}

{
  console.log("创建指定长度的数组:");
  const arrayLength = 20;

// This...
const arr1 = new Array(arrayLength);

// … results in the same array as this
const arr2 = Array(arrayLength);

// This has exactly the same effect
const arr3 = [];
arr3.length = arrayLength;

  arr1[6] = 123;
  arr2[6] = 123;
  arr3[6] = 123;

  console.log("arr1 = ", arr1);
  console.log("arr2 = ", arr2);
  console.log("arr3 = ", arr3);

  assert(arr1.length === arrayLength, "arr1 长度错误");
  assert(arr2.length === arrayLength, "arr2 长度错误");
  assert(arr3.length === arrayLength, "arr3 长度错误");

  // 数组是对象，=== 比较的是引用。要比较内容，可以转换为字符串比较。
  assert(JSON.stringify(arr1) === JSON.stringify(arr2), "arr1 和 arr2 内容不相等");
  assert(JSON.stringify(arr1) === JSON.stringify(arr3), "arr1 和 arr3 内容不相等");
  assert(JSON.stringify(arr2) === JSON.stringify(arr3), "arr2 和 arr3 内容不相等");
}

{
  console.log("在对象属性中创建数组:");

  const element0 = 1;
  const element1 = 2;
  const elementN = 3;
  
// Using an assignment after object creation
const obj = {};
// …
obj.prop = [element0, element1, /* …, */ elementN];

// OR
const obj2 = { prop: [element0, element1, /* …, */ elementN] };

  assert(JSON.stringify(obj) === JSON.stringify(obj2), "obj 和 obj2 内容不相等");
}

{
  console.log("创建不同长度的数组:");

  const arr = [10];
  const arr2 = Array(10);
  
  const arr3 = [];
  arr3.length = 10;

  console.log("arr =", arr, "arr2 =", arr2, "arr3 =", arr3);

  assert(Array.isArray(arr), "arr 不是数组");
  assert(Array.isArray(arr2), "arr2 不是数组");
  assert(Array.isArray(arr3), "arr3 不是数组");

  assert(arr.length === 1, "arr 长度错误");
  assert(arr2.length === 10, "arr2 长度错误");
  assert(arr3.length === 10, "arr3 长度错误");
}

{
  console.log("创建无效长度的数组:");

  try {
    const arr = Array(9.3); // RangeError: Invalid array length
    console.log("arr =", arr);
  } catch (e) {
    // console.log("捕获到错误: " + e.message);
    assert(e instanceof RangeError, "捕获到的错误不是 RangeError");
  }
}

{
  console.log("使用 Array.of 创建数组:");

  const arr = Array.of(9.3); // arr contains only one element 9.3
  console.log("arr =", arr);

  assert(arr.length === 1, "arr 长度错误");
  assert(arr[0] === 9.3, "arr[0] 内容错误");
}

{
  console.log("数组的基本操作:");

  const arr = ["one", "two", "three"];
  assert(arr[0] === "one", "arr[0] 内容错误");
  assert(arr[1] === "two", "arr[1] 内容错误");
  assert(arr[2] === "three", "arr[2] 内容错误");
  assert(arr.length === 3, "arr 长度错误");
}

{
  console.log("使用非整数属性创建数组:");

  const arr = [];
  arr[3.4] = "Oranges";
  console.log(arr.length); // 0
  console.log(Object.hasOwn(arr, 3.4)); // true
  assert(arr.length === 0, "arr 长度错误");
  assert(Object.hasOwn(arr, 3.4), "arr 不包含属性 3.4");
}

{
  console.log("使用不同方式创建数组:");

  let myVar = 42;
  const myArray1 = new Array("Hello", myVar, 3.14159);
  // OR
  const myArray2 = ["Mango", "Apple", "Orange"];
  console.log("myArray1 =", myArray1);
  console.log("myArray2 =", myArray2);
}

{
  console.log("数组的稀疏性示例:");

  const cats = [];
  cats[30] = ["Dusty"];
  console.log(cats.length); // 31
  assert(cats.length === 31, "cats 长度错误");
}

{
  console.log("修改数组的 length 属性:");

  const cats = ["Dusty", "Misty", "Twiggy"];
  console.log(cats.length); // 3

  cats.length = 2;
  console.log(cats); // [ 'Dusty', 'Misty' ] - Twiggy has been removed

  cats.length = 0;
  console.log(cats); // []; the cats array is empty

  cats.length = 3;
  console.log(cats); // [ <3 empty items> ]
  console.log("JSON(cats):", JSON.stringify(cats));
  console.log("JSON([,,,]):", JSON.stringify([ , , , ]));
  assert(JSON.stringify(cats) === JSON.stringify([ , , , ]), "cats 内容错误");
}

{
  console.log("遍历数组的不同方法:");

  const colors = ["red", "green", "blue"];
  for (let i = 0; i < colors.length; i++) {
    console.log(colors[i]);
  }

  // Or use forEach
  colors.forEach((color) => console.log(color));

  // Or use for...of
  for (const color of colors) {
    console.log(color);
  }
}

{
  console.log("稀疏数组与非稀疏数组的区别:");

  const sparseArray = ["first", "second", , "fourth"];

  sparseArray.forEach((element) => {
    console.log(element);
  });
  // Logs:
  // first
  // second
  // fourth
  if (sparseArray[2] === undefined) {
    console.log("sparseArray[2] is undefined"); // true
  }

  const nonsparseArray = ["first", "second", undefined, "fourth"];

  nonsparseArray.forEach((element) => {
    console.log(element);
  });
  // Logs:
  // first
  // second
  // undefined
  // fourth

  const arr = Array.from({ length: 3 });
  console.log("arr =", arr, "JSON:", JSON.stringify(arr)); // [ <3 empty items> ]
  assert(JSON.stringify(arr) === JSON.stringify([ , , , ]), "arr 内容错误");
}

{
  console.log("数组的 concat 方法示例:");

  let myArray = ["1", "2", "3"];
  myArray = myArray.concat("b", "c", "a");
  // myArray is now ["1", "2", "3", "b", "c", "a"]
  console.log("myArray =", myArray);
  assert(JSON.stringify(myArray) === JSON.stringify(["1", "2", "3", "b", "c", "a"]), "myArray 内容错误");
}

{
  console.log("数组的 join 方法示例:");

  const myArray = ["Wind", "Rain", "Fire"];
  const list = myArray.join(" - "); // list is "Wind - Rain - Fire"
  console.log("list =", list);
  assert(list === "Wind - Rain - Fire", "list 内容错误");
}

{
  console.log("数组的 push 方法示例:");

  const myArray = ["1", "2"];
  myArray.push("3"); // myArray is now ["1", "2", "3"]
  console.log("myArray =", myArray);
  assert(JSON.stringify(myArray) === JSON.stringify(["1", "2", "3"]), "myArray 内容错误");
}

{
  console.log("数组的 pop 方法示例:");

  const myArray = ["1", "2", "3"];
  const last = myArray.pop();
  // myArray is now ["1", "2"], last = "3"
  console.log("myArray =", myArray, "last =", last);
  assert(JSON.stringify(myArray) === JSON.stringify(["1", "2"]), "myArray 内容错误");
  assert(last === "3", "last 内容错误");
}

{
  console.log("数组的 shift 方法示例:");

  const myArray = ["1", "2", "3"];
  const first = myArray.shift();
  // myArray is now ["2", "3"], first is "1"
  console.log("myArray =", myArray, "first =", first);
  assert(JSON.stringify(myArray) === JSON.stringify(["2", "3"]), "myArray 内容错误");
  assert(first === "1", "first 内容错误");
}

{
  console.log("数组的 unshift 方法示例:");

  const myArray = ["1", "2", "3"];
  myArray.unshift("4", "5");
  // myArray becomes ["4", "5", "1", "2", "3"]
  console.log("myArray =", myArray);
  assert(JSON.stringify(myArray) === JSON.stringify(["4", "5", "1", "2", "3"]), "myArray 内容错误");
}

{
  console.log("数组的 slice 方法示例:");

  let myArray = ["a", "b", "c", "d", "e"];
  myArray = myArray.slice(1, 4); // [ "b", "c", "d"]
  // starts at index 1 and extracts all elements until index 3
  console.log("myArray =", myArray);
  assert(JSON.stringify(myArray) === JSON.stringify(["b", "c", "d"]), "myArray 内容错误");
}

{
  console.log("数组的 at 方法示例:");

  const myArray = ["a", "b", "c", "d", "e"];
  const element = myArray.at(-2); // "d", the second-last element of myArray
  console.log("element =", element);
  assert(element === "d", "element 内容错误");

  // Positive index
  assert(myArray.at(0) === 'a', "at(0) should be 'a'");
  assert(myArray.at(2) === 'c', "at(2) should be 'c'");

  // Negative index
  assert(myArray.at(-1) === 'e', "at(-1) should be 'e'");
  assert(myArray.at(-2) === 'd', "at(-2) should be 'd'");

  // Out of bounds
  assert(myArray.at(5) === undefined, "at(5) should be undefined");
  assert(myArray.at(-6) === undefined, "at(-6) should be undefined");
}

{
  console.log("数组的 splice 方法示例:");

  const myArray = ["1", "2", "3", "4", "5"];
  let res = myArray.splice(1, 3, "a", "b", "c", "d");
  // myArray is now ["1", "a", "b", "c", "d", "5"]
  // This code started at index one (or where the "2" was),
  // removed 3 elements there, and then inserted all consecutive
  // elements in its place.
  console.log("myArray =", myArray, "res =", res);
  assert(JSON.stringify(myArray) === JSON.stringify(["1", "a", "b", "c", "d", "5"]), "myArray 内容错误");
  assert(JSON.stringify(res) === JSON.stringify(["2", "3", "4"]), "res 内容错误");
}

// Test shrinking
{
    console.log("数组的 splice 方法示例（缩小数组）:");

    const arr = ["1", "2", "3", "4", "5"];
    // Remove 3 elements at index 1, insert 1 element "a"
    // Result should be ["1", "a", "5"]
    const res = arr.splice(1, 3, "a");
    
    console.log("arr =", JSON.stringify(arr));
    console.log("res =", JSON.stringify(res));
    
    assert(JSON.stringify(arr) === JSON.stringify(["1", "a", "5"]), "Shrinking failed: arr content");
    assert(JSON.stringify(res) === JSON.stringify(["2", "3", "4"]), "Shrinking failed: res content");
    assert(arr.length === 3, "Shrinking failed: length");
}

// Test sparse array splice
{
    console.log("数组的 splice 方法示例（稀疏数组）:");

    const arr = ["1", , "3"]; // index 1 is hole
    // Remove 1 element at index 0, insert nothing
    // Result should be [, "3"] (hole at 0, "3" at 1)
    const res = arr.splice(0, 1);
    
    console.log("arr =", JSON.stringify(arr));
    console.log("res =", JSON.stringify(res));
    
    // JSON.stringify converts holes to null
    assert(JSON.stringify(arr) === '[null,"3"]', "Sparse splice failed: arr content");
    assert(JSON.stringify(res) === '["1"]', "Sparse splice failed: res content");
    assert(arr.length === 2, "Sparse splice failed: length");
    assert(!("0" in arr), "Index 0 should be a hole");
}

{
  console.log("数组的 reverse 方法示例:");

  const myArray = ["1", "2", "3"];
  myArray.reverse();
  // transposes the array so that myArray = ["3", "2", "1"]
  console.log("myArray =", myArray);
  assert(JSON.stringify(myArray) === JSON.stringify(["3", "2", "1"]), "myArray 内容错误");
}

{
  console.log("数组的 flat 方法示例:");

  let myArray = [1, 2, [3, 4]];
  myArray = myArray.flat();
  // myArray is now [1, 2, 3, 4], since the [3, 4] subarray is flattened
  console.log("myArray =", myArray);
  assert(JSON.stringify(myArray) === JSON.stringify([1, 2, 3, 4]), "myArray 内容错误");
}

{
  console.log("数组的 flatMap 方法示例:");

  const arr = [1, 2, 3];
  const mapped = arr.flatMap(x => [x, x * 2]);
  console.log("mapped =", JSON.stringify(mapped));
  assert(JSON.stringify(mapped) === JSON.stringify([1, 2, 2, 4, 3, 6]), "flatMap failed");
  assert(Array.isArray(mapped), "flatMap should return an array");
}

{
  console.log("数组的 sort 方法示例:");

  const myArray = ["Wind", "Rain", "Fire"];
  myArray.sort();
  // sorts the array so that myArray = ["Fire", "Rain", "Wind"]
  console.log("myArray =", myArray);
  assert(JSON.stringify(myArray) === JSON.stringify(["Fire", "Rain", "Wind"]), "myArray 内容错误");
}

{
  console.log("数组的 sort 方法示例（自定义排序函数）:");

  const myArray = ["Wind", "Rain", "Fire"];
  const sortFn = (a, b) => {
    if (a[a.length - 1] < b[b.length - 1]) {
      return -1; // Negative number => a < b, a comes before b
    } else if (a[a.length - 1] > b[b.length - 1]) {
      return 1; // Positive number => a > b, a comes after b
    }
    return 0; // Zero => a = b, a and b keep their original order
  };
  myArray.sort(sortFn);
  // sorts the array so that myArray = ["Wind","Fire","Rain"]
  console.log("myArray =", myArray);
  assert(JSON.stringify(myArray) === JSON.stringify(["Wind", "Fire", "Rain"]), "myArray 内容错误");
}

{
  console.log("数组的 indexOf 方法示例:");

  const a = ["a", "b", "a", "b", "a"];
  assert(a.indexOf("b") === 1, "Index of 'b' should be 1");

  // Now try again, starting from after the last match
  assert(a.indexOf("b", 2) === 3, "Index of 'b' starting from 2 should be 3");
  assert(a.indexOf("z") === -1, "Index of 'z' should be -1 because 'z' was not found");
}

{
  console.log("数组的 lastIndexOf 方法示例:");

  const a = ["a", "b", "c", "d", "a", "b"];
  assert(a.lastIndexOf("b") === 5, "Last index of 'b' should be 5");

  // Now try again, starting from before the last match
  assert(a.lastIndexOf("b", 4) === 1, "Last index of 'b' starting from 4 should be 1");
  assert(a.lastIndexOf("z") === -1, "Last index of 'z' should be -1 because 'z' was not found");
}

{
  console.log("数组的 forEach 方法示例:");

  const a = ["a", "b", "c"];
  a.forEach((element) => {
    console.log(element);
  });
  // Logs:
  // a
  // b
  // c
}

{
  console.log("数组的 map 方法示例:");

  const a1 = ["a", "b", "c"];
  const a2 = a1.map((item) => item.toUpperCase());
  console.log(a2); // ['A', 'B', 'C']
  assert(JSON.stringify(a2) === JSON.stringify(['A', 'B', 'C']), "a2 内容错误");
}

{
  console.log("数组的 flatMap 方法示例:");
  const a1 = ["a", "b", "c"];
  const a2 = a1.flatMap((item) => [item.toUpperCase(), item.toLowerCase()]);
  console.log(a2); // ['A', 'a', 'B', 'b', 'C', 'c']  
  assert(JSON.stringify(a2) === JSON.stringify(['A', 'a', 'B', 'b', 'C', 'c']), "a2 内容错误");
}

{
  console.log("数组的 filter 方法示例:");

  const a1 = ["a", 10, "b", 20, "c", 30];
  const a2 = a1.filter((item) => typeof item === "number");
  console.log(a2); // [10, 20, 30]
  assert(JSON.stringify(a2) === JSON.stringify([10, 20, 30]), "a2 内容错误");
}

{
  console.log("数组的 find 方法示例:");

  const a1 = ["a", 10, "b", 20, "c", 30];
  const i = a1.find((item) => typeof item === "number");
  console.log(i); // 10
  assert(i === 10, "i 内容错误");
}

{
  console.log("数组的 findLast 方法示例:");

  const a1 = ["a", 10, "b", 20, "c", 30];
  const i = a1.findLast((item) => typeof item === "number");
  console.log(i); // 30
  assert(i === 30, "i 内容错误");
}

{
  console.log("数组的 findIndex 方法示例:");

  const a1 = ["a", 10, "b", 20, "c", 30];
  const i = a1.findIndex((item) => typeof item === "number");
  console.log(i); // 1
  assert(i === 1, "i 内容错误");
}

{
  console.log("数组的 findLastIndex 方法示例:");

  const a1 = ["a", 10, "b", 20, "c", 30];
  const i = a1.findLastIndex((item) => typeof item === "number");
  console.log(i); // 5
  assert(i === 5, "i 内容错误");
}

{
  console.log("数组的 every 方法示例:");

  function isNumber(value) {
    return typeof value === "number";
  }

  const a1 = [1, 2, 3];
  console.log(a1.every(isNumber)); // true
  assert(a1.every(isNumber) === true, "a1.every 内容错误");

  const a2 = [1, "2", 3];
  console.log(a2.every(isNumber)); // false
  assert(a2.every(isNumber) === false, "a2.every 内容错误");
}

{
  console.log("数组的 some 方法示例:");

  function isNumber(value) {
    return typeof value === "number";
  }

  const a1 = [1, 2, 3];
  console.log(a1.some(isNumber)); // true
  assert(a1.some(isNumber) === true, "a1.some 内容错误");

  const a2 = [1, "2", 3];
  console.log(a2.some(isNumber)); // true
  assert(a2.some(isNumber) === true, "a2.some 内容错误");

  const a3 = ["1", "2", "3"];
  console.log(a3.some(isNumber)); // false
  assert(a3.some(isNumber) === false, "a3.some 内容错误");
}

{
  console.log("数组的 reduce 方法示例:");

  const a = [10, 20, 30];
  const total = a.reduce(
    (accumulator, currentValue) => accumulator + currentValue,
    0,
  );
  console.log(total); // 60
  assert(total === 60, "total 内容错误");
}

{
  console.log("数组的 reduceRight 方法示例:");

  const a = [10, 20, 30];
  const total = a.reduceRight(
    (accumulator, currentValue) => accumulator + currentValue,
    0,
  );
  console.log(total); // 60
  assert(total === 60, "total 内容错误");
}

{
  console.log("Object.groupBy 方法示例:");

  const inventory = [
    { name: "asparagus", type: "vegetables" },
    { name: "bananas", type: "fruit" },
    { name: "goat", type: "meat" },
    { name: "cherries", type: "fruit" },
    { name: "fish", type: "meat" },
  ];
  const result = Object.groupBy(inventory, ({ type }) => type);
  console.log(result);
  console.log("vegetables:", result.vegetables);
  console.log("JSON:", JSON.stringify(result));
  // Logs
  // {
  //   vegetables: [{ name: 'asparagus', type: 'vegetables' }],
  //   fruit: [
  //     { name: 'bananas', type: 'fruit' },
  //     { name: 'cherries', type: 'fruit' }
  //   ],
  //   meat: [
  //     { name: 'goat', type: 'meat' },
  //     { name: 'fish', type: 'meat' }
  //   ]
  // }
}

{
  console.log("数组的 Array 构造函数示例:");

  // Array constructor:
  const a = Array(5); // [ <5 empty items> ]
  console.log(a);

  // Consecutive commas in array literal:
  const b = [1, 2, , , 5]; // [ 1, 2, <2 empty items>, 5 ]
  console.log(b);

  // Directly setting a slot with index greater than array.length:
  const c = [1, 2];
  c[4] = 5; // [ 1, 2, <2 empty items>, 5 ]
  console.log(c);

  // Elongating an array by directly setting .length:
  const d = [1, 2];
  d.length = 5; // [ 1, 2, <3 empty items> ]
  console.log(d);

  // Deleting an element:
  const e = [1, 2, 3, 4, 5];
  delete e[2]; // [ 1, 2, <1 empty item>, 4, 5 ]
  console.log(e);
}

{
  console.log("稀疏数组的不同访问方式:");

  const arr = [1, 2, , , 5]; // Create a sparse array

  // Indexed access
  console.log(arr[2]); // undefined

  // For...of
  for (const i of arr) {
    console.log(i);
  }
  // Logs: 1 2 undefined undefined 5

  // Spreading
  const another = [...arr]; // "another" is [ 1, 2, undefined, undefined, 5 ]
  console.log(another);

  assert(JSON.stringify(arr) === JSON.stringify(another), "arr and another 内容不相等");
}

{
  console.log("Array methods with sparse arrays:");

  const arr = [1, 2, , , 5]; // Create a sparse array
  console.log("arr =", JSON.stringify(arr));
  
  const mapped = arr.map((i) => i + 1); // [ 2, 3, <2 empty items>, 6 ]
  console.log("JSON(mapped):", JSON.stringify(mapped));
  assert(JSON.stringify(mapped) === JSON.stringify([2, 3, , , 6]), "mapped 内容错误");

  arr.forEach((i) => console.log(i)); // 1 2 5

  const filtered = arr.filter(() => true); // [ 1, 2, 5 ]
  console.log("JSON(filtered):", JSON.stringify(filtered));
  assert(JSON.stringify(filtered) === JSON.stringify([1, 2, 5]), "filtered 内容错误");

  const hasFalsy = arr.some((k) => !k); // false
  console.log("hasFalsy:", hasFalsy);
  assert(hasFalsy === false, "hasFalsy 内容错误");

  // Property enumeration
  const keys = Object.keys(arr); // [ '0', '1', '4' ]
  for (const key in arr) {
    console.log(key);
  }
  console.log("keys =", keys);
  assert(JSON.stringify(keys) === JSON.stringify(['0', '1', '4']), "keys 内容错误");

  // Spreading into an object uses property enumeration, not the array's iterator
  const objectSpread = { ...arr }; // { '0': 1, '1': 2, '4': 5 }
  console.log("objectSpread =", JSON.stringify(objectSpread));
  assert(JSON.stringify(objectSpread) === JSON.stringify({ '0': 1, '1': 2, '4': 5 }), "objectSpread 内容错误");
}

{
  console.log("扩展数组的 length 属性:");

  var arr = [1, 2, , , 5];
  console.log("Original arr =", arr, "JSON:", JSON.stringify(arr));
  arr.length = 10;
  console.log("Extended arr =", arr, "JSON:", JSON.stringify(arr));
  assert(arr.length === 10, "arr 长度错误");

  arr.property = "value";
  console.log("property in arr:", arr.property); // "value"
}

{
  console.log("创建二维数组:");
  const a = new Array(4);
  for (let i = 0; i < 4; i++) {
    a[i] = new Array(4);
    for (let j = 0; j < 4; j++) {
      a[i][j] = `[${i}, ${j}]`;
    }
  }
  console.log("2D array a =", a);
}

{
  console.log("使用 Array.prototype.forEach 处理字符串:");

  Array.prototype.forEach.call("a string", (chr) => {
    console.log(chr);
  });
}

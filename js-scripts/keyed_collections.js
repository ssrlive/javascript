function assert(condition, message) {
    if (!condition) {
        throw new Error(message);
    }
}

{
  console.log("=== Map Example ===");

  const sayings = new Map();
  sayings.set("dog", "woof");
  sayings.set("cat", "meow");
  sayings.set("elephant", "toot");
  assert(sayings.size === 3, "Map size should be 3"); // sayings.size; // 3

  console.log("sayings dog =", sayings.get("dog")); // woof
  console.log("sayings fox =", sayings.get("fox")); // undefined
  console.log("sayings has bird =", sayings.has("bird")); // false

  sayings.delete("dog");
  console.log("sayings has dog =", sayings.has("dog")); // false

  for (const [key, value] of sayings) {
    console.log(`${key} goes ${value}`);
  }
  // "cat goes meow"
  // "elephant goes toot"

  sayings.clear();
  assert(sayings.size === 0, "Map size should be 0");
}

{
  console.log("=== WeakMap Example ===");
  const privates = new WeakMap();

  function Public() {
    const me = {
      // Private data goes here
      registered: [],
    };
    privates.set(this, me);
  }

  Public.prototype.method = function () {
    const me = privates.get(this);
    if (me) {
      console.log("Private data accessed:", me);
      me.registered.push("test");
      console.log("Updated private data:", me.registered);
    } else {
      throw new Error("Private data not found!");
    }
  };

  const instance = new Public();
  instance.method();
}

{
  console.log("=== Set Example ===");
  const mySet = new Set();
  mySet.add(1);
  mySet.add("some text");
  mySet.add("foo");

  console.log("mySet has 1:", mySet.has(1)); // true
  assert(mySet.has(1) === true, "Set should have 1");

  mySet.delete("foo");
  console.log("mySet size after delete:", mySet.size); // 2
  assert(mySet.size === 2, "Set size should be 2");

  console.log("Iterating over Set:");
  for (const item of mySet) {
    console.log(item);
  }
  // 1
  // "some text"
}

{
  console.log("=== Array <-> Set Conversion Example ===");

  // 1. Create Set from Array
  const mySet = new Set([1, 2, 3, 4]);
  console.log("Set created from [1, 2, 3, 4], size:", mySet.size);
  assert(mySet.size === 4, "Set size should be 4");

  // 2. Convert Set to Array using Array.from
  const arr1 = Array.from(mySet);
  console.log("Array.from(mySet):", arr1);
  assert(arr1.length === 4, "arr1 length should be 4");
  assert(arr1[0] === 1, "arr1[0] should be 1");

  // 3. Convert Set to Array using spread syntax
  const arr2 = [...mySet];
  console.log("[...mySet]:", arr2);
  assert(arr2.length === 4, "arr2 length should be 4");
  assert(arr2[3] === 4, "arr2[3] should be 4");
}

{
  console.log("=== Array.from Map Example ===");
  const map = new Map();
  map.set("k1", "v1");
  map.set("k2", "v2");
  
  const arr = Array.from(map);
  console.log("Array.from(map):", arr);
  // Expected: [["k1", "v1"], ["k2", "v2"]]
  assert(arr.length === 2, "Array from map should have length 2");
  assert(Array.isArray(arr[0]), "First element should be an array");
  assert(arr[0][0] === "k1", "First key should be k1");
  assert(arr[0][1] === "v1", "First value should be v1");
}

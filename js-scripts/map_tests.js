function assert(condition, message) {
    if (!condition) {
        throw new Error(message || "Assertion failed");
    }
}

console.log("=== Testing Map integration... ===");

let map = new Map();

console.log(map.size); // should be 0
assert(map.size === 0, "Initial map size should be 0");
map.set('a', 1);
console.log(map.size); // should be 1
assert(map.size === 1, "Map size should be 1 after adding one element");
console.log(map.get('a')); // 1
assert(map.get('a') === 1, "Value for key 'a' should be 1");
console.log(map.has('a')); // true
assert(map.has('a') === true, "Map should have key 'a'");
console.log(map.has('b')); // false
assert(map.has('b') === false, "Map should not have key 'b'");
map.set('a', 2);
console.log(map.get('a')); // 2
assert(map.get('a') === 2, "Value for key 'a' should be updated to 2");

map.delete('a');
console.log(map.size); // should be 0
assert(map.size === 0, "Map size should be 0 after deleting the element");

let map2 = new Map();
console.log(map2.size); // 0
assert(map2.size === 0, "New map size should be 0");

var map3 = new Map([ ['x', 10], ['y', 20] ]);
console.log("map3 size", map3.size); // 2
assert(map3.size === 2, "Map initialized with iterable should have correct size");
console.log("map3 x", map3.get('x')); // 10
assert(map3.get('x') === 10, "Value for key 'x' should be 10");
console.log("map3 y", map3.get('y')); // 20
assert(map3.get('y') === 20, "Value for key 'y' should be 20");
console.log("map3 has x", map3.has('x')); // true
assert(map3.has('x') === true, "Map should have key 'x'");

const iterator = map3.keys();
assert(typeof iterator.next === 'function', "Iterator should have next() method");
assert(iterator.next().value === 'x', "First key should be 'x'");
assert(iterator.next().value === 'y', "Second key should be 'y'");
assert(iterator.next().done === true, "Iterator should be done after two keys");

const entriesIterator = map3.entries();
let entry = entriesIterator.next();
assert(entry.value[0] === 'x' && entry.value[1] === 10, "First entry should be ['x', 10]");
entry = entriesIterator.next();
assert(entry.value[0] === 'y' && entry.value[1] === 20, "Second entry should be ['y', 20]");
assert(entriesIterator.next().done === true, "Entries iterator should be done after two entries");

const valuesIterator = map3.values();
assert(valuesIterator.next().value === 10, "First value should be 10");
assert(valuesIterator.next().value === 20, "Second value should be 20");
assert(valuesIterator.next().done === true, "Values iterator should be done after two values");

console.log("All tests passed");

true

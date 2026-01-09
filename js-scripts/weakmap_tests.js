let wm = new WeakMap();
let o = {};
wm.set(o, 42);
console.log(wm.get(o));
console.log(wm.has(o));
wm.delete(o);
console.log(wm.has(o));

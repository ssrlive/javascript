let ws = new WeakSet();
let o = {};
ws.add(o);
console.log(ws.has(o));
ws.delete(o);
console.log(ws.has(o));

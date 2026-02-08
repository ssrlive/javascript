const variants = [
  "a?.b().c",
  "(a?.b)().c",
  "a.b?.().c",
  "(a.b)?.().c",
  "a?.b?.().c",
  "(a?.b)?.().c"
];
const a = { b() { return this._b; }, _b: { c: 42 } };
for (let i = 0; i < variants.length; i++) {
  try {
    console.log(String(i+1), eval(variants[i]));
  } catch (e) {
    console.log(String(i+1) + ' err', e && e.message);
  }
}

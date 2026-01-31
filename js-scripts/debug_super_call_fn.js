var executed = false;
function f() {
  eval('executed = true; super();');
}
try {
  f();
  console.log('no throw');
} catch (e) {
  console.log('caught typeof =', typeof e);
  console.log('caught.constructor name =', e && e.constructor && e.constructor.name);
  console.log('caught.constructor === SyntaxError =', e && e.constructor === SyntaxError);
  console.log('caught instanceof SyntaxError =', e instanceof SyntaxError);
  try { console.log('toString:', String(e)); } catch (err) { console.log('toString thrown', err); }
  try { console.log('keys:', Object.getOwnPropertyNames(e)); } catch (err) { console.log('getOwnPropertyNames thrown', err); }
  console.log('raw e:', e);
}
console.log('executed=', executed);

// feature probe for 'symbols-as-weakmap-keys'
try {
  var wm = new WeakMap();
  var s = Symbol('probe');
  wm.set(s, 1);
  if (wm.get(s) !== 1) throw new Error('fail');
  console.log('OK');
} catch (_) {
  console.log('NO');
}

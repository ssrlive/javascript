// feature probe for 'ShadowRealm'
try {
  if (typeof ShadowRealm !== 'function') throw new Error('ShadowRealm missing');
  var r = new ShadowRealm();
  if (r.evaluate('1 + 1') !== 2) throw new Error('evaluate failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

// feature probe for 'default-parameters' support
try {
  function f(a = 3) { return a; }
  if (f() !== 3) throw new Error('default-parameters failed');
  if (f(5) !== 5) throw new Error('default-parameters override failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

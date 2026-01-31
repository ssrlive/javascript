// feature probe for 'arrow-function' support
try {
  const f = (x) => x + 1;
  if (f(1) !== 2) throw new Error('arrow function failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

// feature probe for 'logical-assignment-operators'
try {
  let a = 1;
  let b = 0;
  let c = null;
  a &&= 2;
  b ||= 3;
  c ??= 4;
  if (a !== 2 || b !== 3 || c !== 4) throw new Error('logical assignment failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

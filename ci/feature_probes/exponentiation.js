// feature probe for 'exponentiation'
try {
  if (2 ** 3 !== 8) throw new Error('exponentiation failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

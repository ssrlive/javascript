// feature probe for 'object-rest' support
try {
  const src = { a: 1, b: 2, c: 3 };
  const { a, ...rest } = src;
  if (a !== 1) throw new Error('object-rest a failed');
  if (rest.b !== 2 || rest.c !== 3) throw new Error('object-rest rest failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

// feature probe for 'const' support
try {
  const x = 1;
  if (x !== 1) throw new Error('const failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

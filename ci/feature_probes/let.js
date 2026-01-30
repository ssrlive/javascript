// feature probe for 'let' support
try {
  let x = 1;
  if (x !== 1) throw new Error('let failed');
  console.log('OK');
} catch (e) {
  process.exit(1);
}

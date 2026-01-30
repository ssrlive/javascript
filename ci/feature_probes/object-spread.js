// feature probe for 'object-spread' support
try {
  const a = {x:1, y:2};
  const b = {...a, z:3};
  if (b.x !== 1 || b.y !== 2 || b.z !== 3) throw new Error('object-spread failed');
  console.log('OK');
} catch (e) {
  process.exit(1);
}

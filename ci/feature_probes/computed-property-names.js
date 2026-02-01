// feature probe for 'computed-property-names' support
try {
  const k = 'a' + 'b';
  const obj = { [k]: 42 };
  if (obj.ab !== 42) throw new Error('computed-property-names failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

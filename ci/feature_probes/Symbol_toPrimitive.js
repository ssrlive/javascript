// feature probe for 'Symbol.toPrimitive'
try {
  if (typeof Symbol === 'undefined' || typeof Symbol.toPrimitive === 'undefined') throw new Error('Symbol.toPrimitive missing');
  const obj = { [Symbol.toPrimitive]() { return 42; } };
  // Coercion should call the method
  if (String(obj) !== '42') throw new Error('Symbol.toPrimitive coercion failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

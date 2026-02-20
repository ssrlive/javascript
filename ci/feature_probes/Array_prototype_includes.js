// feature probe for 'Array.prototype.includes'
try {
  if (typeof Array.prototype.includes !== 'function') throw new Error('Array.prototype.includes missing');
  if (![1, 2, 3].includes(2)) throw new Error('includes returned wrong result');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

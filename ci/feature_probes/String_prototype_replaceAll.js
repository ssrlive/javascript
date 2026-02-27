// feature probe for 'String.prototype.replaceAll'
try {
  if (typeof ''.replaceAll !== 'function') throw new Error('replaceAll missing');
  if ('aXbXc'.replaceAll('X', '_') !== 'a_b_c') throw new Error('replaceAll wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }

// feature probe for 'Array.prototype.at'
try {
  if (typeof [].at !== 'function') throw new Error('at missing');
  if ([1,2,3].at(-1) !== 3) throw new Error('at wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }

// feature probe for 'Object.hasOwn'
try {
  if (typeof Object.hasOwn !== 'function') throw new Error('Object.hasOwn missing');
  if (!Object.hasOwn({a:1},'a'))thrownewError('hasOwnwrong');
  if (Object.hasOwn({a:1}, 'b')) throw new Error('hasOwn false positive');
  console.log('OK');
} catch (e) { console.log('NO'); }

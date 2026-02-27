// feature probe for 'array-find-from-last'
try {
  if (typeof [].findLast !== 'function') throw new Error('findLast missing');
  if (typeof [].findLastIndex !== 'function') throw new Error('findLastIndex missing');
  if ([1,2,3].findLast(function(x){return x<3;}) !== 2) throw new Error('findLast wrong');
  if ([1,2,3].findLastIndex(function(x){return x<3;}) !== 1) throw new Error('findLastIndex wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }

// feature probe for 'stable-array-sort'
try {
  var arr = [];
  for (var i = 0; i < 20; i++) arr.push({k: i % 3, v: i});
  arr.sort(function(a, b) { return a.k - b.k; });
  // Check stability: within same key, original order preserved
  var lastV = -1;
  for (var j = 0; j < arr.length; j++) {
    if (arr[j].k === 0) {
      if (arr[j].v < lastV) throw new Error('unstable');
      lastV = arr[j].v;
    }
  }
  console.log('OK');
} catch (e) { console.log('NO'); }

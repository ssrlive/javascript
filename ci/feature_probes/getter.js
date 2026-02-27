// feature probe for 'getter'
try {
  var obj = { get x() { return 42; } };
  if (obj.x !== 42) throw new Error('getter failed');
  console.log('OK');
} catch (e) { console.log('NO'); }

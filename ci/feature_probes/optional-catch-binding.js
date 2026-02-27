// feature probe for 'optional-catch-binding'
try {
  eval('try { throw 1; } catch { }');
  console.log('OK');
} catch (e) { console.log('NO'); }

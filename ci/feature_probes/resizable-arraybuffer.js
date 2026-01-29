try {
  new ArrayBuffer(1, { maxByteLength: 1 });
  console.log('OK');
} catch (e) { console.log('NO'); }

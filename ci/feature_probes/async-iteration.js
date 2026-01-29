try {
  async function* f() {}
  console.log('OK');
} catch (e) { console.log('NO'); }

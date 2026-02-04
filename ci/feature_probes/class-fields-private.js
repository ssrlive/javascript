try {
  class C {
    #x = 1;
  }
  new C();
  console.log('OK');
} catch (e) {
  console.log('NO');
}

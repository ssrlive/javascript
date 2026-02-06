// feature probe for 'class-static-block'
try {
  class C {
    static {
      this.value = 1;
    }
  }
  if (C.value !== 1) throw new Error('class static block failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

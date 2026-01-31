// feature probe for 'super' support (in class methods)
try {
  class A { m() { return 1; } }
  class B extends A { m() { return super.m() + 1; } }
  const b = new B();
  if (b.m() !== 2) throw new Error('super failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

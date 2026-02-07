try {
  class C {
    #x = 1;

    check() {
      // Use of `#x in obj` syntax (private-in) should be parsed and evaluated
      if (!(#x in this)) throw new Error('private-in evaluation failed');
      return true;
    }
  }

  const c = new C();
  if (!c.check()) throw new Error('check returned false');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

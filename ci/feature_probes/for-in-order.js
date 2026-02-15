try {
  var o = { p1: 'p1', p2: 'p2', p3: 'p3' };
  o.p4 = 'p4';
  o[2] = '2';
  o[0] = '0';
  o[1] = '1';
  delete o.p1;
  delete o.p3;
  o.p1 = 'p1';

  var keys = [];
  for (var key in o) {
    keys.push(key);
  }

  var expected = ['0', '1', '2', 'p2', 'p4', 'p1'];
  if (keys.length !== expected.length) {
    console.log('NO');
  } else {
    var ok = true;
    for (var i = 0; i < expected.length; i++) {
      if (keys[i] !== expected[i]) {
        ok = false;
        break;
      }
    }
    console.log(ok ? 'OK' : 'NO');
  }
} catch (e) {
  console.log('NO');
}

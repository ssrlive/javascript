try {
  var iterationCount = 0;
  var x;

  for ([x] of [[0]]) {
    if (x !== 0) {
      console.log('NO');
      throw new Error('bad destructuring value');
    }
    iterationCount += 1;
  }

  if (iterationCount === 1) {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}

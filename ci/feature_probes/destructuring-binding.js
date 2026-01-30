try {
  // Test object and array destructuring in binding position
  var { x } = { x: 1 };
  let [ y, ...rest ] = [ 2, 3, 4 ];
  const { a: { b } } = { a: { b: 3 } };
  if (x === 1 && y === 2 && b === 3 && rest.length === 2) {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}

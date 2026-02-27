// feature probe for 'destructuring-assignment'
try {
  eval('var [a, b] = [1, 2]; if (a !== 1 || b !== 2) throw new Error("wrong");');
  eval('var {x, y} = {x: 3, y: 4}; if (x !== 3 || y !== 4) throw new Error("wrong");');
  console.log('OK');
} catch (e) { console.log('NO'); }

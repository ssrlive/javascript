function side(n){ console.log("SIDE "+n); return 999; }

// 1: base null — should short-circuit and NOT call side(1)
console.log('CASE 1', (null)?.f?.(side(1)));

// 2: object with method — should call method and evaluate side(2)
console.log('CASE 2', ({f: function(x){ console.log('IN F'); return x; }}).f?.(side(2)));

// 3: bare function expression optional-call — should call it
console.log('CASE 3', (function(){ return 42; })?.());

// 4: primitive string optional property length
console.log('CASE 4', ("abc")?.length);

// 5: getter returning a function — getter should run, then function called
let obj = { get a(){ console.log('GETTER A'); return function(){ return 123 } } };
console.log('CASE 5', obj?.a?.());

// 6: index optional-call with null base — should short-circuit and not evaluate side(6)
console.log('CASE 6', (null)?.['f']?.(side(6)));

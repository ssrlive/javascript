// feature probe for 'BigInt'
try {
  // Try BigInt literal and constructor
  let a = 1n;
  if (typeof a !== 'bigint') throw new Error('BigInt literal failed');
  let b = BigInt(2);
  if (typeof b !== 'bigint') throw new Error('BigInt constructor failed');
  // Basic arithmetic
  if (a + b !== 3n) throw new Error('BigInt arithmetic failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}

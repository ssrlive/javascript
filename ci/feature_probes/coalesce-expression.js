const a = null ?? 1;
const b = 0 ?? 2;
const c = undefined ?? 3;

if (a === 1 && b === 0 && c === 3) {
  console.log('OK');
} else {
  console.log('NO');
}

const obj = { prop: 42 };
const val = obj?.prop;
const missing = obj?.missing;

if (val === 42 && missing === undefined) {
    console.log('OK');
} else {
    console.log('NO');
}

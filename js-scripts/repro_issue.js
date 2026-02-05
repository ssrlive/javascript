async function* inner() {
    yield 1;
    yield 2;
}

async function* gen() {
    // Create a promise that resolves to an async iterator
    const p = Promise.resolve(inner());
    // This line tests if 'await' correctly suspends and unwraps 'p'
    // before 'yield*' attempts to delegate to it.
    yield* await p;
    yield 3;
}

async function main() {
    const result = [];
    try {
        for await (const val of gen()) {
            result.push(val);
        }
        if (result.length === 3 && result[0] === 1 && result[1] === 2 && result[2] === 3) {
            console.log("PASSED");
        } else {
            console.log("FAILED: Got " + JSON.stringify(result));
        }
    } catch (e) {
        console.log("CRASHED: " + e);
    }
}

main();

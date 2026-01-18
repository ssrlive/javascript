"use strict";

function assert(condition, message) {
    if (!condition) {
        throw new Error(message);
    }
}

/*
{
    function failed() {
        console.log('In failed function');
        throw new Error('Intentional failure');
    }

    function function_1() {
        console.log('In function_1');
        failed();
    }

    function function_2() {
        console.log('In function_2');
        function_1();
    }

    function function_3() {
        console.log('In function_3');
        function_2();
    }

    try {
        function_3();
    } catch (e) {
        console.log('Caught error:', e);
    }

    function_3();
}
// */

{
    console.log('=== Sample promise chain with .then() and .catch() ===');

    // Simple promise-based helpers used by the sample chain below
    function doSomething() {
        console.log('Starting doSomething');
        // throw new Error('Something went wrong in doSomething');
        return new Promise((resolve) => {
            // Simulate async work and resolve with a number
            setTimeout(() => resolve(1), 10);
        });
    }

    function doSomethingElse(result) {
        console.log('Starting doSomethingElse with', result);
        // throw new Error('Something went wrong in doSomethingElse');
        return new Promise((resolve) => {
            // Take previous result and return a new promise
            setTimeout(() => resolve(result + 2), 10);
        });
    }

    function doThirdThing(result) {
        console.log('Starting doThirdThing with', result);
        // throw new Error('Something went wrong in doThirdThing');
        return new Promise((resolve) => {
            // Transform the result further
            setTimeout(() => resolve(result * 3), 10);
        });
    }

    function failureCallback(err) {
        console.log('Chain failed:', err);
        // throw new Error('Error in failureCallback');
    }

    doSomething()
        .then((result) => doSomethingElse(result))
        .then((newResult) => doThirdThing(newResult))
        .then((finalResult) => {
            // throw new Error('Error after final result');
            console.log(`Got the final result: ${finalResult}`);
        })
        .catch(failureCallback);

    try {
        assert(false, 'Execution continues after promise chain');
    } catch (e) {
        console.log('Caught error:', e);
    }
}

{
    console.log('=== Sample promise chain with async/await ===');
    
    function doSomething() {
        console.log('[async/await] Starting doSomething');
        return new Promise((resolve) => {
            setTimeout(() => resolve(2), 10);
        });
    }

    function doSomethingElse(result) {
        console.log('[async/await] Starting doSomethingElse with', result);
        return new Promise((resolve) => {
            setTimeout(() => resolve(result + 2), 10);
        });
    }

    function doThirdThing(result) {
        console.log('[async/await] Starting doThirdThing with', result);
        return new Promise((resolve) => {
            setTimeout(() => resolve(result * 3), 10);
        });
    }

    function failureCallback(err) {
        console.log('[async/await] Chain failed (global):', err);
    }

    async function foo() {
        try {
            const result = await doSomething();
            const newResult = await doSomethingElse(result);
            const finalResult = await doThirdThing(newResult);
            console.log(`[async/await] Got the final result: ${finalResult}`);
        } catch (error) {
            failureCallback(error);
        }
    }
    // invoke the async function so the example runs
    foo().catch(failureCallback);
}

{
    console.log('=== Sample promise chain with optional steps ===');

    // Local helpers for this control flow example
    function doSomethingCritical() {
        console.log('[control flow] Starting critical work === 1 ===');
        // throw new Error('[control flow] something went wrong in doSomethingCritical === 1.5 ===');
        return new Promise((resolve, reject) => {
            setTimeout(() => resolve('crit-ok'), 10);
        });
    }

    function doSomethingOptional() {
        console.log('[control flow] Starting optional work === 2 ===');
        // throw new Error('[control flow] something went wrong in doSomethingOptional === 2.5 ===');
        return new Promise((resolve, reject) => {
            console.log('[debug] typeof Error =', typeof Error);
            // simulate occasional failure; don't let it abort the main chain
            if (Math.random() < 0.5) {
                setTimeout(() => resolve('opt-result'), 10);
            } else {
                setTimeout(() => reject(new Error('[control flow] optional failed')), 10);
            }
        }).catch(e => {
            // swallow optional error and return undefined so the chain continues
            console.log(`[control flow] optional failed (internal): ${e.message}`);
            return undefined;
        });
    }

    function doSomethingExtraNice(optionalResult) {
        console.log('[control flow] Starting extra nice work === 3 === with', optionalResult);
        // throw new Error('[control flow] something went wrong in doSomethingExtraNice === 3.5 ===');
        return new Promise((resolve) => {
            setTimeout(() => resolve(`extra-${optionalResult}`), 10);
        });
    }

    function moreCriticalStuff() {
        console.log('[control flow] Doing more critical work === 4 ===');
        // throw new Error('[control flow] something went wrong in moreCriticalStuff === 4.5 ===');
        return new Promise((resolve) => setTimeout(() => resolve('all-done'), 10));
    }

    doSomethingCritical()
        .then((result) =>
            doSomethingOptional()
                .then((optionalResult) => doSomethingExtraNice(optionalResult))
                .catch((e) => { console.log(`[control flow] optional failed === 3.5 ===: ${e.message}`); }),
        ) // 即便可选操作失败了，也会继续执行
        .then(() => moreCriticalStuff())
        .catch((e) => console.log(`[control flow] 严重失败 === 5 ===: ${e.message}`));

    async function main() {
        try {
            const result = await doSomethingCritical();
            try {
                const optionalResult = await doSomethingOptional(result);
                await doSomethingExtraNice(optionalResult);
            } catch (e) {
                // 忽略可选步骤的失败并继续执行。
                console.log(`[control flow] optional failed (async/await) === 3.5 ===: ${e.message}`);
            }
            await moreCriticalStuff();
        } catch (e) {
            console.error(`[control flow] 严重失败 (async/await) === 5 ===: ${e.message}`);
        }
    }
    main();
}

{
    console.log('=== Sample promise chain demonstrating .then() after .catch() ===');

    function doSomething() {
        console.log('Starting doSomething for .then() after .catch() example');
        return new Promise((resolve) => {
            setTimeout(() => resolve(), 10);
        });
    }

    doSomething()
        .then(() => {
            throw new Error("Something failed");
            console.log("Do this");
        })
        .catch(() => {
            console.error("Do that");
        })
        .then(() => {
            console.log("Do this, no matter what happened before");
        });

    async function main() {
        try {
            await doSomething();
            throw new Error("Something failed");
            console.log("Do this");
        } catch (e) {
            console.error("Do that");
        }
        console.log("Do this, no matter what happened before");
    }
    main();
}

{
    console.log('=== Sample promise chain demonstrating execution order ===');

    const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

    wait().then(() => console.log(4));
    Promise.resolve()
        .then(() => console.log(2))
        .then(() => console.log(3));
    console.log(1); // 1, 2, 3, 4
}

{
    const promise = new Promise((resolve, reject) => {
            console.log("Promise callback");
            resolve();
        }).then((result) => {
            console.log("Promise callback (.then)");
        });

    setTimeout(() => {
        console.log("event-loop cycle: Promise (fulfilled)", promise);
    }, 0);

    console.log("Promise (pending)", promise);
}

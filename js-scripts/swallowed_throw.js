(function () {
    let promise = Promise.all([Promise.resolve(1)])
        .then(() => {
            throw 'This exception should not be swallowed';
        })
        .catch((reason) => {
            if (reason !== 'This exception should not be swallowed') {
                throw new Error(`Unexpected rejection reason: ${String(reason)}`);
            }

            return 'PASS swallowed_throw';
        })
        .then((value) => {
            console.log(value);
            return value;
        });

    return promise;
})()

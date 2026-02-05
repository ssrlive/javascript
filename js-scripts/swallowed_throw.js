
Promise.all([Promise.resolve(1)]).then(() => {
    throw "This exception should not be swallowed";
});

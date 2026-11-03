// For engines on their own: appended to hash.js. Prints one line for each result.
var say = typeof print === "function" ? print : console.log;
Hash.runAll(function (name, ms, result) {
    say(name + ": " + Math.round(ms * 10) / 10 + " ms " + result);
}, Hash.SIZES);

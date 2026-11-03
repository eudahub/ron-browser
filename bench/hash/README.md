# How fast a login page can hash

A page may hash a password before sending it, which is about the hardest thing a page asks of a
JavaScript engine: Argon2 multiplies numbers of 64 bits, which JavaScript does not have, and writes
all over a large block of memory. This compares engines on it, to help choose how the browser will
run scripts. It does not run by itself and is not a test.

`hash.c` and `hash.js` both do Argon2id and repeated SHA-256 on one thread, check themselves with
the example from RFC 9106 and give the same results. They are for measuring only: real passwords
need a real library. `hash.js` uses no `BigInt`, and was not tuned for any one engine.

## Running it

```sh
gcc -O2 -DBENCH_NATIVE hash.c -o hash-native && ./hash-native
clang --target=wasm32 -O2 -nostdlib -fvisibility=hidden -c hash.c -o hash.o
wasm-ld --no-entry --export-dynamic -o hash.wasm hash.o
python3 serve.py 8771 &            # prints what the page sends back, then ends
google-chrome --headless=new --user-data-dir=/tmp/profile http://127.0.0.1:8771/bench.html
cat hash.js shell.js > all.js      # for an engine on its own, as qjs all.js
```

A Firefox installed as a snap cannot be stopped from every shell, and goes on after the page is done.
It helps to start it with a new profile.

## Results

On a Ryzen 9 9900X, in November 2026, from one or two runs each. Times marked ~ are worked out from
the smallest size, which takes too long otherwise.

| | SHA-256, 10000 times | Argon2id, 19 MB, 2 passes | Argon2id, 64 MB, 3 passes |
|---|---|---|---|
| C, native | 1.6 ms | 13 ms | 69 ms |
| WebAssembly in Chrome | 3 ms | 19 ms | 103 ms |
| WebAssembly in Firefox | 2 ms | 56 ms | 124 ms |
| WebAssembly in Wasmtime | ~2 ms | | 130 ms |
| WebAssembly in wasmi, an interpreter | ~26 ms | | 1.0 s |
| JavaScript in Chrome | 5 ms | 150 ms | 0.76 s |
| JavaScript in Firefox | 3 ms | 265 ms | 1.3 s |
| QuickJS | 124 ms | 6.3 s | ~32 s |
| QuickJS within Wasmtime | 171 ms | 8.4 s | ~42 s |
| Boa | 728 ms | ~42 s | ~211 s |
| QuickJS within wasmi | 1.9 s | ~117 s | ~589 s |

The times for Wasmtime and wasmi are of the whole program, with compiling the module.

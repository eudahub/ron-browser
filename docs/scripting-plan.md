# Plan: JavaScript and WebAssembly

The browser runs no scripts yet. This is how it is meant to run them, without being less safe than
Firefox, and in what order. Nothing here is built yet. What is measured is in
[`bench/hash`](../bench/hash/README.md), and what is measured here on the side was run once by hand.

## What must hold

- A script cannot change how the browser behaves. Back, the address line and the menus are out of its
  reach, and Back goes to before any redirect, also one made by a script.
- A script reaches the network, the disk and the user only by asking the browser, which can say no.
- A page without scripts costs nothing: no process and no memory for any of this.
- A script that runs too long or takes too much memory ends, and the page stays.

## Layers of protection

An attack has to get through all of them, one after the other. Any one of them failing is a bug, not
yet a break-in.

1. **No compiling while a page runs, as a rule.** A compiler that turns what a page wrote into machine
   code at run time is a frequent source of dangerous bugs in browsers. This costs speed, so it is the
   one layer that has exceptions: stage 4 compiles simple, often used procedures, only for pages
   that need it, and in a process of their own. Everything else goes without, which also lets the
   system forbid such a process from making any new code executable.
2. **The engine is shut inside WebAssembly.** A bug in the engine stays within its own memory.
3. **A separate process with the system's limits** (seccomp, namespaces), without network or disk. This
   also holds if something gets out of the WebAssembly sandbox itself, which has happened to Wasmtime:
   CVE-2026-34987 and CVE-2026-34971.
4. **A very narrow way to the DOM.** This is the riskiest place, as logic errors are easy to make there,
   so each call is checked on the Rust side.

## Why this order

Time for the same work, on one thread (details in `bench/hash`):

| | SHA-256, 10000 times | Argon2id, 64 MB, 3 passes |
|---|---|---|
| WebAssembly in Chrome | 3 ms | 103 ms |
| WebAssembly in Wasmtime | ~2 ms | 130 ms |
| WebAssembly in wasmi | ~26 ms | 1.0 s |
| JavaScript in Chrome | 5 ms | 0.76 s |
| QuickJS within Wasmtime | 171 ms | ~42 s |
| Boa | 728 ms | ~211 s |
| QuickJS within wasmi | 1.9 s | ~589 s |

- WebAssembly in a sandbox comes close to what browsers do, so what pages write in it for speed stays
  fast. It comes first, and is simpler than JavaScript.
- An engine within a sandbox is 40 to 55 times slower than Chrome on this work, the hardest there is
  for it. An `onclick` does not notice, while Argon2 written in JavaScript would.
- JavaScript is not turned into WebAssembly. The engine is, once, when the browser is built, and a
  script is only data for it. Real conversion gains little without knowing the types, which are only
  known while running, and LLVM is far too large and slow to use while a page runs.

## Stages

### 1. WebAssembly from pages, in wasmi

wasmi is an interpreter, so nothing gets compiled. In a separate process with layer 3, with limits on
memory and on steps, which wasmi can count. Without threads or shared memory, as these give the exact
timers that attacks on the processor need. At this stage it only gets what JavaScript passes to it, so
it is usable with stage 2; the sandboxing process and limits are what get built and tested first.

### 2. JavaScript in an engine compiled ahead of time

The engine is our own unchanging code, so Wasmtime can compile it when the browser is built. The
browser loads the finished file and holds no compiler at all, so nothing that a page does has any
effect on machine code. Tried by hand with QuickJS: compiling takes 0.1 s into a file of 3 MB, which
runs as fast as otherwise and starts in under 10 ms, in 21 MB.

Wasmtime has separate `runtime` and `cranelift` features, but a browser built with only `runtime` is
yet to be tried, as is what it takes to forbid new executable memory with it loading such a file.

To be chosen by measuring, on scripts saved with `--save-page`:

| | For | Against |
|---|---|---|
| QuickJS | About 6 times faster than Boa here. Small. | Its core is C, though that is in the sandbox. |
| Boa | Pure Rust, about 96% of Test262. | Slowest of the engines compared. |

The choice can stay open if the engine stays behind one interface.

Taken out at first: `SharedArrayBuffer` and `Atomics`, exact timers, and `eval` unless pages turn out to
need it. Most security is in leaving out what scripts can do, rather than what they can write.

The same measuring counts which constructions and interfaces pages really use, to decide what to
offer first. A saved page is not always complete: scripts named only within scripts are not found yet.

### 3. Interfaces done in Rust, outside the engine

`crypto.subtle` and the like run natively, so hashing a password does not depend on how fast the
engine is. The way to the DOM is built here as well, a call at a time, each with its checks, as in
layer 4. Scripts get handles to nodes rather than anything about memory.

### 4. Compiling often used simple procedures of WebAssembly

First wasmi for everything, then procedures that run many times and are found to be simple get compiled
to machine code, rather than the whole module:

- Simple means only arithmetic, loops and memory access with a check of its range, with no calls to the
  browser, no tables of functions and no references. The hash functions of Argon2 and SHA are like this.
- WebAssembly is confined by its validation in any case, so simple here is about how little the
  compiler has to do, which is what could go wrong.

What to solve then:

- The compiled code has to work on the same memory as wasmi. Wasmtime cannot do that, so it takes
  Cranelift directly or a small compiler of our own.
- A procedure called once with a long loop only switches at its next call. Switching within a loop is
  much harder, and is left for later.
- Such a process cannot forbid making code executable, so this runs only for pages that need it, in a
  process of its own, by the limit of steps or by the user's choice.

### 5. Often used JavaScript functions as WebAssembly

Functions that run many times and only work on numbers and typed arrays get translated from the bytecode
of the engine to WebAssembly, with checks of the types, going back to the interpreter when these fail.
This gains the most and is the hardest, so it comes last, and only if stage 3 turns out not to be enough.

## Open questions

- Which engine, after measuring (stage 2).
- How a page in one process talks to the DOM in another fast enough. Each call crosses processes, so
  calls may have to go in batches.
- How the system's limits are done outside Linux.
- Whether to run Wasmtime with its own interpreter for pages, which has not been measured.

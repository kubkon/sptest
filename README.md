# sptest
A self-contained example program that tests running WebAssembly in standalone SpiderMonkey
engine 60.

## Pre-requisites
In order to build the program, you will need `rustc nightly`. Assuming you're running `rustup`,
this can easily be accomplished like so:

```
$ rustup override set nightly
```

## Building
In order to build, simply run:

```
$ cargo build
```

If you would like to enable logging and debugging info, run instead:

```
$ cargo build --features "log-debug"
```

## Running
The program requires you to specify WebAssembly cross-compiled using Emscripten compiler with filenames hardcoded
to `main.js` for the glue code, and `main.wasm` for the actually Wasm binary. Make sure they are in the root of
this project, and run:

```
$ cargo run
```

Or,

```
$ ./target/debug/sptest
```

## License
[MIT](License)
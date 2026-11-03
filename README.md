# ron-browser
Web browser built on the [ron](https://github.com/eudahub/ron) engine, with a window in Qt.

The current stage lays out local pages: blocks with margins, padding, borders and backgrounds,
wrapped text with fonts, colors, alignment and decorations, and list markers. There are no
images, floats, table grids, links or network yet, and table parts stack as blocks.

## How it is split

Everything that reads untrusted page content is Rust, in `core/`, with no unsafe code and no
C++. Layout is there too, and asks Qt only for the widths of text. `bridge/` connects it to C++
and holds the only unsafe code, which is generated. The C++ part in `ui/` handles the window and
draws text runs and rectangles at the positions it gets; Qt never interprets content as HTML.

The exception for now is decoding: [cpg](https://github.com/eudahub/cpg), in C++, turns legacy
encodings into UTF-8. It will move to Rust before the browser reads from a network.

## Encodings

The encoding comes from, in this order: `--encoding`, a byte order mark, a `<meta>` declaration
in the first 1024 bytes, the content being valid UTF-8, then detection by cpg across all its
languages. The status bar shows which encoding was used and why, including when detection was
uncertain. Bytes invalid in that encoding are replaced.

## Building

Needs a C++20 compiler, CMake, Cargo, Qt 6 with Widgets, and an installed cpg with ICU and zlib.
The [ron](https://github.com/eudahub/ron) repository must be checked out next to this one.

```sh
cmake -S . -B build
cmake --build build -j4
build/ron-browser page.html
build/ron-browser --print page.html            # the same as text, without a window
build/ron-browser --screenshot out.png page.html
build/ron-browser --encoding mazovia page.htm  # any encoding name cpg knows
```

The tests need no Qt:

```sh
cargo test
```

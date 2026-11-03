# ron-browser
Web browser built on the [ron](https://github.com/eudahub/ron) engine, with a window in Qt.

The current stage lays out local pages: blocks with margins, padding, borders and backgrounds,
wrapped text with fonts, colors, alignment and decorations, list markers, images, background
images and floats. Text aims to land where it does in Firefox. There are no table grids, links or
network yet, and table parts stack as blocks.

## Selecting text

Text can be selected with the mouse and copied with Ctrl+C or from the Edit menu; Ctrl+A selects
everything, and on systems with a selection clipboard, selected text goes there at once. A double
click takes a word and a triple click a block, and dragging from them extends by words or blocks.

It is made to be exact, so that selecting up to the end of a block takes not one character more:
past the end of a line means the end of that line, the empty space between blocks counts for the
block the drag came from until the mouse reaches the next text, and a selection never ends on what
separates blocks. Only the text itself gets highlighted. Blocks are separated by a line end when
copied, and by an empty line when there is room between them. List markers are not copied.

## How it is split

Everything that reads untrusted page content is Rust, in `core/`, with no unsafe code and no
C++. Layout is there too, and asks Qt only for the widths of text. `bridge/` connects it to C++
and holds the only unsafe code, which is generated. The C++ part in `ui/` handles the window and
draws text runs and rectangles at the positions it gets; Qt never interprets content as HTML.

Images are decoded in Rust too: PNG, JPEG, GIF (its first frame), WebP and BMP, with limits on
their size that apply before anything is allocated. A local page can only load images from its own
directory and below it: no other scheme, and no way out by `..`, an absolute path or a symbolic link.

Legacy encodings are turned into UTF-8 by [cpg-rs](https://github.com/eudahub/cpg-rs). The exception
for now is detecting the encoding of a page that does not declare one: that is still
[cpg](https://github.com/eudahub/cpg), in C++. It will move to Rust before the browser reads from a
network.

## Encodings

The encoding comes from, in this order: `--encoding`, a byte order mark, a `<meta>` declaration
in the first 1024 bytes, the content being valid UTF-8, then detection by cpg across all its
languages. The status bar shows which encoding was used and why, including when detection was
uncertain. Bytes invalid in that encoding are replaced.

## Building

Needs a C++20 compiler, CMake, Cargo, Qt 6 with Widgets, and an installed cpg with ICU and zlib.
The repositories [ron](https://github.com/eudahub/ron) and [cpg-rs](https://github.com/eudahub/cpg-rs)
must be checked out next to this one.

```sh
cmake -S . -B build
cmake --build build -j4
build/ron-browser page.html
build/ron-browser --print page.html            # the same as text, without a window
build/ron-browser --screenshot out.png page.html
build/ron-browser --encoding mazovia page.htm  # any encoding name cpg knows
```

## Installing

```sh
sudo cmake --install build                  # to /usr/local
cmake --install build --prefix ~/.local     # for the current user only
```

This installs `ron-browser` to `bin`, `share/applications/ron-browser.desktop` and the icon
`share/icons/hicolor/scalable/apps/ron-browser.svg`, and refreshes the caches of the desktop, so the
browser appears in its menu and side bar at once. `--strip` makes the program smaller. Showing the
icon needs the SVG plugin of Qt (`qt6-svg-plugins`), and detecting encodings the data of the installed
cpg. Remove the files listed in `build/install_manifest.txt` to uninstall.

Install again after every build you want to use from the menu: the installed files are copies.

## Comparing with Firefox

`tools/compare_with_firefox.py` shows pages in Firefox and in ron-browser, lists the rows that
differ and leaves shots of both. It needs Firefox and Pillow, and uses its own empty Firefox profile.

The Rust tests need no Qt. The tests of the window run without a display:

```sh
cargo test
ctest --test-dir build --output-on-failure
```

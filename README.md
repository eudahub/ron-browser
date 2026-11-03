# ron-browser
Web browser built on the [ron](https://github.com/eudahub/ron) engine, with a window in Qt.

The current stage shows pages from files and from the web: blocks with margins, padding, borders and
backgrounds, wrapped text with fonts, colors, alignment and decorations, list markers, images,
background images, floats, inline blocks, image maps and style sheets. Text aims to land where it does
in Firefox. There are no table grids yet, and table parts stack as blocks; flexible and grid layouts
stack too, and positioned boxes stay in the flow, so many pages on the web still look quite different
from how they should. Form controls do not draw yet.

## Links and history

A click on a link or a part of an image map goes there, and a drag that starts on one selects text
as usual. The status bar shows where the link under the mouse leads. Back, Forward (also Alt+Left,
Alt+Right and the mouse's own buttons), Reload and Stop are in the toolbar, and Back returns to how
far down the page was. The address line takes an address or a path to a file (Ctrl+L); with no scheme
it means HTTPS.

Only what the user does adds to the history or moves in it, so that a page can never hold the user
back. A redirect takes one place in it, so Back goes to before it. Links open pages on the web, and
local pages with `.htm`, `.html`, `.shtml` or `.xhtml` in their names, or the index page of a
directory. Other files are only named in the status bar, and schemes like `javascript:` and `data:` are
not followed at all. `<base>` is ignored for now.

## The network

Pages, style sheets and images are read on threads of their own, so the window goes on. A new page
shows as it comes, once there is something to show and the style sheets named so far have come, for
at most 10 seconds; until then the old page stays. It can be scrolled as far as it has come, and
images show as they arrive. Stop keeps what has come so far. A page on the disk larger than 1 MB
comes in pieces the same way. HTTPS goes through [rustls](https://github.com/rustls/rustls), with the root certificates
built in, and a page with a bad certificate does not open: there is no way past it yet. Pages over
plain HTTP do open, and the status bar says they are not secure.

A local page takes its images and style sheets from its own directory and below it, and style sheets
also over HTTPS, as many saved pages need them, but from no address in the user's own network. So a
saved page can tell its server that it was opened. It loads no images from the network.

Requests carry no cookies, no `Referer` and no language. A page on the network cannot link to local
files, or make the browser reach addresses inside the user's own network, also through a name that
leads there. A redirect cannot go from HTTPS to HTTP, and a page over HTTPS loads no images or style
sheets over HTTP. What a server does not call CSS is not read as a style sheet. A page may be up to
64 MB, an image 32 MB and a style sheet 16 MB, counted after unpacking, with at most 128 style sheets,
and a request may take 60 seconds. Forms are not sent yet, and pages in encodings that cpg-rs lacks, like Shift-JIS, come out wrong.

## Saving a page

File > Save Page with Scripts, or `--save-page` without any window, saves a page from the network
for looking at what it runs: into a new directory named after its server, `page.html` exactly as the
server gave it, with its scripts in `scripts/` and style sheets in `styles/`, also those they import.
Links are not changed and images stay out, so this is not for viewing from the disk. `files.txt`
tells for each file where it came from, and what could not be read and why.

A hostile page decides nothing on the disk: nothing is overwritten or made executable, and a name
from a server keeps only harmless characters, with the ending `.js` or `.css`. The same rules as for
showing a page hold for what is read, and nothing is run. Opening the saved `page.html` in another
browser would run its scripts, so look at it as text. The page is read anew for this, and scripts
only named from within other scripts are not found.

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

Legacy encodings are found and turned into UTF-8 by [cpg-rs](https://github.com/eudahub/cpg-rs), also
in Rust. Its language models are built in, so the program needs no data files.

## Encodings

The encoding comes from, in this order: `--encoding`, a byte order mark, a `<meta>` declaration
in the first 1024 bytes, the content being valid UTF-8, then detection by cpg-rs across all its
languages. The status bar shows which encoding was used and why, including when detection was
uncertain. Bytes invalid in that encoding are replaced.

## Building

Needs a C++20 compiler, CMake, Cargo and Qt 6 with Widgets.
The repositories [ron](https://github.com/eudahub/ron) and [cpg-rs](https://github.com/eudahub/cpg-rs)
must be checked out next to this one.

```sh
cmake -S . -B build
cmake --build build -j4
build/ron-browser page.html
build/ron-browser --print page.html            # the same as text, without a window
build/ron-browser --screenshot out.png page.html
build/ron-browser --encoding mazovia page.htm  # any encoding name cpg-rs knows
build/ron-browser --slow-load 30 page.html     # a file arrives as if over a slow network, over 30 s
build/ron-browser --save-page DIR https://...  # the page with its scripts and style sheets, unchanged
```

## Installing

```sh
sudo cmake --install build                  # to /usr/local
cmake --install build --prefix ~/.local     # for the current user only
```

This installs `ron-browser` to `bin`, `share/applications/ron-browser.desktop` and the icon
`share/icons/hicolor/scalable/apps/ron-browser.svg`, and refreshes the caches of the desktop, so the
browser appears in its menu and side bar at once. `--strip` makes the program smaller. Showing the
icon needs the SVG plugin of Qt (`qt6-svg-plugins`). Remove the files listed in
`build/install_manifest.txt` to uninstall.

Install again after every build you want to use from the menu: the installed files are copies.

## Comparing with Firefox

`tools/compare_with_firefox.py` shows pages in Firefox and in ron-browser, lists the rows that
differ and leaves shots of both. It needs Firefox and Pillow, and uses its own empty Firefox profile.

The Rust tests need no Qt. The tests of the window run without a display:

```sh
cargo test
ctest --test-dir build --output-on-failure
```

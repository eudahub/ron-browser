#!/usr/bin/env python3
"""Shows a page in Firefox and in ron-browser and reports where they differ.

Needs Firefox and Pillow, and a built ron-browser. It runs a separate Firefox with an empty
profile, so a running one is left alone, and its results do not depend on your settings.

usage: compare_with_firefox.py [--width N] [--height N] [--encoding NAME] [--out DIR] FILE...

For each page it prints the rows of the page that hold something, in both browsers, marks
those that differ, and gives the share of pixels that differ. In DIR it leaves the two shots,
one with both side by side, and one that shows the differences.
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    from PIL import Image, ImageChops
except ImportError:
    sys.exit("This tool needs Pillow: pip install pillow")

ROOT = Path(__file__).resolve().parent.parent
# How much a color must differ from the row's background to count, as a sum over channels.
INK_THRESHOLD = 90
# Differences in position that are small enough to come from how letters are drawn.
ROW_TOLERANCE = 1
COLUMN_TOLERANCE = 3
MAX_COPIED_FILE_BYTES = 64 * 1024 * 1024


def work_directory():
    """A Firefox from a snap only sees part of the file system, so work where it can look."""
    firefox = shutil.which("firefox") or ""
    is_snap = "/snap/" in os.path.realpath(firefox)
    if not is_snap and firefox:
        try:
            is_snap = "snap" in Path(firefox).read_text(errors="ignore")
        except OSError:
            pass
    if is_snap:
        base = Path.home() / "snap/firefox/common"
        return Path(tempfile.mkdtemp(prefix="ron-compare-", dir=base))
    return Path(tempfile.mkdtemp(prefix="ron-compare-"))


def prepare_page(source, directory, browser, encoding):
    """Copies a page and what lies next to it, and makes the text UTF-8.

    Firefox does not know every encoding that ron-browser does, so both get the same bytes.
    """
    for entry in source.parent.iterdir():
        if entry.is_file() and entry.stat().st_size <= MAX_COPIED_FILE_BYTES:
            shutil.copy(entry, directory / entry.name)
    arguments = [browser, "--decode"]
    if encoding:
        arguments += ["--encoding", encoding]
    decoded = subprocess.run(
        arguments + [source], capture_output=True, timeout=60, check=True
    ).stdout
    # A byte order mark says UTF-8 to both, and unlike a tag it cannot change how a page parses.
    decoded = b"\xef\xbb\xbf" + decoded
    page = directory / source.name
    page.write_bytes(decoded)
    return page


def shoot_firefox(page, out, profile, width, height):
    subprocess.run(
        ["firefox", "--headless", "--no-remote", "--profile", str(profile),
         f"--window-size={width},{height}", "--screenshot", str(out), page.as_uri()],
        capture_output=True, timeout=120, check=False,
    )
    if not out.exists():
        sys.exit(f"Firefox made no shot of {page}")


def shoot_browser(browser, page, out, width, height):
    environment = dict(os.environ, QT_QPA_PLATFORM="offscreen")
    subprocess.run(
        [browser, "--page-shot", out, "--size", f"{width}x{height}", page],
        capture_output=True, timeout=120, check=True, env=environment,
    )


def bands(image):
    """Finds the stretches of rows that hold something, with how far they reach across."""
    width, height = image.size
    pixels = image.load()
    result = []
    current = None
    for y in range(height):
        row = [pixels[x, y] for x in range(width)]
        background = max(set(row), key=row.count)
        inked = [
            x for x, color in enumerate(row)
            if sum(abs(a - b) for a, b in zip(color, background)) > INK_THRESHOLD
        ]
        if inked:
            left, right = inked[0], inked[-1]
            if current is None:
                current = [y, y, left, right]
            else:
                current[1] = y
                current[2] = min(current[2], left)
                current[3] = max(current[3], right)
        elif current is not None:
            result.append(tuple(current))
            current = None
    if current is not None:
        result.append(tuple(current))
    return result


def same(first, second):
    return (
        abs(first[0] - second[0]) <= ROW_TOLERANCE
        and abs(first[1] - second[1]) <= ROW_TOLERANCE + 1
        and abs(first[2] - second[2]) <= COLUMN_TOLERANCE
        and abs(first[3] - second[3]) <= COLUMN_TOLERANCE
    )


def describe(band):
    if band is None:
        return "-"
    return f"y {band[0]}-{band[1]} x {band[2]}-{band[3]}"


def compare(firefox_shot, browser_shot, out_directory, name):
    firefox = Image.open(firefox_shot).convert("RGB")
    browser = Image.open(browser_shot).convert("RGB")
    size = (max(firefox.width, browser.width), max(firefox.height, browser.height))
    canvas_a = Image.new("RGB", size, "white")
    canvas_b = Image.new("RGB", size, "white")
    canvas_a.paste(firefox)
    canvas_b.paste(browser)

    first, second = bands(canvas_a), bands(canvas_b)
    print(f"{'firefox':>28}  {'ron-browser':>28}")
    differing = 0
    for index in range(max(len(first), len(second))):
        a = first[index] if index < len(first) else None
        b = second[index] if index < len(second) else None
        different = a is None or b is None or not same(a, b)
        differing += different
        print(f"{describe(a):>28}  {describe(b):>28}{'  <--' if different else ''}")

    difference = ImageChops.difference(canvas_a, canvas_b)
    changed = sum(value > 40 for value in difference.convert("L").tobytes())
    share = 100 * changed / max(1, size[0] * size[1])
    side_by_side = Image.new("RGB", (size[0] * 2 + 8, size[1]), "red")
    side_by_side.paste(canvas_a, (0, 0))
    side_by_side.paste(canvas_b, (size[0] + 8, 0))
    side_by_side.save(out_directory / f"{name}-side.png")
    difference.save(out_directory / f"{name}-diff.png")
    print(
        f"{name}: firefox {firefox.width}x{firefox.height}, ron-browser "
        f"{browser.width}x{browser.height}, {differing} of {max(len(first), len(second))} "
        f"rows differ, {share:.2f}% of pixels"
    )
    return differing


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("files", nargs="+", type=Path)
    parser.add_argument("--width", type=int, default=900)
    parser.add_argument("--height", type=int, default=700)
    parser.add_argument("--encoding")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--browser", type=Path, default=ROOT / "build/ron-browser")
    arguments = parser.parse_args()
    if not shutil.which("firefox"):
        sys.exit("Firefox was not found")
    if not arguments.browser.exists():
        sys.exit(f"Build ron-browser first: {arguments.browser}")

    out_directory = arguments.out or Path(tempfile.mkdtemp(prefix="ron-compare-shots-"))
    out_directory.mkdir(parents=True, exist_ok=True)
    work = work_directory()
    total = 0
    try:
        (work / "profile").mkdir()
        for index, source in enumerate(arguments.files):
            pages = work / f"page{index}"
            pages.mkdir()
            page = prepare_page(source.resolve(), pages, arguments.browser, arguments.encoding)
            name = source.stem
            firefox_shot = out_directory / f"{name}-firefox.png"
            browser_shot = out_directory / f"{name}-ron.png"
            # Firefox writes where it may, so the shot comes back through the work directory.
            shot = work / f"{name}-firefox.png"
            # Firefox only shoots its window, so make that as tall as the whole page.
            shoot_browser(arguments.browser, page, browser_shot, arguments.width, arguments.height)
            height = Image.open(browser_shot).height
            shoot_firefox(page, shot, work / "profile", arguments.width, height)
            shutil.copy(shot, firefox_shot)
            shoot_browser(arguments.browser, page, browser_shot, arguments.width, height)
            total += compare(firefox_shot, browser_shot, out_directory, name)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print(f"Shots are in {out_directory}")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())

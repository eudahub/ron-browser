#ifndef RON_BROWSER_PAGELOADER_H
#define RON_BROWSER_PAGELOADER_H

#include <QString>

#include <rust/cxx.h>

#include <cstdint>
#include <string>

namespace ron_browser {
struct ChosenEncoding;
}

// Reads a page's bytes from a file and turns them into UTF-8. The engine only takes UTF-8,
// so this is where legacy encodings end. Large pages take a lot of memory, so the text moves along
// rather than being copied.
struct DecodedPage {
    rust::String utf8;
    // The encoding used, as cpg-rs names it, and how it was chosen.
    std::string encoding;
    QString encodingSource;
    QString error;
};

// `forcedEncoding` is a cpg-rs codepage name chosen by the user, or empty.
DecodedPage loadPage(const QString &path, const std::string &forcedEncoding);
// Says how this encoding was chosen.
QString encodingSourceName(const ron_browser::ChosenEncoding &chosen);
// What is added to that when something invalid had to be replaced.
QString replacedBytesNote();

#endif

#ifndef RON_BROWSER_PAGELOADER_H
#define RON_BROWSER_PAGELOADER_H

#include <QString>

#include <string>
#include <string_view>

// Reads a page's bytes from a file and turns them into UTF-8. The engine only takes UTF-8,
// so this is where legacy encodings end.
struct DecodedPage {
    std::string utf8;
    // The encoding used, as cpg-rs names it, and how it was chosen.
    std::string encoding;
    QString encodingSource;
    QString error;
};

// `forcedEncoding` is a cpg-rs codepage name chosen by the user, or empty.
DecodedPage loadPage(const QString &path, const std::string &forcedEncoding);
// For bytes from the network. `charset` is what the server says, or empty.
DecodedPage decodeFetchedPage(std::string_view bytes, const std::string &charset);

#endif

#include "pageloader.h"

#include <QFile>
#include <QLocale>
#include <QObject>

#include <ron-browser-bridge/src/lib.rs.h>

#include <algorithm>
#include <cctype>
#include <string_view>
#include <utility>

namespace {

// Maps a web charset label to a cpg-rs name; empty when cpg-rs does not have it.
std::string codepageForLabel(std::string label)
{
    std::transform(label.begin(), label.end(), label.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    if (label == "utf-8" || label == "utf8") {
        return "utf8";
    }
    // Web pages that say Latin-1 or ASCII really mean Windows-1252.
    if (label == "iso-8859-1" || label == "latin1" || label == "us-ascii" || label == "ascii") {
        return "cp1252";
    }
    if (label.rfind("windows-", 0) == 0) {
        label = "cp" + label.substr(8);
    }
    return ron_browser::is_known_encoding(label) ? label : std::string();
}

rust::Slice<const uint8_t> asSlice(const rust::Vec<std::uint8_t> &bytes)
{
    return {bytes.data(), bytes.size()};
}

void decode(DecodedPage &page, rust::Vec<std::uint8_t> bytes, const std::string &encoding, bool skipBom = false)
{
    page.encoding = encoding;
    auto decoded = ron_browser::decode_bytes(std::move(bytes), encoding, skipBom);
    if (decoded.replaced) {
        page.encodingSource += QObject::tr(", with invalid bytes replaced");
    }
    page.utf8 = std::move(decoded.utf8);
}

void decodeContent(DecodedPage &page, rust::Vec<std::uint8_t> bytes, const std::string &forcedEncoding,
                   const std::string &serverCharset)
{
    const std::string_view view(reinterpret_cast<const char *>(bytes.data()), bytes.size());
    const std::string fromServer = codepageForLabel(serverCharset);
    if (!forcedEncoding.empty() && !ron_browser::is_known_encoding(forcedEncoding)) {
        page.error = QObject::tr("Unknown encoding: %1").arg(QString::fromStdString(forcedEncoding));
    } else if (!forcedEncoding.empty()) {
        page.encodingSource = QObject::tr("chosen");
        decode(page, std::move(bytes), forcedEncoding);
    } else if (view.starts_with("\xEF\xBB\xBF")) {
        page.encodingSource = QObject::tr("byte order mark");
        decode(page, std::move(bytes), "utf8", true);
    } else if (!fromServer.empty()) {
        page.encodingSource = QObject::tr("from the server");
        decode(page, std::move(bytes), fromServer);
    } else {
        const std::string declared = codepageForLabel(std::string(ron_browser::declared_charset(asSlice(bytes))));
        if (!declared.empty()) {
            page.encodingSource = QObject::tr("declared");
            decode(page, std::move(bytes), declared);
        } else if (ron_browser::is_utf8(asSlice(bytes))) {
            // Text in another encoding is very rarely valid UTF-8 by chance.
            page.encodingSource = QObject::tr("valid as such");
            decode(page, std::move(bytes), "utf8");
        } else {
            const auto detection = ron_browser::detect_encoding(asSlice(bytes));
            page.encodingSource = detection.certain ? QObject::tr("detected") : QObject::tr("detected, uncertain");
            decode(page, std::move(bytes), std::string(detection.encoding));
        }
    }
}

} // namespace

DecodedPage loadPage(const QString &path, const std::string &forcedEncoding)
{
    DecodedPage page;
    const QByteArray pathBytes = path.toUtf8();
    auto file = ron_browser::read_page_file(rust::Str(pathBytes.constData(), static_cast<size_t>(pathBytes.size())));
    if (!file.error.empty()) {
        page.error = QString::fromUtf8(file.error.data(), static_cast<qsizetype>(file.error.size()));
        return page;
    }
    decodeContent(page, std::move(file.bytes), forcedEncoding, std::string());
    return page;
}

DecodedPage decodeFetchedPage(rust::Vec<std::uint8_t> bytes, const std::string &charset)
{
    DecodedPage page;
    decodeContent(page, std::move(bytes), std::string(), charset);
    return page;
}

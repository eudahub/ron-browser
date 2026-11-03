#include "pageloader.h"

#include <QFile>
#include <QLocale>
#include <QObject>

#include <ron-browser-bridge/src/lib.rs.h>

#include <utility>

namespace {

void decode(DecodedPage &page, rust::Vec<std::uint8_t> bytes, const std::string &encoding, bool skipBom = false)
{
    page.encoding = encoding;
    auto decoded = ron_browser::decode_bytes(std::move(bytes), encoding, skipBom);
    if (decoded.replaced) {
        page.encodingSource += replacedBytesNote();
    }
    page.utf8 = std::move(decoded.utf8);
}

} // namespace

QString encodingSourceName(const ron_browser::ChosenEncoding &chosen)
{
    switch (chosen.source) {
    case ron_browser::EncodingSource::ByteOrderMark:
        return QObject::tr("byte order mark");
    case ron_browser::EncodingSource::Server:
        return QObject::tr("from the server");
    case ron_browser::EncodingSource::Declared:
        return QObject::tr("declared");
    case ron_browser::EncodingSource::Detected:
        return QObject::tr("detected");
    case ron_browser::EncodingSource::Uncertain:
        return QObject::tr("detected, uncertain");
    default:
        return QObject::tr("valid as such");
    }
}

QString replacedBytesNote()
{
    return QObject::tr(", with invalid bytes replaced");
}

DecodedPage loadPage(const QString &path, const std::string &forcedEncoding)
{
    DecodedPage page;
    const QByteArray pathBytes = path.toUtf8();
    auto file = ron_browser::read_page_file(rust::Str(pathBytes.constData(), static_cast<size_t>(pathBytes.size())));
    if (!file.error.empty()) {
        page.error = QString::fromUtf8(file.error.data(), static_cast<qsizetype>(file.error.size()));
        return page;
    }
    if (!forcedEncoding.empty() && !ron_browser::is_known_encoding(forcedEncoding)) {
        page.error = QObject::tr("Unknown encoding: %1").arg(QString::fromStdString(forcedEncoding));
    } else if (!forcedEncoding.empty()) {
        page.encodingSource = QObject::tr("chosen");
        decode(page, std::move(file.bytes), forcedEncoding);
    } else {
        const auto chosen = ron_browser::choose_encoding({file.bytes.data(), file.bytes.size()});
        page.encodingSource = encodingSourceName(chosen);
        decode(page, std::move(file.bytes), std::string(chosen.encoding),
               chosen.source == ron_browser::EncodingSource::ByteOrderMark);
    }
    return page;
}

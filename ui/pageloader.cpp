#include "pageloader.h"

#include <QFile>
#include <QLocale>
#include <QObject>

#include <cpg/CpManager.h>
#include <cpg/Detector.h>
#include <cpg/Language.h>
#include <cpg/NgramModel.h>

#include <ron-browser-bridge/src/lib.rs.h>

#include <algorithm>
#include <cctype>
#include <string_view>

namespace {

// Larger files are refused, so that opening one cannot exhaust memory.
const qint64 maxPageBytes = 256 * 1024 * 1024;
// Detection looks at this much of the page; the rest would only take time.
const size_t detectionBytes = 256 * 1024;

// Maps a web charset label to a cpg name; empty when cpg does not have it.
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

struct Detection {
    std::string encoding;
    bool certain;
};

// Looks across all languages: the language of the system says little about a page's. The language
// models are large, so they are loaded only for a page that needs detecting, and let go of afterwards.
Detection detectEncoding(std::string_view bytes)
{
    Languages languages;
    languages.readFromFile(CPG_LANGUAGES_FILE);
    NgramModel model;
    model.loadDirectory(CPG_MODELS_DIR);
    CpManager codepages;
    Detector detector(codepages, languages, &model);
    const auto results = detector.detectCodepage(bytes.substr(0, detectionBytes));
    const auto options = Detector::plausible(results);
    if (options.empty()) {
        return {"cp1252", false};
    }
    return {options.front().codepage, options.size() == 1 && options.front().confidence >= 0.95};
}

rust::Slice<const uint8_t> asSlice(std::string_view bytes)
{
    return {reinterpret_cast<const uint8_t *>(bytes.data()), bytes.size()};
}

void decode(DecodedPage &page, std::string_view bytes, const std::string &encoding)
{
    page.encoding = encoding;
    const auto decoded = ron_browser::decode_bytes(asSlice(bytes), encoding);
    if (decoded.replaced) {
        page.encodingSource += QObject::tr(", with invalid bytes replaced");
    }
    page.utf8 = std::string(decoded.utf8);
}

} // namespace

DecodedPage loadPage(const QString &path, const std::string &forcedEncoding)
{
    DecodedPage page;
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        page.error = file.errorString();
        return page;
    }
    if (file.size() > maxPageBytes) {
        page.error = QObject::tr("The file is too large to open.");
        return page;
    }
    const QByteArray content = file.readAll();
    std::string_view bytes(content.constData(), static_cast<size_t>(content.size()));

    if (!forcedEncoding.empty() && !ron_browser::is_known_encoding(forcedEncoding)) {
        page.error = QObject::tr("Unknown encoding: %1").arg(QString::fromStdString(forcedEncoding));
    } else if (!forcedEncoding.empty()) {
        page.encodingSource = QObject::tr("chosen");
        decode(page, bytes, forcedEncoding);
    } else if (bytes.starts_with("\xEF\xBB\xBF")) {
        page.encodingSource = QObject::tr("byte order mark");
        decode(page, bytes.substr(3), "utf8");
    } else {
        const std::string declared = codepageForLabel(std::string(ron_browser::declared_charset(asSlice(bytes))));
        if (!declared.empty()) {
            page.encodingSource = QObject::tr("declared");
            decode(page, bytes, declared);
        } else if (ron_browser::is_utf8(asSlice(bytes))) {
            // Text in another encoding is very rarely valid UTF-8 by chance.
            page.encodingSource = QObject::tr("valid as such");
            decode(page, bytes, "utf8");
        } else {
            const Detection detection = detectEncoding(bytes);
            page.encodingSource = detection.certain ? QObject::tr("detected") : QObject::tr("detected, uncertain");
            decode(page, bytes, detection.encoding);
        }
    }
    return page;
}

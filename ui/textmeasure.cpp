#include "textmeasure.h"

#include <QFontMetricsF>
#include <QGuiApplication>
#include <QScreen>
#include <QString>
#include <QStringList>

#include <ron-browser-bridge/src/lib.rs.h>

#include <algorithm>
#include <string>
#include <unordered_map>
#include <vector>

namespace {

// Pages cannot make this grow without end: further fonts reuse the last one.
const size_t maxFonts = 4096;

struct KnownFont {
    QFont font;
    QFontMetricsF metrics;
};

std::vector<KnownFont> &fonts()
{
    static std::vector<KnownFont> known;
    return known;
}

// Only fonts of the system are used. Generic names go to Qt's style hints.
QFont makeFont(const ron_browser::FontSpec &spec)
{
    QFont font;
    QStringList families;
    QFont::StyleHint hint = QFont::AnyStyle;
    const QString family = QString::fromUtf8(spec.family.data(), static_cast<qsizetype>(spec.family.size()));
    for (const QString &part : family.split(u',', Qt::SkipEmptyParts)) {
        QString name = part.trimmed();
        if (name.size() >= 2 && (name.startsWith(u'"') || name.startsWith(u'\''))) {
            name = name.mid(1, name.size() - 2);
        }
        const QString lower = name.toLower();
        if (lower == u"serif") {
            hint = QFont::Serif;
        } else if (lower == u"sans-serif" || lower == u"system-ui") {
            hint = QFont::SansSerif;
        } else if (lower == u"monospace") {
            hint = QFont::Monospace;
        } else if (lower == u"cursive") {
            hint = QFont::Cursive;
        } else if (lower == u"fantasy") {
            hint = QFont::Fantasy;
        } else {
            families.append(name);
        }
    }
    if (!families.isEmpty()) {
        font.setFamilies(families);
    } else if (hint == QFont::Monospace) {
        font.setFamily(QStringLiteral("monospace"));
    } else if (hint == QFont::Serif) {
        font.setFamily(QStringLiteral("serif"));
    } else if (hint == QFont::SansSerif) {
        font.setFamily(QStringLiteral("sans-serif"));
    }
    font.setStyleHint(hint);
    // CSS sizes are in pixels, and QFont only takes whole ones, so go through points.
    const QScreen *screen = QGuiApplication::primaryScreen();
    const qreal dpi = screen != nullptr ? screen->logicalDotsPerInchY() : 96.0;
    font.setPointSizeF(std::max<qreal>(spec.size, 1.0) * 72.0 / dpi);
    font.setWeight(static_cast<QFont::Weight>(std::clamp<int>(spec.weight, 1, 1000)));
    font.setItalic(spec.italic);
    return font;
}

} // namespace

namespace ron_browser {

std::uint32_t register_font(const FontSpec &spec)
{
    static std::unordered_map<std::string, std::uint32_t> ids;
    const std::string key = std::string(spec.family) + '\n' + std::to_string(spec.size) + '\n'
        + std::to_string(spec.weight) + (spec.italic ? "i" : "");
    const auto found = ids.find(key);
    if (found != ids.end()) {
        return found->second;
    }
    auto &known = fonts();
    if (known.size() >= maxFonts) {
        return static_cast<std::uint32_t>(known.size() - 1);
    }
    const QFont font = makeFont(spec);
    known.push_back({font, QFontMetricsF(font)});
    const auto id = static_cast<std::uint32_t>(known.size() - 1);
    ids.emplace(key, id);
    return id;
}

float text_width(std::uint32_t font, rust::Str text)
{
    const QString string = QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
    return static_cast<float>(fonts()[font].metrics.horizontalAdvance(string));
}

float font_ascent(std::uint32_t font)
{
    return static_cast<float>(fonts()[font].metrics.ascent());
}

float font_descent(std::uint32_t font)
{
    return static_cast<float>(fonts()[font].metrics.descent());
}

} // namespace ron_browser

const QFont &fontForId(std::uint32_t id)
{
    return fonts()[id].font;
}

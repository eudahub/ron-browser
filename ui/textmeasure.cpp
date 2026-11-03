#include "textmeasure.h"

#include <QFontMetricsF>
#include <QString>
#include <QStringList>

#include <ron-browser-bridge/src/lib.rs.h>

#include <algorithm>
#include <cmath>
#include <string>
#include <unordered_map>
#include <vector>

namespace {

// Pages cannot make this grow without end: further fonts reuse the last one.
const size_t maxFonts = 4096;
// See KnownFont::textWidth.
const size_t maxPairs = 20000;
const char16_t firstComplexLetter = 0x250;

struct KnownFont {
    QFont font;
    QFontMetricsF metrics;
    // Qt only has fonts of whole pixels, also when asked in points, while CSS sizes such as 12.8
    // are common. So text is measured and drawn at the nearest whole size and scaled by this.
    qreal scale;
    // How far the font reaches, at its real size. Small sizes in Qt are hinted, and so a little off.
    qreal ascent;
    qreal descent;
    // Shaping each word takes most of the time of measuring, while pages use few pairs of letters.
    // So a word counts as its letters, with what each pair adds to them. Ligatures of more than two
    // letters come out up to about a pixel off.
    std::unordered_map<char16_t, qreal> letters = {};
    std::unordered_map<std::uint32_t, qreal> pairs = {};

    qreal letterWidth(QChar letter)
    {
        const auto found = letters.find(letter.unicode());
        if (found != letters.end()) {
            return found->second;
        }
        const qreal width = metrics.horizontalAdvance(QString(letter));
        letters.emplace(letter.unicode(), width);
        return width;
    }

    qreal pairWidth(QChar first, QChar second)
    {
        const std::uint32_t key = (static_cast<std::uint32_t>(first.unicode()) << 16) | second.unicode();
        const auto found = pairs.find(key);
        if (found != pairs.end()) {
            return found->second;
        }
        QString pair;
        pair += first;
        pair += second;
        const qreal width = metrics.horizontalAdvance(pair);
        pairs.emplace(key, width);
        return width;
    }

    qreal textWidth(const QString &text)
    {
        // Scripts beyond the Latin ones can need all of a word to know its shapes. A page with very
        // many pairs is measured the slow way, as otherwise it could fill memory.
        const bool simple = pairs.size() < maxPairs && std::all_of(text.begin(), text.end(), [](QChar letter) {
            return letter.unicode() < firstComplexLetter;
        });
        if (!simple) {
            return metrics.horizontalAdvance(text);
        }
        qreal width = text.isEmpty() ? 0 : letterWidth(text.front());
        for (qsizetype index = 1; index < text.size(); ++index) {
            width += pairWidth(text[index - 1], text[index]) - letterWidth(text[index - 1]);
        }
        return width;
    }
};

// The size that tells the true proportions of a font, large enough to leave out hinting.
const int proportionsSize = 1024;

int wholeSize(float size)
{
    return std::max(qRound(size), 1);
}

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
    // Letters as wide as the font says, not rounded to whole pixels each, which adds up.
    font.setHintingPreference(QFont::PreferVerticalHinting);
    font.setPixelSize(wholeSize(spec.size));
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
    const qreal scale = std::max<qreal>(spec.size, 0.01) / wholeSize(spec.size);
    QFont large = font;
    large.setPixelSize(proportionsSize);
    const QFontMetricsF proportions(large);
    const qreal size = std::max<qreal>(spec.size, 0.01);
    known.push_back({font, QFontMetricsF(font), scale, proportions.ascent() * size / proportionsSize,
                     proportions.descent() * size / proportionsSize});
    const auto id = static_cast<std::uint32_t>(known.size() - 1);
    ids.emplace(key, id);
    return id;
}

float text_width(std::uint32_t font, rust::Str text)
{
    const QString string = QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
    KnownFont &known = fonts()[font];
    return static_cast<float>(known.textWidth(string) * known.scale);
}

// Rounded up to whole pixels, as Firefox does, which gives the same line heights.
float font_ascent(std::uint32_t font)
{
    return static_cast<float>(std::ceil(fonts()[font].ascent));
}

float font_descent(std::uint32_t font)
{
    return static_cast<float>(std::ceil(fonts()[font].descent));
}

} // namespace ron_browser

const QFont &fontForId(std::uint32_t id)
{
    return fonts()[id].font;
}

qreal fontScaleForId(std::uint32_t id)
{
    return fonts()[id].scale;
}

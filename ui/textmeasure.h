#ifndef RON_BROWSER_TEXTMEASURE_H
#define RON_BROWSER_TEXTMEASURE_H

#include <QFont>

#include <cstdint>

// The Qt font for a number that register_font gave out.
const QFont &fontForId(std::uint32_t id);
// How much to scale what is drawn in that font, which has a whole size in pixels.
qreal fontScaleForId(std::uint32_t id);

#endif

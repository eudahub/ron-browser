#ifndef RON_BROWSER_TEXTMEASURE_H
#define RON_BROWSER_TEXTMEASURE_H

#include <QFont>

#include <cstdint>

// The Qt font for a number that register_font gave out.
const QFont &fontForId(std::uint32_t id);

#endif

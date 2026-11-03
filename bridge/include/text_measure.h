#ifndef RON_BROWSER_TEXT_MEASURE_H
#define RON_BROWSER_TEXT_MEASURE_H

#include "rust/cxx.h"

#include <cstdint>

namespace ron_browser {

struct FontSpec;

// Implemented in ui/textmeasure.cpp, with Qt.
std::uint32_t register_font(const FontSpec &spec);
float text_width(std::uint32_t font, rust::Str text);
float font_ascent(std::uint32_t font);
float font_descent(std::uint32_t font);

} // namespace ron_browser

#endif

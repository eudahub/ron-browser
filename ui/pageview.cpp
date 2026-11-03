#include "pageview.h"

#include "textmeasure.h"

#include <QPaintEvent>
#include <QPainter>
#include <QResizeEvent>
#include <QScrollBar>
#include <QTimer>
#include <QtMath>

#include <ron-browser-bridge/src/lib.rs.h>

#include <algorithm>
#include <optional>

namespace {

const std::uint8_t underlineBit = 1;
const std::uint8_t overlineBit = 2;
const std::uint8_t lineThroughBit = 4;

} // namespace

struct PageView::PageHolder {
    std::optional<rust::Box<ron_browser::Page>> page;
};

PageView::PageView(QWidget *parent)
    : QAbstractScrollArea(parent), holder(std::make_unique<PageHolder>()), resizeTimer(new QTimer(this))
{
    // While the window is being resized, long pages are only laid out once it stops.
    resizeTimer->setSingleShot(true);
    resizeTimer->setInterval(50);
    connect(resizeTimer, &QTimer::timeout, this, &PageView::layOut);
    // Always there, so that it appearing cannot change the width and force a new layout.
    setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOn);
    verticalScrollBar()->setSingleStep(32);
    horizontalScrollBar()->setSingleStep(32);
    viewport()->setAutoFillBackground(false);
}

PageView::~PageView() = default;

void PageView::setPage(const std::string &utf8)
{
    holder->page = ron_browser::open_page(utf8);
    verticalScrollBar()->setValue(0);
    horizontalScrollBar()->setValue(0);
    layOut();
}

void PageView::clearPage()
{
    holder->page.reset();
    updateScrollBars();
    viewport()->update();
}

QString PageView::pageTitle() const
{
    if (!holder->page) {
        return {};
    }
    const rust::String title = (*holder->page)->title();
    return QString::fromUtf8(title.data(), static_cast<qsizetype>(title.size()));
}

void PageView::layOutNow()
{
    resizeTimer->stop();
    layOut();
}

void PageView::layOut()
{
    if (holder->page) {
        const QSize size = viewport()->size();
        (*holder->page)->lay_out(static_cast<float>(size.width()), static_cast<float>(size.height()));
        laidOutWidth = size.width();
    }
    updateScrollBars();
    viewport()->update();
}

void PageView::updateScrollBars()
{
    const QSize size = viewport()->size();
    int contentWidth = 0;
    int contentHeight = 0;
    if (holder->page) {
        contentWidth = qCeil((*holder->page)->layout_width());
        contentHeight = qCeil((*holder->page)->layout_height());
    }
    horizontalScrollBar()->setRange(0, std::max(0, contentWidth - size.width()));
    horizontalScrollBar()->setPageStep(size.width());
    verticalScrollBar()->setRange(0, std::max(0, contentHeight - size.height()));
    verticalScrollBar()->setPageStep(size.height());
}

void PageView::resizeEvent(QResizeEvent *event)
{
    QAbstractScrollArea::resizeEvent(event);
    updateScrollBars();
    // Lines only change with the width.
    if (holder->page && viewport()->width() != laidOutWidth) {
        resizeTimer->start();
    }
}

void PageView::paintEvent(QPaintEvent *event)
{
    QPainter painter(viewport());
    const QRect dirty = event->rect();
    QColor canvas(Qt::white);
    if (holder->page) {
        const std::uint32_t color = (*holder->page)->canvas_color();
        if (color != 0) {
            canvas = QColor::fromRgba(color);
        }
    }
    painter.fillRect(dirty, canvas);
    if (!holder->page) {
        return;
    }
    const auto &page = *holder->page;
    const qreal scrollX = horizontalScrollBar()->value();
    const qreal scrollY = verticalScrollBar()->value();
    painter.translate(-scrollX, -scrollY);
    const QRectF visible = QRectF(dirty).translated(scrollX, scrollY);
    const auto items = page->items();
    for (size_t index = 0; index < items.size(); ++index) {
        const auto &item = items[index];
        const QRectF box(item.x, item.y, item.width, item.height);
        if (box.top() > visible.bottom() || box.bottom() < visible.top()) {
            continue;
        }
        const QColor color = QColor::fromRgba(item.color);
        if (!item.is_text) {
            painter.fillRect(box, color);
            continue;
        }
        const rust::Str text = page->item_text(index);
        painter.setPen(color);
        const QFont &font = fontForId(item.font);
        painter.setFont(font);
        const QPointF start(item.x, item.baseline);
        painter.drawText(start, QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size())));
        if (item.decoration != 0) {
            const QFontMetricsF metrics(font);
            const qreal thickness = std::max<qreal>(metrics.lineWidth(), 1.0);
            const auto drawLine = [&](qreal offset) {
                painter.fillRect(QRectF(item.x, item.baseline + offset, item.width, thickness), color);
            };
            if ((item.decoration & underlineBit) != 0) {
                drawLine(metrics.underlinePos());
            }
            if ((item.decoration & overlineBit) != 0) {
                drawLine(-metrics.overlinePos());
            }
            if ((item.decoration & lineThroughBit) != 0) {
                drawLine(-metrics.strikeOutPos());
            }
        }
    }
}

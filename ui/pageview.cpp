#include "pageview.h"

#include "textmeasure.h"

#include <QApplication>
#include <QClipboard>
#include <QMouseEvent>
#include <QPaintEvent>
#include <QPixmap>
#include <QStyleHints>
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
    : QAbstractScrollArea(parent), holder(std::make_unique<PageHolder>()), resizeTimer(new QTimer(this)),
      dragScrollTimer(new QTimer(this))
{
    dragScrollTimer->setInterval(50);
    connect(dragScrollTimer, &QTimer::timeout, this, &PageView::scrollWhileDragging);
    viewport()->setCursor(Qt::IBeamCursor);
    setFocusPolicy(Qt::StrongFocus);
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

QPointF PageView::pagePosition(QPointF viewPosition) const
{
    return viewPosition + QPointF(horizontalScrollBar()->value(), verticalScrollBar()->value());
}

void PageView::pressAt(QMouseEvent *event, int clicks)
{
    if (!holder->page || event->button() != Qt::LeftButton) {
        return;
    }
    const QPointF at = pagePosition(event->position());
    if (clicks == 1 && (event->modifiers() & Qt::ShiftModifier) != 0) {
        (*holder->page)->extend(static_cast<float>(at.x()), static_cast<float>(at.y()));
    } else {
        (*holder->page)->press(static_cast<float>(at.x()), static_cast<float>(at.y()), clicks);
    }
    dragging = true;
    lastDragPosition = event->position();
    dragScrollTimer->start();
    viewport()->update();
}

void PageView::mousePressEvent(QMouseEvent *event)
{
    const int distance = QApplication::startDragDistance();
    const bool isThird = lastDoubleClick.isValid()
        && lastDoubleClick.elapsed() < QApplication::styleHints()->mouseDoubleClickInterval()
        && (event->position() - lastDoubleClickPosition).manhattanLength() < distance;
    pressAt(event, isThird ? 3 : 1);
}

void PageView::mouseDoubleClickEvent(QMouseEvent *event)
{
    lastDoubleClick.start();
    lastDoubleClickPosition = event->position();
    pressAt(event, 2);
}

void PageView::dragTo(QPointF viewPosition)
{
    if (holder->page) {
        const QPointF at = pagePosition(viewPosition);
        (*holder->page)->drag(static_cast<float>(at.x()), static_cast<float>(at.y()));
        viewport()->update();
    }
}

void PageView::mouseMoveEvent(QMouseEvent *event)
{
    if (dragging) {
        lastDragPosition = event->position();
        dragTo(lastDragPosition);
    }
}

// Moves the page while the mouse is held beyond its edges, so that selections can span it.
void PageView::scrollWhileDragging()
{
    const QSize size = viewport()->size();
    const qreal y = lastDragPosition.y();
    const qreal x = lastDragPosition.x();
    const int verticalStep = y < 0 ? static_cast<int>(y) : (y > size.height() ? static_cast<int>(y - size.height()) : 0);
    const int horizontalStep = x < 0 ? static_cast<int>(x) : (x > size.width() ? static_cast<int>(x - size.width()) : 0);
    if (verticalStep != 0 || horizontalStep != 0) {
        verticalScrollBar()->setValue(verticalScrollBar()->value() + verticalStep);
        horizontalScrollBar()->setValue(horizontalScrollBar()->value() + horizontalStep);
        dragTo(lastDragPosition);
    }
}

void PageView::mouseReleaseEvent(QMouseEvent *event)
{
    if (event->button() == Qt::LeftButton && dragging) {
        dragging = false;
        dragScrollTimer->stop();
        offerSelection();
    }
}

// On systems that have one, the selection clipboard gets what is selected at once.
void PageView::offerSelection()
{
    QClipboard *clipboard = QGuiApplication::clipboard();
    if (holder->page && clipboard->supportsSelection()) {
        const rust::String text = (*holder->page)->selected_text();
        if (!text.empty()) {
            clipboard->setText(QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size())),
                               QClipboard::Selection);
        }
    }
}

void PageView::copySelection()
{
    if (holder->page) {
        const rust::String text = (*holder->page)->selected_text();
        if (!text.empty()) {
            QGuiApplication::clipboard()->setText(
                QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size())));
        }
    }
}

void PageView::selectAll()
{
    if (holder->page) {
        (*holder->page)->select_all();
        offerSelection();
        viewport()->update();
    }
}

QString PageView::selectForTest(QPointF from, QPointF to, int clicks)
{
    if (!holder->page) {
        return {};
    }
    auto &page = *holder->page;
    page->press(static_cast<float>(from.x()), static_cast<float>(from.y()), clicks);
    page->drag(static_cast<float>(to.x()), static_cast<float>(to.y()));
    viewport()->update();
    const rust::String text = page->selected_text();
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

QPixmap PageView::grabWholePage(QSize atLeast)
{
    setFrameShape(QFrame::NoFrame);
    setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    resize(atLeast);
    layOutNow();
    if (holder->page) {
        resize(std::max(atLeast.width(), qCeil((*holder->page)->layout_width())),
               std::max(atLeast.height(), qCeil((*holder->page)->layout_height())));
    }
    return viewport()->grab();
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
    // Over the text and translucent, so that it works on any background and text color.
    const auto highlights = page->highlights(static_cast<float>(visible.top()), static_cast<float>(visible.bottom()));
    QColor highlight = palette().color(QPalette::Highlight);
    highlight.setAlpha(110);
    for (const auto &rect : highlights) {
        painter.fillRect(QRectF(rect.x, rect.y, rect.width, rect.height), highlight);
    }
}

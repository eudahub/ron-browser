#include "pageview.h"

#include "textmeasure.h"

#include <QApplication>
#include <QImage>
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
#include <cmath>
#include <optional>
#include <utility>
#include <vector>

#if defined(__GLIBC__)
#include <malloc.h>
#endif

namespace {

const std::uint8_t underlineBit = 1;
const std::uint8_t overlineBit = 2;
const std::uint8_t lineThroughBit = 4;
// The longest side of a shot of a whole page; Qt cannot paint much beyond this anyway.
const int maxShotSide = 32000;
// Opening a page takes much more memory at once than it keeps. What is freed stays with the C
// library for later, unless it is asked to give it back to the system.
void giveBackFreeMemory()
{
#if defined(__GLIBC__)
    malloc_trim(0);
#endif
}

// How often more of an arriving page comes in, when it is made to take its time.
const int arrivalPieceMs = 50;
// How much of a large local page comes at a time, between which the window goes on.
const size_t fastPieceBytes = 512 * 1024;
// How often images may cause a new layout of a page that is still arriving, all of it each time.
const qint64 minImageLayoutMs = 1000;
// A page shows after this long without the style sheets that have not come by then.
const qint64 maxSheetWaitMs = 10000;

} // namespace

struct PageView::PageHolder {
    std::optional<rust::Box<ron_browser::Page>> page;
    std::optional<rust::Box<ron_browser::ImageFetch>> imageFetch;
    std::optional<rust::Box<ron_browser::Page>> incoming;
    std::optional<rust::Box<ron_browser::SheetFetch>> sheetFetch;
    // All of a page that is arriving from a file, of which arrivedBytes have been given to it.
    rust::String arrivingText;
    // Whether the pieces come from outside, rather than from arrivingText.
    bool arrivalIsFed = false;
    // A page is arriving, or what it needs is, from when it starts to when it is complete.
    bool arrivalActive = false;
    // All of the page has come, though style sheets might not have.
    bool arrivalEnded = false;
    // Whether the incoming page is one that comes in pieces, and so can show before it is complete.
    bool incomingInPieces = false;
    // Each wraps the decoded pixels on the Rust side, without copying them.
    std::vector<std::optional<QImage>> images;
    std::vector<std::optional<QPixmap>> tiles;

    const QImage &image(std::uint32_t number)
    {
        if (number >= images.size()) {
            images.resize(number + 1);
        }
        if (!images[number]) {
            const auto &loaded = *page;
            const auto pixels = loaded->image_pixels(number);
            images[number] = QImage(pixels.data(), static_cast<int>(loaded->image_width(number)),
                                    static_cast<int>(loaded->image_height(number)),
                                    QImage::Format_RGBA8888);
        }
        return *images[number];
    }

    const QPixmap &tile(std::uint32_t number)
    {
        if (number >= tiles.size()) {
            tiles.resize(number + 1);
        }
        if (!tiles[number]) {
            tiles[number] = QPixmap::fromImage(image(number));
        }
        return *tiles[number];
    }

    void forgetImages()
    {
        images.clear();
        tiles.clear();
    }

    // What is arriving: the page that shows, unless it is still coming unseen.
    rust::Box<ron_browser::Page> &arrivingPage()
    {
        return incoming ? *incoming : *page;
    }

    bool hasArrivingPage() const
    {
        return arrivalActive && (incoming || page);
    }
};

PageView::PageView(QWidget *parent)
    : QAbstractScrollArea(parent), holder(std::make_unique<PageHolder>()), resizeTimer(new QTimer(this)),
      dragScrollTimer(new QTimer(this)), imageTimer(new QTimer(this)), arrivalTimer(new QTimer(this))
{
    arrivalTimer->setInterval(arrivalPieceMs);
    connect(arrivalTimer, &QTimer::timeout, this, &PageView::feedNextPiece);
    // Each new image takes a new layout, so several arriving together should share one.
    imageTimer->setInterval(150);
    connect(imageTimer, &QTimer::timeout, this, &PageView::takeImages);
    dragScrollTimer->setInterval(50);
    connect(dragScrollTimer, &QTimer::timeout, this, &PageView::scrollWhileDragging);
    viewport()->setMouseTracking(true);
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

void PageView::setPage(rust::String utf8, const QString &directory)
{
    forgetPage();
    const QByteArray path = directory.toUtf8();
    const QSize size = viewport()->size();
    holder->page = ron_browser::open_page(std::move(utf8), rust::Str(path.constData(), static_cast<size_t>(path.size())),
                                          static_cast<float>(size.width()), static_cast<float>(size.height()));
    verticalScrollBar()->setValue(0);
    horizontalScrollBar()->setValue(0);
    setHoveredLink(QString());
    pressedLink.clear();
    layOut();
    giveBackFreeMemory();
}

void PageView::clearPage()
{
    forgetPage();
    holder->page.reset();
    setHoveredLink(QString());
    pressedLink.clear();
    updateScrollBars();
    viewport()->update();
}

void PageView::beginArrival(const QString &address)
{
    holder->arrivalActive = true;
    holder->arrivalEnded = false;
    holder->incomingInPieces = true;
    incomingAddress = address;
    incomingWait.start();
    arrivalSize = viewport()->size();
    arrivalTimer->start();
}

void PageView::startArriving(rust::String utf8, const QString &directory, const QString &address, int seconds)
{
    const QByteArray path = directory.toUtf8();
    const QSize size = viewport()->size();
    holder->incoming = ron_browser::open_arriving_page(rust::Str(path.constData(), static_cast<size_t>(path.size())),
                                                       static_cast<float>(size.width()), static_cast<float>(size.height()));
    holder->arrivingText = std::move(utf8);
    holder->arrivalIsFed = false;
    arrivedBytes = 0;
    if (seconds > 0) {
        const size_t pieces = std::max<size_t>(static_cast<size_t>(seconds) * 1000 / arrivalPieceMs, 1);
        bytesPerPiece = std::max<size_t>(holder->arrivingText.size() / pieces, 1);
        arrivalTimer->setInterval(arrivalPieceMs);
    } else {
        bytesPerPiece = fastPieceBytes;
        arrivalTimer->setInterval(0);
    }
    beginArrival(address);
}

void PageView::beginFetchedArrival(const QString &address)
{
    const QByteArray bytes = address.toUtf8();
    const QSize size = viewport()->size();
    holder->incoming = ron_browser::open_arriving_fetched_page(rust::Str(bytes.constData(), static_cast<size_t>(bytes.size())),
                                                               static_cast<float>(size.width()),
                                                               static_cast<float>(size.height()));
    holder->arrivalIsFed = true;
    arrivalTimer->setInterval(arrivalPieceMs);
    beginArrival(address);
}

void PageView::feedFetched(rust::Str piece)
{
    if (holder->hasArrivingPage() && !holder->arrivalEnded) {
        holder->arrivingPage()->feed(piece);
        startWaitingSheets();
        if (!holder->incoming) {
            updateScrollBars();
            viewport()->update();
        }
    }
}

void PageView::endFetchedArrival()
{
    if (holder->hasArrivingPage() && !holder->arrivalEnded) {
        finishArrival();
    }
}

bool PageView::isArriving() const
{
    return holder->arrivalActive;
}

void PageView::stopArriving()
{
    if (holder->arrivalActive) {
        // The sheets and images it still waits for go too: this is the end of all that comes for it.
        imageTimer->stop();
        holder->imageFetch.reset();
        holder->sheetFetch.reset();
        if (holder->arrivalEnded) {
            endArrival();
        } else {
            finishArrival();
            holder->sheetFetch.reset();
            if (holder->arrivalActive) {
                endArrival();
            }
        }
    }
}

void PageView::feedNextPiece()
{
    if (!holder->hasArrivingPage()) {
        arrivalTimer->stop();
        return;
    }
    takeArrivingSheets();
    startWaitingSheets();
    if (holder->arrivalEnded) {
        if (!holder->sheetFetch) {
            endArrival();
        }
        return;
    }
    if (holder->arrivalIsFed) {
        return;
    }
    const rust::String &text = holder->arrivingText;
    size_t end = std::min(arrivedBytes + bytesPerPiece, text.size());
    // A piece ends between characters, as all text in UTF-8 has to.
    while (end < text.size() && (static_cast<unsigned char>(text.data()[end]) & 0xC0) == 0x80) {
        ++end;
    }
    holder->arrivingPage()->feed(rust::Str(text.data() + arrivedBytes, end - arrivedBytes));
    arrivedBytes = end;
    startWaitingSheets();
    if (!holder->incoming) {
        updateScrollBars();
        viewport()->update();
    }
    if (arrivedBytes >= text.size()) {
        finishArrival();
    } else {
        emit arrivalProgress(static_cast<int>(arrivedBytes * 100 / text.size()));
    }
}

void PageView::startWaitingSheets()
{
    if (!holder->sheetFetch && holder->arrivingPage()->has_waiting_sheets()) {
        const QByteArray address = incomingAddress.toUtf8();
        holder->sheetFetch = holder->arrivingPage()->start_sheet_fetch(rust::Str(address.constData(), static_cast<size_t>(address.size())));
    }
}

void PageView::takeArrivingSheets()
{
    if (!holder->sheetFetch) {
        return;
    }
    const QByteArray address = incomingAddress.toUtf8();
    auto &page = holder->arrivingPage();
    if (page->take_sheets(**holder->sheetFetch, rust::Str(address.constData(), static_cast<size_t>(address.size())))) {
        holder->sheetFetch.reset();
        page->apply_sheets();
        if (holder->incoming) {
            arrivalSize = viewport()->size();
            page->lay_out(static_cast<float>(arrivalSize.width()), static_cast<float>(arrivalSize.height()));
        } else {
            layOut();
        }
    }
}

void PageView::finishArrival()
{
    holder->arrivingText = rust::String();
    holder->arrivingPage()->finish_arriving();
    holder->arrivalEnded = true;
    startWaitingSheets();
    if (!holder->incoming) {
        updateScrollBars();
        viewport()->update();
    }
    giveBackFreeMemory();
    if (!holder->sheetFetch) {
        endArrival();
    }
}

void PageView::endArrival()
{
    arrivalTimer->stop();
    holder->arrivalActive = false;
    emit arrivalFinished();
}

void PageView::dropArrival()
{
    arrivalTimer->stop();
    holder->arrivalActive = false;
    holder->arrivingText = rust::String();
    holder->sheetFetch.reset();
}

void PageView::forgetShownImages()
{
    imageTimer->stop();
    holder->imageFetch.reset();
    imagesWaiting = false;
    holder->forgetImages();
}

void PageView::forgetPage()
{
    dropArrival();
    holder->incoming.reset();
    holder->incomingInPieces = false;
    forgetShownImages();
}

bool PageView::setLocalPage(rust::String utf8, const QString &directory, const QString &address)
{
    const QByteArray path = directory.toUtf8();
    const QSize size = viewport()->size();
    holder->incoming = ron_browser::open_page(std::move(utf8), rust::Str(path.constData(), static_cast<size_t>(path.size())),
                                              static_cast<float>(size.width()), static_cast<float>(size.height()));
    if (!(*holder->incoming)->has_waiting_sheets()) {
        showIncoming();
        return true;
    }
    const QByteArray bytes = address.toUtf8();
    holder->sheetFetch = (*holder->incoming)->start_sheet_fetch(rust::Str(bytes.constData(), static_cast<size_t>(bytes.size())));
    incomingAddress = address;
    incomingWait.start();
    return false;
}

bool PageView::hasIncomingPage() const
{
    return holder->incoming.has_value();
}

bool PageView::incomingPageIsReady()
{
    if (!holder->incoming) {
        return true;
    }
    const bool waitedLongEnough = incomingWait.elapsed() > maxSheetWaitMs;
    if (holder->incomingInPieces) {
        const bool hasSomething = (*holder->incoming)->has_content() || holder->arrivalEnded;
        return hasSomething && (!holder->sheetFetch || waitedLongEnough);
    }
    if (!holder->sheetFetch) {
        return true;
    }
    const QByteArray bytes = incomingAddress.toUtf8();
    const rust::Str str(bytes.constData(), static_cast<size_t>(bytes.size()));
    return (*holder->incoming)->take_sheets(**holder->sheetFetch, str) || waitedLongEnough;
}

void PageView::showIncomingPage()
{
    if (holder->incoming) {
        if (!holder->incomingInPieces) {
            (*holder->incoming)->apply_sheets();
        }
        showIncoming();
    }
}

void PageView::showIncoming()
{
    forgetShownImages();
    const bool inPieces = holder->incomingInPieces;
    if (!inPieces) {
        holder->sheetFetch.reset();
    }
    holder->incomingInPieces = false;
    holder->page = std::move(holder->incoming);
    holder->incoming.reset();
    verticalScrollBar()->setValue(0);
    horizontalScrollBar()->setValue(0);
    setHoveredLink(QString());
    pressedLink.clear();
    // What arrived is laid out already, unless the window has changed since.
    if (inPieces && arrivalSize == viewport()->size()) {
        laidOutWidth = arrivalSize.width();
        updateScrollBars();
        viewport()->update();
    } else {
        layOut();
    }
    giveBackFreeMemory();
}

void PageView::dropIncomingPage()
{
    if (holder->incomingInPieces) {
        dropArrival();
    }
    holder->incomingInPieces = false;
    holder->sheetFetch.reset();
    holder->incoming.reset();
}

void PageView::fetchImages(const QString &pageAddress)
{
    if (holder->page) {
        imageAddress = pageAddress;
        sinceImageLayout.start();
        takeImages();
        imageTimer->start();
    }
}

bool PageView::isLoadingImages() const
{
    return imageTimer->isActive();
}

void PageView::takeImages()
{
    if (!holder->page) {
        imageTimer->stop();
        return;
    }
    const bool pageArrives = holder->arrivalActive && !holder->incoming;
    if (holder->imageFetch) {
        imagesWaiting |= (*holder->page)->take_images(**holder->imageFetch);
        if ((*holder->imageFetch)->is_done()) {
            holder->imageFetch.reset();
        }
    }
    // The numbers of images stay, so what is cached stays right.
    if (imagesWaiting && (!pageArrives || sinceImageLayout.elapsed() > minImageLayoutMs)) {
        imagesWaiting = false;
        sinceImageLayout.start();
        layOut();
    }
    if (holder->imageFetch) {
        return;
    }
    // Only one set goes at a time, so that a page cannot have too many on the way at once.
    const QByteArray address = imageAddress.toUtf8();
    auto fetch = (*holder->page)->start_image_fetch(rust::Str(address.constData(), static_cast<size_t>(address.size())));
    if (fetch->requested() > 0) {
        holder->imageFetch = std::move(fetch);
    } else if (!pageArrives && !imagesWaiting) {
        imageTimer->stop();
    }
}

bool PageView::hasPage() const
{
    return holder->page.has_value();
}

int PageView::scrollY() const
{
    return verticalScrollBar()->value();
}

void PageView::setScrollY(int y)
{
    verticalScrollBar()->setValue(y);
}

bool PageView::scrollToFragment(const QString &fragment)
{
    if (!holder->page) {
        return false;
    }
    const QByteArray name = fragment.toUtf8();
    const float y = (*holder->page)->fragment_y(rust::Str(name.constData(), static_cast<size_t>(name.size())));
    if (y < 0) {
        return false;
    }
    setScrollY(qFloor(y));
    return true;
}

QString PageView::linkAt(QPointF viewPosition) const
{
    if (!holder->page) {
        return {};
    }
    const QPointF at = pagePosition(viewPosition);
    const rust::Str link = (*holder->page)->link_at(static_cast<float>(at.x()), static_cast<float>(at.y()));
    return QString::fromUtf8(link.data(), static_cast<qsizetype>(link.size()));
}

void PageView::setHoveredLink(const QString &address)
{
    if (address != hoveredLink) {
        hoveredLink = address;
        viewport()->setCursor(address.isEmpty() ? Qt::ArrowCursor : Qt::PointingHandCursor);
        emit linkHovered(address);
    }
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
    if (event->button() == Qt::BackButton) {
        emit backButtonPressed();
        return;
    }
    if (event->button() == Qt::ForwardButton) {
        emit forwardButtonPressed();
        return;
    }
    pressedLink.clear();
    if (event->button() == Qt::LeftButton && (event->modifiers() & Qt::ShiftModifier) == 0) {
        pressedLink = linkAt(event->position());
        pressPosition = event->position();
    }
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
    pressedLink.clear();
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
    } else {
        setHoveredLink(linkAt(event->position()));
    }
}

void PageView::leaveEvent(QEvent *event)
{
    QAbstractScrollArea::leaveEvent(event);
    setHoveredLink(QString());
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
        const QString clicked = std::exchange(pressedLink, QString());
        const qreal moved = (event->position() - pressPosition).manhattanLength();
        if (!clicked.isEmpty() && moved < QApplication::startDragDistance() && linkAt(event->position()) == clicked) {
            // Last, as this can replace the page.
            emit linkActivated(clicked);
        }
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
        const int width = std::max(atLeast.width(), qCeil((*holder->page)->layout_width()));
        const int height = std::max(atLeast.height(), qCeil((*holder->page)->layout_height()));
        resize(std::min(width, maxShotSide), std::min(height, maxShotSide));
    }
    return viewport()->grab();
}

void PageView::layOutNow()
{
    resizeTimer->stop();
    // Lines only change with the width, and a long page takes a while to lay out.
    if (viewport()->width() != laidOutWidth) {
        layOut();
    }
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
    painter.setRenderHint(QPainter::SmoothPixmapTransform);
    const QRectF visible = QRectF(dirty).translated(scrollX, scrollY);
    const auto drawTiles = [&](const ron_browser::DrawItem &item, const QRectF &area) {
        const QPixmap &pixmap = holder->tile(item.image);
        if (pixmap.isNull()) {
            return;
        }
        // How far into the image the area starts, counting from where a copy begins.
        const QPointF into(std::fmod(area.left() - item.origin_x, pixmap.width()),
                           std::fmod(area.top() - item.origin_y, pixmap.height()));
        painter.drawTiledPixmap(area, pixmap,
                                QPointF(into.x() < 0 ? into.x() + pixmap.width() : into.x(),
                                        into.y() < 0 ? into.y() + pixmap.height() : into.y()));
    };
    const auto canvasTile = page->canvas_tile();
    if (canvasTile.kind == 3) {
        drawTiles(canvasTile, visible);
    }
    const auto items = page->visible_items(static_cast<float>(visible.top()), static_cast<float>(visible.bottom()));
    for (const auto &item : items) {
        const QRectF box(item.x, item.y, item.width, item.height);
        const QColor color = QColor::fromRgba(item.color);
        if (item.kind == 0) {
            painter.fillRect(box, color);
            continue;
        }
        if (item.kind == 2) {
            painter.drawImage(box, holder->image(item.image));
            continue;
        }
        if (item.kind == 3) {
            drawTiles(item, box);
            continue;
        }
        const rust::Str text = page->item_text(item.index);
        painter.setPen(color);
        const QFont &font = fontForId(item.font);
        const qreal scale = fontScaleForId(item.font);
        painter.setFont(font);
        const QPointF start(item.x, item.baseline);
        const QString string = QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
        if (scale == 1.0) {
            painter.drawText(start, string);
        } else {
            painter.save();
            painter.translate(start);
            painter.scale(scale, scale);
            painter.drawText(QPointF(0, 0), string);
            painter.restore();
        }
        if (item.decoration != 0) {
            const QFontMetricsF metrics(font);
            const qreal thickness = std::max<qreal>(metrics.lineWidth(), 1.0);
            const auto drawLine = [&](qreal offset) {
                painter.fillRect(QRectF(item.x, item.baseline + offset, item.width, thickness), color);
            };
            if ((item.decoration & underlineBit) != 0) {
                drawLine(metrics.underlinePos() * scale);
            }
            if ((item.decoration & overlineBit) != 0) {
                drawLine(-metrics.overlinePos() * scale);
            }
            if ((item.decoration & lineThroughBit) != 0) {
                drawLine(-metrics.strikeOutPos() * scale);
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

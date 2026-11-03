#ifndef RON_BROWSER_PAGEVIEW_H
#define RON_BROWSER_PAGEVIEW_H

#include <QAbstractScrollArea>
#include <QElapsedTimer>
#include <QPointF>

#include <rust/cxx.h>

#include <memory>
#include <string>

class QPixmap;
class QTimer;
class QMouseEvent;

namespace ron_browser {
struct Page;
}

// Draws a laid out page. Page content reaches Qt only as text runs and rectangles at
// positions that the Rust side has worked out.
class PageView : public QAbstractScrollArea {
    Q_OBJECT

public:
    explicit PageView(QWidget *parent = nullptr);
    ~PageView() override;

    // Images load from `directory`, the one the page is in, and from nowhere else.
    void setPage(const std::string &utf8, const QString &directory = QString());
    void clearPage();
    // A page from the network waits unseen until its style sheets have come, or for long enough.
    void prepareFetchedPage(const std::string &utf8, const QString &address);
    bool hasIncomingPage() const;
    bool incomingPageIsReady();
    void showIncomingPage();
    void dropIncomingPage();
    // Starts reading the images of a page that came from this address. They show as they arrive.
    void fetchImages(const QString &pageAddress);
    bool isLoadingImages() const;
    QString pageTitle() const;
    bool hasPage() const;
    int scrollY() const;
    void setScrollY(int y);
    // Shows the place of this name, and tells whether the page has one.
    bool scrollToFragment(const QString &fragment);
    void layOutNow();
    // Puts the selected text on the clipboard.
    void copySelection();
    void selectAll();
    // The whole page without scroll bars, at least this large.
    QPixmap grabWholePage(QSize atLeast);
    // For a test of what a drag gives, without a mouse: selects as a drag between two points
    // of the page would, and returns the text.
    QString selectForTest(QPointF from, QPointF to, int clicks);

signals:
    // The mouse came to a link with this address, or left all of them with an empty one.
    void linkHovered(const QString &address);
    // The user clicked a link. A drag that starts on one selects instead.
    void linkActivated(const QString &address);
    void backButtonPressed();
    void forwardButtonPressed();

protected:
    void paintEvent(QPaintEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseDoubleClickEvent(QMouseEvent *event) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;
    void leaveEvent(QEvent *event) override;

private:
    void takeImages();
    void forgetPage();
    QString linkAt(QPointF viewPosition) const;
    void setHoveredLink(const QString &address);
    void layOut();
    void updateScrollBars();
    QPointF pagePosition(QPointF viewPosition) const;
    void pressAt(QMouseEvent *event, int clicks);
    void dragTo(QPointF viewPosition);
    void scrollWhileDragging();
    void offerSelection();

    struct PageHolder;
    std::unique_ptr<PageHolder> holder;
    QTimer *resizeTimer;
    int laidOutWidth = -1;
    bool dragging = false;
    QPointF lastDragPosition;
    // For telling a third click from a new first one.
    QElapsedTimer lastDoubleClick;
    QPointF lastDoubleClickPosition;
    QTimer *dragScrollTimer;
    QTimer *imageTimer;
    QString incomingAddress;
    QElapsedTimer incomingWait;
    QString hoveredLink;
    // Where the left button went down on a link, for telling a click from a drag.
    QString pressedLink;
    QPointF pressPosition;
};

#endif

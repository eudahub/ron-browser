#ifndef RON_BROWSER_PAGEVIEW_H
#define RON_BROWSER_PAGEVIEW_H

#include <QAbstractScrollArea>
#include <QElapsedTimer>
#include <QPointF>

#include <rust/cxx.h>

#include <memory>
#include <string>

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

    void setPage(const std::string &utf8);
    void clearPage();
    QString pageTitle() const;
    void layOutNow();
    // Puts the selected text on the clipboard.
    void copySelection();
    void selectAll();
    // For a test of what a drag gives, without a mouse: selects as a drag between two points
    // of the page would, and returns the text.
    QString selectForTest(QPointF from, QPointF to, int clicks);

protected:
    void paintEvent(QPaintEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseDoubleClickEvent(QMouseEvent *event) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;

private:
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
};

#endif

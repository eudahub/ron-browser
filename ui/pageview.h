#ifndef RON_BROWSER_PAGEVIEW_H
#define RON_BROWSER_PAGEVIEW_H

#include <QAbstractScrollArea>

#include <rust/cxx.h>

#include <memory>
#include <string>

class QTimer;

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

protected:
    void paintEvent(QPaintEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;

private:
    void layOut();
    void updateScrollBars();

    struct PageHolder;
    std::unique_ptr<PageHolder> holder;
    QTimer *resizeTimer;
    int laidOutWidth = -1;
};

#endif

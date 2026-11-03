// The way from mouse events to the clipboard, which the Rust tests cannot reach. Positions are
// found from the page itself, as text widths depend on the fonts of the system.
#include "pageview.h"

#include <QApplication>
#include <QClipboard>
#include <QScrollBar>
#include <QSignalSpy>
#include <QStyleHints>
#include <QTest>

#include <ron-browser-bridge/src/lib.rs.h>

#include <cmath>

namespace {

const char page[] = "<!DOCTYPE html><style>body{margin:0}p{margin:20px 0;font-size:16px}</style>"
                    "<p>first block</p><p>second block</p><ul><li>item</ul>";
const char linkPage[] = "<!DOCTYPE html><style>body{margin:0}</style><a href=next.html>a link to the next page</a>";

} // namespace

class PageViewTest : public QObject {
    Q_OBJECT

private slots:
    void init();
    void cleanup();
    void draggingPastALineStopsAtTheEndOfTheBlock();
    void draggingIntoTheGapStopsAtTheEndOfTheBlock();
    void draggingIntoTheNextBlockCrossesIt();
    void doubleClickTakesAWord();
    void tripleClickTakesTheBlock();
    void slowThirdClickStartsAgain();
    void shiftClickExtends();
    void plainClickClears();
    void selectAllTakesEverything();
    void copyWithNothingSelectedLeavesTheClipboard();
    void listMarkersAreNotCopied();
    void selectionSurvivesScrollAndResize();
    void fontsHaveTheirExactSizes();
    void clickOnALinkActivatesIt();
    void dragFromALinkSelectsInstead();
    void hoverTellsAboutLinks();

private:
    QString copied();
    void drag(QPoint from, QPoint to);

    PageView *view = nullptr;
    // Lines start 20 down, are about 20 high, and the next is about 40 further.
    const QPoint inFirst{20, 28};
    const QPoint farRightOfFirst{600, 28};
    const QPoint inGap{600, 50};
};

void PageViewTest::init()
{
    view = new PageView;
    view->resize(700, 400);
    view->show();
    QVERIFY(QTest::qWaitForWindowExposed(view));
    view->setPage(page);
    view->layOutNow();
    QGuiApplication::clipboard()->clear();
    QTest::qWait(600); // Longer than a double click, so that tests do not join up.
}

void PageViewTest::cleanup()
{
    delete view;
}

QString PageViewTest::copied()
{
    view->copySelection();
    return QGuiApplication::clipboard()->text();
}

void PageViewTest::drag(QPoint from, QPoint to)
{
    QWidget *target = view->viewport();
    QTest::mousePress(target, Qt::LeftButton, Qt::NoModifier, from);
    QTest::mouseMove(target, to);
    QTest::mouseRelease(target, Qt::LeftButton, Qt::NoModifier, to);
}

void PageViewTest::draggingPastALineStopsAtTheEndOfTheBlock()
{
    drag(QPoint(0, 28), farRightOfFirst);
    QCOMPARE(copied(), QStringLiteral("first block"));
}

void PageViewTest::draggingIntoTheGapStopsAtTheEndOfTheBlock()
{
    drag(QPoint(0, 28), inGap);
    QCOMPARE(copied(), QStringLiteral("first block"));
    drag(QPoint(0, 28), QPoint(0, 50));
    QCOMPARE(copied(), QStringLiteral("first block"));
}

void PageViewTest::draggingIntoTheNextBlockCrossesIt()
{
    drag(QPoint(0, 28), QPoint(600, 65));
    QCOMPARE(copied(), QStringLiteral("first block\n\nsecond block"));
}

void PageViewTest::doubleClickTakesAWord()
{
    QWidget *target = view->viewport();
    QTest::mouseDClick(target, Qt::LeftButton, Qt::NoModifier, inFirst);
    QCOMPARE(copied(), QStringLiteral("first"));
}

void PageViewTest::tripleClickTakesTheBlock()
{
    QWidget *target = view->viewport();
    QTest::mouseDClick(target, Qt::LeftButton, Qt::NoModifier, inFirst);
    QTest::mouseClick(target, Qt::LeftButton, Qt::NoModifier, inFirst);
    QCOMPARE(copied(), QStringLiteral("first block"));
}

void PageViewTest::slowThirdClickStartsAgain()
{
    QWidget *target = view->viewport();
    QTest::mouseDClick(target, Qt::LeftButton, Qt::NoModifier, inFirst);
    QTest::qWait(QApplication::styleHints()->mouseDoubleClickInterval() + 100);
    QTest::mouseClick(target, Qt::LeftButton, Qt::NoModifier, inFirst);
    QGuiApplication::clipboard()->clear();
    view->copySelection();
    QCOMPARE(QGuiApplication::clipboard()->text(), QString());
}

void PageViewTest::shiftClickExtends()
{
    QWidget *target = view->viewport();
    QTest::mouseClick(target, Qt::LeftButton, Qt::NoModifier, QPoint(0, 28));
    QTest::mouseClick(target, Qt::LeftButton, Qt::ShiftModifier, farRightOfFirst);
    QCOMPARE(copied(), QStringLiteral("first block"));
}

void PageViewTest::plainClickClears()
{
    drag(QPoint(0, 28), farRightOfFirst);
    QTest::qWait(QApplication::styleHints()->mouseDoubleClickInterval() + 100);
    QTest::mouseClick(view->viewport(), Qt::LeftButton, Qt::NoModifier, inFirst);
    QGuiApplication::clipboard()->clear();
    view->copySelection();
    QCOMPARE(QGuiApplication::clipboard()->text(), QString());
}

void PageViewTest::selectAllTakesEverything()
{
    view->selectAll();
    QCOMPARE(copied(), QStringLiteral("first block\n\nsecond block\n\nitem"));
}

void PageViewTest::copyWithNothingSelectedLeavesTheClipboard()
{
    QGuiApplication::clipboard()->setText(QStringLiteral("kept"));
    view->copySelection();
    QCOMPARE(QGuiApplication::clipboard()->text(), QStringLiteral("kept"));
}

void PageViewTest::listMarkersAreNotCopied()
{
    // The list starts about 100 down; left of its text is where the marker is.
    drag(QPoint(0, 108), QPoint(600, 300));
    QCOMPARE(copied(), QStringLiteral("item"));
}

void PageViewTest::selectionSurvivesScrollAndResize()
{
    view->setPage(QByteArray("<!DOCTYPE html><style>body{margin:0}</style>")
                      .append(QByteArray("<p>line</p>").repeated(200))
                      .toStdString());
    view->layOutNow();
    drag(QPoint(0, 20), QPoint(600, 20));
    QCOMPARE(copied(), QStringLiteral("line"));
    view->verticalScrollBar()->setValue(500);
    QCOMPARE(copied(), QStringLiteral("line"));
    view->resize(400, 300);
    view->layOutNow();
    QCOMPARE(copied(), QStringLiteral("line"));
}

void PageViewTest::fontsHaveTheirExactSizes()
{
    // Qt only has fonts of whole pixels, and left to itself gives 13 for 12.8.
    const auto font = [](float size) {
        return ron_browser::register_font(ron_browser::FontSpec{"sans-serif", size, 400, false});
    };
    const rust::Str text("as of this commit");
    const float whole = ron_browser::text_width(font(16.0F), text);
    QVERIFY(std::abs(ron_browser::text_width(font(12.8F), text) - whole * 0.8F) < 0.5F);
    QVERIFY(std::abs(ron_browser::text_width(font(13.0F), text) - whole * 13.0F / 16.0F) < 0.5F);
    QVERIFY(ron_browser::text_width(font(12.8F), text) < ron_browser::text_width(font(13.0F), text));
}

void PageViewTest::clickOnALinkActivatesIt()
{
    view->setPage(linkPage);
    view->layOutNow();
    QSignalSpy activated(view, &PageView::linkActivated);
    QTest::mouseClick(view->viewport(), Qt::LeftButton, Qt::NoModifier, QPoint(10, 10));
    QCOMPARE(activated.size(), 1);
    QCOMPARE(activated.front().front().toString(), QStringLiteral("next.html"));
    QTest::mouseClick(view->viewport(), Qt::LeftButton, Qt::NoModifier, QPoint(10, 300));
    QCOMPARE(activated.size(), 1);
}

void PageViewTest::dragFromALinkSelectsInstead()
{
    view->setPage(linkPage);
    view->layOutNow();
    QSignalSpy activated(view, &PageView::linkActivated);
    drag(QPoint(0, 10), QPoint(30, 10));
    QCOMPARE(activated.size(), 0);
    QVERIFY(!copied().isEmpty());
}

void PageViewTest::hoverTellsAboutLinks()
{
    view->setPage(linkPage);
    view->layOutNow();
    QSignalSpy hovered(view, &PageView::linkHovered);
    QTest::mouseMove(view->viewport(), QPoint(10, 10));
    QTRY_COMPARE(hovered.size(), 1);
    QCOMPARE(hovered.front().front().toString(), QStringLiteral("next.html"));
    QCOMPARE(view->viewport()->cursor().shape(), Qt::PointingHandCursor);
    QTest::mouseMove(view->viewport(), QPoint(10, 300));
    QTRY_COMPARE(hovered.size(), 2);
    QVERIFY(hovered.back().front().toString().isEmpty());
    QCOMPARE(view->viewport()->cursor().shape(), Qt::ArrowCursor);
}

QTEST_MAIN(PageViewTest)
#include "pageview_test.moc"

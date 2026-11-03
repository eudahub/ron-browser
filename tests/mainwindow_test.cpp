// Going between pages in a whole window, with real files.
#include "mainwindow.h"
#include "pageview.h"

#include <QAction>
#include <QFile>
#include <QScrollBar>
#include <QStatusBar>
#include <QTemporaryDir>
#include <QTest>

#include <algorithm>

namespace {

const QByteArray head = "<!DOCTYPE html><style>body{margin:0}p{margin:0;height:100px}</style>";

QByteArray firstPage()
{
    QByteArray tall;
    for (int line = 0; line < 50; ++line) {
        tall += "<p>line</p>";
    }
    return "<title>First</title>" + head + "<a href=second.html>to the second page</a>" + tall;
}

} // namespace

class MainWindowTest : public QObject {
    Q_OBJECT

private slots:
    void init();
    void cleanup();
    void aClickGoesToTheOtherPage();
    void backReturnsToWhereThePageWas();
    void forwardGoesAgainAndANewPlaceForgetsIt();
    void aFragmentShowsItsPlaceAndBackReturns();
    void aLinkToNothingLeavesThePage();
    void linksToElsewhereStayNamedOnly();
    void aButtonOfTheMouseGoesBack();

private:
    void write(const QString &name, const QByteArray &content);
    void click(QPoint at = {10, 8});
    QAction *action(const QString &text) const;

    QTemporaryDir directory;
    MainWindow *window = nullptr;
};

void MainWindowTest::write(const QString &name, const QByteArray &content)
{
    QFile file(directory.filePath(name));
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(content);
}

void MainWindowTest::init()
{
    write("first.html", firstPage());
    write("second.html",
          "<title>Second</title>" + head + "<a href=third.html#end>on</a><p>x</p>");
    write("third.html", "<title>Third</title>" + head + "<p>a</p><p>b</p><p>c</p><p>d</p><p>e</p><p>f</p><p>g</p><p>h</p>"
                            "<p>i</p><p>j</p><p>k</p><p>l</p><p id=end>the end</p><p>m</p><p>n</p><p>o</p>");
    write("broken.html", "<title>Broken</title>" + head + "<a href=missing.html>nowhere</a>");
    write("outside.html", "<title>Outside</title>" + head
                              + "<p><a href=https://example.com/>net</a></p>"
                                "<p><a href=javascript:alert(1)>script</a></p>");
    window = new MainWindow({});
    window->show();
    QVERIFY(QTest::qWaitForWindowExposed(window));
    window->openPage(directory.filePath("first.html"));
    window->layOutNow();
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
}

void MainWindowTest::cleanup()
{
    delete window;
    window = nullptr;
}

void MainWindowTest::click(QPoint at)
{
    QTest::mouseClick(window->pageView()->viewport(), Qt::LeftButton, Qt::NoModifier, at);
}

QAction *MainWindowTest::action(const QString &text) const
{
    for (QAction *found : window->findChildren<QAction *>()) {
        if (found->text().remove(u'&') == text) {
            return found;
        }
    }
    return nullptr;
}

void MainWindowTest::aClickGoesToTheOtherPage()
{
    QVERIFY(!action("Back")->isEnabled());
    click();
    QCOMPARE(window->windowTitle(), QStringLiteral("Second"));
    QVERIFY(action("Back")->isEnabled());
    QVERIFY(!action("Forward")->isEnabled());
}

void MainWindowTest::backReturnsToWhereThePageWas()
{
    PageView *view = window->pageView();
    view->setScrollY(700);
    window->openPage(directory.filePath("second.html"));
    QCOMPARE(view->scrollY(), 0);
    action("Back")->trigger();
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
    QCOMPARE(view->scrollY(), 700);
}

void MainWindowTest::forwardGoesAgainAndANewPlaceForgetsIt()
{
    click();
    action("Back")->trigger();
    QVERIFY(action("Forward")->isEnabled());
    action("Forward")->trigger();
    QCOMPARE(window->windowTitle(), QStringLiteral("Second"));
    action("Back")->trigger();
    window->openPage(directory.filePath("broken.html"));
    QVERIFY(!action("Forward")->isEnabled());
}

void MainWindowTest::aFragmentShowsItsPlaceAndBackReturns()
{
    click();
    click();
    QCOMPARE(window->windowTitle(), QStringLiteral("Third"));
    PageView *view = window->pageView();
    // Twelve paragraphs of 100 stand before it, as far as the view can scroll.
    QCOMPARE(view->scrollY(), std::min(1200, view->verticalScrollBar()->maximum()));
    QVERIFY(view->scrollY() > 0);
    action("Back")->trigger();
    QCOMPARE(window->windowTitle(), QStringLiteral("Second"));
    QCOMPARE(view->scrollY(), 0);
}

void MainWindowTest::aLinkToNothingLeavesThePage()
{
    window->openPage(directory.filePath("broken.html"));
    click();
    QCOMPARE(window->windowTitle(), QStringLiteral("Broken"));
    QVERIFY(window->statusBar()->currentMessage().contains(QStringLiteral("missing.html")));
    action("Back")->trigger();
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
}

void MainWindowTest::linksToElsewhereStayNamedOnly()
{
    window->openPage(directory.filePath("outside.html"));
    click({10, 8});
    QCOMPARE(window->windowTitle(), QStringLiteral("Outside"));
    QVERIFY(window->statusBar()->currentMessage().contains(QStringLiteral("example.com")));
    click({10, 108});
    QCOMPARE(window->windowTitle(), QStringLiteral("Outside"));
    QVERIFY(window->statusBar()->currentMessage().contains(QStringLiteral("javascript")));
}

void MainWindowTest::aButtonOfTheMouseGoesBack()
{
    click();
    QTest::mouseClick(window->pageView()->viewport(), Qt::BackButton, Qt::NoModifier, QPoint(300, 300));
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
    QTest::mouseClick(window->pageView()->viewport(), Qt::ForwardButton, Qt::NoModifier, QPoint(300, 300));
    QCOMPARE(window->windowTitle(), QStringLiteral("Second"));
}

QTEST_MAIN(MainWindowTest)
#include "mainwindow_test.moc"

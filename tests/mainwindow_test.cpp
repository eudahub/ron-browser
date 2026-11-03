// Going between pages in a whole window, with real files.
#include "mainwindow.h"
#include "pageview.h"

#include <QAction>
#include <QBuffer>
#include <QFile>
#include <QImage>
#include <QLineEdit>
#include <QPixmap>
#include <QScrollBar>
#include <QStatusBar>
#include <QTcpServer>
#include <QTcpSocket>
#include <QTemporaryDir>
#include <QTest>
#include <QTimer>

#include <algorithm>

namespace {

const QByteArray head = "<!DOCTYPE html><style>body{margin:0}p{margin:0;height:100px}</style>";
// How long the server holds back the rest of a page that it sends slowly.
const int slowAnswerMs = 1500;

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
    void theAddressLineFollowsAndLeads();
    void pagesComeFromTheNetworkWithTheirImages();
    void aPageShowsWithItsStyleSheet();
    void stoppingWhileASheetComesLeavesThePage();
    void backGoesToBeforeARedirect();
    void whatFailsLeavesThePageAndTheTypedAddress();
    void aPageOnTheNetworkCannotOpenLocalFiles();
    void stoppingLeavesThePageAsItWas();
    void aPageShowsWhileItArrives();
    void aPageFromTheNetworkShowsBeforeAllOfItHasCome();
    void stoppingAPageFromTheNetworkKeepsWhatCame();
    void scrollingStaysWhileAPageArrives();
    void stoppingAnArrivalKeepsWhatCame();
    void backDropsAPageThatDoesNotShowYet();
    void aButtonOfTheMouseGoesBack();

private:
    void answerRequest(QTcpSocket *socket);
    QString serverAddress(const QString &path) const;
    void write(const QString &name, const QByteArray &content);
    void click(QPoint at = {10, 8});
    QAction *action(const QString &text) const;

    QTemporaryDir directory;
    MainWindow *window = nullptr;
    QTcpServer server;
    QStringList served;
};

QString MainWindowTest::serverAddress(const QString &path) const
{
    return QStringLiteral("http://127.0.0.1:%1%2").arg(server.serverPort()).arg(path);
}

void MainWindowTest::answerRequest(QTcpSocket *socket)
{
    const QByteArray request = socket->readAll();
    const QString path = QString::fromUtf8(request.split(' ').value(1));
    served << path;
    const auto respond = [&](const QByteArray &status, const QByteArray &headers, const QByteArray &body) {
        socket->write("HTTP/1.1 " + status + "\r\n" + headers + "Content-Length: " + QByteArray::number(body.size())
                      + "\r\nConnection: close\r\n\r\n" + body);
        socket->disconnectFromHost();
    };
    const QByteArray html = "Content-Type: text/html\r\n";
    if (path == QStringLiteral("/slowly.html")) {
        QByteArray start = "<title>Slowly</title>" + head;
        for (int line = 0; line < 20; ++line) {
            start += "<p>line</p>";
        }
        socket->write("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n" + start);
        QTimer::singleShot(slowAnswerMs, socket, [socket] {
            QByteArray rest;
            for (int line = 0; line < 30; ++line) {
                rest += "<p>line</p>";
            }
            socket->write(rest);
            socket->disconnectFromHost();
        });
    } else if (path == QStringLiteral("/net.html")) {
        respond("200 OK", html,
                "<title>On the net</title>" + head + "<a href=next.html>on</a><br><img src=picture.png>");
    } else if (path == QStringLiteral("/next.html")) {
        respond("200 OK", html, "<title>Next</title>" + head + "<p>the end</p>");
    } else if (path == QStringLiteral("/to-file.html")) {
        respond("200 OK", html, "<title>To a file</title>" + head + "<a href=file:///etc/passwd>a secret</a>");
    } else if (path == QStringLiteral("/moved")) {
        respond("302 Found", "Location: /next.html\r\n", "");
    } else if (path == QStringLiteral("/picture.png")) {
        QImage image(40, 40, QImage::Format_RGB32);
        image.fill(QColor(10, 20, 30));
        QByteArray png;
        QBuffer buffer(&png);
        buffer.open(QIODevice::WriteOnly);
        image.save(&buffer, "PNG");
        respond("200 OK", "Content-Type: image/png\r\n", png);
    } else if (path == QStringLiteral("/photo.jpg")) {
        respond("200 OK", "Content-Type: image/jpeg\r\n", "not really");
    } else if (path == QStringLiteral("/styled.html")) {
        respond("200 OK", html, "<title>Styled</title><link rel=stylesheet href=styled.css><p>x");
    } else if (path == QStringLiteral("/styled.css")) {
        respond("200 OK", "Content-Type: text/css\r\n", "html{background:rgb(0,128,0)}");
    } else if (path == QStringLiteral("/waits.html")) {
        respond("200 OK", html, "<title>Waits</title><link rel=stylesheet href=never.css><p>x");
    } else if (path != QStringLiteral("/never") && path != QStringLiteral("/never.css")) {
        respond("404 Not Found", "", "gone");
    }
}

void MainWindowTest::write(const QString &name, const QByteArray &content)
{
    QFile file(directory.filePath(name));
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(content);
}

void MainWindowTest::init()
{
    served.clear();
    if (!server.isListening()) {
        QVERIFY(server.listen(QHostAddress::LocalHost));
        connect(&server, &QTcpServer::newConnection, this, [this] {
            while (QTcpSocket *socket = server.nextPendingConnection()) {
                connect(socket, &QTcpSocket::readyRead, this, [this, socket] { answerRequest(socket); });
                connect(socket, &QTcpSocket::disconnected, socket, &QObject::deleteLater);
            }
        });
    }
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

void MainWindowTest::theAddressLineFollowsAndLeads()
{
    auto *line = window->findChild<QLineEdit *>();
    QVERIFY(line != nullptr);
    QVERIFY(line->text().startsWith(QStringLiteral("file:///")));
    QVERIFY(line->text().endsWith(QStringLiteral("/first.html")));
    click();
    QVERIFY(line->text().endsWith(QStringLiteral("/second.html")));
    click();
    QVERIFY(line->text().endsWith(QStringLiteral("/third.html#end")));
    action("Back")->trigger();
    QVERIFY(line->text().endsWith(QStringLiteral("/second.html")));

    line->setText(directory.filePath("broken.html"));
    QTest::keyClick(line, Qt::Key_Return);
    QCOMPARE(window->windowTitle(), QStringLiteral("Broken"));
    QVERIFY(line->text().startsWith(QStringLiteral("file:///")));

    line->setText(QStringLiteral("javascript:alert(1)"));
    QTest::keyClick(line, Qt::Key_Return);
    QCOMPARE(window->windowTitle(), QStringLiteral("Broken"));
    QVERIFY(window->statusBar()->currentMessage().contains(QStringLiteral("javascript")));

    line->setText(directory.filePath("no-such-file.html"));
    QTest::keyClick(line, Qt::Key_Return);
    QCOMPARE(window->windowTitle(), QStringLiteral("Broken"));
}

void MainWindowTest::pagesComeFromTheNetworkWithTheirImages()
{
    auto *line = window->findChild<QLineEdit *>();
    line->setText(serverAddress("/net.html"));
    QTest::keyClick(line, Qt::Key_Return);
    // The window goes on while it is being read.
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
    QTRY_COMPARE(window->windowTitle(), QStringLiteral("On the net"));
    QCOMPARE(line->text(), serverAddress("/net.html"));
    QVERIFY(window->statusBar()->currentMessage().contains(QStringLiteral("Not secure")));
    QTRY_VERIFY(served.contains(QStringLiteral("/picture.png")));

    // The image shows as soon as it has come, without anything else happening.
    QTRY_COMPARE(window->pageView()->viewport()->grab().toImage().pixelColor(5, 30), QColor(10, 20, 30));

    click();
    QTRY_COMPARE(window->windowTitle(), QStringLiteral("Next"));
    action("Back")->trigger();
    QTRY_COMPARE(window->windowTitle(), QStringLiteral("On the net"));
    action("Back")->trigger();
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
}

void MainWindowTest::aPageShowsWithItsStyleSheet()
{
    auto *line = window->findChild<QLineEdit *>();
    line->setText(serverAddress("/styled.html"));
    QTest::keyClick(line, Qt::Key_Return);
    QTRY_COMPARE(window->windowTitle(), QStringLiteral("Styled"));
    QVERIFY(served.contains(QStringLiteral("/styled.css")));
    // The sheet was there when the page first showed.
    QCOMPARE(window->pageView()->viewport()->grab().toImage().pixelColor(5, 5), QColor(0, 128, 0));
}

void MainWindowTest::stoppingWhileASheetComesLeavesThePage()
{
    auto *line = window->findChild<QLineEdit *>();
    line->setText(serverAddress("/waits.html"));
    QTest::keyClick(line, Qt::Key_Return);
    QTRY_VERIFY(served.contains(QStringLiteral("/never.css")));
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
    QVERIFY(action("Stop")->isEnabled());
    action("Stop")->trigger();
    QVERIFY(!action("Stop")->isEnabled());
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
    QVERIFY(!action("Back")->isEnabled());
}

void MainWindowTest::backGoesToBeforeARedirect()
{
    auto *line = window->findChild<QLineEdit *>();
    line->setText(serverAddress("/moved"));
    QTest::keyClick(line, Qt::Key_Return);
    QTRY_COMPARE(window->windowTitle(), QStringLiteral("Next"));
    QCOMPARE(line->text(), serverAddress("/next.html"));
    action("Back")->trigger();
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
    QVERIFY(!action("Back")->isEnabled());
}

void MainWindowTest::whatFailsLeavesThePageAndTheTypedAddress()
{
    auto *line = window->findChild<QLineEdit *>();
    for (const QString &address : {serverAddress("/nothing"), serverAddress("/photo.jpg")}) {
        line->setText(address);
        QTest::keyClick(line, Qt::Key_Return);
        QTRY_VERIFY(!window->statusBar()->currentMessage().startsWith(QStringLiteral("Loading")));
        QCOMPARE(window->windowTitle(), QStringLiteral("First"));
        QCOMPARE(line->text(), address);
        QVERIFY(!action("Back")->isEnabled());
    }
}

void MainWindowTest::aPageOnTheNetworkCannotOpenLocalFiles()
{
    auto *line = window->findChild<QLineEdit *>();
    line->setText(serverAddress("/to-file.html"));
    QTest::keyClick(line, Qt::Key_Return);
    QTRY_COMPARE(window->windowTitle(), QStringLiteral("To a file"));
    click();
    QCOMPARE(window->windowTitle(), QStringLiteral("To a file"));
    QVERIFY(window->statusBar()->currentMessage().contains(QStringLiteral("not followed")));
}

void MainWindowTest::stoppingLeavesThePageAsItWas()
{
    auto *line = window->findChild<QLineEdit *>();
    line->setText(serverAddress("/never"));
    QTest::keyClick(line, Qt::Key_Return);
    QVERIFY(action("Stop")->isEnabled());
    action("Stop")->trigger();
    QVERIFY(!action("Stop")->isEnabled());
    QCOMPARE(window->windowTitle(), QStringLiteral("First"));
    QVERIFY(!action("Back")->isEnabled());
}

void MainWindowTest::aPageShowsWhileItArrives()
{
    window->setArrivalSeconds(2);
    window->openPage(directory.filePath("third.html"));
    PageView *view = window->pageView();
    QVERIFY(window->isLoading());
    QVERIFY(action("Stop")->isEnabled());
    // What was there stays until there is something to show of the new page.
    QTRY_COMPARE_WITH_TIMEOUT(window->windowTitle(), QStringLiteral("Third"), 5000);
    // The range of the scroll bar only grows, and the window goes on in the meantime.
    int range = 0;
    int grew = 0;
    QTRY_VERIFY_WITH_TIMEOUT(
        [&] {
            const int now = view->verticalScrollBar()->maximum();
            if (now < range) {
                qFatal("the range went back from %d to %d", range, now);
            }
            grew += now > range ? 1 : 0;
            range = now;
            return !window->isLoading();
        }(),
        10000);
    QVERIFY(grew > 1);
    QCOMPARE(window->windowTitle(), QStringLiteral("Third"));
    QVERIFY(!action("Stop")->isEnabled());
    QVERIFY(window->statusBar()->currentMessage().contains(QStringLiteral("Encoding")));
}

void MainWindowTest::aPageFromTheNetworkShowsBeforeAllOfItHasCome()
{
    window->openPage(serverAddress("/slowly.html"));
    PageView *view = window->pageView();
    QTRY_COMPARE_WITH_TIMEOUT(window->windowTitle(), QStringLiteral("Slowly"), slowAnswerMs - 500);
    QVERIFY(window->isLoading());
    QVERIFY(action("Stop")->isEnabled());
    QTRY_VERIFY_WITH_TIMEOUT(view->verticalScrollBar()->maximum() > 0, slowAnswerMs - 500);
    const int early = view->verticalScrollBar()->maximum();
    QVERIFY(early < 5000 - view->viewport()->height());
    QTRY_VERIFY_WITH_TIMEOUT(!window->isLoading(), 10000);
    QVERIFY(view->verticalScrollBar()->maximum() > early);
    QVERIFY(!action("Stop")->isEnabled());
    QVERIFY(window->statusBar()->currentMessage().contains(QStringLiteral("from the server")));
    QVERIFY(action("Back")->isEnabled());
}

void MainWindowTest::stoppingAPageFromTheNetworkKeepsWhatCame()
{
    window->openPage(serverAddress("/slowly.html"));
    QTRY_COMPARE_WITH_TIMEOUT(window->windowTitle(), QStringLiteral("Slowly"), slowAnswerMs - 500);
    action("Stop")->trigger();
    QVERIFY(!window->isLoading());
    QVERIFY(!action("Stop")->isEnabled());
    QCOMPARE(window->windowTitle(), QStringLiteral("Slowly"));
    QVERIFY(window->statusBar()->currentMessage().contains(QStringLiteral("Encoding")));
    QVERIFY(window->pageView()->verticalScrollBar()->maximum() > 0);
}

void MainWindowTest::scrollingStaysWhileAPageArrives()
{
    window->setArrivalSeconds(2);
    window->openPage(directory.filePath("third.html"));
    PageView *view = window->pageView();
    QTRY_COMPARE_WITH_TIMEOUT(window->windowTitle(), QStringLiteral("Third"), 5000);
    QTRY_VERIFY_WITH_TIMEOUT(view->verticalScrollBar()->maximum() >= 100, 5000);
    view->setScrollY(100);
    QTRY_VERIFY_WITH_TIMEOUT(!window->isLoading(), 10000);
    QCOMPARE(view->scrollY(), 100);
}

void MainWindowTest::stoppingAnArrivalKeepsWhatCame()
{
    QByteArray lines;
    for (int line = 0; line < 2000; ++line) {
        lines += "<p>line</p>";
    }
    write("long.html", "<title>Long</title>" + head + lines);
    write("longer.html", "<title>Longer</title>" + head + lines + lines);
    window->setArrivalSeconds(30);
    window->openPage(directory.filePath("long.html"));
    QTRY_COMPARE_WITH_TIMEOUT(window->windowTitle(), QStringLiteral("Long"), 5000);
    action("Stop")->trigger();
    QVERIFY(!window->isLoading());
    QVERIFY(!action("Stop")->isEnabled());
    QCOMPARE(window->windowTitle(), QStringLiteral("Long"));
    // Back also leaves a page that is still arriving, for where the user was.
    window->openPage(directory.filePath("longer.html"));
    QTRY_COMPARE_WITH_TIMEOUT(window->windowTitle(), QStringLiteral("Longer"), 5000);
    QVERIFY(window->isLoading());
    action("Back")->trigger();
    QVERIFY(action("Forward")->isEnabled());
}

void MainWindowTest::backDropsAPageThatDoesNotShowYet()
{
    click();
    window->setArrivalSeconds(30);
    window->openPage(directory.filePath("third.html"));
    QVERIFY(window->isLoading());
    action("Back")->trigger();
    QVERIFY(!window->isLoading());
    QCOMPARE(window->windowTitle(), QStringLiteral("Second"));
    QVERIFY(!action("Forward")->isEnabled());
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

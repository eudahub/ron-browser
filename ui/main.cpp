#include "mainwindow.h"
#include "pageloader.h"

#include <QApplication>
#include <QCoreApplication>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QFileInfo>
#include <QIcon>
#include <QLineF>
#include <QPixmap>
#include <QSize>
#include <QStringList>

#include <ron-browser-bridge/src/lib.rs.h>

#include "pageview.h"

#include <chrono>
#include <iostream>
#include <optional>
#include <string>
#include <thread>
#include <utility>

namespace {

// What the styles of a page count on when it is printed as text, with no window.
const float printWidth = 1024;
const float printHeight = 768;

// How long a screenshot waits for a page and its images to arrive.
const int maxScreenshotWaitMs = 60000;

const char usage[] =
    "usage: ron-browser [--encoding NAME] [--slow-load SECONDS] [FILE or ADDRESS]\n"
    "       ron-browser [--encoding NAME] --print FILE\n"
    "       ron-browser [--encoding NAME] --screenshot PNG [--size WIDTHxHEIGHT] [--select X1 Y1 X2 Y2] [FILE or ADDRESS]\n"
    "       ron-browser [--encoding NAME] --page-shot PNG [--size WIDTHxHEIGHT] FILE\n"
    "       ron-browser [--encoding NAME] --decode FILE\n"
    "       ron-browser --save-page DIRECTORY ADDRESS\n";

int printPage(const QString &path, const std::string &encoding)
{
    DecodedPage decoded = loadPage(path, encoding);
    if (!decoded.error.isEmpty()) {
        std::cerr << path.toStdString() << ": " << decoded.error.toStdString() << "\n";
        return 1;
    }
    const auto page = ron_browser::open_page(std::move(decoded.utf8), "", printWidth, printHeight);
    std::cout << "encoding: " << decoded.encoding << " (" << decoded.encodingSource.toStdString() << ")\n"
              << "title: " << std::string(page->title()) << "\n\n"
              << std::string(page->plain_text()) << "\n";
    return 0;
}

// Writes the page as UTF-8, for tools that show it to something else.
int decodePage(const QString &path, const std::string &encoding)
{
    const DecodedPage decoded = loadPage(path, encoding);
    if (!decoded.error.isEmpty()) {
        std::cerr << path.toStdString() << ": " << decoded.error.toStdString() << "\n";
        return 1;
    }
    std::cout << decoded.utf8;
    return 0;
}

// Saves a page without showing any of it. Needs no window, and so no display.
int savePage(const QString &directory, const QString &address)
{
    const QByteArray addressBytes = address.toUtf8();
    const auto target = ron_browser::resolve_address(rust::Str(addressBytes.constData(), static_cast<size_t>(addressBytes.size())));
    if (target.kind != ron_browser::LinkKind::Page || !ron_browser::local_path(target.place).empty()) {
        std::cerr << address.toStdString() << ": only a page on the network can be saved\n";
        return 1;
    }
    const QByteArray parent = directory.toUtf8();
    auto save = ron_browser::start_page_save(target.place, rust::Str(parent.constData(), static_cast<size_t>(parent.size())));
    while (true) {
        const ron_browser::SaveEvent event = save->poll();
        if (event.kind == ron_browser::SaveEventKind::Finished) {
            std::cout << "saved to " << std::string(event.directory) << ": " << event.saved << " files, "
                      << event.failed << " could not be read\n";
            return 0;
        }
        if (event.kind == ron_browser::SaveEventKind::Failed) {
            std::cerr << address.toStdString() << ": " << std::string(event.error) << "\n";
            return 1;
        }
        if (event.kind == ron_browser::SaveEventKind::Nothing) {
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
    }
}

// A shot of the whole page alone, without the window around it, for comparing with others.
int shootPage(const QString &path, const std::string &encoding, const QString &out, QSize size)
{
    DecodedPage decoded = loadPage(path, encoding);
    if (!decoded.error.isEmpty()) {
        std::cerr << path.toStdString() << ": " << decoded.error.toStdString() << "\n";
        return 1;
    }
    PageView view;
    view.resize(size);
    view.show();
    view.setPage(std::move(decoded.utf8), QFileInfo(path).absolutePath());
    return view.grabWholePage(size).save(out) ? 0 : 1;
}

} // namespace

int main(int argc, char *argv[])
{
    std::string encoding;
    QString path;
    QString screenshot;
    QString saveDirectory;
    bool print = false;
    bool decode = false;
    QString pageShot;
    QSize pageShotSize(900, 700);
    int slowLoadSeconds = 0;
    std::optional<QLineF> selection;
    for (int index = 1; index < argc; ++index) {
        const std::string argument = argv[index];
        if (argument == "--encoding" && index + 1 < argc) {
            encoding = argv[++index];
        } else if (argument == "--screenshot" && index + 1 < argc) {
            screenshot = QString::fromLocal8Bit(argv[++index]);
        } else if (argument == "--select" && index + 4 < argc) {
            selection = QLineF(std::stod(argv[index + 1]), std::stod(argv[index + 2]),
                               std::stod(argv[index + 3]), std::stod(argv[index + 4]));
            index += 4;
        } else if (argument == "--page-shot" && index + 1 < argc) {
            pageShot = QString::fromLocal8Bit(argv[++index]);
        } else if (argument == "--size" && index + 1 < argc) {
            const QStringList parts = QString::fromLocal8Bit(argv[++index]).split(u'x');
            if (parts.size() == 2) {
                pageShotSize = QSize(parts[0].toInt(), parts[1].toInt());
            }
        } else if (argument == "--slow-load" && index + 1 < argc) {
            slowLoadSeconds = std::stoi(argv[++index]);
        } else if (argument == "--save-page" && index + 1 < argc) {
            saveDirectory = QString::fromLocal8Bit(argv[++index]);
        } else if (argument == "--print") {
            print = true;
        } else if (argument == "--decode") {
            decode = true;
        } else if (argument.rfind("-", 0) != 0 && path.isEmpty()) {
            path = QString::fromLocal8Bit(argument);
        } else {
            std::cerr << usage;
            return 2;
        }
    }
    if (!saveDirectory.isEmpty()) {
        if (path.isEmpty()) {
            std::cerr << usage;
            return 2;
        }
        return savePage(saveDirectory, path);
    }
    if (print || decode) {
        if (path.isEmpty()) {
            std::cerr << usage;
            return 2;
        }
        // Needs no window, and so no display.
        QCoreApplication application(argc, argv);
        return print ? printPage(path, encoding) : decodePage(path, encoding);
    }

    QApplication application(argc, argv);
    // The installed ron-browser.desktop; on Wayland it ties the window to its icon.
    QApplication::setDesktopFileName(QStringLiteral("ron-browser"));
    QApplication::setWindowIcon(QIcon::fromTheme(QStringLiteral("ron-browser"),
                                                 QIcon(QStringLiteral(":/ron-browser.svg"))));
    if (!pageShot.isEmpty()) {
        return shootPage(path, encoding, pageShot, pageShotSize);
    }
    MainWindow window(encoding);
    window.setArrivalSeconds(slowLoadSeconds);
    window.resize(pageShotSize);
    window.show();
    if (!path.isEmpty()) {
        window.openPage(path);
    }
    if (!screenshot.isEmpty()) {
        QElapsedTimer waited;
        waited.start();
        while (window.isLoading() && waited.elapsed() < maxScreenshotWaitMs) {
            QCoreApplication::processEvents(QEventLoop::AllEvents, 50);
        }
        window.layOutNow();
        if (selection) {
            // Shows and prints what a drag between these points of the page gives.
            std::cout << window.pageView()->selectForTest(selection->p1(), selection->p2(), 1).toStdString() << "\n";
        }
        QCoreApplication::processEvents();
        return window.grab().save(screenshot) ? 0 : 1;
    }
    return QApplication::exec();
}

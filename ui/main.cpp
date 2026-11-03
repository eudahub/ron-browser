#include "mainwindow.h"
#include "pageloader.h"

#include <QApplication>
#include <QCoreApplication>
#include <QLineF>
#include <QStringList>

#include <ron-browser-bridge/src/lib.rs.h>

#include "pageview.h"

#include <iostream>
#include <optional>
#include <string>

namespace {

const char usage[] =
    "usage: ron-browser [--encoding NAME] [FILE]\n"
    "       ron-browser [--encoding NAME] --print FILE\n"
    "       ron-browser [--encoding NAME] --screenshot PNG [--select X1 Y1 X2 Y2] FILE\n";

int printPage(const QString &path, const std::string &encoding)
{
    const DecodedPage decoded = loadPage(path, encoding);
    if (!decoded.error.isEmpty()) {
        std::cerr << path.toStdString() << ": " << decoded.error.toStdString() << "\n";
        return 1;
    }
    const auto page = ron_browser::open_page(decoded.utf8);
    std::cout << "encoding: " << decoded.encoding << " (" << decoded.encodingSource.toStdString() << ")\n"
              << "title: " << std::string(page->title()) << "\n\n"
              << std::string(page->plain_text()) << "\n";
    return 0;
}

} // namespace

int main(int argc, char *argv[])
{
    std::string encoding;
    QString path;
    QString screenshot;
    bool print = false;
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
        } else if (argument == "--print") {
            print = true;
        } else if (argument.rfind("-", 0) != 0 && path.isEmpty()) {
            path = QString::fromLocal8Bit(argument);
        } else {
            std::cerr << usage;
            return 2;
        }
    }
    if (print) {
        if (path.isEmpty()) {
            std::cerr << usage;
            return 2;
        }
        // Needs no window, and so no display.
        QCoreApplication application(argc, argv);
        return printPage(path, encoding);
    }

    QApplication application(argc, argv);
    MainWindow window(encoding);
    window.show();
    if (!path.isEmpty()) {
        window.openPage(path);
    }
    if (!screenshot.isEmpty()) {
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

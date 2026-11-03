#include "mainwindow.h"

#include "pageloader.h"

#include <QFileDialog>
#include <QFileInfo>
#include <QKeySequence>
#include <QMenuBar>
#include <QPlainTextEdit>
#include <QStatusBar>

#include <ron-browser-core/src/bridge.rs.h>

MainWindow::MainWindow(std::string forcedEncoding)
    : view(new QPlainTextEdit(this)), forcedEncoding(std::move(forcedEncoding))
{
    // Page content reaches Qt only as plain text, never as anything Qt would interpret.
    view->setReadOnly(true);
    setCentralWidget(view);
    setWindowTitle(tr("ron-browser"));
    resize(900, 700);

    auto *fileMenu = menuBar()->addMenu(tr("&File"));
    fileMenu->addAction(tr("&Open..."), QKeySequence::Open, this, &MainWindow::chooseFile);
    fileMenu->addAction(tr("&Quit"), QKeySequence::Quit, this, &QWidget::close);
}

void MainWindow::chooseFile()
{
    const QString path = QFileDialog::getOpenFileName(
        this, tr("Open page"), QString(), tr("Web pages (*.htm *.html);;All files (*)"));
    if (!path.isEmpty()) {
        openPage(path);
    }
}

void MainWindow::openPage(const QString &path)
{
    const DecodedPage decoded = loadPage(path, forcedEncoding);
    if (!decoded.error.isEmpty()) {
        view->clear();
        setWindowTitle(tr("ron-browser"));
        statusBar()->showMessage(tr("Cannot open %1: %2").arg(path, decoded.error));
        return;
    }
    const auto page = ron_browser::open_page(decoded.utf8);
    const QString title = QString::fromStdString(std::string(page->title()));
    view->setPlainText(QString::fromStdString(std::string(page->plain_text())));
    setWindowTitle(title.isEmpty() ? QFileInfo(path).fileName() : title);
    statusBar()->showMessage(tr("Encoding: %1 (%2)")
                                 .arg(QString::fromStdString(decoded.encoding), decoded.encodingSource));
}

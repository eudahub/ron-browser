#include "mainwindow.h"

#include "pageloader.h"
#include "pageview.h"

#include <QAction>
#include <QFileDialog>
#include <QFileInfo>
#include <QKeySequence>
#include <QLineEdit>
#include <QMenuBar>
#include <QStatusBar>
#include <QStyle>
#include <QToolBar>
#include <QUrl>

#include <ron-browser-bridge/src/lib.rs.h>

namespace {

// Addresses come from pages, and can be very long or hold what a status bar should not show.
const int maxShownAddress = 300;

QString shownAddress(const QString &address)
{
    return address.simplified().left(maxShownAddress);
}

rust::Str asStr(const QByteArray &bytes)
{
    return {bytes.constData(), static_cast<size_t>(bytes.size())};
}

QString asQString(const rust::Str text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

QString asQString(const rust::String &text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

} // namespace

MainWindow::MainWindow(std::string forcedEncoding)
    : view(new PageView(this)), forcedEncoding(std::move(forcedEncoding)), history(ron_browser::new_history()),
      addressBar(new QLineEdit(this))
{
    setCentralWidget(view);
    statusBar();
    setWindowTitle(tr("ron-browser"));
    resize(900, 700);

    auto *fileMenu = menuBar()->addMenu(tr("&File"));
    fileMenu->addAction(tr("&Open..."), QKeySequence::Open, this, &MainWindow::chooseFile);
    fileMenu->addAction(tr("&Quit"), QKeySequence::Quit, this, &QWidget::close);
    auto *editMenu = menuBar()->addMenu(tr("&Edit"));
    editMenu->addAction(tr("&Copy"), QKeySequence::Copy, view, &PageView::copySelection);
    editMenu->addAction(tr("Select &All"), QKeySequence::SelectAll, view, &PageView::selectAll);
    auto *goMenu = menuBar()->addMenu(tr("&Go"));
    backAction = goMenu->addAction(style()->standardIcon(QStyle::SP_ArrowBack), tr("&Back"), QKeySequence::Back, this,
                                   &MainWindow::goBack);
    forwardAction = goMenu->addAction(style()->standardIcon(QStyle::SP_ArrowForward), tr("&Forward"),
                                      QKeySequence::Forward, this, &MainWindow::goForward);
    reloadAction = goMenu->addAction(style()->standardIcon(QStyle::SP_BrowserReload), tr("&Reload"),
                                     QKeySequence::Refresh, this, &MainWindow::reload);
    goMenu->addAction(tr("Go to &Address"), QKeySequence(Qt::CTRL | Qt::Key_L), this, [this] {
        addressBar->setFocus();
        addressBar->selectAll();
    });
    auto *toolBar = addToolBar(tr("Navigation"));
    toolBar->setMovable(false);
    toolBar->addAction(backAction);
    toolBar->addAction(forwardAction);
    toolBar->addAction(reloadAction);
    addressBar->setPlaceholderText(tr("Address or file"));
    toolBar->addWidget(addressBar);

    connect(addressBar, &QLineEdit::returnPressed, this, &MainWindow::goToTyped);
    connect(view, &PageView::linkActivated, this, &MainWindow::followLink);
    connect(view, &PageView::linkHovered, this, &MainWindow::showLinkInStatus);
    connect(view, &PageView::backButtonPressed, this, &MainWindow::goBack);
    connect(view, &PageView::forwardButtonPressed, this, &MainWindow::goForward);
    updateNavigation();
}

MainWindow::~MainWindow() = default;

void MainWindow::layOutNow()
{
    view->layOutNow();
}

PageView *MainWindow::pageView() const
{
    return view;
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
    const QByteArray bytes = path.toUtf8();
    const QString address = asQString(ron_browser::file_address(asStr(bytes)));
    if (address.isEmpty()) {
        statusBar()->showMessage(tr("Cannot open %1").arg(path));
        return;
    }
    goTo(address, QString());
}

bool MainWindow::loadAddress(const QString &address)
{
    const QByteArray bytes = address.toUtf8();
    const QString path = asQString(ron_browser::local_path(asStr(bytes)));
    if (path.isEmpty()) {
        statusBar()->showMessage(tr("This browser has no network yet: %1").arg(shownAddress(address)));
        return false;
    }
    const DecodedPage decoded = loadPage(path, forcedEncoding);
    if (!decoded.error.isEmpty()) {
        statusBar()->showMessage(tr("Cannot open %1: %2").arg(path, decoded.error));
        return false;
    }
    view->setPage(decoded.utf8, QFileInfo(path).absolutePath());
    loadedAddress = address;
    const QString title = view->pageTitle();
    setWindowTitle(title.isEmpty() ? QUrl(address).fileName() : title);
    pageStatus = tr("Encoding: %1 (%2)").arg(QString::fromStdString(decoded.encoding), decoded.encodingSource);
    statusBar()->showMessage(pageStatus);
    return true;
}

void MainWindow::goTo(const QString &address, const QString &fragment)
{
    history->save_scroll(view->scrollY());
    if (address != loadedAddress && !loadAddress(address)) {
        return;
    }
    const QByteArray addressBytes = address.toUtf8();
    const QByteArray fragmentBytes = fragment.toUtf8();
    history->visit(asStr(addressBytes), asStr(fragmentBytes));
    view->scrollToFragment(fragment);
    updateNavigation();
}

void MainWindow::goBack()
{
    history->save_scroll(view->scrollY());
    if (history->go_back()) {
        showCurrentEntry();
    }
}

void MainWindow::goForward()
{
    history->save_scroll(view->scrollY());
    if (history->go_forward()) {
        showCurrentEntry();
    }
}

void MainWindow::showCurrentEntry()
{
    const QString address = asQString(history->current_address());
    if (address == loadedAddress || loadAddress(address)) {
        view->setScrollY(history->current_scroll());
    }
    updateNavigation();
}

void MainWindow::reload()
{
    if (!loadedAddress.isEmpty()) {
        const int scroll = view->scrollY();
        if (loadAddress(loadedAddress)) {
            view->setScrollY(scroll);
        }
    }
}

void MainWindow::goToTyped()
{
    const QByteArray typed = addressBar->text().toUtf8();
    followTarget(ron_browser::resolve_address(asStr(typed)));
}

void MainWindow::followLink(const QString &address)
{
    const QByteArray page = loadedAddress.toUtf8();
    const QByteArray href = address.toUtf8();
    followTarget(ron_browser::resolve_link(asStr(page), asStr(href)));
}

void MainWindow::followTarget(const ron_browser::LinkTarget &target)
{
    const QString place = shownAddress(asQString(target.place));
    switch (target.kind) {
    case ron_browser::LinkKind::SamePage:
        goTo(loadedAddress, asQString(target.fragment));
        break;
    case ron_browser::LinkKind::Page:
        goTo(asQString(target.place), asQString(target.fragment));
        break;
    case ron_browser::LinkKind::NotAPage:
        statusBar()->showMessage(tr("Not a web page: %1").arg(place));
        break;
    case ron_browser::LinkKind::Missing:
        statusBar()->showMessage(tr("Not found: %1").arg(place));
        break;
    case ron_browser::LinkKind::Unsupported:
        statusBar()->showMessage(tr("This browser does not handle this: %1").arg(place));
        break;
    default:
        statusBar()->showMessage(tr("Links of this kind are not followed: %1").arg(place));
        break;
    }
}

void MainWindow::showLinkInStatus(const QString &address)
{
    statusBar()->showMessage(address.isEmpty() ? pageStatus : shownAddress(address));
}

void MainWindow::updateNavigation()
{
    backAction->setEnabled(history->can_go_back());
    forwardAction->setEnabled(history->can_go_forward());
    reloadAction->setEnabled(!loadedAddress.isEmpty());
    const QString fragment = asQString(history->current_fragment());
    addressBar->setText(fragment.isEmpty() ? loadedAddress : loadedAddress + u'#' + fragment);
}

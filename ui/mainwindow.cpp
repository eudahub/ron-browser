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
#include <QTimer>
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
      addressBar(new QLineEdit(this)), fetchTimer(new QTimer(this))
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
    stopAction = goMenu->addAction(style()->standardIcon(QStyle::SP_BrowserStop), tr("&Stop"),
                                   QKeySequence(Qt::Key_Escape), this, &MainWindow::stopFetch);
    goMenu->addAction(tr("Go to &Address"), QKeySequence(Qt::CTRL | Qt::Key_L), this, [this] {
        addressBar->setFocus();
        addressBar->selectAll();
    });
    auto *toolBar = addToolBar(tr("Navigation"));
    toolBar->setMovable(false);
    toolBar->addAction(backAction);
    toolBar->addAction(forwardAction);
    toolBar->addAction(reloadAction);
    toolBar->addAction(stopAction);
    addressBar->setPlaceholderText(tr("Address or file"));
    toolBar->addWidget(addressBar);

    fetchTimer->setInterval(30);
    connect(fetchTimer, &QTimer::timeout, this, &MainWindow::checkFetch);

    connect(addressBar, &QLineEdit::returnPressed, this, &MainWindow::goToTyped);
    connect(view, &PageView::linkActivated, this, &MainWindow::followLink);
    connect(view, &PageView::linkHovered, this, &MainWindow::showLinkInStatus);
    connect(view, &PageView::backButtonPressed, this, &MainWindow::goBack);
    connect(view, &PageView::forwardButtonPressed, this, &MainWindow::goForward);
    connect(view, &PageView::arrivalProgress, this, [this](int percent) {
        pageStatus = tr("Loading %1%").arg(percent);
        statusBar()->showMessage(pageStatus);
        const QString title = view->pageTitle();
        if (!title.isEmpty()) {
            setWindowTitle(title);
        }
    });
    connect(view, &PageView::arrivalFinished, this, [this] {
        pageStatus = fetchedStatus;
        statusBar()->showMessage(pageStatus);
        const QString title = view->pageTitle();
        if (!title.isEmpty()) {
            setWindowTitle(title);
        }
        updateNavigation();
    });
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

void MainWindow::openPage(const QString &address)
{
    const QByteArray bytes = address.toUtf8();
    followTarget(ron_browser::resolve_address(asStr(bytes)), QString());
}

bool MainWindow::isLoading() const
{
    return isFetching() || view->isLoadingImages();
}

MainWindow::LocalLoad MainWindow::loadLocalFile(const QString &address, const QString &path)
{
    DecodedPage decoded = loadPage(path, forcedEncoding);
    if (!decoded.error.isEmpty()) {
        statusBar()->showMessage(tr("Cannot open %1: %2").arg(path, decoded.error));
        return LocalLoad::Failed;
    }
    const QString status = tr("Encoding: %1 (%2)").arg(QString::fromStdString(decoded.encoding), decoded.encodingSource);
    if (arrivalSeconds > 0) {
        view->startArriving(std::move(decoded.utf8), QFileInfo(path).absolutePath(), address, arrivalSeconds);
        loadedAddress = address;
        setWindowTitle(QUrl(address).fileName());
        fetchedStatus = status;
        pageStatus = tr("Loading 0%");
        statusBar()->showMessage(pageStatus);
        return LocalLoad::Shown;
    }
    if (!view->setLocalPage(std::move(decoded.utf8), QFileInfo(path).absolutePath(), address)) {
        fetchedAddress = address;
        fetchedStatus = status;
        fetchTimer->start();
        pageStatus = tr("Loading %1").arg(shownAddress(address));
        statusBar()->showMessage(pageStatus);
        updateNavigation();
        return LocalLoad::Waiting;
    }
    loadedAddress = address;
    const QString title = view->pageTitle();
    setWindowTitle(title.isEmpty() ? QUrl(address).fileName() : title);
    pageStatus = status;
    statusBar()->showMessage(pageStatus);
    return LocalLoad::Shown;
}

void MainWindow::goTo(const QString &address, const QString &fragment, const QString &askingPage)
{
    history->save_scroll(view->scrollY());
    stopFetch();
    const QByteArray bytes = address.toUtf8();
    const QString path = asQString(ron_browser::local_path(asStr(bytes)));
    if (address != loadedAddress) {
        fetchFragment = fragment;
        fetchScroll.reset();
        if (path.isEmpty()) {
            startFetch(address, askingPage);
            return;
        }
        if (loadLocalFile(address, path) != LocalLoad::Shown) {
            return;
        }
    }
    const QByteArray fragmentBytes = fragment.toUtf8();
    history->visit(asStr(bytes), asStr(fragmentBytes));
    view->scrollToFragment(fragment);
    updateNavigation();
}

void MainWindow::startFetch(const QString &address, const QString &askingPage)
{
    const QByteArray addressBytes = address.toUtf8();
    const QByteArray pageBytes = askingPage.toUtf8();
    pageFetch = ron_browser::start_page_fetch(asStr(addressBytes), asStr(pageBytes));
    fetchTimer->start();
    pageStatus = tr("Loading %1").arg(shownAddress(address));
    statusBar()->showMessage(pageStatus);
    updateNavigation();
}

void MainWindow::stopFetch()
{
    if (view->isArriving()) {
        view->stopArriving();
        updateNavigation();
    }
    if (isWaitingForPage()) {
        fetchTimer->stop();
        pageFetch.reset();
        view->dropIncomingPage();
        pageStatus.clear();
        statusBar()->clearMessage();
        updateNavigation();
    }
}

bool MainWindow::isFetching() const
{
    return isWaitingForPage() || view->isArriving();
}

void MainWindow::stopArrivalOnly()
{
    if (view->isArriving()) {
        view->stopArriving();
    }
}

bool MainWindow::isWaitingForPage() const
{
    return pageFetch.has_value() || view->hasIncomingPage();
}

void MainWindow::setArrivalSeconds(int seconds)
{
    arrivalSeconds = seconds;
}

void MainWindow::checkFetch()
{
    if (view->hasIncomingPage()) {
        if (view->incomingPageIsReady()) {
            fetchTimer->stop();
            showFetchedPage();
        }
        return;
    }
    if (!pageFetch) {
        fetchTimer->stop();
        return;
    }
    ron_browser::FetchedPage fetched = (*pageFetch)->poll();
    if (!fetched.done) {
        return;
    }
    pageFetch.reset();
    if (!fetched.error.empty()) {
        fetchTimer->stop();
        pageStatus.clear();
        statusBar()->showMessage(tr("Cannot open it: %1").arg(shownAddress(asQString(fetched.error))));
        updateNavigation();
        return;
    }
    const QString mime = asQString(fetched.mime);
    const QString address = asQString(fetched.address);
    if (!mime.isEmpty() && mime != QStringLiteral("text/html") && mime != QStringLiteral("application/xhtml+xml")) {
        fetchTimer->stop();
        pageStatus.clear();
        statusBar()->showMessage(tr("Not a web page, but %1: %2").arg(mime.left(100), shownAddress(address)));
        updateNavigation();
        return;
    }
    DecodedPage decoded = decodeFetchedPage(std::move(fetched.bytes), std::string(fetched.charset));
    view->prepareFetchedPage(std::move(decoded.utf8), address);
    fetchedAddress = address;
    const QString security = QUrl(address).scheme() == QStringLiteral("https") ? QString() : tr("Not secure. ");
    fetchedStatus = security + tr("Encoding: %1 (%2)").arg(QString::fromStdString(decoded.encoding), decoded.encodingSource);
}

void MainWindow::showFetchedPage()
{
    view->showIncomingPage();
    const QString address = fetchedAddress;
    loadedAddress = address;
    const QString title = view->pageTitle();
    const QUrl url(address);
    setWindowTitle(!title.isEmpty() ? title : url.isLocalFile() ? url.fileName() : url.host());
    pageStatus = fetchedStatus;
    statusBar()->showMessage(pageStatus);
    if (fetchScroll) {
        view->setScrollY(*fetchScroll);
    } else {
        // Where a redirect ended up is the one place added, so that Back goes to before it.
        const QByteArray addressBytes = address.toUtf8();
        const QByteArray fragmentBytes = fetchFragment.toUtf8();
        history->visit(asStr(addressBytes), asStr(fragmentBytes));
        view->scrollToFragment(fetchFragment);
    }
    // A local page has read its local images, and does not reach for others.
    if (!url.isLocalFile()) {
        view->fetchImages(address);
    }
    updateNavigation();
}

void MainWindow::goBack()
{
    stopArrivalOnly();
    if (isWaitingForPage()) {
        // What was being read never counted as a place, so Back only drops it.
        stopFetch();
        return;
    }
    history->save_scroll(view->scrollY());
    if (history->go_back()) {
        showCurrentEntry();
    }
}

void MainWindow::goForward()
{
    stopFetch();
    history->save_scroll(view->scrollY());
    if (history->go_forward()) {
        showCurrentEntry();
    }
}

void MainWindow::showCurrentEntry()
{
    const QString address = asQString(history->current_address());
    const QByteArray bytes = address.toUtf8();
    const QString path = asQString(ron_browser::local_path(asStr(bytes)));
    const int scroll = history->current_scroll();
    fetchScroll = scroll;
    if (address != loadedAddress && path.isEmpty()) {
        startFetch(address, QString());
        return;
    }
    const LocalLoad load = address == loadedAddress ? LocalLoad::Shown : loadLocalFile(address, path);
    if (load == LocalLoad::Waiting) {
        return;
    }
    if (load == LocalLoad::Shown) {
        view->setScrollY(scroll);
    }
    updateNavigation();
}

void MainWindow::reload()
{
    if (loadedAddress.isEmpty()) {
        return;
    }
    stopFetch();
    const int scroll = view->scrollY();
    const QByteArray bytes = loadedAddress.toUtf8();
    const QString path = asQString(ron_browser::local_path(asStr(bytes)));
    fetchScroll = scroll;
    if (path.isEmpty()) {
        startFetch(loadedAddress, QString());
    } else if (loadLocalFile(loadedAddress, path) == LocalLoad::Shown) {
        view->setScrollY(scroll);
    }
}

void MainWindow::goToTyped()
{
    const QByteArray typed = addressBar->text().toUtf8();
    followTarget(ron_browser::resolve_address(asStr(typed)), QString());
}

void MainWindow::followLink(const QString &address)
{
    const QByteArray page = loadedAddress.toUtf8();
    const QByteArray href = address.toUtf8();
    followTarget(ron_browser::resolve_link(asStr(page), asStr(href)), loadedAddress);
}

void MainWindow::followTarget(const ron_browser::LinkTarget &target, const QString &askingPage)
{
    const QString place = shownAddress(asQString(target.place));
    switch (target.kind) {
    case ron_browser::LinkKind::SamePage:
        goTo(loadedAddress, asQString(target.fragment));
        break;
    case ron_browser::LinkKind::Page:
        goTo(asQString(target.place), asQString(target.fragment), askingPage);
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
    stopAction->setEnabled(isFetching());
    // While something is being read, or after that failed, what the user typed stays to be fixed.
    if (!isWaitingForPage() && pageStatus.isEmpty()) {
        return;
    }
    if (!isWaitingForPage()) {
        const QString fragment = asQString(history->current_fragment());
        addressBar->setText(fragment.isEmpty() ? loadedAddress : loadedAddress + u'#' + fragment);
    }
}

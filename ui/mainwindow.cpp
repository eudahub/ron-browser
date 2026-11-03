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
// A local page larger than this comes in pieces, so that it shows at once.
const size_t arrivingPageBytes = 1024 * 1024;
// How long the window stays with what the network gives, before it goes on with other things.
const qint64 fetchEventsMs = 15;
// What comes from the network goes to the page together this often, as each time takes work and a
// new drawing. On a very fast network it goes sooner, as a large amount at once would hold up the
// window: about 40 ms for each MB.
const qint64 feedEveryMs = 100;
const qsizetype feedEveryBytes = 512 * 1024;
// How often the title and the status bar follow a page that is arriving.
const qint64 statusEveryMs = 300;
// Looking for a place goes over all of a page, so it is not done at every step.
const qint64 placeTryMs = 300;

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
      addressBar(new QLineEdit(this)), fetchTimer(new QTimer(this)), saveTimer(new QTimer(this))
{
    setCentralWidget(view);
    statusBar();
    setWindowTitle(tr("ron-browser"));
    resize(1280, 900);

    auto *fileMenu = menuBar()->addMenu(tr("&File"));
    fileMenu->addAction(tr("&Open..."), QKeySequence::Open, this, &MainWindow::chooseFile);
    saveAction = fileMenu->addAction(tr("&Save Page with Scripts..."), QKeySequence::Save, this,
                                     &MainWindow::chooseSaveDirectory);
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
    saveTimer->setInterval(100);
    connect(saveTimer, &QTimer::timeout, this, &MainWindow::checkSave);

    connect(addressBar, &QLineEdit::returnPressed, this, &MainWindow::goToTyped);
    connect(view, &PageView::linkActivated, this, &MainWindow::followLink);
    connect(view, &PageView::linkHovered, this, &MainWindow::showLinkInStatus);
    connect(view, &PageView::backButtonPressed, this, &MainWindow::goBack);
    connect(view, &PageView::forwardButtonPressed, this, &MainWindow::goForward);
    connect(view, &PageView::arrivalProgress, this, [this](int percent) {
        if (view->hasIncomingPage()) {
            return;
        }
        pageStatus = tr("Loading %1%").arg(percent);
        statusBar()->showMessage(pageStatus);
        const QString title = view->pageTitle();
        if (!title.isEmpty()) {
            setWindowTitle(title);
        }
    });
    connect(view, &PageView::arrivalFinished, this, [this] {
        // Unseen so far, it says all this when it shows.
        if (view->hasIncomingPage()) {
            return;
        }
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

void MainWindow::chooseSaveDirectory()
{
    const QString directory = QFileDialog::getExistingDirectory(this, tr("Save page into"));
    if (!directory.isEmpty()) {
        savePageInto(directory);
    }
}

void MainWindow::savePageInto(const QString &directory)
{
    const QByteArray address = loadedAddress.toUtf8();
    if (loadedAddress.isEmpty() || !asQString(ron_browser::local_path(asStr(address))).isEmpty()) {
        statusBar()->showMessage(tr("Only a page from the network can be saved."));
        return;
    }
    const QByteArray parent = directory.toUtf8();
    pageSave = ron_browser::start_page_save(asStr(address), asStr(parent));
    saveTimer->start();
    saveAction->setEnabled(false);
    statusBar()->showMessage(tr("Saving %1").arg(shownAddress(loadedAddress)));
}

void MainWindow::checkSave()
{
    while (pageSave) {
        const ron_browser::SaveEvent event = (*pageSave)->poll();
        if (event.kind == ron_browser::SaveEventKind::Nothing) {
            return;
        }
        if (event.kind == ron_browser::SaveEventKind::Progress) {
            statusBar()->showMessage(tr("Saving: %1 of %2 files").arg(event.done).arg(event.known));
            continue;
        }
        if (event.kind == ron_browser::SaveEventKind::Finished) {
            statusBar()->showMessage(tr("Saved to %1: %2 files, %3 could not be read")
                                         .arg(asQString(event.directory))
                                         .arg(event.saved)
                                         .arg(event.failed));
        } else {
            statusBar()->showMessage(tr("Cannot save it: %1").arg(shownAddress(asQString(event.error))));
        }
        saveTimer->stop();
        pageSave.reset();
        saveAction->setEnabled(true);
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
    if (arrivalSeconds > 0 || decoded.utf8.size() > arrivingPageBytes) {
        view->startArriving(std::move(decoded.utf8), QFileInfo(path).absolutePath(), address, arrivalSeconds);
        fetchedAddress = address;
        fetchedStatus = status;
        fetchTimer->start();
        pageStatus = tr("Loading %1").arg(shownAddress(address));
        statusBar()->showMessage(pageStatus);
        updateNavigation();
        return LocalLoad::Waiting;
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
    // A place on the page that is arriving leaves the rest of it coming.
    const bool onArrivingPage = address == loadedAddress && !isWaitingForPage() && view->isArriving();
    if (!onArrivingPage) {
        stopFetch();
    }
    const QByteArray bytes = address.toUtf8();
    const QString path = asQString(ron_browser::local_path(asStr(bytes)));
    fetchFragment = fragment;
    fetchScroll.reset();
    if (address != loadedAddress) {
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
    placeWaiting = !view->scrollToFragment(fragment) && onArrivingPage;
    placeScroll = view->scrollY();
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
    placeWaiting = false;
    if (isWaitingForPage()) {
        fetchTimer->stop();
        pageFetch.reset();
        pageDecoder.reset();
        waitingBytes.clear();
        view->dropIncomingPage();
        pageStatus.clear();
        statusBar()->clearMessage();
        updateNavigation();
    } else if (view->isArriving()) {
        // Decides what the status says of it, as it ends here.
        if (pageDecoder) {
            feedWaitingBytes();
            noteFetchedEncoding();
        }
        fetchTimer->stop();
        pageFetch.reset();
        pageDecoder.reset();
        view->stopArriving();
        updateNavigation();
    }
}

bool MainWindow::isFetching() const
{
    return isWaitingForPage() || view->isArriving();
}

bool MainWindow::isWaitingForPage() const
{
    return view->hasIncomingPage() || (pageFetch.has_value() && !view->isArriving());
}

void MainWindow::setArrivalSeconds(int seconds)
{
    arrivalSeconds = seconds;
}

void MainWindow::checkFetch()
{
    const qint64 bytesBefore = fetchedBytes;
    takeFetchEvents();
    const bool hadIncoming = view->hasIncomingPage();
    if (hadIncoming && view->incomingPageIsReady()) {
        showFetchedPage();
    } else if (!hadIncoming && view->isArriving() && pageFetch && fetchedBytes != bytesBefore
               && sinceStatus.elapsed() >= statusEveryMs) {
        sinceStatus.start();
        const QString title = view->pageTitle();
        if (!title.isEmpty() && title != windowTitle()) {
            setWindowTitle(title);
        }
        pageStatus = tr("Loading: %1 KB").arg(fetchedBytes / 1024);
        statusBar()->showMessage(pageStatus);
    }
    if (placeWaiting) {
        goOnToPlace();
    }
    if (!isFetching()) {
        fetchTimer->stop();
    }
}

void MainWindow::takeFetchEvents()
{
    QElapsedTimer spent;
    spent.start();
    while (pageFetch && spent.elapsed() < fetchEventsMs && waitingBytes.size() < feedEveryBytes) {
        ron_browser::FetchEvent event = (*pageFetch)->poll();
        if (event.kind == ron_browser::FetchEventKind::Nothing) {
            break;
        }
        if (!takeFetchEvent(std::move(event))) {
            pageFetch.reset();
            pageDecoder.reset();
            waitingBytes.clear();
        }
    }
    if (!sinceFed.isValid() || sinceFed.elapsed() >= feedEveryMs || waitingBytes.size() >= feedEveryBytes) {
        feedWaitingBytes();
    }
}

bool MainWindow::takeFetchEvent(ron_browser::FetchEvent event)
{
    switch (event.kind) {
    case ron_browser::FetchEventKind::Started: {
        const QString mime = asQString(event.mime);
        const QString address = asQString(event.address);
        if (!mime.isEmpty() && mime != QStringLiteral("text/html") && mime != QStringLiteral("application/xhtml+xml")) {
            pageStatus.clear();
            statusBar()->showMessage(tr("Not a web page, but %1: %2").arg(mime.left(100), shownAddress(address)));
            pageFetch.reset();
            updateNavigation();
            return false;
        }
        pageDecoder = ron_browser::new_stream_decoder(event.charset);
        waitingBytes.clear();
        sinceFed.invalidate();
        sinceStatus.start();
        fetchedBytes = 0;
        fetchedAddress = address;
        fetchedStatus.clear();
        view->beginFetchedArrival(address);
        return true;
    }
    case ron_browser::FetchEventKind::Data: {
        fetchedBytes += static_cast<qint64>(event.bytes.size());
        waitingBytes.append(reinterpret_cast<const char *>(event.bytes.data()), static_cast<qsizetype>(event.bytes.size()));
        return true;
    }
    case ron_browser::FetchEventKind::End:
        endFetchedPage(QString());
        return false;
    case ron_browser::FetchEventKind::Failed: {
        const QString error = shownAddress(asQString(event.error));
        if (view->hasIncomingPage() || !view->isArriving()) {
            // Nothing of it shows yet, so what was there stays.
            pageFetch.reset();
            pageDecoder.reset();
            view->dropIncomingPage();
            pageStatus.clear();
            statusBar()->showMessage(tr("Cannot open it: %1").arg(error));
            updateNavigation();
        } else {
            endFetchedPage(error);
        }
        return false;
    }
    default:
        return true;
    }
}

void MainWindow::feedWaitingBytes()
{
    if (!waitingBytes.isEmpty() && pageDecoder) {
        const rust::String text = (*pageDecoder)->push(
            {reinterpret_cast<const std::uint8_t *>(waitingBytes.constData()), static_cast<size_t>(waitingBytes.size())});
        waitingBytes.clear();
        view->feedFetched(text);
        sinceFed.start();
    }
}

void MainWindow::endFetchedPage(const QString &problem)
{
    feedWaitingBytes();
    const rust::String text = (*pageDecoder)->finish();
    view->feedFetched(text);
    noteFetchedEncoding();
    if (!problem.isEmpty()) {
        fetchedStatus = tr("Only part of it came: %1").arg(problem);
    }
    pageFetch.reset();
    view->endFetchedArrival();
}

void MainWindow::noteFetchedEncoding()
{
    const ron_browser::ChosenEncoding chosen = (*pageDecoder)->encoding();
    const QString note = (*pageDecoder)->replaced() ? replacedBytesNote() : QString();
    const QString security = QUrl(fetchedAddress).scheme() == QStringLiteral("https") ? QString() : tr("Not secure. ");
    fetchedStatus = security + tr("Encoding: %1 (%2%3)").arg(asQString(chosen.encoding), encodingSourceName(chosen), note);
}

void MainWindow::showFetchedPage()
{
    view->showIncomingPage();
    const QString address = fetchedAddress;
    loadedAddress = address;
    const QString title = view->pageTitle();
    const QUrl url(address);
    setWindowTitle(!title.isEmpty() ? title : url.isLocalFile() ? url.fileName() : url.host());
    pageStatus = view->isArriving() ? tr("Loading %1").arg(shownAddress(address)) : fetchedStatus;
    statusBar()->showMessage(pageStatus);
    if (fetchScroll) {
        view->setScrollY(*fetchScroll);
        placeWaiting = view->isArriving() && view->scrollY() != *fetchScroll;
    } else {
        // Where a redirect ended up is the one place added, so that Back goes to before it.
        const QByteArray addressBytes = address.toUtf8();
        const QByteArray fragmentBytes = fetchFragment.toUtf8();
        history->visit(asStr(addressBytes), asStr(fragmentBytes));
        placeWaiting = !view->scrollToFragment(fetchFragment) && view->isArriving();
    }
    placeScroll = view->scrollY();
    sincePlaceTried.start();
    // A local page has read its local images, and does not reach for others.
    if (!url.isLocalFile()) {
        view->fetchImages(address);
    }
    updateNavigation();
}

void MainWindow::goOnToPlace()
{
    if (view->scrollY() != placeScroll) {
        placeWaiting = false;
        return;
    }
    const bool arriving = view->isArriving();
    if (fetchScroll) {
        view->setScrollY(*fetchScroll);
        placeWaiting = arriving && view->scrollY() != *fetchScroll;
    } else if (!arriving || sincePlaceTried.elapsed() >= placeTryMs) {
        sincePlaceTried.start();
        placeWaiting = !view->scrollToFragment(fetchFragment) && arriving;
    }
    placeScroll = view->scrollY();
}

void MainWindow::goBack()
{
    if (isWaitingForPage()) {
        // What was being read never counted as a place, so Back only drops it.
        stopFetch();
        return;
    }
    stopFetch();
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

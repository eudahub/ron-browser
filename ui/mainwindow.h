#ifndef RON_BROWSER_MAINWINDOW_H
#define RON_BROWSER_MAINWINDOW_H

#include <QElapsedTimer>
#include <QMainWindow>
#include <QString>

#include <rust/cxx.h>

#include <optional>
#include <string>

class PageView;
class QAction;
class QLineEdit;
class QTimer;

namespace ron_browser {
struct PageFetch;
struct FetchEvent;
struct StreamDecoder;
struct History;
struct LinkTarget;
}

class MainWindow : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(std::string forcedEncoding);
    ~MainWindow() override;

    // Opens a file or an address, as if the user had typed it.
    void openPage(const QString &address);
    bool isLoading() const;
    // For trying out pages that are still arriving: local files then take this long to come.
    void setArrivalSeconds(int seconds);
    // For a screenshot, which must not wait for the delay after a resize.
    void layOutNow();
    PageView *pageView() const;

private:
    void chooseFile();
    enum class LocalLoad { Failed, Shown, Waiting };
    // Reads a local file into the view. When it cannot be read, says so and leaves what is shown.
    // A large page, or one with style sheets on the network, goes on through fetchTimer.
    LocalLoad loadLocalFile(const QString &address, const QString &path);
    // Goes to a new place, as the user asked. Only this adds to the history. `askingPage` is the
    // address of the page whose link it is, or empty.
    void goTo(const QString &address, const QString &fragment, const QString &askingPage = QString());
    void goToTyped();
    // Starts reading from the network.
    void startFetch(const QString &address, const QString &askingPage);
    void checkFetch();
    // Passes on what the network has given. Stops for a moment now and then for the window to go on.
    void takeFetchEvents();
    // Tells whether the page goes on being read.
    bool takeFetchEvent(ron_browser::FetchEvent event);
    // Ends a page from the network that is there, with what has come of it.
    void endFetchedPage(const QString &problem);
    void feedWaitingBytes();
    void noteFetchedEncoding();
    void showFetchedPage();
    // Goes to the place asked for, once a page has come as far.
    void goOnToPlace();
    void stopFetch();
    bool isFetching() const;
    // Something is on the way that has not replaced what is shown yet.
    bool isWaitingForPage() const;
    void goBack();
    void goForward();
    // Shows the place the history is now at.
    void showCurrentEntry();
    void reload();
    void followLink(const QString &address);
    void followTarget(const ron_browser::LinkTarget &target, const QString &askingPage);
    void showLinkInStatus(const QString &address);
    // Shows where the history is, and what can be done from there.
    void updateNavigation();

    PageView *view;
    std::string forcedEncoding;
    rust::Box<ron_browser::History> history;
    QAction *backAction;
    QAction *forwardAction;
    QAction *reloadAction;
    QAction *stopAction;
    QLineEdit *addressBar;
    QTimer *fetchTimer;
    QString loadedAddress;
    int arrivalSeconds = 0;
    std::optional<rust::Box<ron_browser::PageFetch>> pageFetch;
    std::optional<rust::Box<ron_browser::StreamDecoder>> pageDecoder;
    qint64 fetchedBytes = 0;
    // What has come but not gone to the page yet: each time costs, so it goes together.
    QByteArray waitingBytes;
    QElapsedTimer sinceFed;
    QElapsedTimer sinceStatus;
    // What to do when the page being read arrives: go to a place, adding to the history, or
    // return to how far down the page was, without adding.
    QString fetchFragment;
    std::optional<int> fetchScroll;
    // The place has not come yet, in a page that is arriving. Where the view was left tells
    // whether the user has moved it since, which gives up on the place.
    bool placeWaiting = false;
    int placeScroll = 0;
    QElapsedTimer sincePlaceTried;
    // About the page that is coming, until it is there.
    QString fetchedAddress;
    QString fetchedStatus;
    // What the status bar says about the page, when it is not showing a link.
    QString pageStatus;
};

#endif

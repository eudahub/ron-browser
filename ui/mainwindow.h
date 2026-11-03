#ifndef RON_BROWSER_MAINWINDOW_H
#define RON_BROWSER_MAINWINDOW_H

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
    // For a screenshot, which must not wait for the delay after a resize.
    void layOutNow();
    PageView *pageView() const;

private:
    void chooseFile();
    // Reads a local file into the view. When it cannot be read, says so and leaves what is shown.
    bool loadLocalFile(const QString &address, const QString &path);
    // Goes to a new place, as the user asked. Only this adds to the history. `askingPage` is the
    // address of the page whose link it is, or empty.
    void goTo(const QString &address, const QString &fragment, const QString &askingPage = QString());
    void goToTyped();
    // Starts reading from the network. What happens then depends on `arrival`.
    void startFetch(const QString &address, const QString &askingPage);
    void checkFetch();
    void stopFetch();
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
    std::optional<rust::Box<ron_browser::PageFetch>> pageFetch;
    // What to do when the page being read arrives: go to a place, adding to the history, or
    // return to how far down the page was, without adding.
    QString fetchFragment;
    std::optional<int> fetchScroll;
    // What the status bar says about the page, when it is not showing a link.
    QString pageStatus;
};

#endif

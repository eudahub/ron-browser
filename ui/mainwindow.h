#ifndef RON_BROWSER_MAINWINDOW_H
#define RON_BROWSER_MAINWINDOW_H

#include <QMainWindow>
#include <QString>

#include <rust/cxx.h>

#include <string>

class PageView;
class QAction;
class QLineEdit;

namespace ron_browser {
struct History;
struct LinkTarget;
}

class MainWindow : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(std::string forcedEncoding);
    ~MainWindow() override;

    void openPage(const QString &path);
    // For a screenshot, which must not wait for the delay after a resize.
    void layOutNow();
    PageView *pageView() const;

private:
    void chooseFile();
    // Reads what is at an address into the view. When it cannot be read, says so and leaves what
    // is shown.
    bool loadAddress(const QString &address);
    // Goes to a new place, as the user asked. Only this adds to the history.
    void goTo(const QString &address, const QString &fragment);
    void goToTyped();
    void goBack();
    void goForward();
    // Shows the place the history is now at.
    void showCurrentEntry();
    void reload();
    void followLink(const QString &address);
    void followTarget(const ron_browser::LinkTarget &target);
    void showLinkInStatus(const QString &address);
    // Shows where the history is, and what can be done from there.
    void updateNavigation();

    PageView *view;
    std::string forcedEncoding;
    rust::Box<ron_browser::History> history;
    QAction *backAction;
    QAction *forwardAction;
    QAction *reloadAction;
    QLineEdit *addressBar;
    QString loadedAddress;
    // What the status bar says about the page, when it is not showing a link.
    QString pageStatus;
};

#endif

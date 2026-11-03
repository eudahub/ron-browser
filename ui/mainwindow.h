#ifndef RON_BROWSER_MAINWINDOW_H
#define RON_BROWSER_MAINWINDOW_H

#include <QMainWindow>
#include <QString>

#include <rust/cxx.h>

#include <string>

class PageView;
class QAction;

namespace ron_browser {
struct History;
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
    // Reads a file into the view. When it cannot be read, says so and leaves what is shown.
    bool loadFile(const QString &path);
    // Goes to a new place, as the user asked. Only this adds to the history.
    void goTo(const QString &path, const QString &fragment);
    void goBack();
    void goForward();
    // Shows the place the history is now at.
    void showCurrentEntry();
    void reload();
    void followLink(const QString &address);
    void showLinkInStatus(const QString &address);
    void updateActions();

    PageView *view;
    std::string forcedEncoding;
    rust::Box<ron_browser::History> history;
    QAction *backAction;
    QAction *forwardAction;
    QAction *reloadAction;
    QString loadedPath;
    // What the status bar says about the page, when it is not showing a link.
    QString pageStatus;
};

#endif

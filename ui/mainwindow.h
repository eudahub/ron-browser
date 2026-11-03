#ifndef RON_BROWSER_MAINWINDOW_H
#define RON_BROWSER_MAINWINDOW_H

#include <QMainWindow>

#include <string>

class PageView;

class MainWindow : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(std::string forcedEncoding);

    void openPage(const QString &path);
    // For a screenshot, which must not wait for the delay after a resize.
    void layOutNow();

private:
    void chooseFile();

    PageView *view;
    std::string forcedEncoding;
};

#endif

#ifndef RON_BROWSER_MAINWINDOW_H
#define RON_BROWSER_MAINWINDOW_H

#include <QMainWindow>

#include <string>

class QPlainTextEdit;

class MainWindow : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(std::string forcedEncoding);

    void openPage(const QString &path);

private:
    void chooseFile();

    QPlainTextEdit *view;
    std::string forcedEncoding;
};

#endif

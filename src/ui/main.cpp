#include "ui/MainWindow.h"
#include "ui/Theme.h"

#include <QApplication>

int main(int argc, char** argv)
{
    QApplication app(argc, argv);
    app.setApplicationName("iris");
    app.setApplicationDisplayName("Project Iris");
    app.setOrganizationName("project-iris");
    app.setDesktopFileName("project-iris");
    iris::ui::applyDarkTheme(app);

    iris::ui::MainWindow window;
    window.show();

    const QStringList args = app.arguments();
    if (args.size() > 1)
        window.openPhoto(args.at(1));

    return app.exec();
}

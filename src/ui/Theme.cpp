#include "ui/Theme.h"

#include <QApplication>
#include <QPalette>
#include <QStyleFactory>

namespace iris::ui {

void applyDarkTheme(QApplication& app)
{
    app.setStyle(QStyleFactory::create("Fusion"));

    const QColor window(0x1e, 0x1f, 0x22);
    const QColor base(0x17, 0x18, 0x1a);
    const QColor text(0xd8, 0xd9, 0xdc);
    const QColor dimText(0x7d, 0x80, 0x86);
    const QColor accent(0x4c, 0x8d, 0xf6);

    QPalette p;
    p.setColor(QPalette::Window, window);
    p.setColor(QPalette::WindowText, text);
    p.setColor(QPalette::Base, base);
    p.setColor(QPalette::AlternateBase, window);
    p.setColor(QPalette::ToolTipBase, base);
    p.setColor(QPalette::ToolTipText, text);
    p.setColor(QPalette::Text, text);
    p.setColor(QPalette::Button, QColor(0x2a, 0x2b, 0x2f));
    p.setColor(QPalette::ButtonText, text);
    p.setColor(QPalette::BrightText, Qt::white);
    p.setColor(QPalette::Highlight, accent);
    p.setColor(QPalette::HighlightedText, Qt::white);
    p.setColor(QPalette::Link, accent);
    p.setColor(QPalette::PlaceholderText, dimText);
    p.setColor(QPalette::Disabled, QPalette::Text, dimText);
    p.setColor(QPalette::Disabled, QPalette::ButtonText, dimText);
    p.setColor(QPalette::Disabled, QPalette::WindowText, dimText);
    app.setPalette(p);

    app.setStyleSheet(R"(
        QToolBar#mainToolBar {
            background: #232428; border: none; border-bottom: 1px solid #111214;
            padding: 4px 8px; spacing: 4px;
        }
        QToolBar#mainToolBar QToolButton {
            padding: 5px 12px; border-radius: 4px; color: #d8d9dc;
        }
        QToolBar#mainToolBar QToolButton:hover { background: #33353a; }
        QToolBar#mainToolBar QToolButton:pressed { background: #3d4046; }
        QToolBar#mainToolBar QToolButton:disabled { color: #5c5f65; }
        QLabel#brandLabel { color: #f0f0f2; font-weight: 600; font-size: 14px; padding: 0 14px 0 4px; }

        QWidget#sidePanel { background: #232428; }
        QLabel#panelTitle {
            color: #9ea1a7; font-size: 11px; font-weight: 600; letter-spacing: 1px;
            padding: 10px 12px 6px 12px;
        }
        QLabel#folderLabel { color: #d8d9dc; padding: 0 12px 6px 12px; }
        QLabel#infoKey { color: #7d8086; }
        QLabel#infoValue { color: #d8d9dc; }

        QListWidget#photoList { background: #232428; border: none; outline: none; }
        QListWidget#photoList::item { padding: 5px 12px; border-radius: 0; }
        QListWidget#photoList::item:hover { background: #2c2e32; }
        QListWidget#photoList::item:selected { background: #34528a; color: white; }

        QSplitter::handle { background: #111214; }
        QScrollArea { border: none; background: #232428; }

        QLabel#sectionLabel { color: #c4c6cb; font-weight: 600; padding: 6px 12px 2px 12px; }
        QLabel#sliderLabel { color: #b4b6bb; }
        QDoubleSpinBox#sliderValue { background: transparent; color: #d8d9dc; border: none; padding: 0; }
        QDoubleSpinBox#sliderValue:focus { background: #17181a; }
        QPushButton#smallButton {
            background: #2c2e32; color: #c4c6cb; border: 1px solid #383a3f; border-radius: 3px;
            padding: 2px 8px; font-size: 11px;
        }
        QPushButton#smallButton:hover { background: #35373c; }
        QPushButton#smallButton:checked { background: #34528a; border-color: #4c8df6; color: white; }
        QPushButton#smallButton:disabled { color: #5c5f65; }

        QTabBar#hslTabs::tab {
            background: #2a2b2f; color: #9ea1a7; border: 1px solid #383a3f; padding: 3px 6px;
            font-size: 11px;
        }
        QTabBar#hslTabs::tab:first { border-top-left-radius: 3px; border-bottom-left-radius: 3px; }
        QTabBar#hslTabs::tab:last { border-top-right-radius: 3px; border-bottom-right-radius: 3px; }
        QTabBar#hslTabs::tab:selected { background: #34528a; color: white; border-color: #4c8df6; }
        QTabBar#hslTabs::tab:disabled { color: #5c5f65; }

        QSlider::groove:horizontal { height: 3px; background: #3a3c41; border-radius: 1px; }
        QSlider::handle:horizontal {
            background: #c9cbd0; width: 11px; height: 11px; margin: -4px 0; border-radius: 5px;
        }
        QSlider::handle:horizontal:hover { background: #ffffff; }
        QSlider::handle:horizontal:disabled { background: #55585e; }

        QStatusBar { background: #232428; border-top: 1px solid #111214; color: #9ea1a7; }
        QStatusBar::item { border: none; }
        QStatusBar QLabel { color: #9ea1a7; padding: 0 8px; }
    )");
}

} // namespace iris::ui

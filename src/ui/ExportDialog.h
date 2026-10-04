#pragma once

#include "export/Exporter.h"

#include <QDialog>

class QComboBox;
class QLabel;
class QLineEdit;
class QRadioButton;
class QSpinBox;

namespace iris::ui {

class ExportDialog : public QDialog {
    Q_OBJECT

public:
    // rawPath is used to suggest an output file next to the original.
    ExportDialog(const QString& rawPath, QWidget* parent = nullptr);

    ExportSettings settings() const;
    QString outputPath() const;

    void accept() override;

private:
    ExportFormat format() const;
    void updateControls();
    void browse();
    void saveDefaults() const;

    QString m_rawPath;
    QComboBox* m_format;
    QSpinBox* m_quality;
    QLabel* m_qualityLabel;
    QComboBox* m_bitDepth;
    QLabel* m_bitDepthLabel;
    QRadioButton* m_originalSize;
    QRadioButton* m_resize;
    QSpinBox* m_longEdge;
    QLineEdit* m_path;
};

} // namespace iris::ui

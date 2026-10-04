#pragma once

#include "core/PhotoMetadata.h"

#include <QWidget>

class QFormLayout;
class QLabel;

namespace iris::ui {

// Shows the shooting information of the current photo.
class InfoPanel : public QWidget {
    Q_OBJECT

public:
    explicit InfoPanel(QWidget* parent = nullptr);

    void setMetadata(const iris::PhotoMetadata& metadata, const QString& fileName);
    void clear();

private:
    QLabel* addRow(const QString& name);

    QFormLayout* m_form;
    QLabel* m_file;
    QLabel* m_camera;
    QLabel* m_lens;
    QLabel* m_exposure;
    QLabel* m_iso;
    QLabel* m_focal;
    QLabel* m_date;
    QLabel* m_size;
    QLabel* m_orientation;
};

} // namespace iris::ui

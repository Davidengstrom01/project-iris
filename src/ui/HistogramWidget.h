#pragma once

#include "rendering/Histogram.h"

#include <QWidget>

namespace iris::ui {

// RGB histogram of the photo as currently rendered.
class HistogramWidget : public QWidget {
    Q_OBJECT

public:
    explicit HistogramWidget(QWidget* parent = nullptr);

    void setHistogram(const iris::Histogram& histogram);
    void clear();
    bool hasData() const { return m_histogram.pixels > 0; }

    QSize sizeHint() const override { return {260, 96}; }

protected:
    void paintEvent(QPaintEvent* event) override;

private:
    Histogram m_histogram;
};

} // namespace iris::ui

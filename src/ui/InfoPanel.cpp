#include "ui/InfoPanel.h"

#include <QDateTime>
#include <QFormLayout>
#include <QLabel>
#include <QLocale>
#include <QVBoxLayout>

#include <cmath>

namespace iris::ui {

namespace {

const QString kUnknown = QStringLiteral("—");

QString formatShutter(float seconds)
{
    if (seconds <= 0)
        return {};
    if (seconds >= 0.3f)
        return QString::number(seconds, 'g', 2) + " s";
    return QString("1/%1 s").arg(std::lround(1.0 / seconds));
}

QString formatOrientation(int orientation)
{
    switch (orientation) {
    case 3: return QObject::tr("Rotated 180°");
    case 6: return QObject::tr("Rotated 90° CW");
    case 8: return QObject::tr("Rotated 90° CCW");
    default: return QObject::tr("Normal");
    }
}

void setValue(QLabel* label, const QString& text)
{
    label->setText(text.isEmpty() ? kUnknown : text);
}

} // namespace

InfoPanel::InfoPanel(QWidget* parent) : QWidget(parent)
{
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 12);
    layout->setSpacing(0);

    auto* title = new QLabel(tr("INFO"), this);
    title->setObjectName("panelTitle");
    layout->addWidget(title);

    m_form = new QFormLayout;
    m_form->setContentsMargins(12, 0, 12, 0);
    m_form->setHorizontalSpacing(12);
    m_form->setVerticalSpacing(6);
    m_form->setLabelAlignment(Qt::AlignLeft);
    layout->addLayout(m_form);

    m_file = addRow(tr("File"));
    m_camera = addRow(tr("Camera"));
    m_lens = addRow(tr("Lens"));
    m_exposure = addRow(tr("Exposure"));
    m_iso = addRow(tr("ISO"));
    m_focal = addRow(tr("Focal length"));
    m_date = addRow(tr("Date"));
    m_size = addRow(tr("Size"));
    m_orientation = addRow(tr("Orientation"));
    clear();
}

QLabel* InfoPanel::addRow(const QString& name)
{
    auto* key = new QLabel(name, this);
    key->setObjectName("infoKey");
    auto* value = new QLabel(this);
    value->setObjectName("infoValue");
    value->setWordWrap(true);
    value->setTextInteractionFlags(Qt::TextSelectableByMouse);
    m_form->addRow(key, value);
    return value;
}

void InfoPanel::setMetadata(const iris::PhotoMetadata& m, const QString& fileName)
{
    setValue(m_file, fileName);

    QString make = QString::fromStdString(m.make).trimmed();
    QString model = QString::fromStdString(m.model).trimmed();
    if (model.startsWith(make, Qt::CaseInsensitive))
        make.clear();
    setValue(m_camera, QStringList{make, model}.join(' ').trimmed());
    setValue(m_lens, QString::fromStdString(m.lens).trimmed());

    QStringList exposure;
    if (m.shutterSeconds > 0)
        exposure << formatShutter(m.shutterSeconds);
    if (m.aperture > 0)
        exposure << QString("f/%1").arg(m.aperture, 0, 'g', 2);
    setValue(m_exposure, exposure.join("  ·  "));
    setValue(m_iso, m.iso > 0 ? QString::number(std::lround(m.iso)) : QString());
    setValue(m_focal, m.focalLengthMm > 0 ? QString("%1 mm").arg(m.focalLengthMm, 0, 'g', 4) : QString());
    setValue(m_date, m.timestamp > 0
                         ? QLocale().toString(QDateTime::fromSecsSinceEpoch(m.timestamp), QLocale::ShortFormat)
                         : QString());
    setValue(m_size, m.width > 0 ? QString("%1 × %2  (%3 MP)")
                                       .arg(m.width)
                                       .arg(m.height)
                                       .arg(m.width * double(m.height) / 1e6, 0, 'f', 1)
                                 : QString());
    setValue(m_orientation, formatOrientation(m.orientation));
}

void InfoPanel::clear()
{
    for (QLabel* label : {m_file, m_camera, m_lens, m_exposure, m_iso, m_focal, m_date, m_size, m_orientation})
        label->setText(kUnknown);
}

} // namespace iris::ui

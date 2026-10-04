#include "ui/ExportDialog.h"

#include <QButtonGroup>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QPushButton>
#include <QRadioButton>
#include <QSettings>
#include <QSpinBox>
#include <QVBoxLayout>

namespace iris::ui {

namespace {

QString withExtension(const QString& path, ExportFormat format)
{
    const QFileInfo info(path);
    const QString ext = QString::fromLatin1(fileExtension(format));
    const QString suffix = info.suffix().toLower();
    const bool matches = suffix == ext || (format == ExportFormat::Jpeg && suffix == "jpeg") ||
                         (format == ExportFormat::Tiff && suffix == "tiff");
    if (matches)
        return path;
    const QString base = info.completeBaseName().isEmpty() ? info.fileName() : info.completeBaseName();
    return info.dir().filePath(base + "." + ext);
}

} // namespace

ExportDialog::ExportDialog(const QString& rawPath, QWidget* parent) : QDialog(parent), m_rawPath(rawPath)
{
    setWindowTitle(tr("Export"));
    setMinimumWidth(480);

    QSettings s;
    m_format = new QComboBox(this);
    m_format->addItem("JPEG", int(ExportFormat::Jpeg));
    m_format->addItem("PNG", int(ExportFormat::Png));
    m_format->addItem("TIFF", int(ExportFormat::Tiff));
    m_format->setCurrentIndex(m_format->findData(s.value("export/format", int(ExportFormat::Jpeg)).toInt()));

    m_quality = new QSpinBox(this);
    m_quality->setRange(1, 100);
    m_quality->setValue(s.value("export/quality", 92).toInt());
    m_qualityLabel = new QLabel(tr("Quality"), this);

    m_bitDepth = new QComboBox(this);
    m_bitDepth->addItem(tr("8 bits per channel"), 8);
    m_bitDepth->addItem(tr("16 bits per channel"), 16);
    m_bitDepth->setCurrentIndex(m_bitDepth->findData(s.value("export/bits", 8).toInt()));
    m_bitDepthLabel = new QLabel(tr("Bit depth"), this);

    m_originalSize = new QRadioButton(tr("Original resolution"), this);
    m_resize = new QRadioButton(tr("Long edge"), this);
    auto* sizeGroup = new QButtonGroup(this);
    sizeGroup->addButton(m_originalSize);
    sizeGroup->addButton(m_resize);
    m_longEdge = new QSpinBox(this);
    m_longEdge->setRange(64, 20000);
    m_longEdge->setSuffix(" px");
    m_longEdge->setValue(s.value("export/longEdge", 2048).toInt());
    (s.value("export/resize", false).toBool() ? m_resize : m_originalSize)->setChecked(true);

    auto* sizeRow = new QHBoxLayout;
    sizeRow->addWidget(m_originalSize);
    sizeRow->addSpacing(12);
    sizeRow->addWidget(m_resize);
    sizeRow->addWidget(m_longEdge);
    sizeRow->addStretch();

    const QFileInfo raw(rawPath);
    m_path = new QLineEdit(withExtension(raw.dir().filePath(raw.completeBaseName()), format()), this);
    auto* browseButton = new QPushButton(tr("Browse…"), this);
    auto* pathRow = new QHBoxLayout;
    pathRow->addWidget(m_path, 1);
    pathRow->addWidget(browseButton);

    auto* form = new QFormLayout;
    form->addRow(tr("Format"), m_format);
    form->addRow(m_qualityLabel, m_quality);
    form->addRow(m_bitDepthLabel, m_bitDepth);
    form->addRow(tr("Color space"), new QLabel("sRGB", this));
    form->addRow(tr("Size"), sizeRow);
    form->addRow(tr("File"), pathRow);

    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, this);
    buttons->button(QDialogButtonBox::Ok)->setText(tr("Export"));

    auto* layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addSpacing(8);
    layout->addWidget(buttons);

    connect(buttons, &QDialogButtonBox::accepted, this, &ExportDialog::accept);
    connect(buttons, &QDialogButtonBox::rejected, this, &ExportDialog::reject);
    connect(browseButton, &QPushButton::clicked, this, &ExportDialog::browse);
    connect(m_format, &QComboBox::currentIndexChanged, this, [this] {
        m_path->setText(withExtension(m_path->text(), format()));
        updateControls();
    });
    connect(m_resize, &QRadioButton::toggled, this, &ExportDialog::updateControls);
    updateControls();
}

ExportFormat ExportDialog::format() const
{
    return ExportFormat(m_format->currentData().toInt());
}

void ExportDialog::updateControls()
{
    const bool jpeg = format() == ExportFormat::Jpeg;
    m_quality->setVisible(jpeg);
    m_qualityLabel->setVisible(jpeg);
    m_bitDepth->setVisible(!jpeg);
    m_bitDepthLabel->setVisible(!jpeg);
    m_longEdge->setEnabled(m_resize->isChecked());
}

void ExportDialog::browse()
{
    const QString filter = format() == ExportFormat::Jpeg  ? tr("JPEG images (*.jpg *.jpeg)")
                           : format() == ExportFormat::Png ? tr("PNG images (*.png)")
                                                           : tr("TIFF images (*.tif *.tiff)");
    // Overwrite confirmation happens in accept().
    const QString path = QFileDialog::getSaveFileName(this, tr("Export As"), m_path->text(), filter, nullptr,
                                                      QFileDialog::DontConfirmOverwrite);
    if (!path.isEmpty())
        m_path->setText(withExtension(path, format()));
}

ExportSettings ExportDialog::settings() const
{
    ExportSettings settings;
    settings.format = format();
    settings.jpegQuality = m_quality->value();
    settings.bitsPerChannel = m_bitDepth->currentData().toInt();
    settings.longEdge = m_resize->isChecked() ? m_longEdge->value() : 0;
    return settings;
}

QString ExportDialog::outputPath() const
{
    // The extension always matches the export format, so the RAW file can never be the target.
    return QFileInfo(withExtension(m_path->text().trimmed(), format())).absoluteFilePath();
}

void ExportDialog::accept()
{
    const QString path = outputPath();
    if (m_path->text().trimmed().isEmpty() || QFileInfo(path) == QFileInfo(m_rawPath))
        return;
    if (!QFileInfo(path).dir().exists()) {
        QMessageBox::warning(this, tr("Export"), tr("The folder %1 does not exist.").arg(QFileInfo(path).absolutePath()));
        return;
    }
    if (QFileInfo::exists(path) &&
        QMessageBox::question(this, tr("Export"), tr("%1 already exists. Replace it?").arg(QFileInfo(path).fileName()),
                              QMessageBox::Yes | QMessageBox::No, QMessageBox::No) != QMessageBox::Yes)
        return;
    saveDefaults();
    QDialog::accept();
}

void ExportDialog::saveDefaults() const
{
    QSettings s;
    s.setValue("export/format", int(format()));
    s.setValue("export/quality", m_quality->value());
    s.setValue("export/bits", m_bitDepth->currentData().toInt());
    s.setValue("export/resize", m_resize->isChecked());
    s.setValue("export/longEdge", m_longEdge->value());
}

} // namespace iris::ui

#include "ui/SavePresetDialog.h"

#include "presets/PresetLibrary.h"

#include <QCheckBox>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSettings>
#include <QVBoxLayout>

namespace iris::ui {

SavePresetDialog::SavePresetDialog(const EditState& edits, const QStringList& folders, QWidget* parent)
    : QDialog(parent), m_edits(edits)
{
    setWindowTitle(tr("Save Preset"));
    setMinimumWidth(380);

    m_name = new QLineEdit(this);
    m_name->setPlaceholderText(tr("e.g. Warm Film"));
    m_folder = new QComboBox(this);
    m_folder->setEditable(true);
    m_folder->addItems(folders);
    m_folder->setCurrentText(QSettings().value("presets/lastFolder", PresetLibrary::kDefaultUserFolder).toString());

    auto* form = new QFormLayout;
    form->addRow(tr("Name"), m_name);
    form->addRow(tr("Folder"), m_folder);

    // Presets never include crop, rotation, masks or other photo-specific settings.
    const QList<QPair<QString, std::vector<std::string>>> groups = {
        {tr("Exposure"), {"exposure"}},       {tr("Contrast"), {"contrast"}},
        {tr("Highlights"), {"highlights"}},   {tr("Shadows"), {"shadows"}},
        {tr("Whites"), {"whites"}},           {tr("Blacks"), {"blacks"}},
        {tr("White Balance"), {"temperature", "tint"}},
        {tr("Vibrance"), {"vibrance"}},       {tr("Saturation"), {"saturation"}},
        {tr("Tone Curve"), {kToneCurveKey}},
        {tr("Color (HSL)"), {kHslKey}},
    };
    QSettings settings;
    auto* include = new QGroupBox(tr("Include"), this);
    auto* grid = new QGridLayout(include);
    for (int i = 0; i < groups.size(); ++i) {
        auto* box = new QCheckBox(groups[i].first, include);
        // White balance is photo-specific, so it is off unless the user chose it last time.
        const QString key = "presets/include/" + QString::fromStdString(groups[i].second.front());
        box->setChecked(settings.value(key, groups[i].second.front() != "temperature").toBool());
        grid->addWidget(box, i / 2, i % 2);
        m_groups.append({box, groups[i].second});
    }
    auto* all = new QPushButton(tr("Check All"), include);
    auto* none = new QPushButton(tr("Check None"), include);
    auto* buttonRow = new QHBoxLayout;
    buttonRow->addWidget(all);
    buttonRow->addWidget(none);
    buttonRow->addStretch();
    grid->addLayout(buttonRow, (groups.size() + 1) / 2, 0, 1, 2);
    connect(all, &QPushButton::clicked, this, [this] {
        for (auto& [box, keys] : m_groups)
            box->setChecked(true);
    });
    connect(none, &QPushButton::clicked, this, [this] {
        for (auto& [box, keys] : m_groups)
            box->setChecked(false);
    });

    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel, this);
    m_ok = buttons->button(QDialogButtonBox::Save);
    connect(buttons, &QDialogButtonBox::accepted, this, [this] {
        QSettings s;
        s.setValue("presets/lastFolder", folder());
        for (auto& [box, keys] : m_groups)
            s.setValue("presets/include/" + QString::fromStdString(keys.front()), box->isChecked());
        accept();
    });
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    connect(m_name, &QLineEdit::textChanged, this, &SavePresetDialog::updateOkButton);
    for (auto& [box, keys] : m_groups)
        connect(box, &QCheckBox::toggled, this, &SavePresetDialog::updateOkButton);

    auto* layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addWidget(include);
    layout->addWidget(buttons);
    updateOkButton();
}

void SavePresetDialog::updateOkButton()
{
    bool anyChecked = false;
    for (auto& [box, keys] : m_groups)
        anyChecked = anyChecked || box->isChecked();
    m_ok->setEnabled(!m_name->text().trimmed().isEmpty() && anyChecked);
}

Preset SavePresetDialog::preset() const
{
    std::vector<std::string> keys;
    for (const auto& [box, groupKeys] : m_groups)
        if (box->isChecked())
            keys.insert(keys.end(), groupKeys.begin(), groupKeys.end());
    return presetFromEdits(m_name->text().trimmed().toStdString(), m_edits, keys);
}

QString SavePresetDialog::folder() const
{
    return m_folder->currentText().trimmed();
}

} // namespace iris::ui

#include "ui/EditDocument.h"

#include "persistence/Sidecar.h"

namespace iris::ui {

namespace {

constexpr qint64 kMergeWindowMs = 1500;

} // namespace

EditDocument::EditDocument(QObject* parent) : QObject(parent) {}

QString EditDocument::load(const QString& rawPath, const WhiteBalance& asShot)
{
    m_rawPath = rawPath;
    m_defaults = defaultEditState(asShot);
    const SidecarResult result = readSidecar(rawPath, m_defaults);
    const EditState initial = result.edits.value_or(m_defaults);
    m_saved = result.edits ? initial : m_defaults;
    m_history.reset(initial);
    m_loaded = true;
    m_lastEdit.invalidate();
    emit editsReplaced(edits());
    emit editsChanged(edits());
    emit stateChanged();
    return result.error;
}

void EditDocument::clear()
{
    m_loaded = false;
    m_rawPath.clear();
    m_history.reset({});
    emit stateChanged();
}

QString EditDocument::sidecarPath() const
{
    return m_rawPath.isEmpty() ? QString() : sidecarPathFor(m_rawPath);
}

void EditDocument::edit(const EditState& state, const QString& label, bool mergeable)
{
    if (!m_loaded || state == edits())
        return;
    const bool merge = mergeable && m_lastEdit.isValid() && m_lastEdit.elapsed() < kMergeWindowMs &&
                       m_history.latestLabel() == label.toStdString();
    m_history.record(state, label.toStdString(), merge);
    if (mergeable)
        m_lastEdit.start();
    else
        m_lastEdit.invalidate();
    emit editsChanged(edits());
    emit stateChanged();
}

void EditDocument::undo()
{
    if (!canUndo())
        return;
    m_history.undo();
    m_lastEdit.invalidate();
    emit editsReplaced(edits());
    emit editsChanged(edits());
    emit stateChanged();
}

void EditDocument::redo()
{
    if (!canRedo())
        return;
    m_history.redo();
    m_lastEdit.invalidate();
    emit editsReplaced(edits());
    emit editsChanged(edits());
    emit stateChanged();
}

QString EditDocument::save()
{
    if (!m_loaded)
        return {};
    const QString error = writeSidecar(sidecarPath(), m_rawPath, edits());
    if (error.isEmpty()) {
        m_saved = edits();
        emit stateChanged();
    }
    return error;
}

QString EditDocument::saveAs(const QString& path)
{
    if (!m_loaded)
        return {};
    const QString error = writeSidecar(path, m_rawPath, edits());
    if (error.isEmpty() && path == sidecarPath()) {
        m_saved = edits();
        emit stateChanged();
    }
    return error;
}

} // namespace iris::ui

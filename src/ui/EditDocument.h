#pragma once

#include "core/EditHistory.h"
#include "core/EditState.h"

#include <QElapsedTimer>
#include <QObject>
#include <QString>

namespace iris::ui {

// The edits of the open photo: current state, undo history, and whether they differ from
// what is saved in the photo's sidecar.
class EditDocument : public QObject {
    Q_OBJECT

public:
    explicit EditDocument(QObject* parent = nullptr);

    // Starts editing a photo: loads its sidecar if there is one, otherwise the defaults.
    // Returns a warning if a sidecar exists but could not be read.
    QString load(const QString& rawPath, const iris::WhiteBalance& asShot);
    void clear();

    bool isLoaded() const { return m_loaded; }
    QString rawPath() const { return m_rawPath; }
    const EditState& edits() const { return m_history.current(); }
    const EditState& defaults() const { return m_defaults; }
    bool isDirty() const { return m_loaded && edits() != m_saved; }
    QString sidecarPath() const;

    // Records a change. Mergeable changes with the same label that follow each other
    // quickly (a slider drag) become one undo step.
    void edit(const iris::EditState& state, const QString& label, bool mergeable = false);
    // Between these, mergeable edits with the same label become one undo step however long
    // the gesture takes (a brush stroke), and never merge with an earlier step.
    void beginGesture();
    void endGesture();

    bool canUndo() const { return m_history.canUndo(); }
    bool canRedo() const { return m_history.canRedo(); }
    QString undoLabel() const { return QString::fromStdString(m_history.undoLabel()); }
    QString redoLabel() const { return QString::fromStdString(m_history.redoLabel()); }
    void undo();
    void redo();

    // Each returns an error message, or an empty string on success.
    QString save();
    QString saveAs(const QString& sidecarPath);

signals:
    // The edits changed for any reason other than a direct user edit (load, undo, redo).
    void editsReplaced(const iris::EditState& edits);
    // The edits changed in any way.
    void editsChanged(const iris::EditState& edits);
    void stateChanged(); // undo/redo availability or dirty flag changed

private:
    bool m_loaded = false;
    QString m_rawPath;
    EditState m_defaults;
    EditState m_saved;
    EditHistory m_history;
    QElapsedTimer m_lastEdit;
    bool m_inGesture = false;
    bool m_gestureRecorded = false; // the current gesture has its undo step
};

} // namespace iris::ui

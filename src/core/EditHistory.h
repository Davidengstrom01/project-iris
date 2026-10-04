#pragma once

#include "core/EditState.h"

#include <cstddef>
#include <string>
#include <vector>

namespace iris {

// Undo/redo as a list of immutable EditState snapshots.
class EditHistory {
public:
    explicit EditHistory(const EditState& initial = {}) { reset(initial); }

    void reset(const EditState& initial);

    const EditState& current() const { return m_steps[m_index].state; }

    // Records a new state as one undoable step, discarding any redo steps. With
    // mergeWithLatest the latest step is updated instead, so a whole slider drag is one
    // step. A step that ends up equal to the state before it is dropped.
    void record(const EditState& state, const std::string& label, bool mergeWithLatest = false);

    bool canUndo() const { return m_index > 0; }
    bool canRedo() const { return m_index + 1 < m_steps.size(); }
    // Label of the step that undo() / redo() would revert / re-apply.
    const std::string& undoLabel() const;
    const std::string& redoLabel() const;
    // Label of the latest step (for merging decisions).
    const std::string& latestLabel() const { return m_steps[m_index].label; }

    const EditState& undo();
    const EditState& redo();

    static constexpr std::size_t kMaxSteps = 500;

private:
    struct Step {
        EditState state;
        std::string label;
    };
    std::vector<Step> m_steps;
    std::size_t m_index = 0;
};

} // namespace iris

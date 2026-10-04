#include "core/EditHistory.h"

namespace iris {

namespace {
const std::string kEmpty;
}

void EditHistory::reset(const EditState& initial)
{
    m_steps.clear();
    m_steps.push_back({initial, {}});
    m_index = 0;
}

void EditHistory::record(const EditState& state, const std::string& label, bool mergeWithLatest)
{
    if (state == current())
        return;
    m_steps.resize(m_index + 1); // drop redo steps

    if (mergeWithLatest && m_index > 0 && m_steps[m_index].label == label) {
        m_steps[m_index].state = state;
        if (m_steps[m_index - 1].state == state) { // the drag ended where it started
            m_steps.pop_back();
            --m_index;
        }
        return;
    }

    m_steps.push_back({state, label});
    ++m_index;
    if (m_steps.size() > kMaxSteps) {
        m_steps.erase(m_steps.begin());
        --m_index;
    }
}

const std::string& EditHistory::undoLabel() const
{
    return canUndo() ? m_steps[m_index].label : kEmpty;
}

const std::string& EditHistory::redoLabel() const
{
    return canRedo() ? m_steps[m_index + 1].label : kEmpty;
}

const EditState& EditHistory::undo()
{
    if (canUndo())
        --m_index;
    return current();
}

const EditState& EditHistory::redo()
{
    if (canRedo())
        ++m_index;
    return current();
}

} // namespace iris

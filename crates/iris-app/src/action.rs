//! What the user asked for this frame. Panels and shortcuts push actions; the app applies
//! them after drawing, so every change goes through one place (undo, rendering, saving).

use std::path::PathBuf;

use iris_core::{BasicAdjustments, HslAdjustments, Mask, MaskType, ToneCurve, WhiteBalance};
use iris_persist::{Preset, PresetEntry};

pub enum Action {
    // Develop panel.
    /// A slider moved (one undo step per drag).
    EditBasic(BasicAdjustments),
    ResetBasic,
    SetWhiteBalance(WhiteBalance),
    AutoWhiteBalance,
    SetEyedropper(bool),

    // Tone curve and colour.
    /// A curve point was dragged.
    EditCurve(ToneCurve),
    /// A curve preset or reset.
    ChooseCurve(ToneCurve),
    EditHsl(HslAdjustments, String),
    ResetHsl,

    // Masks.
    AddMask(MaskType),
    DeleteMask(usize),
    SelectMask(Option<usize>),
    /// A mask panel slider or checkbox changed the selected mask.
    EditMask(Mask, String),
    /// The mask tool, brush or overlay changed.
    MaskToolChanged,

    // Presets.
    ApplyPreset(Preset),
    ShowSavePreset,
    RenamePreset(PresetEntry),
    MovePreset(PresetEntry),
    DeletePreset(PresetEntry),

    // Commands.
    OpenPhoto(PathBuf),
    ShowOpen,
    Save,
    SaveAs,
    ShowExport,
    Undo,
    Redo,
    Reset,
    ToggleBefore,
    ToggleSplit,
    Fit,
    ActualSize,
    ZoomIn,
    ZoomOut,
    ToggleMasks,
    ToggleOverlay,
    Escape,
    BrushSize(i32),
    TogglePanels,
    Quit,
}

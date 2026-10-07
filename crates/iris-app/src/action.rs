//! What the user asked for this frame. Panels and shortcuts push actions; the app applies
//! them after drawing, so every change goes through one place (undo, rendering, saving).

use std::path::PathBuf;

use iris_core::{BasicAdjustments, HslAdjustments, Mask, MaskType, ToneCurve, WhiteBalance};
use iris_persist::{Preset, PresetEntry};

pub enum Action {
    /// A tool tab was clicked.
    SelectTool(crate::panels::ToolTab),

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
    /// A sharpening or noise reduction slider moved.
    EditDetail(iris_core::Detail),
    ResetDetail,

    // Masks.
    AddMask(MaskType),
    DeleteMask(usize),
    SelectMask(Option<usize>),
    /// A mask panel slider or checkbox changed the selected mask.
    EditMask(Mask, String),
    /// The mask tool, brush or overlay changed.
    MaskToolChanged,

    // Crop and rotation.
    /// Start or finish cropping on the photo.
    ToggleCrop,
    SetCropAspect(crate::crop_tool::Aspect),
    /// Portrait <-> landscape.
    SwapCropAspect,
    /// Turn by 90°: clockwise when true.
    RotateQuarter(bool),
    /// The straighten slider moved (degrees).
    Straighten(f32),
    ResetCrop,

    // Retouch.
    /// S / H: the Retouch tool in clone or heal mode.
    Retouch(iris_core::RetouchMode),
    ClearRetouch,
    RemoveLastRetouch,
    /// B: paint on a mask.
    MaskBrush,

    // Presets.
    ApplyPreset(Preset),
    ShowSavePreset,
    RenamePreset(PresetEntry),
    MovePreset(PresetEntry),
    DeletePreset(PresetEntry),

    // Favorites.
    /// Mark or unmark a photo as a favorite.
    ToggleFavorite(PathBuf),
    /// F: mark or unmark the open photo.
    ToggleFavoriteCurrent,
    ExportFavorites,
    /// Ask for a folder, then move the favorites there.
    MoveFavorites,

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
    /// Enter: finish cropping.
    Confirm,
    BrushSize(i32),
    TogglePanels,
    /// Arrow keys: the previous (-1) or next (+1) photo in the library.
    StepPhoto(isize),
    Quit,
}

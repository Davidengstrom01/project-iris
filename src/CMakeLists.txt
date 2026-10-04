# Engine libraries (no UI dependencies). Dependency direction:
#   ui / cli  ->  export       ->  rendering  ->  core
#   ui / cli  ->  raw          ->  core
#   ui / cli  ->  persistence  ->  core        (sidecars, QtCore JSON)
#   ui        ->  presets      ->  persistence ->  core

add_library(iris_core STATIC
    core/Image.h core/PhotoMetadata.h
    core/EditState.h core/EditState.cpp
    core/Curve.h core/Curve.cpp
    core/EditHistory.h core/EditHistory.cpp
    core/ColorScience.h core/ColorScience.cpp)
target_include_directories(iris_core PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})

add_library(iris_raw STATIC
    raw/RawDecoder.h raw/RawDecoder.cpp)
target_link_libraries(iris_raw PUBLIC iris_core PRIVATE PkgConfig::LIBRAW OpenMP::OpenMP_CXX)

add_library(iris_rendering STATIC
    rendering/ColorTransform.h rendering/ColorTransform.cpp
    rendering/Resample.h rendering/Resample.cpp
    rendering/Tone.h rendering/Tone.cpp
    rendering/Histogram.h rendering/Histogram.cpp
    rendering/WhiteBalanceTools.h rendering/WhiteBalanceTools.cpp
    rendering/Pipeline.h rendering/Pipeline.cpp)
target_link_libraries(iris_rendering PUBLIC iris_core PRIVATE PkgConfig::LCMS2 OpenMP::OpenMP_CXX)

add_library(iris_export STATIC
    export/Exporter.h export/Exporter.cpp)
target_link_libraries(iris_export PUBLIC iris_rendering Qt6::Gui)

add_library(iris_persistence STATIC
    persistence/EditStateJson.h persistence/EditStateJson.cpp
    persistence/Sidecar.h persistence/Sidecar.cpp)
target_link_libraries(iris_persistence PUBLIC iris_core Qt6::Core)

add_library(iris_presets STATIC
    presets/Preset.h presets/Preset.cpp
    presets/PresetLibrary.h presets/PresetLibrary.cpp)
target_link_libraries(iris_presets PUBLIC iris_core iris_persistence Qt6::Core)
# Built-in presets are plain preset files, bundled as Qt resources under :/presets/builtin.
file(GLOB IRIS_BUILTIN_PRESETS CONFIGURE_DEPENDS ${PROJECT_SOURCE_DIR}/presets/builtin/*.json)
foreach(preset ${IRIS_BUILTIN_PRESETS})
    get_filename_component(name ${preset} NAME)
    set_source_files_properties(${preset} PROPERTIES QT_RESOURCE_ALIAS presets/builtin/${name})
endforeach()
qt_add_resources(iris_presets builtin_presets PREFIX / FILES ${IRIS_BUILTIN_PRESETS})

# Desktop application (UI code lives in a library so tests can drive it)
qt_add_library(iris_ui STATIC
    ui/Theme.h ui/Theme.cpp
    ui/MainWindow.h ui/MainWindow.cpp
    ui/ImageView.h ui/ImageView.cpp
    ui/PhotoSession.h ui/PhotoSession.cpp
    ui/LibraryPanel.h ui/LibraryPanel.cpp
    ui/InfoPanel.h ui/InfoPanel.cpp
    ui/AdjustmentSlider.h ui/AdjustmentSlider.cpp
    ui/DevelopPanel.h ui/DevelopPanel.cpp
    ui/ExportDialog.h ui/ExportDialog.cpp
    ui/EditDocument.h ui/EditDocument.cpp
    ui/PresetPanel.h ui/PresetPanel.cpp
    ui/SavePresetDialog.h ui/SavePresetDialog.cpp
    ui/HistogramWidget.h ui/HistogramWidget.cpp
    ui/CurveEditor.h ui/CurveEditor.cpp
    ui/ToneCurvePanel.h ui/ToneCurvePanel.cpp)
target_link_libraries(iris_ui PUBLIC iris_raw iris_export iris_persistence iris_presets Qt6::Widgets Qt6::Concurrent)

qt_add_executable(iris ui/main.cpp)
target_link_libraries(iris PRIVATE iris_ui)

# Headless command-line renderer (proves the engine is UI-independent)
add_executable(iris-cli cli/main.cpp)
target_link_libraries(iris-cli PRIVATE iris_raw iris_export iris_persistence Qt6::Core)

install(TARGETS iris iris-cli RUNTIME DESTINATION bin)

# Engine libraries (no UI dependencies). Dependency direction:
#   ui / cli  ->  export  ->  rendering  ->  core
#   ui / cli  ->  raw     ->  core

add_library(iris_core INTERFACE)
target_include_directories(iris_core INTERFACE ${CMAKE_CURRENT_SOURCE_DIR})

add_library(iris_raw STATIC
    raw/RawDecoder.h raw/RawDecoder.cpp)
target_link_libraries(iris_raw PUBLIC iris_core PRIVATE PkgConfig::LIBRAW OpenMP::OpenMP_CXX)

add_library(iris_rendering STATIC
    rendering/ColorTransform.h rendering/ColorTransform.cpp
    rendering/Resample.h rendering/Resample.cpp
    rendering/Pipeline.h rendering/Pipeline.cpp)
target_link_libraries(iris_rendering PUBLIC iris_core PRIVATE PkgConfig::LCMS2 OpenMP::OpenMP_CXX)

add_library(iris_export STATIC
    export/Exporter.h export/Exporter.cpp)
target_link_libraries(iris_export PUBLIC iris_rendering Qt6::Gui)

# Desktop application (UI code lives in a library so tests can drive it)
qt_add_library(iris_ui STATIC
    ui/Theme.h ui/Theme.cpp
    ui/MainWindow.h ui/MainWindow.cpp
    ui/ImageView.h ui/ImageView.cpp
    ui/PhotoSession.h ui/PhotoSession.cpp
    ui/LibraryPanel.h ui/LibraryPanel.cpp
    ui/InfoPanel.h ui/InfoPanel.cpp
    ui/ExportDialog.h ui/ExportDialog.cpp)
target_link_libraries(iris_ui PUBLIC iris_raw iris_export Qt6::Widgets Qt6::Concurrent)

qt_add_executable(iris ui/main.cpp)
target_link_libraries(iris PRIVATE iris_ui)

# Headless command-line renderer (proves the engine is UI-independent)
add_executable(iris-cli cli/main.cpp)
target_link_libraries(iris-cli PRIVATE iris_raw iris_export Qt6::Core)

install(TARGETS iris iris-cli RUNTIME DESTINATION bin)

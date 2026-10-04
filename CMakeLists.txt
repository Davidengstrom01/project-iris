cmake_minimum_required(VERSION 3.21)
project(ProjectIris VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "Build type" FORCE)
endif()

find_package(Qt6 6.4 REQUIRED COMPONENTS Core Gui Widgets Concurrent)
find_package(PkgConfig REQUIRED)
find_package(OpenMP REQUIRED)
pkg_check_modules(LIBRAW REQUIRED IMPORTED_TARGET libraw_r)
pkg_check_modules(LCMS2 REQUIRED IMPORTED_TARGET lcms2)

qt_standard_project_setup()

add_subdirectory(src)

option(IRIS_BUILD_TESTS "Build the test suite" ON)
if(IRIS_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

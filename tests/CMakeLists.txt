find_package(Qt6 REQUIRED COMPONENTS Test)

qt_add_executable(test_engine test_engine.cpp)
target_link_libraries(test_engine PRIVATE iris_export Qt6::Test)
add_test(NAME engine COMMAND test_engine)
set_tests_properties(engine PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

# Needs a real RAW file: set IRIS_TEST_RAW=/path/to/photo.ARW (skipped otherwise).
qt_add_executable(test_ui_smoke test_ui_smoke.cpp)
target_link_libraries(test_ui_smoke PRIVATE iris_ui Qt6::Test)
add_test(NAME ui_smoke COMMAND test_ui_smoke)
set_tests_properties(ui_smoke PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

qt_add_executable(test_editing test_editing.cpp)
target_link_libraries(test_editing PRIVATE iris_persistence iris_presets Qt6::Test)
add_test(NAME editing COMMAND test_editing)

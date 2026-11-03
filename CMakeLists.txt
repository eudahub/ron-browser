cmake_minimum_required(VERSION 3.21)
project(ron-browser LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release)
endif()

find_package(Qt6 REQUIRED COMPONENTS Widgets)
find_package(cpg 1.2 REQUIRED)
find_program(CARGO cargo REQUIRED)

# The Rust part is a static library. Cargo decides itself whether anything changed.
set(CARGO_TARGET_DIR "${CMAKE_BINARY_DIR}/cargo")
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(CARGO_PROFILE_FLAG "")
    set(CARGO_PROFILE_DIR "debug")
else()
    set(CARGO_PROFILE_FLAG "--release")
    set(CARGO_PROFILE_DIR "release")
endif()
set(CORE_LIBRARY "${CARGO_TARGET_DIR}/${CARGO_PROFILE_DIR}/libron_browser_bridge.a")
add_custom_target(core_build ALL
    COMMAND "${CARGO}" build ${CARGO_PROFILE_FLAG}
            --manifest-path "${CMAKE_SOURCE_DIR}/bridge/Cargo.toml"
            --target-dir "${CARGO_TARGET_DIR}"
    BYPRODUCTS "${CORE_LIBRARY}"
    COMMENT "Building the Rust part"
    USES_TERMINAL)

add_library(core STATIC IMPORTED)
set_target_properties(core PROPERTIES IMPORTED_LOCATION "${CORE_LIBRARY}")
add_dependencies(core core_build)

set(CXXBRIDGE_INCLUDES
    "${CARGO_TARGET_DIR}/cxxbridge"
    "${CMAKE_SOURCE_DIR}/bridge/include")

add_executable(ron-browser
    ui/main.cpp
    ui/mainwindow.cpp
    ui/mainwindow.h
    ui/pageloader.cpp
    ui/pageloader.h
    ui/pageview.cpp
    ui/pageview.h
    ui/textmeasure.cpp
    ui/textmeasure.h)
target_include_directories(ron-browser PRIVATE ${CXXBRIDGE_INCLUDES})
target_compile_definitions(ron-browser PRIVATE
    CPG_LANGUAGES_FILE="${CPG_LANGUAGES_FILE}"
    CPG_MODELS_DIR="${CPG_MODELS_DIR}")
target_link_libraries(ron-browser PRIVATE core Qt6::Widgets cpg::cpg ${CMAKE_DL_LIBS})
# The window icon, also when the icon is not installed (:/ron-browser.svg).
qt_add_resources(ron-browser icons
    PREFIX "/"
    BASE "${CMAKE_SOURCE_DIR}/res"
    FILES "${CMAKE_SOURCE_DIR}/res/ron-browser.svg")

# Installation: cmake --install build [--prefix DIR], by default to /usr/local.
include(GNUInstallDirs)
install(TARGETS ron-browser RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
install(FILES res/ron-browser.desktop DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)
install(FILES res/ron-browser.svg
        DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/scalable/apps)
# Refresh the icon and desktop file caches, so that a running desktop (e.g. GNOME on Wayland,
# whose shell cannot be restarted without logging out) shows the icon at once. Skipped for
# packaging (DESTDIR) and when the tools are missing.
install(CODE "
    if(\"\$ENV{DESTDIR}\" STREQUAL \"\")
        set(data \"\${CMAKE_INSTALL_PREFIX}/${CMAKE_INSTALL_DATADIR}\")
        find_program(gtk_update_icon_cache gtk-update-icon-cache)
        find_program(update_desktop_database update-desktop-database)
        if(gtk_update_icon_cache)
            message(STATUS \"Updating the icon cache of \${data}/icons/hicolor\")
            execute_process(COMMAND \"\${gtk_update_icon_cache}\" -f -t
                            \"\${data}/icons/hicolor\" OUTPUT_QUIET ERROR_QUIET)
        endif()
        if(update_desktop_database)
            execute_process(COMMAND \"\${update_desktop_database}\"
                            \"\${data}/applications\" OUTPUT_QUIET ERROR_QUIET)
        endif()
    endif()
")

# Runs without a display: ctest --test-dir build --output-on-failure
option(BUILD_TESTING "Build the tests of the window" ON)
if(BUILD_TESTING)
    enable_testing()
    find_package(Qt6 REQUIRED COMPONENTS Test)
    add_executable(pageview_test
        tests/pageview_test.cpp
        ui/pageview.cpp
        ui/pageview.h
        ui/textmeasure.cpp
        ui/textmeasure.h)
    target_include_directories(pageview_test PRIVATE ui ${CXXBRIDGE_INCLUDES})
    target_link_libraries(pageview_test PRIVATE core Qt6::Widgets Qt6::Test ${CMAKE_DL_LIBS})
    add_test(NAME pageview_test COMMAND pageview_test)
    set_tests_properties(pageview_test PROPERTIES
        ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
        TIMEOUT 60)
endif()

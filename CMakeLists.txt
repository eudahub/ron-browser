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
set(CORE_LIBRARY "${CARGO_TARGET_DIR}/${CARGO_PROFILE_DIR}/libron_browser_core.a")
add_custom_target(core_build ALL
    COMMAND "${CARGO}" build ${CARGO_PROFILE_FLAG}
            --manifest-path "${CMAKE_SOURCE_DIR}/Cargo.toml"
            --target-dir "${CARGO_TARGET_DIR}"
    BYPRODUCTS "${CORE_LIBRARY}"
    COMMENT "Building the Rust part"
    USES_TERMINAL)

add_library(core STATIC IMPORTED)
set_target_properties(core PROPERTIES IMPORTED_LOCATION "${CORE_LIBRARY}")
add_dependencies(core core_build)

add_executable(ron-browser
    ui/main.cpp
    ui/mainwindow.cpp
    ui/mainwindow.h
    ui/pageloader.cpp
    ui/pageloader.h)
target_include_directories(ron-browser PRIVATE "${CARGO_TARGET_DIR}/cxxbridge")
target_compile_definitions(ron-browser PRIVATE
    CPG_LANGUAGES_FILE="${CPG_LANGUAGES_FILE}"
    CPG_MODELS_DIR="${CPG_MODELS_DIR}")
target_link_libraries(ron-browser PRIVATE core Qt6::Widgets cpg::cpg ${CMAKE_DL_LIBS})

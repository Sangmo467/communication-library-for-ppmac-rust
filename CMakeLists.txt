cmake_minimum_required(VERSION 3.20)
project(pmac_rust VERSION 2.0.0 LANGUAGES C CXX)
include(GNUInstallDirs)
if(NOT CMAKE_CONFIGURATION_TYPES AND NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "Build configuration" FORCE)
endif()

find_program(CARGO_EXECUTABLE cargo REQUIRED HINTS "$ENV{USERPROFILE}/.cargo/bin" "$ENV{HOME}/.cargo/bin")
set(PMAC_RUST_TARGET "" CACHE STRING "Rust target triple; empty uses the host target")
set(PMAC_CARGO_TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target" CACHE PATH "Cargo artifact directory")
set(PMAC_CARGO_ARGS --locked --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml" --target-dir "${PMAC_CARGO_TARGET_DIR}")
set(PMAC_CARGO_DIR "${PMAC_CARGO_TARGET_DIR}")
if(PMAC_RUST_TARGET)
    list(APPEND PMAC_CARGO_ARGS --target "${PMAC_RUST_TARGET}")
    string(APPEND PMAC_CARGO_DIR "/${PMAC_RUST_TARGET}")
    if(PMAC_RUST_TARGET MATCHES "^(i686|armv7)-" AND NOT CMAKE_SIZEOF_VOID_P EQUAL 4)
        message(FATAL_ERROR "32-bit Rust target requires a 32-bit C/C++ compiler")
    elseif(PMAC_RUST_TARGET MATCHES "^(x86_64|aarch64)-" AND NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
        message(FATAL_ERROR "64-bit Rust target requires a 64-bit C/C++ compiler")
    endif()
endif()

add_custom_target(pmac_native ALL
    COMMAND "${CARGO_EXECUTABLE}" build -p pmac-ffi ${PMAC_CARGO_ARGS} $<$<NOT:$<CONFIG:Debug>>:--release>
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
    COMMENT "Building pmac Rust C ABI"
    COMMAND_EXPAND_LISTS VERBATIM)

install(FILES include/pmac.h DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})
install(DIRECTORY bindings/cpp/ DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}/pmac FILES_MATCHING PATTERN "*.hpp")
if(WIN32)
    install(FILES "${PMAC_CARGO_DIR}/$<IF:$<CONFIG:Debug>,debug,release>/pmac.dll" DESTINATION ${CMAKE_INSTALL_BINDIR})
    install(FILES "${PMAC_CARGO_DIR}/$<IF:$<CONFIG:Debug>,debug,release>/pmac.dll.lib" DESTINATION ${CMAKE_INSTALL_LIBDIR})
else()
    install(FILES "${PMAC_CARGO_DIR}/$<IF:$<CONFIG:Debug>,debug,release>/libpmac.so" DESTINATION ${CMAKE_INSTALL_LIBDIR})
endif()

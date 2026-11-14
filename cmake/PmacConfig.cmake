# 供二进制 SDK 使用；调用方只需 find_package(Pmac CONFIG REQUIRED)。
get_filename_component(_pmac_root "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
if(NOT TARGET Pmac::pmac)
    file(READ "${_pmac_root}/manifest.json" _pmac_manifest)
    string(JSON _pmac_bits GET "${_pmac_manifest}" nativeIdentity pointerBits)
    string(JSON _pmac_target GET "${_pmac_manifest}" target)
    math(EXPR _pmac_process_bits "${CMAKE_SIZEOF_VOID_P} * 8")
    if(NOT _pmac_process_bits EQUAL _pmac_bits)
        message(FATAL_ERROR "SDK is ${_pmac_bits}-bit; compiler is ${_pmac_process_bits}-bit. Select a matching compiler/SDK.")
    endif()
    if((WIN32 AND NOT _pmac_target MATCHES "windows") OR (NOT WIN32 AND NOT _pmac_target MATCHES "linux"))
        message(FATAL_ERROR "SDK platform ${_pmac_target} does not match this compiler")
    endif()
    add_library(Pmac::pmac SHARED IMPORTED)
    set_target_properties(Pmac::pmac PROPERTIES
        INTERFACE_INCLUDE_DIRECTORIES "${_pmac_root}/include;${_pmac_root}/bindings/cpp")
    if(WIN32)
        set_target_properties(Pmac::pmac PROPERTIES
            IMPORTED_LOCATION "${_pmac_root}/native/pmac.dll"
            IMPORTED_IMPLIB "${_pmac_root}/native/pmac.dll.lib")
    else()
        set_target_properties(Pmac::pmac PROPERTIES IMPORTED_LOCATION "${_pmac_root}/native/libpmac.so")
    endif()
endif()

# cmake-rs passes /MT as a flag, but CMake >= 3.15 appends its own /MD after it.
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded")

# Building Qt with Cargo

Rust supports building C++ source code using the CC crate. Qt is implemented using C++.
Can we just build the Qt sources directly?

    % clang++ qglobal.cpp
    qglobal.cpp:41:10: fatal error: 'qplatformdefs.h' file not found
    #include "qplatformdefs.h"
         ^~~~~~~~~~~~~~~~~
    1 error generated.

Not so easy; at the wery least we need to provide configuration header files.

## Getting started

The ambition is that including Qt should be as easy as adding e.g.

    qtcore = "6.2"

to Cargo.toml; however currently some manual setup is required.

1. Check out the Qt source code (currently qtbase only)

    git submodule init

2. Create a build directory, which will be shared between all
   modules for a given build:

    qt-6.2-build

3. Set environment variables pointing to the source and build dir

    export QT_CARGO_SRC = /path/to/qt-6.2

    export QT_CARGO_BUILD = /path/to/qt-6.2-build

4. Build Qt:

   cargo build

   Or, open one of the sub-crates and build that crate directly.

## Crates

(Tentative)

Build system implementation

    qt-cargo-base (no host tools support)
    qt-cargo      (with host tools support)

Qt host tools crates (moc, rcc, uic, etc)

    qtcore-host-tools-src
    qtcore-host-tools-sys

Qt library source crates

    qtcore-src
    qtgui-src
    qtdeclarative-src

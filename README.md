# Building Qt with Cargo

Rust supports building C++ source code using the CC crate. Can we just build the Qt
sources directly?

    % clang++ qglobal.cpp
    qglobal.cpp:41:10: fatal error: 'qplatformdefs.h' file not found
    #include "qplatformdefs.h"
         ^~~~~~~~~~~~~~~~~
    1 error generated.

Not so easy; at the very least we need to provide configuration header files.

## Getting started (for developers of this crate)

1. Clone the rust-vendored-qt repository:

    git clone https://github.com/msorvig/rust-vendored-qt

2. Fetch Qt source code:

    git submodule update --init --recursive

3. Run the test suite:

    cargo test

The tests write qt-configure and build artifacts to $TEMP. Cargo writes
to to the default target/ directroy when building, as usual.

This project is organizes as a workspace, where the qt-cargo-base crate contains
the the majority of the implementation.

## Getting started (for users of this crate)

TODO! (but ideally as easy as adding e.g. "qtcore-src = 6.2" to Cargo.toml)

## Crates

(Tentative)

Build system implementation

    qt-cargo-base (no host tools support)
    qt-cargo      (with host tools support)

Qt host tools crates (moc, rcc, uic, etc)

    qtcore-host-tools

Qt library source crates

    qtcore-src
    qtgui-src
    qtdeclarative-src

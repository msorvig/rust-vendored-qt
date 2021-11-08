//! A library for build scripts to compile Qt source code
//!
//! This library is intended to be used as a `build-dependencies` entry in
//! `Cargo.toml`:
//!
//! ```toml
//! [build-dependencies]
//! qt_cargo = "0.1"
//! ```
//!
//! The purpose of this crate is to provide the utility functions necessary to
//! compile a Qt module code into a static archive which is then linked into a Rust crate.
//!
//! # Licensing
//!
//! This crate is licensed as "MIT OR Apache-2.0" and can be distributed according
//! to the terms of either license. However, this does not change the
//! licensing of Qt itself; you also need to abide by Qt's license if you distribute
//! the Qt libraries in source or binary form. That license will typically be either
//! the LGPL/GPL or the Qt Commercial license.
//!

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

mod configure;
pub mod sources;
pub mod util;

/// Configures the given cc::Build object for building Qt. qt_build_path can optionally
/// spesificy where build output files should be placed; if not specified then the OUT_DIR
/// environment varibale must be set (this will be the case when calling from a build.rs script).
///
/// Returns the path where Qt config headers should be placed.
///
/// This function currently hardcodes "x86_64-unknown-linux" as the build target.
pub fn configure_for_qt_build<P>(builder: &mut cc::Build, qt_build_path: Option<P>) -> PathBuf
where
    P: AsRef<Path>,
{
    // This function can be called either from a build.rs script or in a standalone
    // context, for example from a test function. In the first case Cargo sets environment
    // variables like OUT_DIR, HOST, and TARGET for us, but in the second case
    // they have to be provided here.
    let out_dir_env = std::env::var("OUT_DIR");
    if let Err(_) = out_dir_env {
        builder
            .host("x86_64-unknown-linux") // ### FIXME make configurable
            .target("x86_64-unknown-linux")
            .opt_level(0);

        // The CC crate defaults to 4 parallel compile tasks, increase
        // to the number of CPU cores.
        std::env::set_var("NUM_JOBS", num_cpus::get().to_string());
    }

    // Build output location. There are two inputs:
    //  - OUT_DIR: Set by Cargo if this function is called from build.rs
    //  - qt_build_path: Set by the user code, for example a test.
    // And two outputs:
    //  - cc out_dir, where cargo writes build artifacts
    //  - Qt configure output dir, where this crate writes Qt config files
    //
    // Mix the inputs to the outputs such that:
    //  - OUT_DIR is used when set to support the build.rs use case
    //  - qt_build_path is used when set to support e.g. auto testing
    //  - Both are used if both are set. This enables sharing Qt configure output
    //    across several module builds.
    let qt_config_out_dir = match out_dir_env {
        Ok(var) => PathBuf::from_str(&var).expect("OUT_DIR is not a valid path"),
        Err(_) => {
            let qt_config_dir =
                qt_build_path.expect("build_dir must be provided if not called from build.rs");
            builder.out_dir(&qt_config_dir);
            qt_config_dir.as_ref().to_path_buf()
        }
    };

    builder.cpp(true).flag("-std=c++17");

    qt_config_out_dir
}

/// Writes the default (linux) Qt configuration
pub fn write_default_qt_configuration<P, Q>(
    builder: &mut cc::Build,
    destination_path: P,
    qt_source_path: Q,
) where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let config_headers_path = destination_path.as_ref().join("qt_config_headers");
    let mut qt_configuration = configure::QtConfiguration::new();
    configure::set_default_configuration(&mut qt_configuration);
    configure::write_configuration(
        &qt_configuration,
        &config_headers_path,
        Some(&qt_source_path),
    );
    builder.include(&config_headers_path);
    builder.include(&config_headers_path.join("QtCore"));
}

/// Writes forwarding headers for QtCore
pub fn write_qtcore_forwarding_headers<P, Q>(
    builder: &mut cc::Build,
    destination_path: P,
    headers_search_path: Q,
) where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let forwarding_headers_path = destination_path.as_ref().join("qt_forwarding_headers");
    let forwarding_headers_dest = forwarding_headers_path.join("QtCore"); // FIXME
    configure::write_all_forwarding_headers(headers_search_path, &forwarding_headers_dest);
    builder.include(&forwarding_headers_path);
    builder.include(&forwarding_headers_dest);
}

/// Configures the build for the linux target; writes Qt QtCore configuration files and forwarding heders;
/// configures the builder with appropirate options. qt_source_path is the path to a (top-level) Qt checkout;
/// optionally destination_path can be set to specify where Qt configuration files should be written. The builder
/// writes build artifacts to the location pointed to by the OUT_DIR environment variable (typically set by Cargo).
/// If OUT_DIR is not set then the builder is configured to use destination_path.
pub fn configure_qtcore_for_linux<P, Q>(
    builder: &mut cc::Build,
    destination_path: Option<P>,
    qt_source_path: Q,
) where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let qt_config_path = configure_for_qt_build(builder, destination_path);
    write_default_qt_configuration(builder, &qt_config_path, &qt_source_path);
    write_qtcore_forwarding_headers(
        builder,
        qt_config_path,
        &qt_source_path.as_ref().join("qtbase/src/corelib"),
    );
}

pub fn add_path_prefixed_files<P, Q>(builder: &mut cc::Build, path: P, files: Q)
where
    P: AsRef<Path>,
    Q: IntoIterator,
    Q::Item: AsRef<Path>,
{
    let prefixed_files = files.into_iter().map(|e| path.as_ref().join(e.as_ref()));
    builder.files(prefixed_files);
}

mod features;

#[cfg(test)]
mod qt_cargo_base_tests {
    use crate::*;

    fn qt_build_temp_dir() -> tempdir::TempDir {
        // Set up a temp dir for build artifacts
        tempdir::TempDir::new("qt-cargo-base-test").expect("unable to create temp dir")
    }

    #[test]
    fn build_qglobal() {
        let qt_source = util::qt_src_path();
        let temp = qt_build_temp_dir();
        let qt_build = temp.path();
        //std::mem::forget(temp); // leak build config in /temp/ for inspection

        let mut builder = cc::Build::new();
        configure_qtcore_for_linux(&mut builder, Some(&qt_build), &qt_source);
        builder.file(qt_source.join("qtbase/src/corelib/global/qglobal.cpp"));

        builder.compile("qglobal"); // No panic -> test pass
    }

    #[test]
    fn build_moc() {
        let qt_source = util::qt_src_path();
        let temp = qt_build_temp_dir();
        let qt_build = temp.path();

        let mut builder = cc::Build::new();
        configure_qtcore_for_linux(&mut builder, Some(&qt_build), &qt_source);
        add_path_prefixed_files(
            &mut builder,
            qt_source.join(crate::sources::MOC_PATH),
            crate::sources::MOC_SOURCES,
        );

        builder.include(qt_source.join("qtbase/src/3rdparty/tinycbor/src/"));
        builder.include(qt_source.join("qtbase/src/tools/shared"));

        builder.compile("moc"); // No panic -> test pass
    }

    #[test]
    fn build_bootstrap_library() {
        let qt_source = util::qt_src_path();
        let temp = qt_build_temp_dir();
        let qt_build = temp.path();

        let mut builder = cc::Build::new();

        configure_qtcore_for_linux(&mut builder, Some(&qt_build), &qt_source);
        add_path_prefixed_files(
            &mut builder,
            qt_source.join(crate::sources::BOOTSTRAP_PATH),
            crate::sources::BOOTSTRAP_SOURCES,
        );

        builder.define("HAVE_CONFIG_H", None);
        builder.define("QT_VERSION_MAJOR", "6");
        builder.define("QT_VERSION_MINOR", "2");
        builder.define("QT_VERSION_PATCH", "0");
        builder.define("QT_VERSION_STR", "\"6.2.0\"");
        builder.define("QT_USE_QSTRINGBUILDER", None);
        builder.define("QT_BOOTSTRAPPED", None);
        builder.define("QT_NO_CAST_FROM_ASCII", None);
        builder.define("QT_NO_CAST_TO_ASCII", None);
        builder.define("QT_NO_FOREACH", None);

        builder.include(qt_source.join("qtbase/src/3rdparty/tinycbor/src/"));

        builder.compile("bootstrap"); // No panic -> test pass
    }
}

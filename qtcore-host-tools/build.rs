use std::path::PathBuf;

extern crate cc;

fn main() {
    let qt_source = qt_cargo_base::util::qt_src_path();

    let mut builder = cc::Build::new();
    let no_path: Option<PathBuf> = None;
    qt_cargo_base::configure_qtcore_for_linux(&mut builder, no_path, &qt_source);

    // Add moc files
    qt_cargo_base::add_path_prefixed_files(
        &mut builder,
        qt_source.join(qt_cargo_base::sources::MOC_PATH),
        qt_cargo_base::sources::MOC_SOURCES,
    );
    builder.include(qt_source.join("qtbase/src/3rdparty/tinycbor/src/"));
    builder.include(qt_source.join("qtbase/src/tools/shared"));
    builder.define("main", "hiddenmocmain"); // build.rs provides main(), hide the one in moc.cpp

    // Add bootstrap library files
    qt_cargo_base::add_path_prefixed_files(
        &mut builder,
        qt_source.join(qt_cargo_base::sources::BOOTSTRAP_PATH),
        qt_cargo_base::sources::BOOTSTRAP_SOURCES,
    );
    qt_cargo_base::add_path_prefixed_files(
        &mut builder,
        qt_source.join(qt_cargo_base::sources::BOOTSTRAP_PATH),
        qt_cargo_base::sources::BOOTSTRAP_SOURCES_UNIX,
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

    builder.compile("qtcore_host_tools");

    // Note: This goes last! We are providing the dependencies for
    // qtcore_host_tools (built above), and the "-l pcre2-16" must
    // appear after the "-l static=qtcore_host_tools" on the rustc
    // compiler line.
    system_deps::Config::new().probe().unwrap();
}

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
//! # Examples
//!
//! Use the `QtModuleBuilder` struct to compile `qglobal.cpp`:
//!
//! ```no_run
//! fn main() {
//!     qt_cargo::QtModuleBuilder::new()
//!         .source_path("path/to/qt")
//!         .build_path("/tmp/throwawaybuild")
//!         .name("QtCore")
//!         .module_features(vec![("foo", true)])
//!         .module_source_path("qtbase/src/corelib")
//!         .source_file("qglobal.cpp")
//!         .compile();
//! }
//! ```
//!

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};
use walkdir::WalkDir;

mod configure;

#[derive(Clone, Debug)]
pub struct QtModuleBuilder {
    module_name: String,
    qt_source_path: PathBuf,
    qt_build_path: Option<PathBuf>,
    module_source_path_: PathBuf,
    module_headers: Vec<PathBuf>,
    module_sources: Vec<PathBuf>,

    global_features_: Vec<(String, bool)>,
    global_private_features_: Vec<(String, bool)>,
    global_defines_: Vec<(String, String)>,
    module_features_: Vec<(String, bool)>,
    module_private_features_: Vec<(String, bool)>,
    module_defines_: Vec<(String, String)>,
    module_private_defines_: Vec<(String, String)>,
    qplatformdefs_: Option<PathBuf>,
    build: cc::Build,
}

impl QtModuleBuilder {
    /// Constructs a new QtModuleBuilder, taking the path to the module source
    /// code as the argument.
    pub fn new() -> QtModuleBuilder {
        let mut module_builder = QtModuleBuilder {
            module_name: "".into(),
            qt_source_path: ".".into(),
            qt_build_path: None,
            module_source_path_: ".".into(),
            module_headers: Vec::new(),
            module_sources: Vec::new(),
            global_features_: Vec::new(),
            global_private_features_: Vec::new(),
            global_defines_: Vec::new(),
            module_features_: Vec::new(),
            module_private_features_: Vec::new(),
            module_defines_: Vec::new(),
            module_private_defines_: Vec::new(),
            qplatformdefs_: Option::None,
            build: cc::Build::new(),
        };

        // Set Qt default features and defines, but not any module defines
        // since these depend on the module in question.
        module_builder.global_defines(features::global_defines());
        module_builder.global_features(features::global_features());
        module_builder.global_private_features(features::global_private_features());
        module_builder
    }

    /// Sets the module name.
    pub fn name(&mut self, name: &str) -> &mut QtModuleBuilder {
        self.module_name = name.into();
        self
    }

    /// Sets the path to Qt source code. The path will typically point to
    /// a directory with a checkout of Qt repositories (qtbase, qtdeclarative, etc).
    pub fn source_path<P: AsRef<Path>>(&mut self, path: P) -> &mut QtModuleBuilder {
        self.qt_source_path = path.as_ref().into();
        self
    }

    /// Sets path to the module's source code, e.g. "qtbase/src/corelib". The
    /// path is relative to the source path set with source_path().
    pub fn module_source_path<P: AsRef<Path>>(&mut self, path: P) -> &mut QtModuleBuilder {
        self.module_source_path_ = path.as_ref().into();
        self
    }

    /// Sets the path where QtModuleBuilder should store Qt-specific build artifacts,
    /// such as configuration and forwarding headers.
    pub fn build_path<P: AsRef<Path>>(&mut self, path: P) -> &mut QtModuleBuilder {
        self.qt_build_path = Some(path.as_ref().into());
        self
    }

    fn resolved_module_path(&self) -> PathBuf {
        self.qt_source_path.join(&self.module_source_path_)
    }

    fn glob_files<P: AsRef<Path>>(
        path: P,
        file_ext: &'_ OsStr,
    ) -> impl Iterator<Item = PathBuf> + '_ {
        WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(move |e| e.path().extension() == Some(file_ext))
            .map(|e| e.path().to_owned())
    }

    /// Glob headers (.h) recursively from the given dir, relative to the module root
    pub fn glob_headers<P: AsRef<Path>>(&mut self, dir: P) -> &mut QtModuleBuilder {
        let resolved_dir = self.resolved_module_path().join(dir);
        let ext = OsStr::new("h");
        let files = QtModuleBuilder::glob_files(resolved_dir, ext);
        self.module_headers.extend(files);
        self
    }

    /// Glob sources (.cpp) recursively from the given dir, relative to the module root
    pub fn glob_sources<P: AsRef<Path>>(&mut self, dir: P) -> &mut QtModuleBuilder {
        let resolved_dir = self.resolved_module_path().join(dir);
        let ext = OsStr::new("cpp");
        let files = QtModuleBuilder::glob_files(resolved_dir, ext);
        self.module_sources.extend(files);
        self
    }

    /// Add a single source file to the module build
    pub fn source_file<P: AsRef<Path>>(&mut self, path: P) -> &mut QtModuleBuilder {
        self.module_sources
            .push(self.resolved_module_path().join(path.as_ref().to_owned()));
        self
    }

    /// Adds multiple source files
    pub fn sources<'a, P, V>(&mut self, path: P, files: V) -> &mut QtModuleBuilder
    where
        P: AsRef<Path>,
        V: IntoIterator<Item = &'a str>,
    {
        let prefix = self.resolved_module_path().join(path.as_ref());
        self.module_sources
            .extend(files.into_iter().map(|f| prefix.join(f)));
        self
    }

    /// Writes global and module configuration files to the given path. This includes the
    /// qplatformdefs.h header, as well as qconfig.h and qconfig_private.h.
    fn create_config_headears<P>(&mut self, path: P)
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let qtcore_path = path.join("QtCore");
        let qtcore_private_path = qtcore_path.join("private");
        let module_path = path.join(&self.module_name);
        let module_private_path = &module_path.join("private");
        std::fs::create_dir_all(&qtcore_private_path)
            .expect("Unable to create Qt config directory");
        std::fs::create_dir_all(&module_private_path)
            .expect("Unable to create Qt config directory");

        // Write qplatformdefs_p.h forwarding header.
        configure::write_forwarding_header(
            &qtcore_path,
            &self
                .qplatformdefs_
                .as_ref()
                .expect("qplatformdefs path is missing"),
        );

        // Write Qt global public config and features to QtCore/qconfig.h.
        configure::write_config_header(
            qtcore_path.join("qconfig.h"),
            &self.global_defines_,
            &self.global_features_,
        );

        // Write Qt global private config features to QtCore/private/qconfig_p.h
        configure::write_config_header(
            qtcore_private_path.join("qconfig_p.h"),
            &Vec::new(),
            &self.global_private_features_,
        );

        // Write qt$module-config.h (e.g. qtcore-config.h) with public module config
        let module_config_file_name = format!("{}-config.h", &self.module_name.to_lowercase());
        configure::write_config_header(
            module_path.join(module_config_file_name),
            &self.module_defines_,
            &self.module_features_,
        );

        // Write qt$module-config_p.h (e.g. qtcore-config._på.h) with private module config
        let module_config_file_name = format!("{}-config_p.h", &self.module_name.to_lowercase());
        configure::write_config_header(
            module_private_path.join(module_config_file_name),
            &self.module_private_defines_,
            &self.module_private_features_,
        );
    }

    /// Creates syncqt-style forwarding headers. Qt source code expects to be able to
    /// include e.g. <QtCore/qglobal.h> in addition to plain <qglobal.h>, and also
    /// <QByteArray> in addition to <qbytearray.h>. Create an extra set of headers which
    /// forwards to the actual headers.
    fn create_forwarding_headers<P>(&self, path: P)
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().join(&self.module_name);
        //println!("Write forwarding headers  to {:?}", path);

        rayon::scope(|s| {
            s.spawn(|_| configure::write_forwarding_headers(&path, &self.module_headers));
            s.spawn(|_| configure::write_class_forwarding_headers(&path, &self.module_headers));
        });
    }

    /// Creates private forwarding headers, so that e.g <QtCore/private/foo_p.h> type inclues work.
    fn create_private_forwarding_headers<P>(&self, path: P)
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().join(&self.module_name).join("private");
        configure::write_forwarding_headers(&path, &self.module_headers);
    }

    /// Adds Qt global features. These will be written to qconfig.h
    pub fn global_features<'a, P>(&mut self, features: P) -> &mut QtModuleBuilder
    where
        P: IntoIterator<Item = (&'a str, bool)>,
    {
        self.global_features_.clear();
        self.global_features_.extend(
            features
                .into_iter()
                .map(|(name, enable)| (name.into(), enable)),
        );
        self
    }

    /// Sets Qt private global features. These will be written to qconfig_p.h
    pub fn global_private_features<'a, P>(&mut self, features: P) -> &mut QtModuleBuilder
    where
        P: IntoIterator<Item = (&'a str, bool)>,
    {
        self.global_private_features_.clear();
        self.global_private_features_.extend(
            features
                .into_iter()
                .map(|(name, enable)| (name.into(), enable)),
        );
        self
    }

    /// Sets module features. These will be written to qt$module-config.h
    pub fn module_features<'a, P>(&mut self, features: P) -> &mut QtModuleBuilder
    where
        P: IntoIterator<Item = (&'a str, bool)>,
    {
        self.module_features_.clear();
        self.module_features_.extend(
            features
                .into_iter()
                .map(|(name, enable)| (name.into(), enable)),
        );
        self
    }

    /// Adds module private features. These will be written to qt$module-config_p.h
    pub fn module_private_features<'a, P>(&mut self, features: P) -> &mut QtModuleBuilder
    where
        P: IntoIterator<Item = (&'a str, bool)>,
    {
        self.module_private_features_.clear();
        self.module_private_features_.extend(
            features
                .into_iter()
                .map(|(name, enable)| (name.into(), enable)),
        );
        self
    }

    /// Sets global defines.
    pub fn global_defines<'a, P>(&mut self, defines: P) -> &mut QtModuleBuilder
    where
        P: IntoIterator<Item = (&'a str, &'a str)>,
    {
        self.global_defines_.clear();
        self.global_defines_.extend(
            defines
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        );
        self
    }

    /// Sets module defines.
    pub fn module_defines<'a, P>(&mut self, defines: P) -> &mut QtModuleBuilder
    where
        P: IntoIterator<Item = (&'a str, &'a str)>,
    {
        self.module_defines_.clear();
        self.module_defines_.extend(
            defines
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        );
        self
    }

    /// Sets the qplatformsefs file location.
    pub fn qplatformdefs<T: AsRef<Path>>(&mut self, platformdefs_path: T) -> &mut QtModuleBuilder {
        self.qplatformdefs_ = Some(platformdefs_path.as_ref().to_path_buf());
        self
    }

    pub fn write_build_description(&mut self) -> &mut QtModuleBuilder {
        let description = format!(
            "Build: current working dir is {:?}  
                
            \n",
            std::env::current_dir()
        );
        fs::write("build_description.txt", description).unwrap();
        self
    }

    pub fn include<P>(&mut self, dir: P) -> &mut QtModuleBuilder
    where
        P: AsRef<Path>,
    {
        self.build.include(dir);
        self
    }

    pub fn default_global_features() -> Vec<(&'static str, bool)> {
        features::global_features()
    }

    pub fn default_global_private_features() -> Vec<(&'static str, bool)> {
        features::global_private_features()
    }

    pub fn default_bootstrap_fetures() -> Vec<(&'static str, bool)> {
        features::qt_bootstrap_features()
    }

    pub fn default_global_defines() -> Vec<(&'static str, &'static str)> {
        features::global_defines()
    }

    /// Configures the builder to target linux
    pub fn configure_for_linux_target(&mut self) -> &mut QtModuleBuilder {
        self.qplatformdefs(
            self.qt_source_path
                .join("qtbase/mkspecs/linux-clang/qplatformdefs.h"),
        );

        self.global_defines(features::global_defines());
        self.global_features(features::global_features());
        self.global_private_features(features::global_private_features());
        self
    }

    /// Configures the builder for building Qt Core
    pub fn add_qtcore_config(&mut self) -> &mut QtModuleBuilder {
        self.module_defines(features::qt_core_defines());
        self.module_features(features::qt_core_features());
        self.module_private_features(features::qt_core_private_features());
        self
    }

    /// Builds the module using the current settings
    pub fn compile(&mut self) {
        // This function can be called either from a build.rs script or in a standalone
        // context, for example from a test function. In the first case Cargo sets environment
        // variables like OUT_DIR, HOST, and TARGET for us, but in the second case
        // they have to be provided here.
        let out_dir_env = std::env::var("OUT_DIR");
        if let Err(_) = out_dir_env {
            self.build
                .host("x86_64-unknown-linux") // ### FIXME make configurable
                .target("x86_64-unknown-linux")
                .opt_level(0);

            // The CC crate defaults to 4 parallel compile tasks, increase
            // to the number of CPU cores.
            std::env::set_var("NUM_JOBS", num_cpus::get().to_string());
        }

        // Build output location. There are two inputs:
        //  - OUT_DIR: Set by Cargo if this function is called from build.rs
        //  - qt_build_path: Set by the QtModuleBuilder user.
        // And two outputs:
        //  - CC out_dir
        //  - Qt configure output dir
        //
        // Mix the inputs to the outputs such that:
        //  - OUT_DIR is used when set to support the build.rs use case
        //  - qt_build_path is used when set to support e.g. auto testing
        //  - Both are used if both are set. This enables sharing Qt configure output
        //    across several module builds.
        let qt_config_out_dir = match out_dir_env {
            Ok(var) => PathBuf::from_str(&var).expect("OUT_DIR is not a valid path"),
            Err(_) => {
                let qt_config_dir = self
                    .qt_build_path
                    .as_ref()
                    .expect("build_dir must be provided if not called from build.rs");
                self.build.out_dir(qt_config_dir);
                qt_config_dir.to_path_buf()
            }
        };

        println!("Configure...");

        // Write configuration headers
        let config_headers_path = qt_config_out_dir.join("qt_config_headers");
        //println!("Writing config headers to {:?}", config_headers_path);
        self.create_config_headears(&config_headers_path);

        // Write forwarding headers for all Qt headers. FIXME: currently
        // this does not differentiate between _p.h and .h headers.
        let forwarding_headers_path = qt_config_out_dir.join("qt_forwarding_headers");
        rayon::scope(|s| {
            s.spawn(|_| self.create_forwarding_headers(&forwarding_headers_path));
            s.spawn(|_| self.create_private_forwarding_headers(&forwarding_headers_path));
        });

        // Add all include paths to build
        self.build
            .include(&config_headers_path)
            .include(&config_headers_path.join(&self.module_name))
            .include(&forwarding_headers_path)
            .include(&forwarding_headers_path.join(&self.module_name));

        // Configure for c++17
        self.build.cpp(true).flag("-std=c++17");

        // Add source files
        self.build.files(&self.module_sources);

        println!("Build...");
        self.build.compile(&self.module_name.to_lowercase());
    }
}

pub fn build_moc_with_module_builder(builder: &mut QtModuleBuilder) {
    let sources = vec![
        "collectjson.cpp",
        "generator.cpp",
        "main.cpp",
        "moc.cpp",
        "parser.cpp",
        "preprocessor.cpp",
        "token.cpp",
    ];

    let tinycbor_path = builder
        .qt_source_path
        .join("qtbase/src/3rdparty/tinycbor/src/");

    // FIXME: moc is not a module and does not quite fit with the QtModuleBuilder API
    builder
        .name("QtCore")
        .module_source_path("qtbase/src/")
        .add_qtcore_config()
        .glob_headers(".")
        .include(tinycbor_path)
        .sources("tools/moc", sources)
        .compile();
}

mod features;

#[cfg(test)]
mod qt_cargo_base_tests {
    use crate::{build_moc_with_module_builder, QtModuleBuilder};
    use std::path::PathBuf;

    fn qt_src_path() -> PathBuf {
        // This test expects to find the Qt sources in the main vendored-qt workspace,
        // which this crate should be a member of. The path would normally be "../qt-src".
        // However, source file paths prefixed with "../" will make the CC crate output object
        // files with paths containing "../", which may end up resolving to a location outside
        // of the build directory. Side-step this by changing the current directory, if required.
        // FIXME: less hacky
        let current_cwd = std::env::current_dir().unwrap();
        if !current_cwd.ends_with("rust-vendored-qt") {
            std::env::set_current_dir("../").expect("unable to set the current dir");
        }
        let src_path: PathBuf = "qt-src/".into();
        if !src_path.exists() {
            panic!(
                "Could not find Qt source code at {:?} cwd was {:?} is {:?}",
                src_path,
                current_cwd,
                std::env::current_dir()
            );
        }
        src_path
    }

    fn qt_build_temp_dir() -> tempdir::TempDir {
        // Set up a temp dir for build artifacts
        tempdir::TempDir::new("qt-cargo-base-test").expect("unable to create temp dir")
    }

    #[test]
    fn build_qglobal() {
        let qt_source = qt_src_path();
        let temp = qt_build_temp_dir();
        let qt_build = temp.path();

        // Use QtModuleBuilder to build qglobal.cpp from QtCore.
        QtModuleBuilder::new()
            .source_path(qt_source)
            .build_path(qt_build)
            .configure_for_linux_target()
            .name("QtCore")
            .module_source_path("qtbase/src/corelib")
            .add_qtcore_config()
            // Add all headers, and one source file. Both paths are relative
            // to the source_path set above.
            .glob_headers(".")
            .source_file("global/qglobal.cpp")
            .compile();

        //  No panic? -> Test pass
        //std::mem::forget(temp); // leak build config in tempp
    }

    #[test]
    fn build_moc() {
        let qt_source = qt_src_path();
        let temp = qt_build_temp_dir();
        let qt_build = temp.path();

        build_moc_with_module_builder(
            // FIXME: API
            QtModuleBuilder::new()
                .source_path(qt_source)
                .build_path(qt_build)
                .configure_for_linux_target(),
        );

        //std::mem::forget(temp); // leak build config in tempp
    }
}

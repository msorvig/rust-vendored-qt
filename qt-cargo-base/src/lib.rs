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
//!         .name("QtCore")
//!         .qt_source_path("path/to/qt")
//!         .module_features(Vec![("foo", true)])
//!         .source_file("qglobal.cpp")
//!         .compile();
//! }
//! ```
//!

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Clone, Debug)]
pub struct QtModuleBuilder {
    module_name: String,
    qt_source_path: PathBuf,
    qt_build_path: PathBuf,
    module_headers: Vec<PathBuf>,
    module_sources: Vec<PathBuf>,

    global_features_: Vec<(String, bool)>,
    global_private_features_: Vec<(String, bool)>,
    global_defines_: Vec<(String, String)>,
    module_features_: Vec<(String, bool)>,
    module_private_features_: Vec<(String, bool)>,
    module_defines_: Vec<(String, String)>,
    module_private_defines_: Vec<(String, String)>,
    qplatformdefs_: Vec<u8>,
    build_dir_: Option<PathBuf>,
    build: cc::Build,
}

impl QtModuleBuilder {
    /// Constructs a new QtModuleBuilder, taking the path to the module source
    /// code as the argument.
    pub fn new() -> QtModuleBuilder {
        let mut module_builder = QtModuleBuilder {
            module_name: "".into(),
            qt_source_path: ".".into(),
            qt_build_path: ".".into(),
            module_headers: Vec::new(),
            module_sources: Vec::new(),
            global_features_: Vec::new(),
            global_private_features_: Vec::new(),
            global_defines_: Vec::new(),
            module_features_: Vec::new(),
            module_private_features_: Vec::new(),
            module_defines_: Vec::new(),
            module_private_defines_: Vec::new(),
            qplatformdefs_: Vec::new(),
            build_dir_: Option::None,
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

    /// Sets the path to the module source code. QtModuleBuilder will not write to
    /// this directory, which means that read access is sufficent.
    /// The path is prependended to all file paths given to QtModuleBuilder.
    pub fn source_path<P: AsRef<Path>>(&mut self, path: P) -> &mut QtModuleBuilder {
        self.qt_source_path = path.as_ref().into();
        self
    }

    /// Sets the path where QtModuleBuilder should store Qt-spesific build artifacts,
    /// such as configuration and forwarding headers.
    pub fn qt_build_path<P: AsRef<Path>>(&mut self, path: P) -> &mut QtModuleBuilder {
        self.qt_build_path = path.as_ref().into();
        self
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
        let resolved_dir = self.qt_source_path.join(dir);
        let ext = OsStr::new("h");
        let files = QtModuleBuilder::glob_files(resolved_dir, ext);
        self.module_headers.extend(files);
        self
    }

    /// Glob sources (.cpp) recursively from the given dir, relative to the module root
    pub fn glob_sources<P: AsRef<Path>>(&mut self, dir: P) -> &mut QtModuleBuilder {
        let resolved_dir = self.qt_source_path.join(dir);
        let ext = OsStr::new("cpp");
        let files = QtModuleBuilder::glob_files(resolved_dir, ext);
        self.module_sources.extend(files);
        self
    }

    /// Add a single source file to the module build
    pub fn source_file<P: AsRef<Path>>(&mut self, path: P) -> &mut QtModuleBuilder {
        self.module_sources
            .push(self.qt_source_path.join(path.as_ref().to_owned()));
        self
    }

    /// Adds multiple source files
    pub fn sources<'a, P, V>(&mut self, path: P, files: V) -> &mut QtModuleBuilder
    where
        P: AsRef<Path>,
        V: IntoIterator<Item = &'a &'a str>,
    {
        let prefix = self.qt_source_path.join(path.as_ref());
        self.module_sources
            .extend(files.into_iter().map(|f| prefix.join(f)));
        self
    }

    /// Create a string containing QT_FEATURE_foo defines from the given (define, enable) iterable
    fn make_feature_defines(features: &[(String, bool)]) -> String {
        features
            .iter()
            .map(|(name, enable)| {
                let value = match enable {
                    true => "1",
                    false => "-1",
                };
                format!("#define QT_FEATURE_{} {}\n", name, value)
            })
            .collect()
    }

    /// Creates a string containing #defines by concatenating (key, values) from the iteratable
    fn make_define_string<'a, F>(defines: F) -> String
    where
        F: IntoIterator<Item = &'a (String, String)>,
    {
        defines
            .into_iter()
            .map(|(key, value)| format!("#define {} {}\n", key, value))
            .collect()
    }

    /// Creates global and module configuration files. This includes the qplatformdefs.h
    /// header, as well as qconfig.h and qconfig_private.h. Returns the include path,
    /// which should be passed to the compile.
    fn create_global_config_headears(&mut self) -> PathBuf {
        // Set the path name. The name can be anything, since we add it to the compiler include paths.
        let default_path: PathBuf = ".".into();
        let workspace_path = self.build_dir_.as_ref().unwrap_or(&default_path);
        let path = workspace_path.join("buildconfig");

        // Write qplatformdefs.h
        std::fs::create_dir_all(&path).expect("Unable to create directory");
        let qplatformdefs_file_path = path.join("qplatformdefs.h");
        fs::write(qplatformdefs_file_path, &self.qplatformdefs_)
            .expect("Unable to write qplatformdefs file");

        // Write global config headers: qconfig.h and qconfig_p.h. These are written
        // to the QtCore module, where Qt expects to find them. (TODO: is this so?)
        let global_features_defines = QtModuleBuilder::make_feature_defines(&self.global_features_);
        let global_private_features_defines =
            QtModuleBuilder::make_feature_defines(&self.global_private_features_);
        let global_defines = QtModuleBuilder::make_define_string(&self.global_defines_);
        let qconfig_content = format!("{}\n{}", global_features_defines, global_defines);
        let qconfig_private_content = global_private_features_defines;
        std::fs::create_dir_all(&path.join("QtCore")).expect("Unable to create directory");
        fs::write(path.join("QtCore/qconfig.h"), qconfig_content).expect("Unable to write file");
        std::fs::create_dir_all(&path.join("QtCore/private")).expect("Unable to create directory");
        fs::write(
            path.join("QtCore/private/qconfig_p.h"),
            qconfig_private_content,
        )
        .expect("Unable to write file");

        // Write module config headers qt$module-confifg.h and _p.h. These are written to the
        // Qt$Module directoty.
        let module_features_defines = QtModuleBuilder::make_feature_defines(&self.module_features_);
        let module_private_features_defines =
            QtModuleBuilder::make_feature_defines(&self.module_private_features_);
        let moule_defines = QtModuleBuilder::make_define_string(&self.module_defines_);
        let moule_private_defines =
            QtModuleBuilder::make_define_string(&self.module_private_defines_);
        let module_config_content = format!("{}\n{}", module_features_defines, moule_defines);
        let module_private_config_content = format!(
            "{}\n{}",
            module_private_features_defines, moule_private_defines
        );
        let module_name = &self.module_name;
        let module_lowercase_name = module_name.to_lowercase();

        // Write to e.g. QtCore/qtcore-config.h
        let module_config_dir = path.join(module_name);
        fs::create_dir_all(&module_config_dir).expect("could not create directory");
        fs::write(
            module_config_dir.join(format!("{}-config.h", module_lowercase_name)),
            module_config_content,
        )
        .expect("Unable to write file");

        // Write to e.g. QtCore/private/qtcore-config_p.h
        let module_private_config_dir = module_config_dir.join("private");
        fs::create_dir_all(&module_private_config_dir).expect("could not create directory");
        fs::write(
            module_private_config_dir.join(format!("{}-config_p.h", module_lowercase_name)),
            module_private_config_content,
        )
        .expect("Unable to write file");

        // Return the include path
        path
    }

    /// Writes forwarding headers which contain e.g. #include "../diff/path/to/real/header.h"
    /// The headers are written to the given path. Each header points back to the corrsponding
    /// header path in the headers iterable.
    fn write_headers<'a, P, V>(path: P, headers: V)
    where
        P: AsRef<Path>,
        V: IntoIterator<Item = &'a PathBuf>,
    {
        let path = path.as_ref();
        std::fs::create_dir_all(&path).expect("Unable to create directory");
        for header in headers.into_iter() {
            match header.file_name() {
                Some(file_name) => {
                    let forwarding_header_path = path.join(&file_name);
                    let target_header_path =
                        pathdiff::diff_paths(header, &path).expect("Unable to create fwd path");
                    let include_statement =
                        format!("#include \"{}\"", target_header_path.to_str().unwrap());
                    fs::write(forwarding_header_path, include_statement)
                        .expect("Unable to write file");
                }
                None => {}
            }
        }
    }

    /// Creates forwarding headers (like syncqt does). Qt source code expects to include
    /// e.g. <QtCore/qglobal.h> in addition to plain <qglobal.h>, so we need to create
    // an extra set of headers which forwards to the real headers.
    fn create_module_forwarding_headers(&mut self) -> PathBuf {
        let path = PathBuf::from("module_includes").join(&self.module_name);
        QtModuleBuilder::write_headers(&path, &self.module_headers);
        path
    }

    /// Creates private forwarding headers, so that e.g <QtCore/private/foo_p.h> type inclues work.
    fn create_private_forwarding_headers(&mut self) -> PathBuf {
        let path = PathBuf::from("module_includes")
            .join(&self.module_name)
            .join("private");
        QtModuleBuilder::write_headers(&path, &self.module_headers);
        path
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

    /// Sets the content of the qplatformsefs file
    pub fn qplatformdefs<T: AsRef<Path>>(&mut self, platformdefs_file: T) -> &mut QtModuleBuilder {
        self.qplatformdefs_ =
            fs::read(platformdefs_file.as_ref()).expect("unable to read platformdefs file");
        self
    }

    /// Sets cc::Build::out_dir(). Calling this function is normally not needed, since
    /// CC::Build will read the OUT_DIR environment variable set by cargo.
    pub fn build_path<P>(&mut self, out_dir: P) -> &mut QtModuleBuilder
    where
        P: AsRef<Path>,
    {
        self.build_dir_ = Some(out_dir.as_ref().into());
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

    /// Builds the module using the current settings
    pub fn compile(&mut self) {
        // This function can be called either from a build.rs script or standalone,
        // for example from a test function. In the first case Cargo sets environment
        // variables like OUT_DIR, HOST, and TARGET for us, but in the second case
        // they have to be provided here. Use the precense of an excplicitly set build
        // dir to determine which mode we are in.
        if let Some(out_dir) = &self.build_dir_ {
            self.build
                .out_dir(out_dir)
                .host("x86_64-unknown-linux") // ### FIXME make configurable
                .target("x86_64-unknown-linux")
                .opt_level(0);
        }

        // Get build path, either OUT_DIR from cargo or build_dir.
        let workspace_path = self.build_dir_.as_ref().unwrap().clone(); // TODO: OUT_DIR

        // Write Qt configuration headers
        let config_headers_path2 = workspace_path.join("config_headers");
        let platformdefs_path = self.create_global_config_headears();

        // Write Qt forwarding headers
        let forwarding_headers_path2 = workspace_path.join("forwarding_headers");
        let forwarding_headers_path = self.create_module_forwarding_headers();
        let _private_forwarding_heders_path = self.create_private_forwarding_headers();

        // Add all include paths to build
        self.build
            .include(platformdefs_path)
            .include("module_includes")
            .include(forwarding_headers_path);

        // Configure for c++17
        self.build.cpp(true).flag("-std=c++17");

        // Add source files
        self.build.files(&self.module_sources);

        self.build.compile(&self.module_name.to_lowercase());
    }
}

mod features;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::features;
    use crate::QtModuleBuilder;

    #[test]
    fn build_qglobal() {
        // This test expects to find the Qt sources in the main vendored-qt workspace,
        // which this crate should be a member of.
        let qt_source: PathBuf = "../qt-src/".into();
        let qt_build: PathBuf = "../qt-build/".into();

        // Use QtModuleBuilder to build qglobal.cpp from QtCore.
        QtModuleBuilder::new()
            .name("QtCore")
            .source_path(qt_source.join("qtbase/src/corelib"))
            .build_path(qt_build)
            .qplatformdefs(qt_source.join("qtbase/mkspecs/linux-clang/qplatformdefs.h"))
            // Set global Qt config
            .global_defines(features::global_defines())
            .global_features(features::global_features())
            .global_private_features(features::global_private_features())
            // Set QtCore config
            .module_defines(features::qt_core_defines())
            .module_features(features::qt_core_features())
            .module_private_features(features::qt_core_private_features())
            // Add all headers, and one source file. Both paths are relative
            // to the source_path set above.
            .glob_headers(".")
            .source_file("global/qglobal.cpp")
            // Find all headers. We need to:
            // - create forwarding headers for <QtCore/foo.h> includes.
            // - create forwarding headers for <private/foo_p.h> includes.
            // - run moc on som/all headers. (TODO!)
            // (this is actually done during the compile() step below)
            .compile();

        //  No panic? -> Test pass
    }
}

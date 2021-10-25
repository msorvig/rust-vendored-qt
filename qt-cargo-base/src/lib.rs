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
    qplatformdefs_: Option<PathBuf>,
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
            qplatformdefs_: Option::None,
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

    /// Writes global and module configuration files to the given path. This includes the
    /// qplatformdefs.h header, as well as qconfig.h and qconfig_private.h.
    fn create_config_headears<P>(&mut self, path: P)
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let module_path = path.join(&self.module_name);
        std::fs::create_dir_all(&module_path).expect("Unable to create directory");

        // Write qplatformdefs_p.h forwarding header.
        QtModuleBuilder::write_forwarding_header(
            &module_path,
            &self
                .qplatformdefs_
                .as_ref()
                .expect("qplatformdefs path is missing"),
        );

        // Write qconfig.h with Qt global public config and features to QtCore/qconfig.h.
        let global_defines = QtModuleBuilder::make_define_string(&self.global_defines_);
        let global_features_defines = QtModuleBuilder::make_feature_defines(&self.global_features_);
        let qconfig_content = format!("{}\n{}", global_features_defines, global_defines);
        let qconfig_path = module_path.join("qconfig.h");
        //println!("Write qconfig.h to {:?}", qconfig_path);
        fs::write(qconfig_path, qconfig_content).expect("Unable to write file");

        // Write qconfig_p.h with Qt global private config features to QtCore/private/qconfig_p.h
        let module_private_path = module_path.join("private");
        std::fs::create_dir_all(&module_private_path).expect("Unable to create directory");
        let global_private_features_defines =
            QtModuleBuilder::make_feature_defines(&self.global_private_features_);
        let qconfig_private_content = global_private_features_defines;
        let qconfig_p_path = module_private_path.join("qconfig_p.h");
        //println!("Write qconfig_p.h to {:?}", qconfig_p_path);
        fs::write(qconfig_p_path, qconfig_private_content).expect("Unable to write file");

        // Write qt$module-config.h (e.g. qtcore-config.h) with public module config
        let module_lowercase_name = &self.module_name.to_lowercase();
        let module_config_dir = path.join(&self.module_name);
        let module_features_defines = QtModuleBuilder::make_feature_defines(&self.module_features_);
        let module_defines = QtModuleBuilder::make_define_string(&self.module_defines_);
        let module_config_content = format!("{}\n{}", module_features_defines, module_defines);
        let module_config_file_name = format!("{}-config.h", module_lowercase_name);
        let module_config_path = module_config_dir.join(&module_config_file_name);
        fs::create_dir_all(&module_config_dir).expect("could not create directory");
        //println!(
        //    "Write {} to {:?}",
        //    &module_config_file_name, &module_config_path
        //);
        fs::write(module_config_path, module_config_content).expect("Unable to write file");

        // Write qt$module-config_p.h (e.g. qtcore-config._på.h) with private module config
        let module_private_features_defines =
            QtModuleBuilder::make_feature_defines(&self.module_private_features_);
        let moule_private_defines =
            QtModuleBuilder::make_define_string(&self.module_private_defines_);
        let module_private_config_content = format!(
            "{}\n{}",
            module_private_features_defines, moule_private_defines
        );
        let module_private_config_dir = module_config_dir.join("private");
        let module_private_config_file_name = format!("{}-config_p.h", module_lowercase_name);
        let module_private_config_path =
            module_private_config_dir.join(&module_private_config_file_name);

        fs::create_dir_all(&module_private_config_dir).expect("could not create directory");
        //println!(
        //    "Write {} to {:?}",
        //    &module_private_config_file_name, &module_private_config_path
        //);
        fs::write(module_private_config_path, module_private_config_content)
            .expect("Unable to write file");
    }

    /// Writes a forwarding header to destination_path. The forwarding header
    /// file name is taken from target_header_path. The forwarding heder will
    /// contain an "#include" statement which includes the target header.
    /// target_header_path may be a relative path, and will be resolved against
    /// std:::env::current_dir if so.
    fn write_forwarding_header<'a, P, V>(destination_path: P, target_header_path: V)
    where
        P: AsRef<Path>,
        V: AsRef<Path>,
    {
        let destination_path = destination_path.as_ref();
        let target_header_path = target_header_path.as_ref();
        if let Some(file_name) = target_header_path.file_name() {
            let forwarding_header_path = destination_path.join(file_name);
            let target_header_path = match target_header_path.is_relative() {
                true => target_header_path.to_path_buf(),
                false => std::env::current_dir()
                    .unwrap()
                    .as_path()
                    .join(target_header_path),
            }
            .canonicalize()
            .unwrap();
            //println!("{:?} {:?}", &destination_path, target_header_path);
            let target_header_path = pathdiff::diff_paths(target_header_path, &destination_path)
                .expect("Unable to create fwd path");
            //println!("{:?}", target_header_path);
            let include_statement =
                format!("#include \"{}\"\n", target_header_path.to_str().unwrap());
            fs::write(forwarding_header_path, include_statement).expect("Unable to write file");
        }
    }

    /// Writes forwarding headers which contain e.g. #include "../diff/path/to/real/header.h"
    /// The headers are written to the given path. Each header points back to the corrsponding
    /// header path in the headers iterable.
    fn write_forwarding_headers<'a, P, V>(path: P, headers: V)
    where
        P: AsRef<Path>,
        V: IntoIterator<Item = &'a PathBuf>,
    {
        std::fs::create_dir_all(&path).expect("Unable to create directory");
        for header_realative_path in headers.into_iter() {
            QtModuleBuilder::write_forwarding_header(&path, header_realative_path);
        }
    }

    /// Creates syncqt-style forwarding headers. Qt source code expects to be able to
    /// include e.g. <QtCore/qglobal.h> in addition to plain <qglobal.h>, so we need to
    /// create an extra set of headers which forwards to the actual headers.
    fn create_forwarding_headers<P>(&mut self, path: P)
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().join(&self.module_name);
        //println!("Write forwarding headers  to {:?}", path);
        QtModuleBuilder::write_forwarding_headers(path, &self.module_headers);
    }

    /// Creates private forwarding headers, so that e.g <QtCore/private/foo_p.h> type inclues work.
    fn create_private_forwarding_headers<P>(&mut self, path: P)
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().join(&self.module_name).join("private");
        QtModuleBuilder::write_forwarding_headers(&path, &self.module_headers);
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

    /// Configures the builder to target linux
    pub fn configure_for_linux_target<P>(&mut self, qt_source: P) -> &mut QtModuleBuilder
    where
        P: AsRef<Path>,
    {
        self.qplatformdefs(
            qt_source
                .as_ref()
                .join("qtbase/mkspecs/linux-clang/qplatformdefs.h"),
        );

        self.global_defines(features::global_defines());
        self.global_features(features::global_features());
        self.global_private_features(features::global_private_features());
        self
    }

    /// Configures the builder for building Qt Core
    pub fn configure_for_qtcore_module(&mut self) -> &mut QtModuleBuilder {
        self.module_defines(features::qt_core_defines());
        self.module_features(features::qt_core_features());
        self.module_private_features(features::qt_core_private_features());
        self
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

        // Get build path, either OUT_DIR from cargo or self.build_dir.
        let build_path = self.build_dir_.as_ref().unwrap().clone(); // TODO: OUT_DIR
                                                                    //println!("Build workspace {:?}", build_path);

        // Write configuration headers
        let config_headers_path = build_path.join("config_headers");
        //println!("Writing config headers to {:?}", config_headers_path);
        self.create_config_headears(&config_headers_path);

        // Write forwarding headers for all Qt headers. FIXME: currently
        // this does not differentiate between _p.h and .h headers.
        let forwarding_headers_path = build_path.join("forwarding_headers");
        self.create_forwarding_headers(&forwarding_headers_path);
        self.create_private_forwarding_headers(&forwarding_headers_path);

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

        self.build.compile(&self.module_name.to_lowercase());
    }
}

mod features;

#[cfg(test)]
mod tests {
    use crate::QtModuleBuilder;
    use std::path::PathBuf;

    #[test]
    fn build_qglobal() {
        // This test expects to find the Qt sources in the main vendored-qt workspace,
        // which this crate should be a member of. The path would normally be "../qt-src".
        // However, source file paths prefixed with "../" will make the CC crate output object
        // files with paths containing "../", which may end up resolving to a location outside
        // of the build directory. Side-step this by changing the current directory.
        std::env::set_current_dir("../").expect("unable to set the current dir");
        let qt_source: PathBuf = "qt-src/".into();

        // Set up a temp dir for build artifacts
        let temp = tempdir::TempDir::new("qt-cargo-base-test").expect("unable to create temp dir");
        let qt_build = temp.path();

        // Use QtModuleBuilder to build qglobal.cpp from QtCore.
        QtModuleBuilder::new()
            .name("QtCore")
            .source_path(qt_source.join("qtbase/src/corelib"))
            .build_path(qt_build)
            .configure_for_linux_target(&qt_source)
            .configure_for_qtcore_module()
            // Add all headers, and one source file. Both paths are relative
            // to the source_path set above.
            .glob_headers(".")
            .source_file("global/qglobal.cpp")
            .compile();

        //  No panic? -> Test pass
        // std::mem::forget(temp); // leak build config in tempp
    }
}

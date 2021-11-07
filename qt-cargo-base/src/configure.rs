use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use itertools::Itertools;

use crate::{features, util};

// Qt configure implementation
//
// In-scope is anything needed to build Qt, and which has
// other dependencies than a C++ compiler and the Qt source
// code itself. Building Qt applications is out of scope.
//
// Currently only a fixed linux configuration is supported,
// see features.rs
//
// The following is currently implemented:
//
//  - creating a forwarding header to qplatformdefs.h
//  - crating global and QtCore configuration headers
//  - crating forwarding headers and Qt class forwarding headers
//
// This file implements "leaf" helper functions only, the driving
// code is in lib.rs.

// QtConfiguration contains Qt configuration, including a qplatformsefs.h path,
// global defines and feaures, and QtCore defines and features (currently for
// one module). An instance of the module can be passed to write_qt_configuration,
// which will then write the Qt configuration files to disk. The confiuration is
// independent of Qt source and and build location.

#[derive(Default)]
#[allow(dead_code)]
pub struct QtConfiguration {
    qplatformdefs_path: Option<PathBuf>,

    global_features: Vec<(String, bool)>,
    global_private_features: Vec<(String, bool)>,
    global_defines: Vec<(String, String)>,

    qtcore_features: Vec<(String, bool)>,
    qtcore_private_features: Vec<(String, bool)>,
    qtcore_defines: Vec<(String, String)>,
}

impl QtConfiguration {
    #[allow(dead_code)]
    pub fn new() -> QtConfiguration {
        Default::default()
    }
}

#[allow(dead_code)]
pub fn set_default_configuration(qt_configuration: &mut QtConfiguration) {
    qt_configuration.qplatformdefs_path = Some("qtbase/mkspecs/linux-clang/qplatformdefs.h".into()); // Hardcode linux-clang

    qt_configuration.global_features = features::global_features()
        .iter()
        .map(|(a, b)| (a.to_string(), *b))
        .collect();
    qt_configuration.global_private_features = features::global_private_features()
        .iter()
        .map(|(a, b)| (a.to_string(), *b))
        .collect();
    qt_configuration.global_defines = features::global_defines()
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();

    qt_configuration.qtcore_features = features::qt_core_features()
        .iter()
        .map(|(a, b)| (a.to_string(), *b))
        .collect();
    qt_configuration.qtcore_private_features = features::qt_core_private_features()
        .iter()
        .map(|(a, b)| (a.to_string(), *b))
        .collect();
    qt_configuration.qtcore_defines = features::qt_core_defines()
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
}

#[allow(dead_code)]
pub fn write_configuration<P, Q>(
    qt_configuration: &QtConfiguration,
    destination_path: P,
    qt_source_path: Option<Q>,
) where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let qtcore_path = destination_path.as_ref().join("QtCore");
    let qtcore_private_path = qtcore_path.join("private");
    std::fs::create_dir_all(&qtcore_private_path).expect("Unable to create Qt config directory");
    std::fs::create_dir_all(&qtcore_private_path).expect("Unable to create Qt config directory");

    // Write qplatformdefs_p.h forwarding header, if we have Qt source to point it to.
    if let Some(path) = qt_source_path {
        write_forwarding_header(
            &qtcore_path,
            path.as_ref()
                .join(qt_configuration.qplatformdefs_path.as_ref().unwrap()),
        );
    };

    // Write Qt global public config and features to QtCore/qconfig.h.
    write_config_header(
        qtcore_path.join("qconfig.h"),
        &qt_configuration.global_defines,
        &qt_configuration.global_features,
    );

    // Write Qt global private config features to QtCore/private/qconfig_p.h
    write_config_header(
        qtcore_private_path.join("qconfig_p.h"),
        &Vec::new(),
        &qt_configuration.global_private_features,
    );

    write_config_header(
        qtcore_path.join("qtcore-config.h"),
        &qt_configuration.qtcore_defines,
        &qt_configuration.qtcore_features,
    );

    write_config_header(
        qtcore_private_path.join("qtcore-config_p.h"),
        &Vec::new(),
        &qt_configuration.qtcore_private_features,
    );
}

/// Creates a string containing #defines by concatenating (key, values) from the iteratable
pub fn make_define_string<'a, F>(defines: F) -> String
where
    F: IntoIterator<Item = &'a (String, String)>,
{
    defines
        .into_iter()
        .map(|(key, value)| format!("#define {} {}\n", key, value))
        .collect()
}

/// Create a string containing QT_FEATURE_foo defines from the given (define, enable) iterable
pub fn make_feature_defines(features: &[(String, bool)]) -> String {
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

/// Writes a forwarding header to forwarding_header_path which incliudes the header at
/// target_header_path. target_header_path may be a relative path, and will be resolved against
/// std:::env::current_dir if so.
pub fn write_forwarding_header_2<P, V>(forwarding_header_path: P, target_header_path: V)
where
    P: AsRef<Path>,
    V: AsRef<Path>,
{
    let forwarding_header_path = forwarding_header_path.as_ref();
    let target_header_path = target_header_path.as_ref();

    let target_header_path = match target_header_path.is_relative() {
        true => target_header_path.to_path_buf(),
        false => std::env::current_dir()
            .unwrap()
            .as_path()
            .join(target_header_path),
    }
    .canonicalize()
    .unwrap();

    let target_header_path = pathdiff::diff_paths(
        target_header_path,
        &forwarding_header_path.parent().unwrap(),
    )
    .expect("Unable to create fwd path");
    // println!("{:?}", target_header_path);
    let include_statement = format!("#include \"{}\"\n", target_header_path.to_str().unwrap());
    fs::write(forwarding_header_path, include_statement).expect("Unable to write file");
}

/// Writes a forwarding header to destination_path. The forwarding header
/// file name is taken from target_header_path. The forwarding heder will
/// contain an "#include" statement which includes the target header.
/// target_header_path may be a relative path, and will be resolved against
/// std:::env::current_dir if so.
pub fn write_forwarding_header<P, V>(destination_path: P, target_header_path: V)
where
    P: AsRef<Path>,
    V: AsRef<Path>,
{
    let destination_path = destination_path.as_ref();
    let target_header_path = target_header_path.as_ref();
    if let Some(file_name) = target_header_path.file_name() {
        let forwarding_header_path = destination_path.join(file_name);
        write_forwarding_header_2(forwarding_header_path, target_header_path)
    }
}

/// Writes forwarding headers which contain e.g. #include "../diff/path/to/real/header.h"
/// The headers are written to the given path. Each header points back to the corrsponding
/// header path in the headers iterable.
#[allow(dead_code)]
pub fn write_forwarding_headers<'a, P, V>(path: P, headers: V)
where
    P: AsRef<Path>,
    V: IntoIterator<Item = &'a PathBuf>,
{
    std::fs::create_dir_all(&path).expect("Unable to create directory");
    for header_realative_path in headers.into_iter() {
        write_forwarding_header(&path, header_realative_path);
    }
}

/// Looks for Qt classes in the header at target_header_path, then writes "QFoo"-
/// type headers to destination_path.
pub fn write_class_forwarding_header<P, V>(destination_path: P, target_header_path: V)
where
    P: AsRef<Path>,
    V: AsRef<Path>,
{
    let target_header_path = target_header_path.as_ref();
    // println!("Scan {:?} for classes", &target_header_path);

    // Behold, the Qt class name detector
    let is_qt_class = |token: &str| {
        token.starts_with("Q")
            && !token.contains(";")
            && !token.contains(":")
            && !token.contains("#")
            && !token.contains("_")
            && !token.contains("<")
            && !token.contains(">")
    };

    let bytes = std::fs::read(&target_header_path)
        .expect(&format!("Unable to read file {:?}", &target_header_path));
    let tokens = std::str::from_utf8(&bytes)
        .expect("Non-UTF8 source code!")
        .split_whitespace();
    let qt_classes = tokens
        .tuple_windows::<(_, _, _)>()
        .filter_map(|(elem, next, next_next)| {
            // Look for "class QFoo" and "class <some token> QFoo" and emit
            // "QFoo". <some token> is typically a Q_CORE_EXPORT or similar.
            if elem == "class" {
                if is_qt_class(next) {
                    Some(next)
                } else if is_qt_class(next_next) {
                    Some(next_next)
                } else {
                    None
                }
            } else {
                None
            }
        });
    for class in qt_classes {
        // println!("class {:?}", class);
        write_forwarding_header_2(destination_path.as_ref().join(class), target_header_path);
    }
}

/// Writes class forwarding headers for all classes found in the provided headers.
#[allow(dead_code)]
pub fn write_class_forwarding_headers<'a, P, V>(path: P, headers: V)
where
    P: AsRef<Path> + Send + Sync,
    V: IntoIterator<Item = &'a PathBuf> + Send,
{
    std::fs::create_dir_all(&path).expect("Unable to create directory");
    for header_realative_path in headers.into_iter() {
        write_class_forwarding_header(&path, header_realative_path);
    }
    /*
        FIXME: enabling this causes build failures with missing headers
        let heck = headers.into_iter().collect::<Vec<_>>();
        heck.par_iter().for_each(|header| {
            println!("{:?}", header);
            write_class_forwarding_header(&path, header);
        });
    */
}

/// Writes a Qt configuarion header containg defines and features to the given path.
pub fn write_config_header<P>(path: P, defines: &[(String, String)], features: &[(String, bool)])
where
    P: AsRef<Path>,
{
    let qconfig_content = format!(
        "{}\n{}",
        make_define_string(defines),
        make_feature_defines(features)
    );
    fs::write(path.as_ref(), qconfig_content).expect("Unable to write file");
}

/// Writes forwarding headers for all headers (.h) files found in source_path
/// to destination_path. This includes public headers and private headers (_p.h).
/// Private headers are placed under the "private/" prefix in the destination
/// path. Finally, class forwarding headers are
pub fn write_all_forwarding_headers<P, Q>(source_path: P, destination_path: Q)
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let destination_private_path = destination_path.as_ref().join("private");
    std::fs::create_dir_all(&destination_private_path).expect("Unable to create directory");

    let header_paths = util::glob_files(source_path, OsStr::new("h"));
    for header_path in header_paths {
        let is_private = header_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("_p.h");
        if is_private {
            write_forwarding_header(&destination_private_path, header_path);
        } else {
            write_forwarding_header(&destination_path, &header_path);
            write_class_forwarding_header(&destination_path, &header_path)
        }
    }
}

#[cfg(test)]
mod qt_cargo_base_configure_tests {
    use std::fs::read_dir;

    use super::*;

    #[test]
    fn test_write_configuration() {
        let temp = tempdir::TempDir::new("qt-cargo-base-configure-test").unwrap();
        let qt_path: Option<&str> = None;

        let mut config = QtConfiguration::new();
        set_default_configuration(&mut config);
        write_configuration(&config, temp.path(), qt_path);
    }

    #[test]
    fn test_write_forwarding_headers() {
        let temp = tempdir::TempDir::new("qt-cargo-base-configure-test").unwrap();
        let qt_path = util::qt_src_path();

        write_all_forwarding_headers(qt_path.join("qtbase/src/corelib"), &temp);
        let expected_file_count = 523; // for current Qt version and implementation; change as needed.
        assert_eq!(read_dir(&temp).unwrap().count(), expected_file_count);
    }
}

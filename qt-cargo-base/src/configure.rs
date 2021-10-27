use std::fs;
use std::path::{Path, PathBuf};

use itertools::Itertools;

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
            // "QFoo". "some token" is typically a Q_CORE_EXPORT or similar.
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
pub fn write_class_forwarding_headers<'a, P, V>(path: P, headers: V)
where
    P: AsRef<Path>,
    V: IntoIterator<Item = &'a PathBuf>,
{
    std::fs::create_dir_all(&path).expect("Unable to create directory");
    for header_realative_path in headers.into_iter() {
        write_class_forwarding_header(&path, header_realative_path);
    }
}

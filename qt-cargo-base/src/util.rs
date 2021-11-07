use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

// Returns an iterator to all files with a certain extention under the
// given path
pub fn glob_files<P: AsRef<Path>>(
    path: P,
    file_ext: &'_ OsStr,
) -> impl Iterator<Item = PathBuf> + '_ {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(move |e| e.path().extension() == Some(file_ext))
        .map(|e| e.path().to_owned())
}

pub fn qt_src_path() -> PathBuf {
    // Test and build scripts expects to find the Qt sources in the main vendored-qt workspace,
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

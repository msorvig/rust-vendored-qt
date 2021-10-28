use libc::{c_char, c_int};
extern "C" {
    fn _Z6runMociPPc(argc: c_int, argv: *mut *mut c_char); // FIXME: export non-mangled runMoc()
}

pub fn moc() {
    let mut buf = std::ptr::null_mut();
    unsafe {
        _Z6runMociPPc(0, buf);
    }
}

#[cfg(test)]
mod qtcore_host_tools_tests {
    use super::*;

    #[test]
    fn run_moc() {
        println!("Hello");
        moc();
    }
}

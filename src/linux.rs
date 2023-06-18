use std::ffi::{c_int, c_ulong, CStr, CString};
use std::path::Path;

use crate::model::{ContainerRuntimeError, ContainerRuntimeResult};

pub fn mount(src: Option<&str>, target: &Path, fstype: Option<&str>, flags: c_ulong, data: Option<&str>) -> ContainerRuntimeResult<()> {
    let src = src.map(|x| CString::new(x).unwrap());
    let target = CString::new(target.to_str().unwrap()).unwrap();
    let fstype = fstype.map(|x| CString::new(x).unwrap());
    let data = data.map(|x| CString::new(x).unwrap());

    unsafe {
        let result = libc::mount(
            src.as_ref().map(|x| x.as_ptr() as *const _).unwrap_or(std::ptr::null()),
            target.as_ptr() as *const _,
            fstype.as_ref().map(|x| x.as_ptr() as *const _).unwrap_or(std::ptr::null()),
            flags,
            data.as_ref().map(|x| x.as_ptr() as *const _).unwrap_or(std::ptr::null())
        );

        if result == 0 {
            Ok(())
        } else {
            Err(ContainerRuntimeError::Mount(extract_libc_error_message()))
        }
    }
}

pub fn exec(command: &Vec<String>) -> ContainerRuntimeResult<()> {
    let command = command.iter().map(|part| CString::new(part.as_str()).unwrap()).collect::<Vec<_>>();
    let mut command_ptrs = command.iter().map(|part| part.as_ptr()).collect::<Vec<_>>();
    command_ptrs.push(std::ptr::null());

    unsafe {
        if libc::execvp(command_ptrs[0], &command_ptrs[0]) == 0 {
            Ok(())
        } else {
            Err(ContainerRuntimeError::Execute(extract_libc_error_message()))
        }
    }
}

pub fn waitpid(pid: i32) -> ContainerRuntimeResult<i32> {
     unsafe {
        let mut status = 0;
        wrap_libc_error(libc::waitpid(pid, &mut status as *mut c_int, 0))?;
        Ok(status)
    }
}

pub fn wrap_libc_error(result: i32) -> ContainerRuntimeResult<i32> {
    if result >= 0 {
        Ok(result)
    } else {
        Err(ContainerRuntimeError::Libc(extract_libc_error_message()))
    }
}

pub fn extract_libc_error_message() -> String {
    unsafe {
        let error_message = CStr::from_ptr(libc::strerror(*libc::__errno_location()));
        error_message.to_str().unwrap().to_owned()
    }
}
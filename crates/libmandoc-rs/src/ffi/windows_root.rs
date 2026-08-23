//! Windows filesystem resolver for strict, memory-only `.so` expansion.

use std::{
    ffi::{CStr, CString, OsString, c_void},
    fs::{self, File},
    io::{self, Read},
    os::{
        raw::c_char,
        windows::{ffi::OsStringExt, fs::MetadataExt, io::AsRawHandle},
    },
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Component, Path, PathBuf},
    ptr,
};

use flate2::read::MultiGzDecoder;
use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW,
        VOLUME_NAME_DOS,
    },
};

use super::{CResolvedSource, CSourceResolver};

const RESOLVE_NOT_FOUND: i32 = -1;
const RESOLVE_DENIED: i32 = -2;
const RESOLVE_IO: i32 = -3;
const MAX_WINDOWS_PATH_U16: usize = 32_768;

pub(super) struct RootResolver {
    root: PathBuf,
    canonical_root: Option<PathBuf>,
    top_level_path: Option<String>,
    top_level_parent: Option<PathBuf>,
    data: Vec<u8>,
    logical_path: CString,
}

impl RootResolver {
    pub(super) fn new(root: &Path, source_path: &CStr) -> Self {
        let root = absolute_path(root).unwrap_or_else(|_| root.to_path_buf());
        let top_level_path = source_path.to_str().ok().map(str::to_owned);
        let top_level_parent = top_level_path
            .as_deref()
            .and_then(|path| logical_source_parent(&root, Path::new(path)));
        Self {
            root,
            canonical_root: None,
            top_level_path,
            top_level_parent,
            data: Vec::new(),
            logical_path: CString::default(),
        }
    }

    fn resolve(&mut self, requested: &str, current: Option<&str>) -> io::Result<()> {
        let requested = safe_relative_path(requested)?;
        let mut candidates = vec![requested.clone()];
        if let Some(parent) = self.current_parent(current)? {
            let beside = parent.join(&requested);
            if beside != requested {
                candidates.push(beside);
            }
        }

        let mut last_not_found = None;
        for candidate in candidates {
            match self.read_candidate(&candidate) {
                Ok(data) => {
                    let label = logical_label(&candidate)?;
                    self.logical_path = CString::new(label).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "include path contains NUL")
                    })?;
                    self.data = data;
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    last_not_found = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_not_found.unwrap_or_else(|| io::Error::from(io::ErrorKind::NotFound)))
    }

    fn current_parent(&self, current: Option<&str>) -> io::Result<Option<PathBuf>> {
        let Some(current) = current else {
            return Ok(None);
        };
        if self.top_level_path.as_deref() == Some(current) {
            return Ok(self.top_level_parent.clone());
        }
        let relative = safe_relative_path(current)?;
        Ok(relative.parent().map(Path::to_path_buf))
    }

    fn read_candidate(&mut self, logical: &Path) -> io::Result<Vec<u8>> {
        match self.read_exact(logical) {
            Ok(data) => Ok(data),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut compressed = logical.as_os_str().to_os_string();
                compressed.push(".gz");
                let compressed = PathBuf::from(compressed);
                let file = self.open_confined(&compressed)?;
                let mut data = Vec::new();
                MultiGzDecoder::new(file).read_to_end(&mut data)?;
                Ok(data)
            }
            Err(error) => Err(error),
        }
    }

    fn read_exact(&mut self, logical: &Path) -> io::Result<Vec<u8>> {
        let mut file = self.open_confined(logical)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(data)
    }

    fn open_confined(&mut self, logical: &Path) -> io::Result<File> {
        let canonical_root = if let Some(root) = &self.canonical_root {
            root.clone()
        } else {
            let root = fs::canonicalize(&self.root)?;
            self.canonical_root = Some(root.clone());
            root
        };
        let mut candidate = self.root.clone();
        for component in logical.components() {
            let Component::Normal(component) = component else {
                return Err(denied("include path escapes the approved root"));
            };
            candidate.push(component);
            let metadata = fs::symlink_metadata(&candidate)?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(denied("include path traverses a reparse point"));
            }
        }

        let file = File::open(&candidate)?;
        if !file.metadata()?.is_file() {
            return Err(denied("include target is not a regular file"));
        }
        let final_path = final_path(&file)?;
        if final_path == canonical_root || !final_path.starts_with(&canonical_root) {
            return Err(denied("include target resolves outside the approved root"));
        }
        Ok(file)
    }
}

pub(super) fn callback_parts(
    resolver: Option<&mut RootResolver>,
) -> (Option<CSourceResolver>, *mut c_void) {
    resolver.map_or((None, ptr::null_mut()), |resolver| {
        (
            Some(resolve_source as CSourceResolver),
            ptr::from_mut(resolver).cast(),
        )
    })
}

extern "C" fn resolve_source(
    context: *mut c_void,
    requested: *const c_char,
    current: *const c_char,
    output: *mut CResolvedSource,
) -> i32 {
    if context.is_null() || requested.is_null() || output.is_null() {
        return RESOLVE_IO;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let resolver = unsafe { &mut *context.cast::<RootResolver>() };
        let requested = unsafe { CStr::from_ptr(requested) }
            .to_str()
            .map_err(|_| denied("include path is not UTF-8"))?;
        let current = if current.is_null() {
            None
        } else {
            Some(
                unsafe { CStr::from_ptr(current) }
                    .to_str()
                    .map_err(|_| denied("current source path is not UTF-8"))?,
            )
        };
        resolver.resolve(requested, current)?;
        unsafe {
            *output = CResolvedSource {
                path: resolver.logical_path.as_ptr(),
                data: resolver.data.as_ptr(),
                length: resolver.data.len(),
            };
        }
        Ok::<(), io::Error>(())
    }));
    match result {
        Ok(Ok(())) => 1,
        Ok(Err(error)) if error.kind() == io::ErrorKind::NotFound => RESOLVE_NOT_FOUND,
        Ok(Err(error))
            if matches!(
                error.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::PermissionDenied
            ) =>
        {
            RESOLVE_DENIED
        }
        Ok(Err(_)) | Err(_) => RESOLVE_IO,
    }
}

fn safe_relative_path(path: &str) -> io::Result<PathBuf> {
    if path.is_empty() || path.contains('\\') || path.contains(':') {
        return Err(denied("include path is not a relative POSIX path"));
    }
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(denied("include path escapes the approved root"));
    }
    Ok(path.to_path_buf())
}

fn absolute_path(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir().map(|current_dir| current_dir.join(path))
    }
}

fn logical_source_parent(root: &Path, source: &Path) -> Option<PathBuf> {
    let source = absolute_path(source).ok()?;
    let relative = source.strip_prefix(root).ok()?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    relative.parent().map(Path::to_path_buf)
}

fn logical_label(path: &Path) -> io::Result<String> {
    let components = path
        .components()
        .map(|component| match component {
            Component::Normal(component) => component
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| denied("include path is not UTF-8")),
            _ => Err(denied("include path escapes the approved root")),
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(components.join("/"))
}

fn final_path(file: &File) -> io::Result<PathBuf> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut buffer = vec![0_u16; 260];
    loop {
        let length = unsafe {
            GetFinalPathNameByHandleW(
                handle,
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(u32::MAX),
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = usize::try_from(length).unwrap_or(usize::MAX);
        if length < buffer.len() {
            return Ok(PathBuf::from(OsString::from_wide(&buffer[..length])));
        }
        if length > MAX_WINDOWS_PATH_U16 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resolved include path exceeds the Windows path limit",
            ));
        }
        buffer.resize(length.saturating_add(1), 0);
    }
}

fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

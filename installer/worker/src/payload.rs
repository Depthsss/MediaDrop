use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fmt, fs,
    io::{self, Read},
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsiMetadata {
    pub product_name: String,
    pub manufacturer: String,
    pub product_version: String,
    pub upgrade_code: String,
    pub template: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedMsiIdentity {
    size: u64,
    sha256_hex: String,
    metadata: MsiMetadata,
}

#[derive(Debug)]
pub enum PayloadError {
    InvalidExpectedIdentity(&'static str),
    InvalidPath(&'static str),
    Io(io::Error),
    SizeMismatch {
        expected: u64,
        actual: u64,
    },
    HashMismatch,
    MetadataMismatch {
        field: &'static str,
        expected: String,
        actual: String,
    },
    WindowsInstaller {
        operation: &'static str,
        code: u32,
    },
}

impl fmt::Display for PayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidExpectedIdentity(field) => {
                write!(formatter, "invalid compiled MSI identity field: {field}")
            }
            Self::InvalidPath(reason) => write!(formatter, "invalid MSI path: {reason}"),
            Self::Io(error) => write!(formatter, "MSI payload I/O failed: {error}"),
            Self::SizeMismatch { expected, actual } => {
                write!(
                    formatter,
                    "MSI size mismatch: expected {expected}, got {actual}"
                )
            }
            Self::HashMismatch => formatter.write_str("MSI SHA-256 mismatch"),
            Self::MetadataMismatch {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "MSI {field} mismatch: expected {expected}, got {actual}"
            ),
            Self::WindowsInstaller { operation, code } => {
                write!(
                    formatter,
                    "Windows Installer {operation} failed with {code}"
                )
            }
        }
    }
}

impl Error for PayloadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for PayloadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl ExpectedMsiIdentity {
    pub fn new(size: u64, sha256_hex: String, metadata: MsiMetadata) -> Result<Self, PayloadError> {
        if size == 0 {
            return Err(PayloadError::InvalidExpectedIdentity("size"));
        }
        let sha256_hex = sha256_hex.trim().to_ascii_lowercase();
        if sha256_hex.len() != 64 || !sha256_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(PayloadError::InvalidExpectedIdentity("sha256"));
        }
        for (field, value) in [
            ("ProductName", metadata.product_name.as_str()),
            ("Manufacturer", metadata.manufacturer.as_str()),
            ("ProductVersion", metadata.product_version.as_str()),
            ("UpgradeCode", metadata.upgrade_code.as_str()),
            ("Template", metadata.template.as_str()),
        ] {
            if value.trim().is_empty() || value.chars().any(char::is_control) {
                return Err(PayloadError::InvalidExpectedIdentity(field));
            }
        }
        Ok(Self {
            size,
            sha256_hex,
            metadata,
        })
    }

    pub fn compiled() -> Result<Self, PayloadError> {
        let size = option_env!("MEDIADROP_MSI_SIZE")
            .ok_or(PayloadError::InvalidExpectedIdentity("size"))?
            .parse()
            .map_err(|_| PayloadError::InvalidExpectedIdentity("size"))?;
        Self::new(
            size,
            option_env!("MEDIADROP_MSI_SHA256")
                .ok_or(PayloadError::InvalidExpectedIdentity("sha256"))?
                .to_owned(),
            MsiMetadata {
                product_name: required_build_value(
                    option_env!("MEDIADROP_MSI_PRODUCT_NAME"),
                    "ProductName",
                )?,
                manufacturer: required_build_value(
                    option_env!("MEDIADROP_MSI_MANUFACTURER"),
                    "Manufacturer",
                )?,
                product_version: required_build_value(
                    option_env!("MEDIADROP_MSI_PRODUCT_VERSION"),
                    "ProductVersion",
                )?,
                upgrade_code: required_build_value(
                    option_env!("MEDIADROP_MSI_UPGRADE_CODE"),
                    "UpgradeCode",
                )?,
                template: required_build_value(option_env!("MEDIADROP_MSI_TEMPLATE"), "Template")?,
            },
        )
    }

    pub fn metadata(&self) -> &MsiMetadata {
        &self.metadata
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn sha256_hex(&self) -> &str {
        &self.sha256_hex
    }

    pub fn verify_file_bytes(&self, path: &Path) -> Result<(), PayloadError> {
        validate_msi_path(path)?;
        let mut file = fs::OpenOptions::new().read(true).open(path)?;
        let actual_size = file.metadata()?.len();
        if actual_size != self.size {
            return Err(PayloadError::SizeMismatch {
                expected: self.size,
                actual: actual_size,
            });
        }
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 128 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        if format!("{:x}", hasher.finalize()) != self.sha256_hex {
            return Err(PayloadError::HashMismatch);
        }
        Ok(())
    }

    pub fn validate_metadata(&self, actual: &MsiMetadata) -> Result<(), PayloadError> {
        compare(
            "ProductName",
            &self.metadata.product_name,
            &actual.product_name,
            false,
        )?;
        compare(
            "Manufacturer",
            &self.metadata.manufacturer,
            &actual.manufacturer,
            false,
        )?;
        compare(
            "ProductVersion",
            &self.metadata.product_version,
            &actual.product_version,
            false,
        )?;
        compare(
            "UpgradeCode",
            &self.metadata.upgrade_code,
            &actual.upgrade_code,
            true,
        )?;
        compare("Template", &self.metadata.template, &actual.template, true)
    }

    pub fn verify_file(&self, path: &Path) -> Result<MsiMetadata, PayloadError> {
        self.verify_file_bytes(path)?;
        let metadata = read_msi_metadata(path)?;
        self.validate_metadata(&metadata)?;
        Ok(metadata)
    }
}

fn required_build_value(
    value: Option<&'static str>,
    field: &'static str,
) -> Result<String, PayloadError> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or(PayloadError::InvalidExpectedIdentity(field))
}

fn compare(
    field: &'static str,
    expected: &str,
    actual: &str,
    ignore_ascii_case: bool,
) -> Result<(), PayloadError> {
    let matches = if ignore_ascii_case {
        expected.eq_ignore_ascii_case(actual)
    } else {
        expected == actual
    };
    if matches {
        Ok(())
    } else {
        Err(PayloadError::MetadataMismatch {
            field,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}

fn validate_msi_path(path: &Path) -> Result<(), PayloadError> {
    if !path.is_absolute() {
        return Err(PayloadError::InvalidPath("path is not absolute"));
    }
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("msi"))
    {
        return Err(PayloadError::InvalidPath("extension is not .msi"));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(PayloadError::InvalidPath("payload is not a regular file"));
    }
    if metadata.file_type().is_symlink() {
        return Err(PayloadError::InvalidPath("symbolic links are not accepted"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(PayloadError::InvalidPath("reparse points are not accepted"));
        }
    }
    Ok(())
}

#[cfg(windows)]
pub fn read_msi_metadata(path: &Path) -> Result<MsiMetadata, PayloadError> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS},
        System::ApplicationInstallationAndServicing::{
            MsiCloseHandle, MsiDatabaseOpenViewW, MsiGetSummaryInformationW, MsiOpenDatabaseW,
            MsiRecordGetStringW, MsiSummaryInfoGetPropertyW, MsiViewExecute, MsiViewFetch,
            MSIDBOPEN_READONLY, MSIHANDLE, PID_TEMPLATE,
        },
    };

    struct Handle(MSIHANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            if self.0 != 0 {
                // SAFETY: this RAII wrapper uniquely owns the MSI handle.
                unsafe { MsiCloseHandle(self.0) };
            }
        }
    }

    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain([0]).collect()
    }

    unsafe fn property(database: MSIHANDLE, name: &'static str) -> Result<String, PayloadError> {
        let query = format!("SELECT `Value` FROM `Property` WHERE `Property`='{}'", name);
        let query: Vec<u16> = query.encode_utf16().chain([0]).collect();
        let mut view = 0;
        // SAFETY: the database handle and NUL-terminated query are valid.
        let code = unsafe { MsiDatabaseOpenViewW(database, query.as_ptr(), &mut view) };
        if code != ERROR_SUCCESS {
            return Err(PayloadError::WindowsInstaller {
                operation: "open property view",
                code,
            });
        }
        let view = Handle(view);
        // SAFETY: view is a live select view and a null record is correct for this query.
        let code = unsafe { MsiViewExecute(view.0, 0) };
        if code != ERROR_SUCCESS {
            return Err(PayloadError::WindowsInstaller {
                operation: "execute property view",
                code,
            });
        }
        let mut record = 0;
        // SAFETY: record output points to initialized storage owned below.
        let code = unsafe { MsiViewFetch(view.0, &mut record) };
        if code != ERROR_SUCCESS {
            return Err(PayloadError::WindowsInstaller {
                operation: "fetch property",
                code,
            });
        }
        let record = Handle(record);
        // SAFETY: record field 1 is the selected Value column.
        unsafe { record_string(record.0, 1) }
    }

    unsafe fn record_string(handle: MSIHANDLE, field: u32) -> Result<String, PayloadError> {
        let mut length = 0_u32;
        // SAFETY: null output with a zero length probes the required UTF-16 length.
        let probe = unsafe { MsiRecordGetStringW(handle, field, ptr::null_mut(), &mut length) };
        if probe != ERROR_MORE_DATA && probe != ERROR_SUCCESS {
            return Err(PayloadError::WindowsInstaller {
                operation: "measure record string",
                code: probe,
            });
        }
        let mut buffer = vec![0_u16; length as usize + 1];
        let mut capacity = buffer.len() as u32;
        // SAFETY: the buffer has capacity UTF-16 units and remains live for the call.
        let code =
            unsafe { MsiRecordGetStringW(handle, field, buffer.as_mut_ptr(), &mut capacity) };
        if code != ERROR_SUCCESS {
            return Err(PayloadError::WindowsInstaller {
                operation: "read record string",
                code,
            });
        }
        String::from_utf16(&buffer[..capacity as usize])
            .map_err(|_| PayloadError::InvalidPath("MSI metadata is not valid UTF-16"))
    }

    validate_msi_path(path)?;
    let path_wide = wide(path.as_os_str());
    let mut database = 0;
    // SAFETY: path_wide is NUL-terminated; output points to initialized storage.
    let code = unsafe { MsiOpenDatabaseW(path_wide.as_ptr(), MSIDBOPEN_READONLY, &mut database) };
    if code != ERROR_SUCCESS {
        return Err(PayloadError::WindowsInstaller {
            operation: "open database",
            code,
        });
    }
    let database = Handle(database);

    let mut summary = 0;
    // SAFETY: database is live and summary output is initialized storage.
    let code = unsafe { MsiGetSummaryInformationW(database.0, ptr::null(), 0, &mut summary) };
    if code != ERROR_SUCCESS {
        return Err(PayloadError::WindowsInstaller {
            operation: "open summary information",
            code,
        });
    }
    let summary = Handle(summary);
    let mut data_type = 0_u32;
    let mut integer_value = 0_i32;
    let mut file_time = windows_sys::Win32::Foundation::FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut template_length = 0_u32;
    // SAFETY: null output with zero length probes the summary string length.
    let probe = unsafe {
        MsiSummaryInfoGetPropertyW(
            summary.0,
            PID_TEMPLATE,
            &mut data_type,
            &mut integer_value,
            &mut file_time,
            ptr::null_mut(),
            &mut template_length,
        )
    };
    if probe != ERROR_MORE_DATA && probe != ERROR_SUCCESS {
        return Err(PayloadError::WindowsInstaller {
            operation: "measure package template",
            code: probe,
        });
    }
    let mut template_buffer = vec![0_u16; template_length as usize + 1];
    let mut template_capacity = template_buffer.len() as u32;
    // SAFETY: template_buffer is sized from the previous call and remains live.
    let code = unsafe {
        MsiSummaryInfoGetPropertyW(
            summary.0,
            PID_TEMPLATE,
            &mut data_type,
            &mut integer_value,
            &mut file_time,
            template_buffer.as_mut_ptr(),
            &mut template_capacity,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(PayloadError::WindowsInstaller {
            operation: "read package template",
            code,
        });
    }
    let template = String::from_utf16(&template_buffer[..template_capacity as usize])
        .map_err(|_| PayloadError::InvalidPath("MSI template is not valid UTF-16"))?;

    // SAFETY: each query is fixed text and database stays live for the calls.
    unsafe {
        Ok(MsiMetadata {
            product_name: property(database.0, "ProductName")?,
            manufacturer: property(database.0, "Manufacturer")?,
            product_version: property(database.0, "ProductVersion")?,
            upgrade_code: property(database.0, "UpgradeCode")?,
            template,
        })
    }
}

#[cfg(not(windows))]
pub fn read_msi_metadata(_path: &Path) -> Result<MsiMetadata, PayloadError> {
    Err(PayloadError::InvalidPath(
        "MSI metadata can only be read on Windows",
    ))
}

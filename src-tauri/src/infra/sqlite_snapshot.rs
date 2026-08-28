use rusqlite::{
    backup::{Backup, StepResult},
    params,
    types::ValueRef,
    Connection, OpenFlags, OptionalExtension, Row,
};
use std::path::Path;
use std::thread;
use std::time::Duration;

const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(2500);
const BACKUP_STEP_PAUSE: Duration = Duration::from_millis(25);
const BACKUP_MAX_STEPS: usize = 1000;

#[derive(Debug, PartialEq)]
pub(crate) struct ChromiumCookieRow {
    pub(crate) host_key: String,
    pub(crate) name: String,
    pub(crate) plain_value: String,
    pub(crate) encrypted_value: Vec<u8>,
    pub(crate) path: String,
    pub(crate) expires_utc: i64,
    pub(crate) is_secure: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) struct FirefoxCookieRow {
    pub(crate) host: String,
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) path: String,
    pub(crate) expiry: i64,
    pub(crate) is_secure: bool,
}

fn sqlite_error(context: &str, error: rusqlite::Error) -> String {
    format!("{}: {}", context, error)
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)
        .map_err(|error| sqlite_error("SQLite veritabani salt okunur acilamadi", error))?;
    connection
        .busy_timeout(SQLITE_BUSY_TIMEOUT)
        .map_err(|error| sqlite_error("SQLite busy timeout ayarlanamadi", error))?;
    Ok(connection)
}

fn open_backup_target(path: &Path) -> Result<Connection, String> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)
        .map_err(|error| sqlite_error("SQLite snapshot hedefi acilamadi", error))?;
    connection
        .busy_timeout(SQLITE_BUSY_TIMEOUT)
        .map_err(|error| sqlite_error("SQLite snapshot busy timeout ayarlanamadi", error))?;
    Ok(connection)
}

pub(crate) fn backup_to(source: &Path, target: &Path) -> Result<(), String> {
    let source_db = open_readonly(source)?;
    let mut target_db = open_backup_target(target)?;
    let backup = Backup::new(&source_db, &mut target_db)
        .map_err(|error| sqlite_error("SQLite backup baslatilamadi", error))?;
    let mut completed = false;

    for _ in 0..BACKUP_MAX_STEPS {
        match backup
            .step(128)
            .map_err(|error| sqlite_error("SQLite backup adimi tamamlanamadi", error))?
        {
            StepResult::Done => {
                completed = true;
                break;
            }
            StepResult::More | StepResult::Busy | StepResult::Locked => {
                thread::sleep(BACKUP_STEP_PAUSE);
            }
            _ => thread::sleep(BACKUP_STEP_PAUSE),
        }
    }

    if completed {
        Ok(())
    } else {
        Err("SQLite backup zaman asimina ugradi veya kilitli kaldi.".to_string())
    }
}

fn text_column(row: &Row<'_>, index: usize) -> rusqlite::Result<String> {
    Ok(match row.get_ref(index)? {
        ValueRef::Null => String::new(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => value.to_string(),
        ValueRef::Text(value) | ValueRef::Blob(value) => {
            String::from_utf8_lossy(value).into_owned()
        }
    })
}

fn blob_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Vec<u8>> {
    Ok(match row.get_ref(index)? {
        ValueRef::Null => Vec::new(),
        ValueRef::Integer(value) => value.to_string().into_bytes(),
        ValueRef::Real(value) => value.to_string().into_bytes(),
        ValueRef::Text(value) | ValueRef::Blob(value) => value.to_vec(),
    })
}

fn i64_column(row: &Row<'_>, index: usize) -> rusqlite::Result<i64> {
    Ok(match row.get_ref(index)? {
        ValueRef::Null | ValueRef::Blob(_) => 0,
        ValueRef::Integer(value) => value,
        ValueRef::Real(value) => value as i64,
        ValueRef::Text(value) => String::from_utf8_lossy(value).parse().unwrap_or(0),
    })
}

pub(crate) fn read_meta_version(snapshot: &Path) -> Result<i32, String> {
    let connection = open_readonly(snapshot)?;
    let value = connection
        .query_row(
            "SELECT value FROM meta WHERE key = ?1 LIMIT 1",
            params!["version"],
            |row| text_column(row, 0),
        )
        .optional()
        .map_err(|error| sqlite_error("SQLite meta surumu okunamadi", error))?;
    Ok(value
        .as_deref()
        .unwrap_or_default()
        .trim()
        .parse::<i32>()
        .unwrap_or(0))
}

pub(crate) fn read_chromium_cookie_rows(snapshot: &Path) -> Result<Vec<ChromiumCookieRow>, String> {
    let connection = open_readonly(snapshot)?;
    let mut statement = connection
        .prepare(
            "SELECT host_key, name, value, encrypted_value, path, expires_utc, is_secure \
             FROM cookies \
             WHERE host_key = ?1 OR host_key = ?2 OR host_key LIKE ?3",
        )
        .map_err(|error| sqlite_error("Chromium cookie sorgusu hazirlanamadi", error))?;
    let rows = statement
        .query_map(
            params!["instagram.com", ".instagram.com", "%.instagram.com"],
            |row| {
                Ok(ChromiumCookieRow {
                    host_key: text_column(row, 0)?,
                    name: text_column(row, 1)?,
                    plain_value: text_column(row, 2)?,
                    encrypted_value: blob_column(row, 3)?,
                    path: text_column(row, 4)?,
                    expires_utc: i64_column(row, 5)?,
                    is_secure: i64_column(row, 6)? != 0,
                })
            },
        )
        .map_err(|error| sqlite_error("Chromium cookie sorgusu calistirilamadi", error))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| sqlite_error("Chromium cookie satiri okunamadi", error))
}

pub(crate) fn read_firefox_cookie_rows(snapshot: &Path) -> Result<Vec<FirefoxCookieRow>, String> {
    let connection = open_readonly(snapshot)?;
    let mut statement = connection
        .prepare(
            "SELECT host, name, value, path, expiry, isSecure \
             FROM moz_cookies \
             WHERE host = ?1 OR host = ?2 OR host LIKE ?3",
        )
        .map_err(|error| sqlite_error("Firefox cookie sorgusu hazirlanamadi", error))?;
    let rows = statement
        .query_map(
            params!["instagram.com", ".instagram.com", "%.instagram.com"],
            |row| {
                Ok(FirefoxCookieRow {
                    host: text_column(row, 0)?,
                    name: text_column(row, 1)?,
                    value: text_column(row, 2)?,
                    path: text_column(row, 3)?,
                    expiry: i64_column(row, 4)?,
                    is_secure: i64_column(row, 5)? != 0,
                })
            },
        )
        .map_err(|error| sqlite_error("Firefox cookie sorgusu calistirilamadi", error))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| sqlite_error("Firefox cookie satiri okunamadi", error))
}

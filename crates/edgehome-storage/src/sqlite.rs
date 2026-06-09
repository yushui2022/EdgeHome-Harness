use crate::{StorageError, StorageResult};

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Text(String),
    Integer(i64),
    Null,
}

impl SqlValue {
    pub fn as_text(&self, _index: usize) -> StorageResult<Option<String>> {
        match self {
            Self::Text(value) => Ok(Some(value.clone())),
            Self::Integer(value) => Ok(Some(value.to_string())),
            Self::Null => Ok(None),
        }
    }

    pub fn as_i64(&self, index: usize) -> StorageResult<i64> {
        match self {
            Self::Integer(value) => Ok(*value),
            Self::Text(value) => value
                .parse()
                .map_err(|_| StorageError::UnexpectedColumnType(index)),
            Self::Null => Err(StorageError::UnexpectedColumnType(index)),
        }
    }
}

pub fn text(value: impl Into<String>) -> SqlValue {
    SqlValue::Text(value.into())
}

pub fn integer(value: i64) -> SqlValue {
    SqlValue::Integer(value)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlRow {
    values: Vec<SqlValue>,
}

impl SqlRow {
    pub fn new(values: Vec<SqlValue>) -> Self {
        Self { values }
    }

    pub fn text(&self, index: usize) -> StorageResult<String> {
        self.optional_text(index)?
            .ok_or(StorageError::UnexpectedColumnType(index))
    }

    pub fn optional_text(&self, index: usize) -> StorageResult<Option<String>> {
        self.values
            .get(index)
            .ok_or(StorageError::MissingColumn(index))?
            .as_text(index)
    }

    pub fn i64(&self, index: usize) -> StorageResult<i64> {
        self.values
            .get(index)
            .ok_or(StorageError::MissingColumn(index))?
            .as_i64(index)
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::{CStr, CString, c_char, c_int, c_void};
    use std::path::Path;
    use std::ptr::{self, NonNull};
    use std::sync::OnceLock;

    use super::{SqlRow, SqlValue};
    use crate::{StorageError, StorageResult};

    const SQLITE_OK: c_int = 0;
    const SQLITE_ROW: c_int = 100;
    const SQLITE_DONE: c_int = 101;
    const SQLITE_INTEGER: c_int = 1;
    const SQLITE_TEXT: c_int = 3;
    const SQLITE_NULL: c_int = 5;

    type SqliteDestructor = Option<unsafe extern "C" fn(*mut c_void)>;
    type SqliteOpen = unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> c_int;
    type SqliteClose = unsafe extern "C" fn(*mut c_void) -> c_int;
    type SqliteExec = unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        Option<
            unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
        >,
        *mut c_void,
        *mut *mut c_char,
    ) -> c_int;
    type SqliteFree = unsafe extern "C" fn(*mut c_void);
    type SqliteErrmsg = unsafe extern "C" fn(*mut c_void) -> *const c_char;
    type SqlitePrepare = unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        c_int,
        *mut *mut c_void,
        *mut *const c_char,
    ) -> c_int;
    type SqliteFinalize = unsafe extern "C" fn(*mut c_void) -> c_int;
    type SqliteBindText =
        unsafe extern "C" fn(*mut c_void, c_int, *const c_char, c_int, SqliteDestructor) -> c_int;
    type SqliteBindInt64 = unsafe extern "C" fn(*mut c_void, c_int, i64) -> c_int;
    type SqliteBindNull = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
    type SqliteStep = unsafe extern "C" fn(*mut c_void) -> c_int;
    type SqliteColumnCount = unsafe extern "C" fn(*mut c_void) -> c_int;
    type SqliteColumnType = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
    type SqliteColumnText = unsafe extern "C" fn(*mut c_void, c_int) -> *const u8;
    type SqliteColumnInt64 = unsafe extern "C" fn(*mut c_void, c_int) -> i64;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryA(lp_lib_file_name: *const c_char) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, proc_name: *const c_char) -> *mut c_void;
    }

    struct SqliteApi {
        open: SqliteOpen,
        close: SqliteClose,
        exec: SqliteExec,
        free: SqliteFree,
        errmsg: SqliteErrmsg,
        prepare_v2: SqlitePrepare,
        finalize: SqliteFinalize,
        bind_text: SqliteBindText,
        bind_int64: SqliteBindInt64,
        bind_null: SqliteBindNull,
        step: SqliteStep,
        column_count: SqliteColumnCount,
        column_type: SqliteColumnType,
        column_text: SqliteColumnText,
        column_int64: SqliteColumnInt64,
    }

    pub struct SqliteConnection {
        raw: NonNull<c_void>,
    }

    impl SqliteConnection {
        pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
            Self::open_raw(&path.as_ref().to_string_lossy())
        }

        pub fn open_in_memory() -> StorageResult<Self> {
            Self::open_raw(":memory:")
        }

        pub fn execute_batch(&self, sql: &str) -> StorageResult<()> {
            let api = api()?;
            let sql = CString::new(sql)?;
            let mut error_message: *mut c_char = ptr::null_mut();
            let rc = unsafe {
                (api.exec)(
                    self.raw.as_ptr(),
                    sql.as_ptr(),
                    None,
                    ptr::null_mut(),
                    &mut error_message,
                )
            };

            if rc == SQLITE_OK {
                return Ok(());
            }

            let message = if error_message.is_null() {
                self.error_message(api)
            } else {
                let message = unsafe { CStr::from_ptr(error_message) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { (api.free)(error_message.cast()) };
                message
            };
            Err(StorageError::Sqlite(message))
        }

        pub fn execute(&self, sql: &str, params: &[SqlValue]) -> StorageResult<usize> {
            let mut statement = Statement::prepare(self, sql)?;
            statement.bind(params)?;
            let rc = unsafe { (statement.api.step)(statement.raw.as_ptr()) };
            match rc {
                SQLITE_DONE => Ok(1),
                _ => Err(StorageError::Sqlite(self.error_message(statement.api))),
            }
        }

        pub fn query_one(&self, sql: &str, params: &[SqlValue]) -> StorageResult<Option<SqlRow>> {
            let mut rows = self.query_all(sql, params)?;
            Ok(rows.pop())
        }

        pub fn query_all(&self, sql: &str, params: &[SqlValue]) -> StorageResult<Vec<SqlRow>> {
            let mut statement = Statement::prepare(self, sql)?;
            statement.bind(params)?;

            let mut rows = Vec::new();
            loop {
                let rc = unsafe { (statement.api.step)(statement.raw.as_ptr()) };
                match rc {
                    SQLITE_ROW => rows.push(statement.current_row()?),
                    SQLITE_DONE => return Ok(rows),
                    _ => {
                        return Err(StorageError::Sqlite(self.error_message(statement.api)));
                    }
                }
            }
        }

        fn open_raw(path: &str) -> StorageResult<Self> {
            let api = api()?;
            let path = CString::new(path)?;
            let mut raw = ptr::null_mut();
            let rc = unsafe { (api.open)(path.as_ptr(), &mut raw) };
            if rc != SQLITE_OK {
                let message = if raw.is_null() {
                    format!("sqlite open failed with code {rc}")
                } else {
                    let message = unsafe { CStr::from_ptr((api.errmsg)(raw)) }
                        .to_string_lossy()
                        .into_owned();
                    unsafe {
                        (api.close)(raw);
                    }
                    message
                };
                return Err(StorageError::Sqlite(message));
            }

            let raw = NonNull::new(raw)
                .ok_or_else(|| StorageError::Sqlite("sqlite returned null handle".to_owned()))?;
            Ok(Self { raw })
        }

        fn error_message(&self, api: &SqliteApi) -> String {
            unsafe { CStr::from_ptr((api.errmsg)(self.raw.as_ptr())) }
                .to_string_lossy()
                .into_owned()
        }
    }

    impl Drop for SqliteConnection {
        fn drop(&mut self) {
            if let Ok(api) = api() {
                unsafe {
                    (api.close)(self.raw.as_ptr());
                }
            }
        }
    }

    struct Statement<'a> {
        connection: &'a SqliteConnection,
        api: &'static SqliteApi,
        raw: NonNull<c_void>,
        bound_text: Vec<CString>,
    }

    impl<'a> Statement<'a> {
        fn prepare(connection: &'a SqliteConnection, sql: &str) -> StorageResult<Self> {
            let api = api()?;
            let sql = CString::new(sql)?;
            let mut raw = ptr::null_mut();
            let rc = unsafe {
                (api.prepare_v2)(
                    connection.raw.as_ptr(),
                    sql.as_ptr(),
                    -1,
                    &mut raw,
                    ptr::null_mut(),
                )
            };
            if rc != SQLITE_OK {
                return Err(StorageError::Sqlite(connection.error_message(api)));
            }

            let raw = NonNull::new(raw)
                .ok_or_else(|| StorageError::Sqlite("sqlite returned null statement".to_owned()))?;

            Ok(Self {
                connection,
                api,
                raw,
                bound_text: Vec::new(),
            })
        }

        fn bind(&mut self, params: &[SqlValue]) -> StorageResult<()> {
            for (offset, value) in params.iter().enumerate() {
                let index = (offset + 1) as c_int;
                let rc = match value {
                    SqlValue::Text(value) => {
                        let value = CString::new(value.as_str())?;
                        let ptr = value.as_ptr();
                        self.bound_text.push(value);
                        unsafe {
                            (self.api.bind_text)(
                                self.raw.as_ptr(),
                                index,
                                ptr,
                                -1,
                                sqlite_transient(),
                            )
                        }
                    }
                    SqlValue::Integer(value) => unsafe {
                        (self.api.bind_int64)(self.raw.as_ptr(), index, *value)
                    },
                    SqlValue::Null => unsafe { (self.api.bind_null)(self.raw.as_ptr(), index) },
                };

                if rc != SQLITE_OK {
                    return Err(StorageError::Sqlite(
                        self.connection.error_message(self.api),
                    ));
                }
            }
            Ok(())
        }

        fn current_row(&self) -> StorageResult<SqlRow> {
            let column_count = unsafe { (self.api.column_count)(self.raw.as_ptr()) };
            let mut values = Vec::with_capacity(column_count as usize);

            for index in 0..column_count {
                let value_type = unsafe { (self.api.column_type)(self.raw.as_ptr(), index) };
                let value = match value_type {
                    SQLITE_INTEGER => SqlValue::Integer(unsafe {
                        (self.api.column_int64)(self.raw.as_ptr(), index)
                    }),
                    SQLITE_TEXT => {
                        let text_ptr = unsafe { (self.api.column_text)(self.raw.as_ptr(), index) };
                        if text_ptr.is_null() {
                            SqlValue::Null
                        } else {
                            let text = unsafe { CStr::from_ptr(text_ptr.cast()) }
                                .to_string_lossy()
                                .into_owned();
                            SqlValue::Text(text)
                        }
                    }
                    SQLITE_NULL => SqlValue::Null,
                    _ => {
                        let text_ptr = unsafe { (self.api.column_text)(self.raw.as_ptr(), index) };
                        if text_ptr.is_null() {
                            SqlValue::Null
                        } else {
                            let text = unsafe { CStr::from_ptr(text_ptr.cast()) }
                                .to_string_lossy()
                                .into_owned();
                            SqlValue::Text(text)
                        }
                    }
                };
                values.push(value);
            }

            Ok(SqlRow::new(values))
        }
    }

    impl Drop for Statement<'_> {
        fn drop(&mut self) {
            unsafe {
                (self.api.finalize)(self.raw.as_ptr());
            }
        }
    }

    fn sqlite_transient() -> SqliteDestructor {
        unsafe { std::mem::transmute::<isize, SqliteDestructor>(-1) }
    }

    fn api() -> StorageResult<&'static SqliteApi> {
        static API: OnceLock<Result<SqliteApi, String>> = OnceLock::new();
        let result = API.get_or_init(load_api);
        result
            .as_ref()
            .map_err(|message| StorageError::Sqlite(message.clone()))
    }

    fn load_api() -> Result<SqliteApi, String> {
        let library_name = CString::new("winsqlite3.dll").map_err(|error| error.to_string())?;
        let module = unsafe { LoadLibraryA(library_name.as_ptr()) };
        if module.is_null() {
            return Err("failed to load winsqlite3.dll".to_owned());
        }

        unsafe {
            Ok(SqliteApi {
                open: load_fn(module, "sqlite3_open")?,
                close: load_fn(module, "sqlite3_close")?,
                exec: load_fn(module, "sqlite3_exec")?,
                free: load_fn(module, "sqlite3_free")?,
                errmsg: load_fn(module, "sqlite3_errmsg")?,
                prepare_v2: load_fn(module, "sqlite3_prepare_v2")?,
                finalize: load_fn(module, "sqlite3_finalize")?,
                bind_text: load_fn(module, "sqlite3_bind_text")?,
                bind_int64: load_fn(module, "sqlite3_bind_int64")?,
                bind_null: load_fn(module, "sqlite3_bind_null")?,
                step: load_fn(module, "sqlite3_step")?,
                column_count: load_fn(module, "sqlite3_column_count")?,
                column_type: load_fn(module, "sqlite3_column_type")?,
                column_text: load_fn(module, "sqlite3_column_text")?,
                column_int64: load_fn(module, "sqlite3_column_int64")?,
            })
        }
    }

    unsafe fn load_fn<T: Copy>(module: *mut c_void, name: &str) -> Result<T, String> {
        let name = CString::new(name).map_err(|error| error.to_string())?;
        let pointer = unsafe { GetProcAddress(module, name.as_ptr()) };
        if pointer.is_null() {
            return Err(format!(
                "winsqlite3 missing symbol {}",
                name.to_string_lossy()
            ));
        }
        Ok(unsafe { std::mem::transmute_copy(&pointer) })
    }
}

#[cfg(not(windows))]
mod imp {
    use rusqlite::types::ValueRef;

    use super::{SqlRow, SqlValue};
    use crate::{StorageError, StorageResult};

    pub struct SqliteConnection {
        inner: rusqlite::Connection,
    }

    impl SqliteConnection {
        pub fn open(path: impl AsRef<std::path::Path>) -> StorageResult<Self> {
            let inner = rusqlite::Connection::open(path)
                .map_err(|error| StorageError::Sqlite(error.to_string()))?;
            Ok(Self { inner })
        }

        pub fn open_in_memory() -> StorageResult<Self> {
            let inner = rusqlite::Connection::open_in_memory()
                .map_err(|error| StorageError::Sqlite(error.to_string()))?;
            Ok(Self { inner })
        }

        pub fn execute_batch(&self, sql: &str) -> StorageResult<()> {
            self.inner
                .execute_batch(sql)
                .map_err(|error| StorageError::Sqlite(error.to_string()))
        }

        pub fn execute(&self, sql: &str, params: &[SqlValue]) -> StorageResult<usize> {
            let mut statement = self
                .inner
                .prepare(sql)
                .map_err(|error| StorageError::Sqlite(error.to_string()))?;
            bind_params(&mut statement, params)?;
            statement
                .raw_execute()
                .map_err(|error| StorageError::Sqlite(error.to_string()))
        }

        pub fn query_one(&self, sql: &str, params: &[SqlValue]) -> StorageResult<Option<SqlRow>> {
            Ok(self.query_all(sql, params)?.into_iter().next())
        }

        pub fn query_all(&self, sql: &str, params: &[SqlValue]) -> StorageResult<Vec<SqlRow>> {
            let mut statement = self
                .inner
                .prepare(sql)
                .map_err(|error| StorageError::Sqlite(error.to_string()))?;
            let column_count = statement.column_count();
            bind_params(&mut statement, params)?;
            let mut rows = statement.raw_query();
            let mut result = Vec::new();

            while let Some(row) = rows
                .next()
                .map_err(|error| StorageError::Sqlite(error.to_string()))?
            {
                let mut values = Vec::with_capacity(column_count);
                for index in 0..column_count {
                    let value = match row
                        .get_ref(index)
                        .map_err(|error| StorageError::Sqlite(error.to_string()))?
                    {
                        ValueRef::Null => SqlValue::Null,
                        ValueRef::Integer(value) => SqlValue::Integer(value),
                        ValueRef::Real(value) => SqlValue::Text(value.to_string()),
                        ValueRef::Text(value) => {
                            SqlValue::Text(String::from_utf8_lossy(value).into_owned())
                        }
                        ValueRef::Blob(value) => {
                            SqlValue::Text(String::from_utf8_lossy(value).into_owned())
                        }
                    };
                    values.push(value);
                }
                result.push(SqlRow::new(values));
            }

            Ok(result)
        }
    }

    fn bind_params(
        statement: &mut rusqlite::Statement<'_>,
        params: &[SqlValue],
    ) -> StorageResult<()> {
        for (offset, value) in params.iter().enumerate() {
            let index = offset + 1;
            match value {
                SqlValue::Text(value) => statement.raw_bind_parameter(index, value),
                SqlValue::Integer(value) => statement.raw_bind_parameter(index, value),
                SqlValue::Null => statement.raw_bind_parameter(index, rusqlite::types::Null),
            }
            .map_err(|error| StorageError::Sqlite(error.to_string()))?;
        }
        Ok(())
    }
}

pub use imp::SqliteConnection;

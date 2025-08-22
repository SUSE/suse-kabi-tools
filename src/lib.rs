// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::fmt::{Display, Formatter};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;
use std::{error, fmt, hash, io};

pub mod burst;
pub mod cli;
pub mod rules;
pub mod symtypes;
pub mod symvers;
pub mod text;

/// An error type for the crate, annotating standard errors with contextual information and
/// providing custom errors.
#[derive(Debug)]
pub enum Error {
    Context { desc: String, inner_err: Box<Error> },
    CLI(String),
    IO { desc: String, io_err: io::Error },
    Parse(String),
}

impl Error {
    /// Creates a new `Error::Context`.
    pub fn new_context<S: Into<String>>(desc: S, err: Error) -> Self {
        Self::Context {
            desc: desc.into(),
            inner_err: Box::new(err),
        }
    }

    /// Creates a new `Error::CLI`.
    pub fn new_cli<S: Into<String>>(desc: S) -> Self {
        Self::CLI(desc.into())
    }

    /// Creates a new `Error::IO`.
    pub fn new_io<S: Into<String>>(desc: S, io_err: io::Error) -> Self {
        Self::IO {
            desc: desc.into(),
            io_err,
        }
    }

    /// Creates a new `Error::Parse`.
    pub fn new_parse<S: Into<String>>(desc: S) -> Self {
        Self::Parse(desc.into())
    }
}

impl error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Context { desc, inner_err } => {
                write!(f, "{}: ", desc)?;
                inner_err.fmt(f)
            }
            Self::CLI(desc) => write!(f, "{}", desc),
            Self::IO { desc, io_err } => {
                write!(f, "{}: ", desc)?;
                io_err.fmt(f)
            }
            Self::Parse(desc) => write!(f, "{}", desc),
        }
    }
}

/// An elapsed timer to measure time of some operation.
///
/// The time is measured between when the object is instantiated and when it is dropped. A message
/// with the elapsed time is output when the object is dropped.
pub enum Timing {
    Active { desc: String, start: Instant },
    Inactive,
}

impl Timing {
    pub fn new(do_timing: bool, desc: &str) -> Self {
        if do_timing {
            Timing::Active {
                desc: desc.to_string(),
                start: Instant::now(),
            }
        } else {
            Timing::Inactive
        }
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        match self {
            Timing::Active { desc, start } => {
                eprintln!("{}: {:.3?}", desc, start.elapsed());
            }
            Timing::Inactive => {}
        }
    }
}

/// A helper extension trait to map [`std::io::Error`] to [`Error`], as
/// `write!(data).map_io_error(context)`.
trait MapIOErr {
    fn map_io_err(self, desc: &str) -> Result<(), Error>;
}

impl MapIOErr for Result<(), io::Error> {
    fn map_io_err(self, desc: &str) -> Result<(), Error> {
        self.map_err(|err| Error::new_io(desc, err))
    }
}

/// A [`std::fs::File`] wrapper that tracks the file path to provide better error context.
pub struct PathFile {
    path: PathBuf,
    file: File,
}

impl PathFile {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            file: File::open(path)?,
        })
    }

    pub fn create<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            file: File::create(path)?,
        })
    }
}

impl Read for PathFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf).map_err(|err| {
            io::Error::other(Error::new_io(
                format!("Failed to read from the file '{}'", self.path.display()),
                err,
            ))
        })
    }
}

impl Write for PathFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf).map_err(|err| {
            io::Error::other(Error::new_io(
                format!("Failed to write to the file '{}'", self.path.display()),
                err,
            ))
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush().map_err(|err| {
            io::Error::other(Error::new_io(
                format!("Failed to flush the file '{}'", self.path.display()),
                err,
            ))
        })
    }
}

/// Calculates the hash of a given value.
fn hash<T: Hash + ?Sized>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

/// Global debugging level.
pub static DEBUG_LEVEL: OnceLock<usize> = OnceLock::new();

/// Initializes the global debugging level, can be called only once.
pub fn init_debug_level(level: usize) {
    assert!(DEBUG_LEVEL.get().is_none());
    DEBUG_LEVEL.get_or_init(|| level);
}

/// Prints a formatted message to the standard error if debugging is enabled.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if *$crate::DEBUG_LEVEL.get().unwrap_or(&0) > 0 {
            eprintln!($($arg)*);
        }
    }
}

/// Asserts that `actual_desc` matches the shell wildcard pattern `expected_desc`.
#[macro_export]
macro_rules! assert_inexact {
    ($actual_desc:expr, $expected_desc:expr) => {{
        let actual_desc = $actual_desc;
        let expected_desc = $expected_desc;
        assert!(
            $crate::text::matches_wildcard(&actual_desc, &expected_desc),
            "assertion matches_wildcard(actual, expected) failed:\n  actual: {}\nexpected: {}\n",
            actual_desc,
            expected_desc,
        );
    }};
}

/// Asserts that `result` is `Ok(())`, representing success.
#[cfg(any(test, doc))]
#[macro_export]
macro_rules! assert_ok {
    ($result:expr) => {
        match $result {
            Ok(()) => {}
            result => panic!("assertion failed: {:?} is not of type Ok(())", result),
        }
    };
}

/// Asserts that `result` is `Ok` containing the `expected_inner` value.
#[cfg(any(test, doc))]
#[macro_export]
macro_rules! assert_ok_eq {
    ($result:expr, $expected_inner:expr) => {
        match $result {
            Ok(actual_inner) => assert_eq!(actual_inner, $expected_inner),
            result => panic!("assertion failed: {:?} is not of type Ok(_)", result),
        }
    };
}

/// Asserts that `result` is `Err` containing a [`Error::Parse`] error with the description
/// `expected_desc`.
#[cfg(any(test, doc))]
#[macro_export]
macro_rules! assert_parse_err {
    ($result:expr, $expected_desc:expr) => {
        match $result {
            Err($crate::Error::Parse(actual_desc)) => assert_eq!(actual_desc, $expected_desc),
            result => panic!(
                "assertion failed: {:?} is not of type Err(Error::Parse(_))",
                result
            ),
        }
    };
}

/// Asserts that `result` is `Err` containing a [`Error::Parse`] error with a description matching
/// the shell wildcard pattern `expected_desc`.
#[cfg(any(test, doc))]
#[macro_export]
macro_rules! assert_inexact_parse_err {
    ($result:expr, $expected_desc:expr) => {
        match $result {
            Err($crate::Error::Parse(actual_desc)) => {
                $crate::assert_inexact!(actual_desc, $expected_desc)
            }
            result => panic!(
                "assertion failed: {:?} is not of type Err(Error::Parse(_))",
                result
            ),
        }
    };
}

/// Concatenates literals into a string slice and returns it as `&[u8]`.
#[cfg(any(test, doc))]
#[macro_export]
macro_rules! bytes {
      ($($x:expr),* $(,)?) => { concat!($($x),*).as_bytes() };
}

/// Creates a [`Vec`] of [`String`] from a list of string literals.
#[cfg(any(test, doc))]
#[macro_export]
macro_rules! string_vec {
      ($($x:expr),* $(,)?) => (vec![$($x.to_string()),*]);
}

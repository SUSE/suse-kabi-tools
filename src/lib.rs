// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

pub mod diff;
pub mod sym;

/// An error type for the crate, annotating standard errors with contextual information and
/// providing custom errors.
#[derive(Debug)]
pub enum Error {
    IO {
        desc: String,
        io_err: std::io::Error,
    },
    Parse(String),
}

impl Error {
    /// Creates a new `Error::IO`.
    fn new_io(desc: &str, io_err: std::io::Error) -> Self {
        Error::IO {
            desc: desc.to_string(),
            io_err,
        }
    }

    /// Creates a new `Error::Parse`.
    fn new_parse(desc: &str) -> Self {
        Error::Parse(desc.to_string())
    }
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::IO { desc, io_err } => {
                write!(f, "{}: ", desc)?;
                io_err.fmt(f)
            }
            Self::Parse(desc) => write!(f, "{}", desc),
        }
    }
}

/// A helper extension trait to map [`std::io::Error`] to [`crate::Error`], as
/// `write!(data).map_io_error(context)`.
trait MapIOErr {
    fn map_io_err(self, desc: &str) -> Result<(), crate::Error>;
}

impl MapIOErr for Result<(), std::io::Error> {
    fn map_io_err(self, desc: &str) -> Result<(), crate::Error> {
        self.map_err(|err| crate::Error::new_io(desc, err))
    }
}

/// A [`std::fs::File`] wrapper that tracks the file path to provide better error context.
struct PathFile {
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
                &format!("Failed to read data from file '{}'", self.path.display()),
                err,
            ))
        })
    }
}

impl Write for PathFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf).map_err(|err| {
            io::Error::other(Error::new_io(
                &format!("Failed to write data to file '{}'", self.path.display()),
                err,
            ))
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush().map_err(|err| {
            io::Error::other(Error::new_io(
                &format!("Failed to flush data to file '{}'", self.path.display()),
                err,
            ))
        })
    }
}

/// Global debugging level.
pub static DEBUG_LEVEL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

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

/// Asserts that the value is [`Ok(())`](Ok), indicating success.
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

/// Creates a [`Vec`] of [`String`] from a list of string literals.
#[cfg(any(test, doc))]
#[macro_export]
macro_rules! string_vec {
      ($($x:expr),* $(,)?) => (vec![$($x.to_string()),*]);
}

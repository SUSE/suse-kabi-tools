// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::{debug, read_lines, PathFile};
use std::io::prelude::*;
use std::path::Path;

#[cfg(test)]
mod tests;

/// An export record.
#[derive(Debug, PartialEq)]
struct Export {
    crc: u32,
    name: String,
    module: String,
    is_gpl_only: bool,
    namespace: Option<String>,
}

impl Export {
    /// Creates a new `Export` object.
    pub fn new<S: Into<String>, T: Into<String>, U: Into<String>>(
        crc: u32,
        name: S,
        module: T,
        is_gpl_only: bool,
        namespace: Option<U>,
    ) -> Self {
        Export {
            crc,
            name: name.into(),
            module: module.into(),
            is_gpl_only,
            namespace: namespace.map(|n| n.into()),
        }
    }
}

/// A collection of export records.
type Exports = Vec<Export>;

/// A representation of a kernel ABI, loaded from symvers files.
#[derive(Debug, PartialEq)]
pub struct Symvers {
    exports: Exports,
}

impl Symvers {
    /// Creates a new empty `Symvers` object.
    pub fn new() -> Self {
        Self {
            exports: Exports::new(),
        }
    }

    /// Loads symvers data from a specified file.
    ///
    /// New symvers records are appended to the already present ones.
    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<(), crate::Error> {
        let path = path.as_ref();

        let file = PathFile::open(&path).map_err(|err| {
            crate::Error::new_io(&format!("Failed to open file '{}'", path.display()), err)
        })?;

        self.load_buffer(path, file)
    }

    /// Loads symvers data from a specified reader.
    ///
    /// The `path` should point to the symvers file name, indicating the origin of the data. New
    /// symvers records are appended to the already present ones.
    pub fn load_buffer<P: AsRef<Path>, R: Read>(
        &mut self,
        path: P,
        reader: R,
    ) -> Result<(), crate::Error> {
        let path = path.as_ref();
        debug!("Loading '{}'", path.display());

        // Read all content from the file.
        let lines = match read_lines(reader) {
            Ok(lines) => lines,
            Err(err) => return Err(crate::Error::new_io("Failed to read symvers data", err)),
        };

        // Parse all records.
        let mut new_exports = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            let export = parse_export(path, line_idx, line)?;
            new_exports.push(export);
        }

        // Add the new rules.
        self.exports.append(&mut new_exports);

        Ok(())
    }
}

/// Parses a single symvers record.
fn parse_export<P: AsRef<Path>>(
    path: P,
    line_idx: usize,
    line: &str,
) -> Result<Export, crate::Error> {
    let path = path.as_ref();
    let mut words = line.split_ascii_whitespace();

    // Parse the CRC value.
    let crc = words.next().ok_or_else(|| {
        crate::Error::new_parse(&format!(
            "{}:{}: The export does not specify a CRC",
            path.display(),
            line_idx + 1
        ))
    })?;
    if !crc.starts_with("0x") && !crc.starts_with("0X") {
        return Err(crate::Error::new_parse(&format!(
            "{}:{}: Failed to parse the CRC value '{}': string does not start with 0x or 0X",
            path.display(),
            line_idx + 1,
            crc
        )));
    }
    let crc = u32::from_str_radix(&crc[2..], 16).map_err(|err| {
        crate::Error::new_parse(&format!(
            "{}:{}: Failed to parse the CRC value '{}': {}",
            path.display(),
            line_idx + 1,
            crc,
            err
        ))
    })?;

    // Parse the export name.
    let name = words.next().ok_or_else(|| {
        crate::Error::new_parse(&format!(
            "{}:{}: The export does not specify a name",
            path.display(),
            line_idx + 1
        ))
    })?;

    // Parse the module name.
    let module = words.next().ok_or_else(|| {
        crate::Error::new_parse(&format!(
            "{}:{}: The export does not specify a module",
            path.display(),
            line_idx + 1
        ))
    })?;

    // Parse the export type.
    let export_type = words.next().ok_or_else(|| {
        crate::Error::new_parse(&format!(
            "{}:{}: The export does not specify a type",
            path.display(),
            line_idx + 1
        ))
    })?;
    let is_gpl_only = match export_type {
        "EXPORT_SYMBOL" => false,
        "EXPORT_SYMBOL_GPL" => true,
        _ => {
            return Err(crate::Error::new_parse(&format!(
            "{}:{}: Invalid export type '{}', must be either EXPORT_SYMBOL or EXPORT_SYMBOL_GPL",
            path.display(),
            line_idx + 1,
            export_type
        )))
        }
    };

    // Parse an optional namespace.
    let namespace = words.next().map(String::from);

    // Check that nothing else is left on the line.
    if let Some(word) = words.next() {
        return Err(crate::Error::new_parse(&format!(
            "{}:{}: Unexpected string '{}' found at the end of the export record",
            path.display(),
            line_idx + 1,
            word
        )));
    }

    Ok(Export::new(crc, name, module, is_gpl_only, namespace))
}

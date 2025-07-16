// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::rules::Rules;
use crate::text::read_lines;
use crate::{Error, MapIOErr, PathFile, debug};
use std::collections::HashMap;
use std::io::prelude::*;
use std::path::Path;

#[cfg(test)]
mod tests;

/// An export data.
#[derive(Debug, PartialEq)]
struct ExportInfo {
    crc: u32,
    module: String,
    is_gpl_only: bool,
    namespace: Option<String>,
}

impl ExportInfo {
    /// Creates a new `ExportInfo` object.
    pub fn new<S: Into<String>, T: Into<String>>(
        crc: u32,
        module: S,
        is_gpl_only: bool,
        namespace: Option<T>,
    ) -> Self {
        Self {
            crc,
            module: module.into(),
            is_gpl_only,
            namespace: namespace.map(|n| n.into()),
        }
    }

    /// Returns the type as a string slice.
    pub fn type_as_str(&self) -> &str {
        if self.is_gpl_only {
            "EXPORT_SYMBOL_GPL"
        } else {
            "EXPORT_SYMBOL"
        }
    }
}

/// A collection of export records.
type Exports = HashMap<String, ExportInfo>;

/// The format of the output from [`SymversCorpus::compare_with()`].
pub enum CompareFormat {
    Null,
    Pretty,
    Symbols,
}

impl CompareFormat {
    /// Obtains a [`CompareFormat`] matching the given format type, specified as a string.
    pub fn try_from_str(format: &str) -> Result<Self, Error> {
        match format {
            "null" => Ok(Self::Null),
            "pretty" => Ok(Self::Pretty),
            "symbols" => Ok(Self::Symbols),
            _ => Err(Error::new_parse(format!(
                "Unrecognized format '{}'",
                format
            ))),
        }
    }
}

/// A representation of a kernel ABI, loaded from symvers files.
#[derive(Debug, Default, PartialEq)]
pub struct SymversCorpus {
    exports: Exports,
}

impl SymversCorpus {
    /// Creates a new empty `SymversCorpus` object.
    pub fn new() -> Self {
        Self {
            exports: Exports::new(),
        }
    }

    /// Loads symvers data from a specified file.
    ///
    /// New symvers records are appended to the already present ones.
    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let path = path.as_ref();

        let file = PathFile::open(path).map_err(|err| {
            Error::new_io(format!("Failed to open file '{}'", path.display()), err)
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
    ) -> Result<(), Error> {
        let path = path.as_ref();
        debug!("Loading symvers data from '{}'", path.display());

        // Read all content from the file.
        let lines = match read_lines(reader) {
            Ok(lines) => lines,
            Err(err) => return Err(Error::new_io("Failed to read symvers data", err)),
        };

        // Parse all records.
        let mut new_exports = Exports::new();
        for (line_idx, line) in lines.iter().enumerate() {
            let (name, info) = parse_export(path, line_idx, line)?;

            // Check if the record is a duplicate of another one.
            if new_exports.contains_key(&name) || self.exports.contains_key(&name) {
                return Err(Error::new_parse(format!(
                    "{}:{}: Duplicate record '{}'",
                    path.display(),
                    line_idx + 1,
                    name,
                )));
            }

            new_exports.insert(name, info);
        }

        // Add the new exports.
        self.exports.extend(new_exports);

        Ok(())
    }

    /// Compares symbols in `self` and `other_symvers`.
    ///
    /// Reports any found changes to the provided output streams, formatted as requested. Returns
    /// [`Ok`] containing a `bool` indicating whether the corpuses are the same, or [`Err`] on
    /// error.
    pub fn compare_with<W: Write>(
        &self,
        other_symvers: &SymversCorpus,
        maybe_rules: Option<&Rules>,
        writers: &mut [(CompareFormat, W)],
    ) -> Result<bool, Error> {
        // A helper function to handle common logic related to reporting a change. It determines if
        // the change should be tolerated and updates the is_equal result.
        fn process_change(
            maybe_rules: Option<&Rules>,
            name: &str,
            info: &ExportInfo,
            always_tolerated: bool,
            is_equal: &mut bool,
        ) -> bool {
            let tolerated = always_tolerated
                || match maybe_rules {
                    Some(rules) => {
                        rules.is_tolerated(name, &info.module, info.namespace.as_deref())
                    }
                    None => false,
                };
            if !tolerated {
                *is_equal = false;
            }
            tolerated
        }

        // A helper function to obtain the "(tolerated)" suffix string.
        fn tolerated_suffix(tolerated: bool) -> &'static str {
            if tolerated { " (tolerated)" } else { "" }
        }

        let err_desc = "Failed to write a comparison result";

        let mut names = self.exports.keys().collect::<Vec<_>>();
        names.sort();
        let mut other_names = other_symvers.exports.keys().collect::<Vec<_>>();
        other_names.sort();
        let mut is_equal = true;

        // Check for symbols in self but not in other_symvers, and vice versa.
        //
        // Note that this code and all other checks below use the original symvers to consult the
        // severity rules. That is, the original module and namespace values are matched against the
        // rule patterns. A subtle detail is that added symbols, which lack a record in the original
        // symvers, are always tolerated, so no rules come into play.
        for (names_a, exports_a, exports_b, change, always_tolerated) in [
            (
                &names,
                &self.exports,
                &other_symvers.exports,
                "removed",
                false,
            ),
            (
                &other_names,
                &other_symvers.exports,
                &self.exports,
                "added",
                true,
            ),
        ] {
            for &name in names_a {
                if !exports_b.contains_key(name) {
                    let info = exports_a.get(name).unwrap();
                    let tolerated =
                        process_change(maybe_rules, name, info, always_tolerated, &mut is_equal);
                    for (format, writer) in &mut *writers {
                        match format {
                            CompareFormat::Null => {}
                            CompareFormat::Pretty => writeln!(
                                writer,
                                "Export '{}' has been {}{}",
                                name,
                                change,
                                tolerated_suffix(tolerated)
                            )
                            .map_io_err(err_desc)?,
                            CompareFormat::Symbols => {
                                if !tolerated {
                                    writeln!(writer, "{}", name).map_io_err(err_desc)?
                                }
                            }
                        }
                    }
                }
            }
        }

        // Compare symbols that are in both symvers.
        for name in names {
            if let Some(other_info) = other_symvers.exports.get(name) {
                let info = self.exports.get(name).unwrap();
                if info.crc != other_info.crc {
                    let tolerated = process_change(maybe_rules, name, info, false, &mut is_equal);
                    for (format, writer) in &mut *writers {
                        match format {
                            CompareFormat::Null => {}
                            CompareFormat::Pretty => writeln!(
                                writer,
                                "Export '{}' changed CRC from '{:#010x}' to '{:#010x}'{}",
                                name,
                                info.crc,
                                other_info.crc,
                                tolerated_suffix(tolerated)
                            )
                            .map_io_err(err_desc)?,
                            CompareFormat::Symbols => {
                                if !tolerated {
                                    writeln!(writer, "{}", name).map_io_err(err_desc)?
                                }
                            }
                        }
                    }
                }
                if info.is_gpl_only != other_info.is_gpl_only {
                    let tolerated = process_change(
                        maybe_rules,
                        name,
                        info,
                        info.is_gpl_only && !other_info.is_gpl_only,
                        &mut is_equal,
                    );
                    for (format, writer) in &mut *writers {
                        match format {
                            CompareFormat::Null => {}
                            CompareFormat::Pretty => writeln!(
                                writer,
                                "Export '{}' changed type from '{}' to '{}'{}",
                                name,
                                info.type_as_str(),
                                other_info.type_as_str(),
                                tolerated_suffix(tolerated)
                            )
                            .map_io_err(err_desc)?,
                            CompareFormat::Symbols => {
                                if !tolerated {
                                    writeln!(writer, "{}", name).map_io_err(err_desc)?
                                }
                            }
                        }
                    }
                }
            }
        }

        // TODO Flush all buffers.

        Ok(is_equal)
    }
}

/// Parses a single symvers record.
fn parse_export(path: &Path, line_idx: usize, line: &str) -> Result<(String, ExportInfo), Error> {
    let mut words = line.split_ascii_whitespace();

    // Parse the CRC value.
    let crc = words.next().ok_or_else(|| {
        Error::new_parse(format!(
            "{}:{}: The export does not specify a CRC",
            path.display(),
            line_idx + 1
        ))
    })?;
    if !crc.starts_with("0x") && !crc.starts_with("0X") {
        return Err(Error::new_parse(format!(
            "{}:{}: Failed to parse the CRC value '{}': string does not start with 0x or 0X",
            path.display(),
            line_idx + 1,
            crc
        )));
    }
    let crc = u32::from_str_radix(&crc[2..], 16).map_err(|err| {
        Error::new_parse(format!(
            "{}:{}: Failed to parse the CRC value '{}': {}",
            path.display(),
            line_idx + 1,
            crc,
            err
        ))
    })?;

    // Parse the export name.
    let name = words.next().ok_or_else(|| {
        Error::new_parse(format!(
            "{}:{}: The export does not specify a name",
            path.display(),
            line_idx + 1
        ))
    })?;

    // Parse the module name.
    let module = words.next().ok_or_else(|| {
        Error::new_parse(format!(
            "{}:{}: The export does not specify a module",
            path.display(),
            line_idx + 1
        ))
    })?;

    // Parse the export type.
    let export_type = words.next().ok_or_else(|| {
        Error::new_parse(format!(
            "{}:{}: The export does not specify a type",
            path.display(),
            line_idx + 1
        ))
    })?;
    let is_gpl_only = match export_type {
        "EXPORT_SYMBOL" => false,
        "EXPORT_SYMBOL_GPL" => true,
        _ => {
            return Err(Error::new_parse(format!(
                "{}:{}: Invalid export type '{}', must be either EXPORT_SYMBOL or EXPORT_SYMBOL_GPL",
                path.display(),
                line_idx + 1,
                export_type
            )));
        }
    };

    // Parse an optional namespace.
    let namespace = words.next().map(String::from);

    // Check that nothing else is left on the line.
    if let Some(word) = words.next() {
        return Err(Error::new_parse(format!(
            "{}:{}: Unexpected string '{}' found at the end of the export record",
            path.display(),
            line_idx + 1,
            word
        )));
    }

    Ok((
        name.to_string(),
        ExportInfo::new(crc, module, is_gpl_only, namespace),
    ))
}

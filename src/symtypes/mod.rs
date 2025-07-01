// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::text::{Filter, read_lines, unified_diff};
use crate::{MapIOErr, PathFile, debug};
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::{HashMap, HashSet};
use std::io::{BufWriter, prelude::*};
use std::iter::zip;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};
use std::{fs, io, mem, thread};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_format;

// Notes:
// [1] The module uses several HashMaps that are indexed by Strings. Rust allows to do a lookup in
//     such a HashMap using &str. Unfortunately, stable Rust (1.84) currently doesn't offer to do
//     this lookup but insert the key as String if it is missing. Depending on a specific case and
//     what is likely to produce less overhead, the code opts to turn the key already to a String on
//     the first lookup, or opts to run the search again if the key is missing and needs inserting.
// [2] HashSet in the stable Rust (1.84) doesn't provide the entry functionality. It is
//     a nightly-only experimental API and so not used by the module.

/// A token used in the description of a type.
#[derive(Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
enum Token {
    TypeRef(String),
    Atom(String),
}

impl Token {
    /// Creates a new `Token::TypeRef`.
    fn new_typeref<S: Into<String>>(name: S) -> Self {
        Self::TypeRef(name.into())
    }

    /// Creates a new `Token::Atom`.
    fn new_atom<S: Into<String>>(name: S) -> Self {
        Self::Atom(name.into())
    }

    /// Returns the token data as a string slice.
    fn as_str(&self) -> &str {
        match self {
            Self::TypeRef(ref_name) => ref_name.as_str(),
            Self::Atom(word) => word.as_str(),
        }
    }
}

/// A sequence of tokens, describing one type.
type Tokens = Vec<Token>;

/// A collection of all variants of the same type name in a given corpus.
type TypeVariants = Vec<Tokens>;

/// A mapping from a type name to all its known variants.
type Types = HashMap<String, TypeVariants>;

/// A mapping from a symbol name to an index in `SymtypesFiles`, specifying in which file the symbol
/// is defined.
type Exports = HashMap<String, usize>;

/// A mapping from a type name to an index in `TypeVariants`, specifying its variant in a given
/// file.
type FileRecords = HashMap<String, usize>;

/// A representation of a single symtypes file.
#[derive(Debug, Eq, PartialEq)]
struct SymtypesFile {
    path: PathBuf,
    records: FileRecords,
}

/// A collection of symtypes files.
type SymtypesFiles = Vec<SymtypesFile>;

/// A representation of a kernel ABI, loaded from symtypes files.
///
/// * The `types` collection stores all types and their variants.
/// * The `files` collection records types in individual symtypes files. Each type uses an index to
///   reference its variant in `types`.
/// * The `exports` collection provides all exports in the corpus. Each export uses an index to
///   reference its origin in `files`.
///
/// For instance, consider the following corpus consisting of two files `test_a.symtypes` and
/// `test_b.symtypes`:
///
/// * `test_a.symtypes`:
///
///   ```text
///   s#foo struct foo { int a ; }
///   bar int bar ( s#foo )
///   ```
///
/// * `test_b.symtypes`:
///
///   ```text
///   s#foo struct foo { UNKNOWN }
///   baz int baz ( s#foo )
///   ```
///
/// The corpus has two exports `bar` and `baz`, with each referencing structure `foo`, but with
/// different definitions, one is complete and one is incomplete.
///
/// The data would be represented as follows:
///
/// ```text
/// SymtypesCorpus {
///     types: Types {
///         "s#foo": TypeVariants[
///             Tokens[Atom("struct"), Atom("foo"), Atom("{"), Atom("int"), Atom("a"), Atom(";"), Atom("}")],
///             Tokens[Atom("struct"), Atom("foo"), Atom("{"), Atom("UNKNOWN"), Atom("}")],
///         ],
///         "bar": TypeVariants[
///             Tokens[Atom("int"), Atom("bar"), Atom("("), TypeRef("s#foo"), Atom(")")],
///         ],
///         "baz": TypeVariants[
///             Tokens[Atom("int"), Atom("baz"), Atom("("), TypeRef("s#foo"), Atom(")")],
///         ],
///     },
///     exports: Exports {
///         "bar": 0,
///         "baz": 1,
///     },
///     files: SymtypesFiles[
///         SymtypesFile {
///             path: PathBuf("test_a.symtypes"),
///             records: FileRecords {
///                 "s#foo": 0,
///                 "bar": 0,
///             }
///         },
///         SymtypesFile {
///             path: PathBuf("test_b.symtypes"),
///             records: FileRecords {
///                 "s#foo": 1,
///                 "baz": 0,
///             }
///         },
///     ],
/// }
/// ```
///
/// Note importantly that if a `Token` in `TypeVariants` is a `TypeRef` then the reference only
/// specifies a name of the target type, e.g. `s#foo` above. The actual type variant must be
/// determined based on what file is being processed. This allows to trivially merge `Tokens` and
/// limit memory needed to store the corpus. On the other hand, when comparing two `Tokens` vectors
/// for ABI equality, the code needs to consider whether all referenced subtypes are actually equal
/// as well.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct SymtypesCorpus {
    types: Types,
    exports: Exports,
    files: SymtypesFiles,
}

/// A helper struct to provide synchronized access to `SymtypesCorpus` data during parallel loading.
struct LoadContext<'a> {
    types: RwLock<&'a mut Types>,
    exports: Mutex<&'a mut Exports>,
    files: Mutex<&'a mut SymtypesFiles>,
}

/// Type names active during the loading of a specific file, providing for each type its variant and
/// source line index.
type LoadActiveTypes = HashMap<String, (usize, usize)>;

/// Type names processed during the consolidation for a specific file, providing for each type its
/// variant index.
type ConsolidateFileTypes<'a> = HashMap<&'a str, usize>;

/// Changes between two corpuses, recording a tuple of each modified type's name, its old tokens and
/// its new tokens, along with a [`Vec`] of exported symbols affected by the change.
type CompareChangedTypes<'a> = HashMap<(&'a str, &'a Tokens, &'a Tokens), Vec<&'a str>>;

/// Type names processed during the comparison for a specific file.
type CompareFileTypes<'a> = HashSet<&'a str>;

impl SymtypesCorpus {
    /// Creates a new empty corpus.
    pub fn new() -> Self {
        Self {
            types: Types::new(),
            exports: Exports::new(),
            files: SymtypesFiles::new(),
        }
    }

    /// Loads symtypes data from a given location.
    ///
    /// The `path` can point to a single symtypes file or a directory. In the latter case, the
    /// function recursively collects all symtypes in that directory and loads them.
    pub fn load<P: AsRef<Path>>(&mut self, path: P, num_workers: i32) -> Result<(), crate::Error> {
        let path = path.as_ref();

        // Determine if the input is a directory tree or a single symtypes file.
        let md = fs::metadata(path).map_err(|err| {
            crate::Error::new_io(format!("Failed to query path '{}'", path.display()), err)
        })?;

        if md.is_dir() {
            // Recursively collect symtypes files within the directory.
            let mut symfiles = Vec::new();
            Self::collect_symfiles(path, "", &mut symfiles)?;

            // Load all found files.
            self.load_symfiles(path, &symfiles, num_workers)
        } else {
            // Load the single file.
            self.load_symfiles("", &[path], num_workers)
        }
    }

    /// Collects recursively all symtypes files under the given root path and its subpath.
    fn collect_symfiles<P: AsRef<Path>, Q: AsRef<Path>>(
        root: P,
        sub_path: Q,
        symfiles: &mut Vec<PathBuf>,
    ) -> Result<(), crate::Error> {
        let root = root.as_ref();
        let sub_path = sub_path.as_ref();

        let path = root.join(sub_path);

        let dir_iter = fs::read_dir(&path).map_err(|err| {
            crate::Error::new_io(
                format!("Failed to read directory '{}'", path.display()),
                err,
            )
        })?;

        for maybe_entry in dir_iter {
            let entry = maybe_entry.map_err(|err| {
                crate::Error::new_io(
                    format!("Failed to read directory '{}'", path.display()),
                    err,
                )
            })?;

            let entry_path = entry.path();

            let md = fs::symlink_metadata(&entry_path).map_err(|err| {
                crate::Error::new_io(
                    format!("Failed to query path '{}'", entry_path.display()),
                    err,
                )
            })?;

            if md.is_symlink() {
                continue;
            }

            let entry_sub_path = sub_path.join(entry.file_name());

            if md.is_dir() {
                Self::collect_symfiles(root, &entry_sub_path, symfiles)?;
                continue;
            }

            let ext = match entry_sub_path.extension() {
                Some(ext) => ext,
                None => continue,
            };
            if ext == "symtypes" {
                symfiles.push(entry_sub_path);
            }
        }
        Ok(())
    }

    /// Loads all specified symtypes files.
    fn load_symfiles<P: AsRef<Path>, Q: AsRef<Path> + Sync>(
        &mut self,
        root: P,
        symfiles: &[Q],
        num_workers: i32,
    ) -> Result<(), crate::Error> {
        let root = root.as_ref();

        // Load data from the files.
        let next_work_idx = AtomicUsize::new(0);

        let load_context = LoadContext {
            types: RwLock::new(&mut self.types),
            exports: Mutex::new(&mut self.exports),
            files: Mutex::new(&mut self.files),
        };

        thread::scope(|s| {
            let mut workers = Vec::new();
            for _ in 0..num_workers {
                workers.push(s.spawn(|| -> Result<(), crate::Error> {
                    loop {
                        let work_idx = next_work_idx.fetch_add(1, Ordering::Relaxed);
                        if work_idx >= symfiles.len() {
                            return Ok(());
                        }
                        let sub_path = &symfiles[work_idx].as_ref();

                        let path = root.join(sub_path);
                        let file = PathFile::open(&path).map_err(|err| {
                            crate::Error::new_io(
                                format!("Failed to open file '{}'", path.display()),
                                err,
                            )
                        })?;

                        Self::load_inner(sub_path, file, &load_context)?;
                    }
                }));
            }

            // Join all worker threads. Return the first error if any is found, others are silently
            // swallowed which is ok.
            for worker in workers {
                worker.join().unwrap()?
            }

            Ok(())
        })
    }

    /// Loads symtypes data from a specified reader.
    ///
    /// The `path` should point to a symtypes file name, indicating the origin of the data.
    pub fn load_buffer<P: AsRef<Path>, R: Read>(
        &mut self,
        path: P,
        reader: R,
    ) -> Result<(), crate::Error> {
        let load_context = LoadContext {
            types: RwLock::new(&mut self.types),
            exports: Mutex::new(&mut self.exports),
            files: Mutex::new(&mut self.files),
        };

        Self::load_inner(path, reader, &load_context)?;

        Ok(())
    }

    /// Loads symtypes data from a specified reader.
    fn load_inner<P: AsRef<Path>, R: Read>(
        path: P,
        reader: R,
        load_context: &LoadContext,
    ) -> Result<(), crate::Error> {
        let path = path.as_ref();
        debug!("Loading symtypes data from '{}'", path.display());

        // Read all content from the file.
        let lines = match read_lines(reader) {
            Ok(lines) => lines,
            Err(err) => return Err(crate::Error::new_io("Failed to read symtypes data", err)),
        };

        // Detect whether the input is a single or consolidated symtypes file.
        let is_consolidated =
            !lines.is_empty() && lines[0].starts_with("/* ") && lines[0].ends_with(" */");

        let mut file_idx = if !is_consolidated {
            Self::add_file(path, load_context)
        } else {
            usize::MAX
        };

        // Track which records are currently active and all per-file overrides for UNKNOWN
        // definitions if this is a consolidated file.
        let mut active_types = LoadActiveTypes::new();
        let mut local_override = LoadActiveTypes::new();

        let mut records = FileRecords::new();

        // Parse all declarations.
        for (line_idx, line) in lines.iter().enumerate() {
            // Skip empty lines in consolidated files.
            if is_consolidated && line.is_empty() {
                continue;
            }

            // Handle file headers in consolidated files.
            if is_consolidated && line.starts_with("/* ") && line.ends_with(" */") {
                // Complete the current file.
                if file_idx != usize::MAX {
                    Self::close_file(
                        path,
                        file_idx,
                        mem::take(&mut records),
                        mem::take(&mut local_override),
                        &active_types,
                        load_context,
                    )?;
                }

                // Open the new file.
                file_idx = Self::add_file(&line[3..line.len() - 3], load_context);

                continue;
            }

            // Ok, it is a regular record, parse it.
            let (name, tokens, is_local_override) =
                parse_type_record(path, line_idx, line, is_consolidated)?;

            // Check if the record is a duplicate of another one.
            if records.contains_key(&name) {
                return Err(crate::Error::new_parse(format!(
                    "{}:{}: Duplicate record '{}'",
                    path.display(),
                    line_idx + 1,
                    name,
                )));
            }

            // Insert the type into the corpus and file records.
            let variant_idx = Self::merge_type(&name, tokens, load_context);
            if is_export_name(&name) {
                Self::insert_export(&name, file_idx, line_idx, load_context)?;
            }
            records.insert(name.clone(), variant_idx);

            // Record the type as currently active.
            if is_local_override {
                local_override.insert(name, (variant_idx, line_idx));
            } else {
                active_types.insert(name, (variant_idx, line_idx));
            }
        }

        // Complete the file.
        if file_idx != usize::MAX {
            Self::close_file(
                path,
                file_idx,
                records,
                local_override,
                &active_types,
                load_context,
            )?;
        }

        Ok(())
    }

    /// Adds a specified file to the corpus.
    ///
    /// Note that in the case of a consolidated file, unlike most load functions, the `path` should
    /// point to the name of the specific symtypes file.
    fn add_file<P: AsRef<Path>>(path: P, load_context: &LoadContext) -> usize {
        let path = path.as_ref();

        let symfile = SymtypesFile {
            path: path.to_path_buf(),
            records: FileRecords::new(),
        };

        let mut files = load_context.files.lock().unwrap();
        files.push(symfile);
        files.len() - 1
    }

    /// Completes loading of the symtypes file specified by `file_idx` by extrapolating its records,
    /// validating all references, and finally adding the file records to the corpus.
    fn close_file<P: AsRef<Path>>(
        path: P,
        file_idx: usize,
        mut records: FileRecords,
        local_override: LoadActiveTypes,
        active_types: &LoadActiveTypes,
        load_context: &LoadContext,
    ) -> Result<(), crate::Error> {
        let path = path.as_ref();

        // Extrapolate all records and validate references.
        let walk_records = records.keys().map(String::clone).collect::<Vec<_>>();
        for name in walk_records {
            let types = load_context.types.read().unwrap();
            // Note that all explicit types are known, so it is ok to pass usize::MAX as
            // from_line_idx because it is unused.
            Self::complete_file_record(
                path,
                usize::MAX,
                &name,
                true,
                &local_override,
                active_types,
                *types,
                &mut records,
            )?;
        }

        // Add the file records to the corpus.
        let mut files = load_context.files.lock().unwrap();
        files[file_idx].records = records;

        Ok(())
    }

    /// Adds the given type definition to the corpus if not already present, and returns its variant
    /// index.
    fn merge_type(type_name: &str, tokens: Tokens, load_context: &LoadContext) -> usize {
        // Types are often repeated in different symtypes files. Try to find an existing type only
        // under the read lock first.
        {
            let types = load_context.types.read().unwrap();
            if let Some(variants) = types.get(type_name) {
                for (i, variant) in variants.iter().enumerate() {
                    if tokens == *variant {
                        return i;
                    }
                }
            }
        }

        let mut types = load_context.types.write().unwrap();
        match types.get_mut(type_name) {
            Some(variants) => {
                for (i, variant) in variants.iter().enumerate() {
                    if tokens == *variant {
                        return i;
                    }
                }
                variants.push(tokens);
                variants.len() - 1
            }
            None => {
                types.insert(type_name.to_string(), vec![tokens]); // [1]
                0
            }
        }
    }

    /// Registers the specified export in the corpus and validates that it is not a duplicate.
    fn insert_export(
        type_name: &str,
        file_idx: usize,
        line_idx: usize,
        load_context: &LoadContext,
    ) -> Result<(), crate::Error> {
        // Try to add the export, return an error if it is a duplicate.
        let other_file_idx = {
            let mut exports = load_context.exports.lock().unwrap();
            match exports.entry(type_name.to_string()) // [1]
            {
                Occupied(export_entry) => *export_entry.get(),
                Vacant(export_entry) => {
                    export_entry.insert(file_idx);
                    return Ok(());
                }
            }
        };

        let files = load_context.files.lock().unwrap();
        let path = &files[file_idx].path;
        let other_path = &files[other_file_idx].path;
        Err(crate::Error::new_parse(format!(
            "{}:{}: Export '{}' is duplicate, previous occurrence found in '{}'",
            path.display(),
            line_idx + 1,
            type_name,
            other_path.display()
        )))
    }

    /// Completes a type record by validating all its references and, in the case of a consolidated
    /// source, enhances the specified file records with the necessary implicit types.
    ///
    /// In a consolidated file, a file entry can omit types that the file contains if those types
    /// were previously defined by another file. This function finds all such implicit references
    /// and adds them to `records`.
    ///
    /// A caller of this function should pre-fill `records` with all explicit types present in
    /// a file entry and then call this function on each of those types. These root calls should be
    /// invoked with `is_explicit` set to `true`. The function then recursively adds all needed
    /// implicit types that are referenced from these roots.
    fn complete_file_record(
        path: &Path,
        from_line_idx: usize,
        type_name: &str,
        is_explicit: bool,
        local_override: &LoadActiveTypes,
        active_types: &LoadActiveTypes,
        types: &Types,
        records: &mut FileRecords,
    ) -> Result<(), crate::Error> {
        if is_explicit {
            // All explicit symbols need to be added by the caller.
            assert!(records.get(type_name).is_some());
        } else {
            // See if the symbol was already processed.
            if records.get(type_name).is_some() {
                return Ok(());
            }
        }

        let (variant_idx, line_idx) = match local_override.get(type_name) {
            Some(&(variant_idx, line_idx)) => (variant_idx, line_idx),
            None => *active_types.get(type_name).ok_or_else(|| {
                crate::Error::new_parse(format!(
                    "{}:{}: Type '{}' is not known",
                    path.display(),
                    from_line_idx + 1,
                    type_name
                ))
            })?,
        };
        if !is_explicit {
            records.insert(type_name.to_string(), variant_idx); // [1]
        }

        // Look up the type definition.
        // SAFETY: Each type reference is guaranteed to have a corresponding definition.
        let variants = types.get(type_name).unwrap();
        let tokens = &variants[variant_idx];

        // Process recursively all types referenced by this symbol.
        for token in tokens {
            match token {
                Token::TypeRef(ref_name) => {
                    Self::complete_file_record(
                        path,
                        line_idx,
                        ref_name,
                        false,
                        local_override,
                        active_types,
                        types,
                        records,
                    )?;
                }
                Token::Atom(_word) => {}
            }
        }

        Ok(())
    }

    /// Processes a single symbol in a given file and adds it to the consolidated output.
    ///
    /// The specified symbol and its required variant is added to `file_types`, if it's not already
    /// present. All of its type references are then recursively processed in the same way.
    fn consolidate_type<'a>(
        &'a self,
        symfile: &SymtypesFile,
        name: &'a str,
        file_types: &mut ConsolidateFileTypes<'a>,
    ) {
        // See if the symbol was already processed.
        let file_type_entry = match file_types.entry(name) {
            Occupied(_) => return,
            Vacant(file_type_entry) => file_type_entry,
        };

        // Look up the type definition.
        // SAFETY: Each type reference is guaranteed to have a corresponding definition.
        let variant_idx = *symfile.records.get(name).unwrap();
        let variants = self.types.get(name).unwrap();

        // Record that the type is needed by the file.
        file_type_entry.insert(variant_idx);

        // Process recursively all types that the symbol references.
        for token in &variants[variant_idx] {
            match token {
                Token::TypeRef(ref_name) => self.consolidate_type(symfile, ref_name, file_types),
                Token::Atom(_word) => {}
            }
        }
    }

    /// Writes the corpus in the consolidated form into a specified file.
    pub fn write_consolidated<P: AsRef<Path>>(&self, path: P) -> Result<(), crate::Error> {
        let path = path.as_ref();

        // Open the output file.
        let writer: Box<dyn Write> = if path == Path::new("-") {
            Box::new(io::stdout())
        } else {
            match PathFile::create(path) {
                Ok(file) => Box::new(file),
                Err(err) => {
                    return Err(crate::Error::new_io(
                        format!("Failed to create file '{}'", path.display()),
                        err,
                    ));
                }
            }
        };

        self.write_consolidated_buffer(writer)
    }

    /// Writes the corpus in the consolidated form to the provided output stream.
    pub fn write_consolidated_buffer<W: Write>(&self, writer: W) -> Result<(), crate::Error> {
        let mut writer = BufWriter::new(writer);
        let err_desc = "Failed to write a consolidated record";

        // Track which records are currently active, mapping a type name to its active variant
        // index.
        let mut active_types: HashMap<&str, usize> = HashMap::new();

        // Sort all files in the corpus by their path.
        let mut file_indices = (0..self.files.len()).collect::<Vec<_>>();
        file_indices.sort_by_key(|&i| &self.files[i].path);

        // Process the sorted files and add their needed types to the output.
        let mut add_separator = false;
        for i in file_indices {
            let symfile = &self.files[i];

            // Collect sorted exports in the file which are the roots for consolidation.
            let mut exports = symfile
                .records
                .keys()
                .map(String::as_str)
                .filter(|name| is_export_name(name))
                .collect::<Vec<_>>();
            if exports.is_empty() {
                continue;
            }
            exports.sort();

            // Collect the exported types and their needed types.
            let mut file_types = ConsolidateFileTypes::new();
            for name in exports {
                self.consolidate_type(symfile, name, &mut file_types);
            }

            // Sort all output types.
            let mut sorted_types = file_types.into_iter().collect::<Vec<_>>();
            sorted_types.sort_by_cached_key(|&(name, _)| (is_export_name(name), name));

            // Add an empty line to separate individual files.
            if add_separator {
                writeln!(writer).map_io_err(err_desc)?;
            } else {
                add_separator = true;
            }

            // Write the file header.
            writeln!(writer, "/* {} */", symfile.path.display()).map_io_err(err_desc)?;

            // Write all output types.
            for (name, variant_idx) in sorted_types {
                // Look up the type definition.
                // SAFETY: Each type reference is guaranteed to have a corresponding definition.
                let variants = self.types.get(name).unwrap();
                let tokens = &variants[variant_idx];

                // Check if this is an UNKNOWN type definition, and if so, record it as a local
                // override.
                if let Some(short_name) = try_shorten_decl(name, tokens) {
                    writeln!(writer, "{}", short_name).map_io_err(err_desc)?;
                    continue;
                }

                // See if the symbol matches an already active definition, or record it in the
                // output.
                let record = match active_types.entry(name) {
                    Occupied(mut active_type_entry) => {
                        if *active_type_entry.get() != variant_idx {
                            active_type_entry.insert(variant_idx);
                            true
                        } else {
                            false
                        }
                    }
                    Vacant(active_type_entry) => {
                        active_type_entry.insert(variant_idx);
                        true
                    }
                };
                if record {
                    write!(writer, "{}", name).map_io_err(err_desc)?;
                    for token in tokens {
                        write!(writer, " {}", token.as_str()).map_io_err(err_desc)?;
                    }
                    writeln!(writer).map_io_err(err_desc)?;
                }
            }
        }

        Ok(())
    }

    /// Obtains tokens which describe a specified type name, in a given corpus and file.
    fn get_type_tokens<'a>(
        symtypes: &'a SymtypesCorpus,
        file: &SymtypesFile,
        name: &str,
    ) -> &'a Tokens {
        match file.records.get(name) {
            Some(&variant_idx) => match symtypes.types.get(name) {
                Some(variants) => &variants[variant_idx],
                None => {
                    panic!("Type '{}' has a missing declaration", name);
                }
            },
            None => {
                panic!(
                    "Type '{}' is not known in file '{}'",
                    name,
                    file.path.display()
                )
            }
        }
    }

    /// Compares the definition of the symbol `name` in (`corpus`, `file`) with its definition in
    /// (`other_corpus`, `other_file`).
    ///
    /// If the immediate definition of the symbol differs between the two corpuses then it gets
    /// added in `changes`. The `export` parameter identifies the top-level exported symbol affected
    /// by the change.
    ///
    /// The specified symbol is added to `processed_types`, if not already present, and all its type
    /// references get recursively processed in the same way.
    fn compare_types<'a>(
        (corpus, file): (&'a SymtypesCorpus, &'a SymtypesFile),
        (other_corpus, other_file): (&'a SymtypesCorpus, &'a SymtypesFile),
        name: &'a str,
        export: &'a str,
        changes: &Mutex<CompareChangedTypes<'a>>,
        processed: &mut CompareFileTypes<'a>,
    ) {
        // See if the symbol was already processed.
        if processed.get(name).is_some() {
            return;
        }
        processed.insert(name); // [2]

        // Look up how the symbol is defined in each corpus.
        let tokens = Self::get_type_tokens(corpus, file, name);
        let other_tokens = Self::get_type_tokens(other_corpus, other_file, name);

        // Compare the immediate tokens.
        let is_equal = tokens.len() == other_tokens.len()
            && zip(tokens.iter(), other_tokens.iter())
                .all(|(token, other_token)| token == other_token);
        if !is_equal {
            let mut changes = changes.lock().unwrap();
            changes
                .entry((name, tokens, other_tokens))
                .or_default()
                .push(export);
        }

        // Compare recursively same referenced types. This can be done trivially if the tokens are
        // equal. If they are not, try hard (and slowly) to find any matching types.
        if is_equal {
            for token in tokens {
                if let Token::TypeRef(ref_name) = token {
                    Self::compare_types(
                        (corpus, file),
                        (other_corpus, other_file),
                        ref_name.as_str(),
                        export,
                        changes,
                        processed,
                    );
                }
            }
        } else {
            for token in tokens {
                if let Token::TypeRef(ref_name) = token {
                    for other_token in other_tokens {
                        if let Token::TypeRef(other_ref_name) = other_token {
                            if ref_name == other_ref_name {
                                Self::compare_types(
                                    (corpus, file),
                                    (other_corpus, other_file),
                                    ref_name.as_str(),
                                    export,
                                    changes,
                                    processed,
                                );
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Compares symbols in the `self` and `other_corpus`.
    ///
    /// A human-readable report about all found changes is written to the provided output stream.
    pub fn compare_with<W: Write>(
        &self,
        other_corpus: &SymtypesCorpus,
        maybe_filter: Option<&Filter>,
        writer: W,
        num_workers: i32,
    ) -> Result<(), crate::Error> {
        fn matches(maybe_filter: Option<&Filter>, name: &str) -> bool {
            match maybe_filter {
                Some(filter) => filter.matches(name),
                None => true,
            }
        }

        let mut writer = BufWriter::new(writer);
        let err_desc = "Failed to write a comparison result";

        // Check for symbols in self but not in other_corpus, and vice versa.
        for (exports_a, exports_b, change) in [
            (&self.exports, &other_corpus.exports, "removed"),
            (&other_corpus.exports, &self.exports, "added"),
        ] {
            for name in exports_a.keys() {
                if matches(maybe_filter, name) && !exports_b.contains_key(name) {
                    writeln!(writer, "Export '{}' has been {}", name, change)
                        .map_io_err(err_desc)?;
                }
            }
        }

        // Compare symbols that are in both corpuses.
        let works: Vec<_> = self
            .exports
            .iter()
            .filter(|(name, _)| matches(maybe_filter, name))
            .collect();
        let next_work_idx = AtomicUsize::new(0);

        let changes = Mutex::new(CompareChangedTypes::new());

        thread::scope(|s| {
            for _ in 0..num_workers {
                s.spawn(|| {
                    loop {
                        let work_idx = next_work_idx.fetch_add(1, Ordering::Relaxed);
                        if work_idx >= works.len() {
                            break;
                        }
                        let (name, file_idx) = works[work_idx];

                        let file = &self.files[*file_idx];
                        if let Some(other_file_idx) = other_corpus.exports.get(name) {
                            let other_file = &other_corpus.files[*other_file_idx];
                            let mut processed = CompareFileTypes::new();
                            Self::compare_types(
                                (self, file),
                                (other_corpus, other_file),
                                name,
                                name,
                                &changes,
                                &mut processed,
                            );
                        }
                    }
                });
            }
        });

        // Format and output collected changes.
        let changes = changes.into_inner().unwrap(); // Get the inner HashMap.
        let mut changes = changes.into_iter().collect::<Vec<_>>();
        changes.iter_mut().for_each(|(_, exports)| exports.sort());
        changes.sort();

        let mut add_separator = false;
        for ((name, tokens, other_tokens), exports) in changes {
            // Add an empty line to separate individual changes.
            if add_separator {
                writeln!(writer).map_io_err(err_desc)?;
            } else {
                add_separator = true;
            }

            writeln!(
                writer,
                "The following '{}' exports are different:",
                exports.len()
            )
            .map_io_err(err_desc)?;
            for export in exports {
                writeln!(writer, " {}", export).map_io_err(err_desc)?;
            }
            writeln!(writer).map_io_err(err_desc)?;

            writeln!(writer, "because of a changed '{}':", name).map_io_err(err_desc)?;
            write_type_diff(tokens, other_tokens, writer.by_ref())?;
        }

        Ok(())
    }
}

/// Reads words from a given iterator and converts them to `Tokens`.
fn words_into_tokens<'a, I: Iterator<Item = &'a str>>(words: &mut I) -> Tokens {
    let mut tokens = Tokens::new();
    for word in words {
        let mut is_typeref = false;
        if let Some(ch) = word.chars().nth(1) {
            if ch == '#' {
                is_typeref = true;
            }
        }
        tokens.push(if is_typeref {
            Token::new_typeref(word)
        } else {
            Token::new_atom(word)
        });
    }
    tokens
}

/// Returns whether the specified type name is an export definition, as opposed to a `<X>#<foo>`
/// type definition.
fn is_export_name(type_name: &str) -> bool {
    match type_name.chars().nth(1) {
        Some(ch) => ch != '#',
        None => true,
    }
}

/// Tries to shorten the specified type if it represents an UNKNOWN declaration.
///
/// The function maps records like
/// `<short-type>#<name> <type> <name> { UNKNOWN }`
/// to
/// `<short-type>##<name>`.
/// For instance, `s#task_struct struct task_struct { UNKNOWN }` becomes `s##task_struct`.
fn try_shorten_decl(type_name: &str, tokens: &Tokens) -> Option<String> {
    if tokens.len() != 5 {
        return None;
    }

    if let Some((short_type, expanded_type, base_name)) = split_type_name(type_name, "#") {
        let unknown = [expanded_type, base_name, "{", "UNKNOWN", "}"];
        if zip(tokens.iter(), unknown.into_iter()).all(|(token, check)| token.as_str() == check) {
            return Some(format!("{}##{}", short_type, base_name));
        }
    }

    None
}

/// Tries to expand the specified type if it represents an UNKNOWN declaration.
///
/// The function maps records like
/// `<short-type>##<name>`
/// to
/// `<short-type>#<name> <type> <name> { UNKNOWN }`.
/// For instance, `s##task_struct` becomes `s#task_struct struct task_struct { UNKNOWN }`.
fn try_expand_decl(type_name: &str) -> Option<(String, Tokens)> {
    if let Some((short_type, expanded_type, base_name)) = split_type_name(type_name, "##") {
        let type_name = format!("{}#{}", short_type, base_name);
        let tokens = vec![
            Token::new_atom(expanded_type),
            Token::new_atom(base_name),
            Token::new_atom("{"),
            Token::new_atom("UNKNOWN"),
            Token::new_atom("}"),
        ];
        return Some((type_name, tokens));
    }

    None
}

/// Splits the specified type name into three string slices: the short type name, the long type
/// name, and the base name. For instance, `s#task_struct` is split into
/// `("s", "struct", "task_struct")`.
fn split_type_name<'a>(type_name: &'a str, delimiter: &str) -> Option<(&'a str, &'a str, &'a str)> {
    match type_name.split_once(delimiter) {
        Some((short_type, base_name)) => {
            let expanded_type = match short_type {
                "t" => "typedef",
                "e" => "enum",
                "s" => "struct",
                "u" => "union",
                _ => return None,
            };
            Some((short_type, expanded_type, base_name))
        }
        None => None,
    }
}

/// Parses a single symtypes record.
fn parse_type_record<P: AsRef<Path>>(
    path: P,
    line_idx: usize,
    line: &str,
    is_consolidated: bool,
) -> Result<(String, Tokens, bool), crate::Error> {
    let path = path.as_ref();
    let mut words = line.split_ascii_whitespace();

    let raw_name = words.next().ok_or_else(|| {
        crate::Error::new_parse(format!(
            "{}:{}: Expected a record name",
            path.display(),
            line_idx + 1
        ))
    })?;

    if is_consolidated {
        // Check if it is an UNKNOWN override.
        if let Some((name, tokens)) = try_expand_decl(raw_name) {
            // TODO Check that all words have been exhausted.
            return Ok((name, tokens, true));
        }
    }

    // TODO Check that no ## is present in the type name.

    // Turn the remaining words into tokens.
    let tokens = words_into_tokens(&mut words);
    Ok((raw_name.to_string(), tokens, false))
}

/// Processes tokens describing a type and produces its pretty-formatted version as a [`Vec`] of
/// [`String`] lines.
fn pretty_format_type(tokens: &Tokens) -> Vec<String> {
    // Iterate over all tokens and produce the formatted output.
    let mut res = Vec::new();
    let mut indent: usize = 0;

    let mut line = String::new();
    for token in tokens {
        // Handle the closing bracket and parenthesis early, they end any prior line and reduce
        // indentation.
        if token.as_str() == "}" || token.as_str() == ")" {
            if !line.is_empty() {
                res.push(line);
            }
            indent = indent.saturating_sub(1);
            line = String::new();
        }

        // Insert any newline indentation.
        let is_first = line.is_empty();
        if is_first {
            for _ in 0..indent {
                line.push('\t');
            }
        }

        // Check if the token is special and append it appropriately to the output.
        match token.as_str() {
            "{" | "(" => {
                if !is_first {
                    line.push(' ');
                }
                line.push_str(token.as_str());
                res.push(line);
                indent = indent.saturating_add(1);

                line = String::new();
            }
            "}" | ")" => {
                line.push_str(token.as_str());
            }
            ";" => {
                line.push(';');
                res.push(line);

                line = String::new();
            }
            "," => {
                line.push(',');
                res.push(line);

                line = String::new();
            }
            _ => {
                if !is_first {
                    line.push(' ');
                }
                line.push_str(token.as_str());
            }
        };
    }

    if !line.is_empty() {
        res.push(line);
    }

    res
}

/// Formats a unified diff between two supposedly different types and writes it to the provided
/// output stream.
fn write_type_diff<W: Write>(
    tokens: &Tokens,
    other_tokens: &Tokens,
    writer: W,
) -> Result<(), crate::Error> {
    let pretty = pretty_format_type(tokens);
    let other_pretty = pretty_format_type(other_tokens);
    unified_diff(&pretty, &other_pretty, writer)
}

// Copyright (C) 2024 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

//! A representation of a kABI symtypes corpus and tools for working with the data.

use crate::burst;
use crate::burst::JobSlots;
use crate::text::{
    DirectoryWriter, Filter, WriteGenerator, Writer, matches_filter, read_lines, unified_diff,
};
use crate::{Error, MapIOErr, PathFile, debug, hash};
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::{HashMap, HashSet};
use std::io::prelude::*;
use std::iter::zip;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::{fs, iter, mem};

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
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
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
type TypeVariants = Vec<Arc<Tokens>>;

/// A mapping from a type name to all its known variants.
type Types = HashMap<String, TypeVariants>;

/// The size of the `TypeBuckets` collection.
const TYPE_BUCKETS_SIZE: usize = 256;

/// A collection of `Types`, indexed by `type_bucket_idx(type_name)`. This allows each bucket to be
/// protected by a separate lock when reading symtypes data.
type TypeBuckets = Vec<Types>;

/// Computes the index into `TypeBuckets` for a given type name.
fn type_bucket_idx(type_name: &str) -> usize {
    (hash(type_name) % TYPE_BUCKETS_SIZE as u64) as usize
}

/// A mapping from a type name to `Tokens`, specifying the type in a given file.
type FileRecords = HashMap<String, Arc<Tokens>>;

/// A representation of a single symtypes file.
#[derive(Debug, Eq, PartialEq)]
struct SymtypesFile {
    path: PathBuf,
    records: FileRecords,
}

/// A collection of symtypes files, which also provides fast lookup by a symtypes path.
///
/// SAFETY: The `PathBuf` key must match the `path` member of the corresponding `SymtypesFile`
/// value.
type SymtypesFiles = HashMap<PathBuf, Arc<SymtypesFile>>;

/// A mapping from a symbol name to a `SymtypesFile`, specifying in which file the symbol is
/// defined.
type Exports = HashMap<String, Arc<SymtypesFile>>;

/// A representation of a kernel ABI, loaded from symtypes files.
///
/// * The `types` collection stores all types and their variants.
/// * The `files` collection records types in individual symtypes files.
/// * The `exports` collection provides all exports in the corpus. Each record points to a file in
///   which the symbol is defined.
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
/// The example assumes `type_bucket_idx("s#foo") % TYPE_BUCKETS_SIZE` evaluates to 1, and that
/// `type_bucket_idx("bar") % TYPE_BUCKETS_SIZE` and `type_bucket_idx("baz") % TYPE_BUCKETS_SIZE`
/// both evaluate to 3.
///
/// ```text
/// foo_tokens = Arc { Tokens[ Atom("struct"), Atom("foo"), Atom("{"), Atom("int"), Atom("a"), Atom(";"), Atom("}") ] }
/// foo2_tokens = Arc { Tokens[ Atom("struct"), Atom("foo"), Atom("{"), Atom("UNKNOWN"), Atom("}") ] }
/// bar_tokens = Arc { Tokens[ Atom("int"), Atom("bar"), Atom("("), TypeRef("s#foo"), Atom(")") ] }
/// baz_tokens = Arc { Tokens[ Atom("int"), Atom("baz"), Atom("("), TypeRef("s#foo"), Atom(")") ] }
/// test_a_file = Arc { SymtypesFile {
///     path: PathBuf("test_a.symtypes"),
///     records: FileRecords {
///         "s#foo": foo_tokens,
///         "bar": bar_tokens,
///     }
/// } }
/// test_b_file = Arc { SymtypesFile {
///     path: PathBuf("test_b.symtypes"),
///     records: FileRecords {
///         "s#foo": foo2_tokens,
///         "baz": baz_tokens,
///     }
/// } }
/// corpus = SymtypesCorpus {
///     types: TypeBuckets {
///         [0]: Types { },
///         [1]: Types {
///             "s#foo": TypeVariants[ foo_tokens, foo2_tokens ]
///         },
///         [2]: Types { },
///         [3]: Types {
///             "bar": TypeVariants[ bar_tokens ],
///             "baz": TypeVariants[ baz_tokens ],
///         },
///         [4..TYPE_BUCKETS_SIZE] = Types { },
///     },
///     files: SymtypesFiles[ test_a_file, test_b_file ],
///     exports: Exports {
///         "bar": test_a_file,
///         "baz": test_b_file,
///     },
/// }
/// ```
///
/// Note importantly that if a `Token` in `TypeVariants` is a `TypeRef` then the reference only
/// specifies a name of the target type, e.g. `s#foo` above. The actual type variant must be
/// determined based on what file is being processed. This allows to trivially merge `Tokens` and
/// limit memory needed to store the corpus. On the other hand, when comparing two `Tokens` vectors
/// for ABI equality, the code needs to consider whether all referenced subtypes are actually equal
/// as well.
#[derive(Debug, Eq, PartialEq)]
pub struct SymtypesCorpus {
    types: TypeBuckets,
    files: SymtypesFiles,
    exports: Exports,
}

/// An identifier indicating what kind of symtypes data is expected to be loaded.
#[derive(Eq, PartialEq)]
enum LoadKind {
    /// A plain symtypes file.
    Simple,
    /// A consolidated symtypes file.
    Consolidated,
    /// A plain or consolidated symtypes file.
    Any,
}

/// A helper structure to provide synchronized access to all corpus data and a warnings stream
/// during parallel loading.
///
/// The structure holds a reference to the existing corpus and separately tracks all new data that
/// should be added to it if the load succeeds. This approach ensures that the corpus remains
/// unchanged if the load operation fails and returns an error at any point. However, this method
/// adds some complexity to the loading process, as the code must carefully check both the existing
/// and new data when inserting new records.
struct LoadContext<'a> {
    load_kind: LoadKind,
    symtypes: &'a SymtypesCorpus,
    new_types: Vec<RwLock<Types>>,
    new_exports: Mutex<Exports>,
    new_files: Mutex<SymtypesFiles>,
    warnings: Mutex<Box<dyn Write + Send + 'a>>,
}

/// Type names active during the loading of a specific file, providing for each type its tokens and
/// source line index.
type LoadActiveTypes = HashMap<String, (Arc<Tokens>, usize)>;

/// Changes between two corpuses, recording a tuple of each modified type's name, its old tokens and
/// its new tokens, along with a [`Vec`] of exported symbols affected by the change.
type CompareChangedTypes<'a> = HashMap<(&'a str, &'a Tokens, &'a Tokens), Vec<&'a str>>;

/// Type names processed during the comparison for a specific file.
type CompareFileTypes<'a> = HashSet<&'a str>;

impl<'a> LoadContext<'a> {
    /// Creates a new load context from a symtypes corpus and a warnings stream.
    fn from<W: Write + Send + 'a>(
        symtypes: &'a SymtypesCorpus,
        load_kind: LoadKind,
        warnings: W,
    ) -> Self {
        Self {
            load_kind,
            symtypes,
            new_types: iter::repeat_with(|| RwLock::new(Types::new()))
                .take(TYPE_BUCKETS_SIZE)
                .collect(),
            new_exports: Mutex::new(Exports::new()),
            new_files: Mutex::new(SymtypesFiles::new()),
            warnings: Mutex::new(Box::new(warnings)),
        }
    }

    /// Consumes this load context, returning the new data.
    fn into_inner(self) -> (TypeBuckets, Exports, SymtypesFiles) {
        (
            self.new_types
                .into_iter()
                .map(|t| t.into_inner().unwrap())
                .collect(),
            self.new_exports.into_inner().unwrap(),
            self.new_files.into_inner().unwrap(),
        )
    }
}

/// The format of the output from [`SymtypesCorpus::compare_with()`].
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum CompareFormat {
    /// No output.
    Null,
    /// Verbose human-readable output.
    Pretty,
    /// Compact human-readable output.
    Short,
    /// A list of all added, removed, or modified symbols.
    Symbols,
    /// A list of all modified symbols only.
    ModSymbols,
}

impl CompareFormat {
    /// Obtains a [`CompareFormat`] matching the given format type, specified as a string.
    pub fn try_from_str(format: &str) -> Result<Self, Error> {
        match format {
            "null" => Ok(Self::Null),
            "pretty" => Ok(Self::Pretty),
            "short" => Ok(Self::Short),
            "symbols" => Ok(Self::Symbols),
            "mod-symbols" => Ok(Self::ModSymbols),
            _ => Err(Error::new_parse(format!(
                "Unrecognized format '{}'",
                format
            ))),
        }
    }
}

impl Default for SymtypesCorpus {
    fn default() -> Self {
        Self::new()
    }
}

impl SymtypesCorpus {
    /// Creates a new empty corpus.
    pub fn new() -> Self {
        Self {
            types: vec![Types::new(); TYPE_BUCKETS_SIZE],
            files: SymtypesFiles::new(),
            exports: Exports::new(),
        }
    }

    /// Loads symtypes data from the specified location.
    ///
    /// The `path` can point to a single symtypes file or a directory. In the latter case, the
    /// function recursively collects all symtypes in that directory and loads them.
    pub fn load<P: AsRef<Path>, W: Write + Send>(
        &mut self,
        path: P,
        warnings: W,
        job_slots: &mut JobSlots,
    ) -> Result<(), Error> {
        let path = path.as_ref();

        // Determine if the input is a directory tree or a single symtypes file.
        let md = fs::metadata(path).map_err(|err| {
            Error::new_io(
                format!("Failed to query the path '{}'", path.display()),
                err,
            )
        })?;

        if md.is_dir() {
            // Recursively collect symtypes files within the directory.
            let mut symfiles = Vec::new();
            Self::collect_symfiles(path, Path::new(""), &mut symfiles)?;

            // Load all found files.
            self.load_symfiles(
                path,
                &symfiles.iter().map(Path::new).collect::<Vec<&Path>>(),
                LoadKind::Simple,
                warnings,
                job_slots,
            )
        } else {
            // Load the single file.
            self.load_symfiles(Path::new(""), &[path], LoadKind::Any, warnings, job_slots)
        }
    }

    /// Loads consolidated symtypes data from the specified file.
    pub fn load_consolidated<P: AsRef<Path>, W: Write + Send>(
        &mut self,
        path: P,
        warnings: W,
        job_slots: &mut JobSlots,
    ) -> Result<(), Error> {
        let path = path.as_ref();

        // Load the single file.
        self.load_symfiles(
            Path::new(""),
            &[path],
            LoadKind::Consolidated,
            warnings,
            job_slots,
        )
    }

    /// Loads split symtypes data from the specified directory.
    pub fn load_split<P: AsRef<Path>, W: Write + Send>(
        &mut self,
        path: P,
        warnings: W,
        job_slots: &mut JobSlots,
    ) -> Result<(), Error> {
        let path = path.as_ref();

        // Recursively collect symtypes files within the directory.
        let mut symfiles = Vec::new();
        Self::collect_symfiles(path, Path::new(""), &mut symfiles)?;

        // Load all found files.
        self.load_symfiles(
            path,
            &symfiles.iter().map(Path::new).collect::<Vec<&Path>>(),
            LoadKind::Simple,
            warnings,
            job_slots,
        )
    }

    /// Collects recursively all symtypes files under the given root path and its subpath.
    fn collect_symfiles(
        root: &Path,
        sub_path: &Path,
        symfiles: &mut Vec<PathBuf>,
    ) -> Result<(), Error> {
        let path = root.join(sub_path);

        let dir_iter = fs::read_dir(&path).map_err(|err| {
            Error::new_io(
                format!("Failed to read the directory '{}'", path.display()),
                err,
            )
        })?;

        for maybe_entry in dir_iter {
            let entry = maybe_entry.map_err(|err| {
                Error::new_io(
                    format!("Failed to read the directory '{}'", path.display()),
                    err,
                )
            })?;

            let entry_path = entry.path();

            let md = fs::symlink_metadata(&entry_path).map_err(|err| {
                Error::new_io(
                    format!("Failed to query the path '{}'", entry_path.display()),
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
    fn load_symfiles<W: Write + Send>(
        &mut self,
        root: &Path,
        symfiles: &[&Path],
        load_kind: LoadKind,
        warnings: W,
        job_slots: &mut JobSlots,
    ) -> Result<(), Error> {
        let load_context = LoadContext::from(self, load_kind, warnings);

        burst::run_jobs(
            |work_idx| {
                let sub_path = symfiles[work_idx];

                let path = root.join(sub_path);
                let file = PathFile::open(&path).map_err(|err| {
                    Error::new_io(format!("Failed to open the file '{}'", path.display()), err)
                })?;

                Self::load_inner(&path, sub_path, file, &load_context)?;

                Ok(())
            },
            symfiles.len(),
            job_slots,
        )?;

        let (new_types, new_exports, new_files) = load_context.into_inner();
        self.merge_new(new_types, new_exports, new_files);

        Ok(())
    }

    /// Loads symtypes data from the specified reader.
    ///
    /// The `path` should point to a symtypes file name, indicating the origin of the data.
    pub fn load_buffer<P: AsRef<Path>, R: Read, W: Write + Send>(
        &mut self,
        path: P,
        reader: R,
        warnings: W,
    ) -> Result<(), Error> {
        let path = path.as_ref();
        let load_context = LoadContext::from(self, LoadKind::Any, warnings);

        Self::load_inner(path, path, reader, &load_context)?;

        let (new_types, new_exports, new_files) = load_context.into_inner();
        self.merge_new(new_types, new_exports, new_files);

        Ok(())
    }

    /// Completes the loading operation by merging new data into the existing corpus.
    fn merge_new(
        &mut self,
        new_types: TypeBuckets,
        new_exports: Exports,
        new_files: SymtypesFiles,
    ) {
        for (bucket_idx, bucket) in new_types.into_iter().enumerate() {
            for (type_name, mut variants) in bucket {
                match self.types[bucket_idx].entry(type_name) {
                    Occupied(mut types_entry) => types_entry.get_mut().append(&mut variants),
                    Vacant(type_entry) => {
                        type_entry.insert(variants);
                    }
                }
            }
        }
        self.exports.extend(new_exports);
        self.files.extend(new_files);
    }

    /// Loads symtypes data from the specified reader.
    fn load_inner<R: Read>(
        path: &Path,
        sub_path: &Path,
        reader: R,
        load_context: &LoadContext,
    ) -> Result<(), Error> {
        debug!("Loading symtypes data from '{}'", path.display());

        // Read all content from the file.
        let lines = match read_lines(reader) {
            Ok(lines) => lines,
            Err(err) => return Err(Error::new_io("Failed to read symtypes data", err)),
        };

        // Detect whether the input is a single or consolidated symtypes file.
        let is_consolidated =
            !lines.is_empty() && lines[0].starts_with("/* ") && lines[0].ends_with(" */");
        if load_context.load_kind == LoadKind::Simple && is_consolidated {
            return Err(Error::new_parse_format(
                "Expected a plain symtypes file, but found consolidated data",
                path,
                1,
                &lines[0],
            ));
        } else if load_context.load_kind == LoadKind::Consolidated && !is_consolidated {
            return Err(Error::new_parse_format(
                "Expected a consolidated symtypes file, but found an invalid header",
                path,
                1,
                if lines.is_empty() { "" } else { &lines[0] },
            ));
        }

        // Track the name of the currently processed single (inner) file.
        let mut maybe_sub_path = if !is_consolidated {
            Some(sub_path)
        } else {
            None
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
                // Add the current file.
                if let Some(sub_path) = maybe_sub_path {
                    Self::add_file(
                        path,
                        sub_path,
                        &lines,
                        mem::take(&mut records),
                        mem::take(&mut local_override),
                        &active_types,
                        load_context,
                    )?;
                }

                // Open the new file.
                maybe_sub_path = Some(Path::new(&line[3..line.len() - 3]));

                continue;
            }

            // Ok, it is a regular record, parse it.
            let (name, tokens, is_local_override) =
                parse_type_record(path, line_idx, line, is_consolidated)?;

            // Check if the record is a duplicate of another one.
            if records.contains_key(&name) {
                return Err(Error::new_parse_format(
                    &format!("Duplicate record '{}'", name),
                    path,
                    line_idx + 1,
                    &lines[line_idx],
                ));
            }

            // Insert the type into the future corpus and file records.
            let tokens_rc = Self::merge_type(&name, tokens, load_context);
            records.insert(name.clone(), Arc::clone(&tokens_rc));

            // Record the type as currently active.
            if is_local_override {
                local_override.insert(name, (tokens_rc, line_idx));
            } else {
                active_types.insert(name, (tokens_rc, line_idx));
            }
        }

        // Complete the file.
        if let Some(sub_path) = maybe_sub_path {
            Self::add_file(
                path,
                sub_path,
                &lines,
                records,
                local_override,
                &active_types,
                load_context,
            )?;
        }

        Ok(())
    }

    /// Adds the specified file to the newly loaded data.
    ///
    /// Completes the loading of a symtypes file by extrapolating its records, validating all
    /// references, and finally adding the file and its exports to the corpus.
    ///
    /// The `path` is the name of an input file, which can be a consolidated file. The `sub_path` is
    /// the name of a specific symtypes file.
    fn add_file(
        path: &Path,
        sub_path: &Path,
        lines: &Vec<String>,
        mut records: FileRecords,
        local_override: LoadActiveTypes,
        active_types: &LoadActiveTypes,
        load_context: &LoadContext,
    ) -> Result<(), Error> {
        // Extrapolate all records and validate references.
        let walk_records = records.keys().map(String::clone).collect::<Vec<_>>();
        for name in walk_records {
            // Note that all explicit types are known, so it is ok to pass `usize::MAX` as
            // `from_line_idx` because it is unused.
            Self::complete_file_record(
                path,
                lines,
                usize::MAX,
                &name,
                true,
                &local_override,
                active_types,
                &mut records,
            )?;
        }

        // Add the file to the future corpus.
        let symfile_rc = Arc::new(SymtypesFile {
            path: sub_path.to_path_buf(),
            records,
        });

        {
            let mut new_files = load_context.new_files.lock().unwrap();

            // Verify that the file path does not duplicate any existing paths.
            if load_context.symtypes.files.contains_key(&symfile_rc.path)
                || new_files.contains_key(&symfile_rc.path)
            {
                return Err(Error::new_parse(format!(
                    "Duplicate file path '{}'",
                    symfile_rc.path.display()
                )));
            }

            new_files.insert(symfile_rc.path.clone(), Arc::clone(&symfile_rc));
        }

        // Insert all the exports present in the file into the future corpus.
        {
            let mut new_exports = load_context.new_exports.lock().unwrap();

            for type_name in symfile_rc
                .records
                .keys()
                .filter(|&name| is_export_name(name))
            {
                // Add the export, if it is unique.
                let other_symfile_rc = {
                    if let Some(other_symfile_rc) =
                        load_context.symtypes.exports.get(type_name.as_str())
                    {
                        Arc::clone(other_symfile_rc)
                    } else {
                        match new_exports.entry(type_name.clone()) // [1]
                        {
                            Occupied(export_entry) => Arc::clone(export_entry.get()),
                            Vacant(export_entry) => {
                                export_entry.insert(Arc::clone(&symfile_rc));
                                continue;
                            }
                        }
                    }
                };

                // SAFETY: Each export is included in the active types.
                let (_, line_idx) = active_types.get(type_name.as_str()).unwrap();

                // Report the duplicate export as a warning. Although technically an error, some
                // auxiliary kernel components that are not part of vmlinux/modules may reuse logic
                // from the rest of the kernel by including its C/assembly files, which may contain
                // export directives. If these components aren't correctly configured to disable
                // exports, collecting all symtypes from the build will result in duplicate symbols.
                // This should be fixed in the kernel. However, we want to proceed, especially if
                // this is the compare command, where we want to report actual kABI differences.
                let mut warnings = load_context.warnings.lock().unwrap();
                writeln!(
                    warnings,
                    "{}:{}: WARNING: Export '{}' defined in '{}' is duplicate, previous occurrence found in '{}'",
                    path.display(),
                    line_idx + 1,
                    type_name,
                    symfile_rc.path.display(),
                    other_symfile_rc.path.display(),
                )
                .map_io_err("Failed to write a duplicate-export warning")?;
            }
        }

        Ok(())
    }

    /// Adds the given type definition to the newly loaded data if it's not already present, and
    /// returns its reference-counted pointer.
    fn merge_type(type_name: &str, tokens: Tokens, load_context: &LoadContext) -> Arc<Tokens> {
        let bucket_idx = type_bucket_idx(type_name);

        // Search in the current types.
        if let Some(variants) = load_context.symtypes.types[bucket_idx].get(type_name) {
            for variant_rc in variants {
                if tokens == **variant_rc {
                    return Arc::clone(variant_rc);
                }
            }
        }

        // Search in the new types. Note that types are often repeated in different symtypes files,
        // therefore try to find an existing type only under the read lock first.
        {
            let new_types = load_context.new_types[bucket_idx].read().unwrap();
            if let Some(variants) = new_types.get(type_name) {
                for variant_rc in variants {
                    if tokens == **variant_rc {
                        return Arc::clone(variant_rc);
                    }
                }
            }
        }

        let mut new_types = load_context.new_types[bucket_idx].write().unwrap();
        match new_types.get_mut(type_name) {
            Some(variants) => {
                for variant_rc in variants.iter() {
                    if tokens == **variant_rc {
                        return Arc::clone(variant_rc);
                    }
                }
                let tokens_rc = Arc::new(tokens);
                variants.push(Arc::clone(&tokens_rc));
                tokens_rc
            }
            None => {
                let tokens_rc = Arc::new(tokens);
                new_types.insert(type_name.to_string(), vec![Arc::clone(&tokens_rc)]); // [1]
                tokens_rc
            }
        }
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
        lines: &Vec<String>,
        from_line_idx: usize,
        type_name: &str,
        is_explicit: bool,
        local_override: &LoadActiveTypes,
        active_types: &LoadActiveTypes,
        records: &mut FileRecords,
    ) -> Result<(), Error> {
        if is_explicit {
            // All explicit symbols need to be added by the caller.
            assert!(records.get(type_name).is_some());
        } else {
            // See if the symbol was already processed.
            if records.get(type_name).is_some() {
                return Ok(());
            }
        }

        let (tokens_rc, line_idx) = match local_override.get(type_name) {
            Some(&(ref tokens_rc, line_idx)) => (Arc::clone(tokens_rc), line_idx),
            None => match active_types.get(type_name) {
                Some(&(ref tokens_rc, line_idx)) => (Arc::clone(tokens_rc), line_idx),
                None => {
                    return Err(Error::new_parse_format(
                        &format!("Type '{}' is not known", type_name),
                        path,
                        from_line_idx + 1,
                        &lines[from_line_idx],
                    ));
                }
            },
        };
        if !is_explicit {
            records.insert(type_name.to_string(), Arc::clone(&tokens_rc)); // [1]
        }

        // Process recursively all types referenced by this symbol.
        for token in tokens_rc.iter() {
            match token {
                Token::TypeRef(ref_name) => {
                    Self::complete_file_record(
                        path,
                        lines,
                        line_idx,
                        ref_name,
                        false,
                        local_override,
                        active_types,
                        records,
                    )?;
                }
                Token::Atom(_word) => {}
            }
        }

        Ok(())
    }

    /// Writes the corpus in the consolidated form to the specified file.
    pub fn write_consolidated<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        self.write_consolidated_buffer(Writer::new_file(path)?)
    }

    /// Writes the corpus in the consolidated form to the provided output stream.
    pub fn write_consolidated_buffer<W: Write>(&self, mut writer: W) -> Result<(), Error> {
        let err_desc = "Failed to write a consolidated record";

        // Track which records are currently active, mapping a type name to its tokens.
        let mut active_types = HashMap::<&String, &Arc<Tokens>>::new();

        // Sort all files in the corpus by their path.
        let mut sorted_files = self.files.values().collect::<Vec<_>>();
        sorted_files.sort_by_key(|&symfile_rc| &symfile_rc.path);

        // Process the sorted files and add their types to the output.
        let mut add_separator = false;
        for symfile_rc in sorted_files {
            let symfile = symfile_rc.as_ref();

            // Sort all types in the file.
            let mut sorted_types = symfile.records.iter().collect::<Vec<_>>();
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
            for (name, tokens_rc) in sorted_types {
                // Check if this is an UNKNOWN type definition, and if so, record it as a local
                // override.
                if let Some(short_name) = try_shorten_decl(name, tokens_rc) {
                    writeln!(writer, "{}", short_name).map_io_err(err_desc)?;
                    continue;
                }

                // See if the symbol matches an already active definition, or record it in the
                // output.
                let record = match active_types.entry(name) {
                    Occupied(mut active_type_entry) => {
                        if *active_type_entry.get() != tokens_rc {
                            active_type_entry.insert(tokens_rc);
                            true
                        } else {
                            false
                        }
                    }
                    Vacant(active_type_entry) => {
                        active_type_entry.insert(tokens_rc);
                        true
                    }
                };
                if record {
                    write!(writer, "{}", name).map_io_err(err_desc)?;
                    for token in tokens_rc.iter() {
                        write!(writer, " {}", token.as_str()).map_io_err(err_desc)?;
                    }
                    writeln!(writer).map_io_err(err_desc)?;
                }
            }
        }

        writer.flush().map_io_err(err_desc)?;

        Ok(())
    }

    /// Writes the corpus in the split form to the specified directory.
    pub fn write_split<P: AsRef<Path>>(
        &self,
        path: P,
        job_slots: &mut JobSlots,
    ) -> Result<(), Error> {
        self.write_split_buffer(&mut DirectoryWriter::new_file(path), job_slots)
    }

    /// Writes the corpus in the split form to the provided output stream factory.
    pub fn write_split_buffer<W: Write, WG: WriteGenerator<W> + Send>(
        &self,
        dir_writer: WG,
        job_slots: &mut JobSlots,
    ) -> Result<(), Error> {
        let err_desc = "Failed to write a split record";
        let dir_writer = Mutex::new(dir_writer);

        let works = self.files.values().collect::<Vec<_>>();

        burst::run_jobs(
            |work_idx| {
                let symfile = works[work_idx].as_ref();

                // Sort all types in the file.
                let mut sorted_types = symfile.records.iter().collect::<Vec<_>>();
                sorted_types.sort_by_cached_key(|&(name, _)| (is_export_name(name), name));

                // Create an output file.
                let mut writer = {
                    let mut dir_writer = dir_writer.lock().unwrap();
                    dir_writer.create(&symfile.path)?
                };

                // Write all types into the output file.
                for (name, tokens_rc) in sorted_types {
                    write!(writer, "{}", name).map_io_err(err_desc)?;
                    for token in tokens_rc.iter() {
                        write!(writer, " {}", token.as_str()).map_io_err(err_desc)?;
                    }
                    writeln!(writer).map_io_err(err_desc)?;
                }

                // Close the file.
                writer.flush().map_io_err(err_desc)?;
                let mut dir_writer = dir_writer.lock().unwrap();
                dir_writer.close(writer);

                Ok(())
            },
            works.len(),
            job_slots,
        )
    }

    /// Compares the definitions of the given symbol in two files.
    ///
    /// If the immediate definition of the symbol differs between the two files then it gets added
    /// in `changes`. The `export` parameter identifies the top-level exported symbol affected by
    /// the change.
    ///
    /// The specified symbol is added to `processed_types`, if it's not already present, and all its
    /// type references get recursively processed in the same way.
    fn compare_types<'a>(
        symfile: &'a SymtypesFile,
        other_symfile: &'a SymtypesFile,
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

        // Look up how the symbol is defined in each file.
        // SAFETY: Each type reference is guaranteed to have a corresponding definition.
        let tokens = symfile.records.get(name).unwrap().as_ref();
        let other_tokens = other_symfile.records.get(name).unwrap().as_ref();

        // Compare the immediate tokens.
        let is_equal = tokens.len() == other_tokens.len()
            && zip(tokens, other_tokens).all(|(token, other_token)| token == other_token);
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
                        symfile,
                        other_symfile,
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
                        if let Token::TypeRef(other_ref_name) = other_token
                            && ref_name == other_ref_name
                        {
                            Self::compare_types(
                                symfile,
                                other_symfile,
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

    /// Compares the symbols in this corpus with another one.
    ///
    /// Writes reports about any found changes to the specified files, formatted as requested.
    /// Returns `Ok` containing a `bool` indicating whether the corpuses are the same, or
    /// <code>Err([Error])</code> on error.
    pub fn compare_with<P: AsRef<Path>>(
        &self,
        other_symtypes: &SymtypesCorpus,
        maybe_filter: Option<&Filter>,
        writers_conf: &[(CompareFormat, P)],
        job_slots: &mut JobSlots,
    ) -> Result<bool, Error> {
        // Materialize all writers.
        let mut writers = Vec::new();
        for (format, path) in writers_conf {
            writers.push((*format, Writer::new_file(path)?));
        }

        self.compare_with_buffer(other_symtypes, maybe_filter, &mut writers[..], job_slots)
    }

    /// Compares the symbols in this corpus with another one.
    ///
    /// Writes reports about any found changes to the provided output streams, formatted as
    /// requested. Returns `Ok` containing a `bool` indicating whether the corpuses are the same, or
    /// <code>Err([Error])</code> on error.
    pub fn compare_with_buffer<W: Write>(
        &self,
        other_symtypes: &SymtypesCorpus,
        maybe_filter: Option<&Filter>,
        writers: &mut [(CompareFormat, W)],
        job_slots: &mut JobSlots,
    ) -> Result<bool, Error> {
        let err_desc = "Failed to write a comparison result";

        // Track all changed symbols, mapping a symbol name to a boolean. The flag indicates whether
        // the symbol was modified (true), or was added/removed (false).
        let mut output_symbols = HashMap::<&str, bool>::new();

        // Check for symbols in `self` but not in `other_symtypes`, and vice versa.
        for (exports_a, exports_b, change) in [
            (&other_symtypes.exports, &self.exports, "added"),
            (&self.exports, &other_symtypes.exports, "removed"),
        ] {
            let mut changed = exports_a
                .keys()
                .filter(|&name| matches_filter(maybe_filter, name) && !exports_b.contains_key(name))
                .collect::<Vec<_>>();
            changed.sort();
            for name in changed {
                for &mut (format, ref mut writer) in &mut *writers {
                    if format == CompareFormat::Pretty || format == CompareFormat::Short {
                        writeln!(writer, "Export '{}' has been {}", name, change)
                            .map_io_err(err_desc)?
                    }
                }

                output_symbols.insert(name, false);
            }
        }

        // Compare symbols that are in both corpuses.
        let works = self
            .exports
            .iter()
            .filter(|&(name, _)| matches_filter(maybe_filter, name))
            .collect::<Vec<_>>();
        let changes = Mutex::new(CompareChangedTypes::new());

        burst::run_jobs(
            |work_idx| {
                let (name, symfile_rc) = works[work_idx];

                if let Some(other_symfile_rc) = other_symtypes.exports.get(name) {
                    let mut processed = CompareFileTypes::new();
                    Self::compare_types(
                        symfile_rc.as_ref(),
                        other_symfile_rc.as_ref(),
                        name,
                        name,
                        &changes,
                        &mut processed,
                    );
                };

                Ok(())
            },
            works.len(),
            job_slots,
        )?;

        // Format and output collected changes.
        let changes = changes.into_inner().unwrap(); // Get the inner HashMap.
        let mut changes = changes.into_iter().collect::<Vec<_>>();
        changes.iter_mut().for_each(|(_, exports)| exports.sort());
        changes.sort();

        let mut add_separator = false;
        for ((name, tokens, other_tokens), exports) in changes {
            for &mut (format, ref mut writer) in &mut *writers {
                if format == CompareFormat::Pretty || format == CompareFormat::Short {
                    let is_short = format == CompareFormat::Short;

                    // Add an empty line to separate individual changes.
                    if add_separator {
                        writeln!(writer).map_io_err(err_desc)?;
                    }

                    // Output the affected exports, limit the list if the short format is selected.
                    writeln!(
                        writer,
                        "The following '{}' exports are different:",
                        exports.len()
                    )
                    .map_io_err(err_desc)?;
                    let take_count = if is_short { 10 } else { exports.len() };
                    for export in exports.iter().take(take_count) {
                        writeln!(writer, " {}", export).map_io_err(err_desc)?;
                    }
                    if is_short && take_count < exports.len() {
                        writeln!(writer, " <...>").map_io_err(err_desc)?;
                    }
                    writeln!(writer).map_io_err(err_desc)?;

                    // Output the changed type.
                    writeln!(writer, "because of a changed '{}':", name).map_io_err(err_desc)?;
                    write_type_diff(tokens, other_tokens, writer.by_ref())?;
                }
            }
            for export in exports {
                output_symbols.insert(export, true);
            }
            add_separator = true;
        }

        // Format symbol lists.
        let mut sorted_output_symbols = output_symbols
            .iter()
            .map(|(&k, &v)| (k, v))
            .collect::<Vec<_>>();
        sorted_output_symbols.sort();
        for (name, modified) in sorted_output_symbols {
            for &mut (format, ref mut writer) in &mut *writers {
                if format == CompareFormat::Symbols
                    || (format == CompareFormat::ModSymbols && modified)
                {
                    writeln!(writer, "{}", name).map_io_err(err_desc)?;
                }
            }
        }

        for (_, writer) in &mut *writers {
            writer.flush().map_io_err(err_desc)?;
        }

        Ok(output_symbols.is_empty())
    }
}

/// Reads words from the given iterator and converts them to `Tokens`.
fn words_into_tokens<'a, I: Iterator<Item = &'a str>>(words: &mut I) -> Tokens {
    let mut tokens = Tokens::new();
    for word in words {
        let mut is_typeref = false;
        if let Some(ch) = word.chars().nth(1)
            && ch == '#'
        {
            is_typeref = true;
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
        if zip(tokens, unknown).all(|(token, check)| token.as_str() == check) {
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
fn parse_type_record(
    path: &Path,
    line_idx: usize,
    line: &str,
    is_consolidated: bool,
) -> Result<(String, Tokens, bool), Error> {
    let mut words = line.split_ascii_whitespace();

    let raw_name = words.next().ok_or_else(|| {
        Error::new_parse_format("Expected a record name", path, line_idx + 1, line)
    })?;

    if is_consolidated {
        // Check if it is an UNKNOWN override.
        if let Some((name, tokens)) = try_expand_decl(raw_name) {
            if words.next().is_some() {
                return Err(Error::new_parse_format(
                    "Unexpected string found at the end of the override record",
                    path,
                    line_idx + 1,
                    line,
                ));
            }
            return Ok((name, tokens, true));
        }
    }

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
    let comma_wraps = (tokens.len() >= 1 && tokens[0].as_str() == "enum")
        || (tokens.len() >= 2 && tokens[0].as_str() == "typedef" && tokens[1].as_str() == "enum");

    let mut line = String::new();
    for token in tokens {
        // Handle the closing bracket early, it ends any prior line and reduces indentation.
        if token.as_str() == "}" {
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
            "{" => {
                if !is_first {
                    line.push(' ');
                }
                line.push_str(token.as_str());
                res.push(line);
                indent = indent.saturating_add(1);
                line = String::new();
            }
            "}" => {
                line.push_str(token.as_str());
            }
            ";" => {
                line.push(';');
                res.push(line);
                line = String::new();
            }
            "," if comma_wraps => {
                line.push(',');
                res.push(line);
                line = String::new();
            }
            "," => {
                line.push(',');
            }
            _ => {
                if !is_first {
                    line.push(' ');
                }
                line.push_str(token.as_str());
            }
        }
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
) -> Result<(), Error> {
    let pretty = pretty_format_type(tokens);
    let other_pretty = pretty_format_type(other_tokens);
    unified_diff(&pretty, &other_pretty, writer)
}

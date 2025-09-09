// Copyright (C) 2024 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

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
use std::{fs, mem};

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

/// A mapping from a symbol name to an index in `SymtypesFiles`, specifying in which file the symbol
/// is defined.
type Exports = HashMap<String, usize>;

/// A mapping from a type name to `Tokens`, specifying the type in a given file.
type FileRecords = HashMap<String, Arc<Tokens>>;

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
/// * The `files` collection records types in individual symtypes files.
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
/// The example assumes `type_bucket_idx("s#foo") % TYPE_BUCKETS_SIZE` evaluates to 1, and that
/// `type_bucket_idx("bar") % TYPE_BUCKETS_SIZE` and `type_bucket_idx("baz") % TYPE_BUCKETS_SIZE`
/// both evaluate to 3.
///
/// ```text
/// foo_tokens = Arc { Tokens[ Atom("struct"), Atom("foo"), Atom("{"), Atom("int"), Atom("a"), Atom(";"), Atom("}") ] }
/// foo2_tokens = Arc { Tokens[ Atom("struct"), Atom("foo"), Atom("{"), Atom("UNKNOWN"), Atom("}") ] }
/// bar_tokens = Arc { Tokens[ Atom("int"), Atom("bar"), Atom("("), TypeRef("s#foo"), Atom(")") ] }
/// baz_tokens = Arc { Tokens[ Atom("int"), Atom("baz"), Atom("("), TypeRef("s#foo"), Atom(")") ] }
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
///     exports: Exports {
///         "bar": 0,
///         "baz": 1,
///     },
///     files: SymtypesFiles[
///         SymtypesFile {
///             path: PathBuf("test_a.symtypes"),
///             records: FileRecords {
///                 "s#foo": foo_tokens,
///                 "bar": bar_tokens,
///             }
///         },
///         SymtypesFile {
///             path: PathBuf("test_b.symtypes"),
///             records: FileRecords {
///                 "s#foo": foo2_tokens,
///                 "baz": baz_tokens,
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
#[derive(Debug, Eq, PartialEq)]
pub struct SymtypesCorpus {
    types: TypeBuckets,
    exports: Exports,
    files: SymtypesFiles,
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

/// A helper struct to provide synchronized access to all corpus data and a warnings stream during
/// parallel loading.
struct LoadContext<'a> {
    load_kind: LoadKind,
    types: Vec<RwLock<Types>>,
    exports: Mutex<Exports>,
    files: Mutex<SymtypesFiles>,
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
        mut symtypes: SymtypesCorpus,
        load_kind: LoadKind,
        warnings: W,
    ) -> Self {
        Self {
            load_kind,
            types: symtypes.types.into_iter().map(RwLock::new).collect(),
            exports: Mutex::new(mem::take(&mut symtypes.exports)),
            files: Mutex::new(mem::take(&mut symtypes.files)),
            warnings: Mutex::new(Box::new(warnings)),
        }
    }

    /// Consumes this load context, returning the underlying data.
    fn into_inner(self) -> SymtypesCorpus {
        SymtypesCorpus {
            types: self
                .types
                .into_iter()
                .map(|t| t.into_inner().unwrap())
                .collect(),
            exports: self.exports.into_inner().unwrap(),
            files: self.files.into_inner().unwrap(),
        }
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
            exports: Exports::new(),
            files: SymtypesFiles::new(),
        }
    }

    /// Loads symtypes data from a given location.
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
        let load_context = LoadContext::from(mem::take(self), load_kind, warnings);

        burst::run_jobs(
            |work_idx| {
                let sub_path = symfiles[work_idx];

                let path = root.join(sub_path);
                let file = PathFile::open(&path).map_err(|err| {
                    Error::new_io(format!("Failed to open the file '{}'", path.display()), err)
                })?;

                Self::load_inner(sub_path, file, &load_context)?;

                Ok(())
            },
            symfiles.len(),
            job_slots,
        )?;

        *self = load_context.into_inner();

        Ok(())
    }

    /// Loads symtypes data from a specified reader.
    ///
    /// The `path` should point to a symtypes file name, indicating the origin of the data.
    pub fn load_buffer<P: AsRef<Path>, R: Read, W: Write + Send>(
        &mut self,
        path: P,
        reader: R,
        warnings: W,
    ) -> Result<(), Error> {
        let path = path.as_ref();
        let load_context = LoadContext::from(mem::take(self), LoadKind::Any, warnings);

        Self::load_inner(path, reader, &load_context)?;

        *self = load_context.into_inner();

        Ok(())
    }

    /// Loads symtypes data from a specified reader.
    fn load_inner<R: Read>(
        path: &Path,
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
            return Err(Error::new_parse(format!(
                "{}:1: Expected a plain symtypes file, but found consolidated data",
                path.display()
            )));
        } else if load_context.load_kind == LoadKind::Consolidated && !is_consolidated {
            return Err(Error::new_parse(format!(
                "{}:1: Expected a consolidated symtypes file, but found an invalid header",
                path.display()
            )));
        }

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
                file_idx = Self::add_file(Path::new(&line[3..line.len() - 3]), load_context);

                continue;
            }

            // Ok, it is a regular record, parse it.
            let (name, tokens, is_local_override) =
                parse_type_record(path, line_idx, line, is_consolidated)?;

            // Check if the record is a duplicate of another one.
            if records.contains_key(&name) {
                return Err(Error::new_parse(format!(
                    "{}:{}: Duplicate record '{}'",
                    path.display(),
                    line_idx + 1,
                    name,
                )));
            }

            // Insert the type into the corpus and file records.
            let tokens_rc = Self::merge_type(&name, tokens, load_context);
            if is_export_name(&name) {
                Self::insert_export(&name, file_idx, line_idx, load_context)?;
            }
            records.insert(name.clone(), Arc::clone(&tokens_rc));

            // Record the type as currently active.
            if is_local_override {
                local_override.insert(name, (tokens_rc, line_idx));
            } else {
                active_types.insert(name, (tokens_rc, line_idx));
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
    fn add_file(path: &Path, load_context: &LoadContext) -> usize {
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
    fn close_file(
        path: &Path,
        file_idx: usize,
        mut records: FileRecords,
        local_override: LoadActiveTypes,
        active_types: &LoadActiveTypes,
        load_context: &LoadContext,
    ) -> Result<(), Error> {
        // Extrapolate all records and validate references.
        let walk_records = records.keys().map(String::clone).collect::<Vec<_>>();
        for name in walk_records {
            // Note that all explicit types are known, so it is ok to pass usize::MAX as
            // from_line_idx because it is unused.
            Self::complete_file_record(
                path,
                usize::MAX,
                &name,
                true,
                &local_override,
                active_types,
                &mut records,
            )?;
        }

        // Add the file records to the corpus.
        let mut files = load_context.files.lock().unwrap();
        files[file_idx].records = records;

        Ok(())
    }

    /// Adds the given type definition to the corpus if it's not already present, and returns its
    /// reference-counted pointer.
    fn merge_type(type_name: &str, tokens: Tokens, load_context: &LoadContext) -> Arc<Tokens> {
        // Types are often repeated in different symtypes files. Try to find an existing type only
        // under the read lock first.
        {
            let types = load_context.types[type_bucket_idx(type_name)]
                .read()
                .unwrap();
            if let Some(variants) = types.get(type_name) {
                for variant_rc in variants {
                    if tokens == **variant_rc {
                        return Arc::clone(variant_rc);
                    }
                }
            }
        }

        let mut types = load_context.types[type_bucket_idx(type_name)]
            .write()
            .unwrap();
        match types.get_mut(type_name) {
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
                types.insert(type_name.to_string(), vec![Arc::clone(&tokens_rc)]); // [1]
                tokens_rc
            }
        }
    }

    /// Registers the specified export in the corpus and validates that it is not a duplicate.
    fn insert_export(
        type_name: &str,
        file_idx: usize,
        line_idx: usize,
        load_context: &LoadContext,
    ) -> Result<(), Error> {
        // Add the export, if it is unique.
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

        // Report the duplicate export as a warning. Although technically an error, some auxiliary
        // kernel components that are not part of vmlinux/modules may reuse logic from the rest of
        // the kernel by including its C/assembly files, which may contain export directives. If
        // these components aren't correctly configured to disable exports, collecting all symtypes
        // from the build will result in duplicate symbols. This should be fixed in the kernel.
        // However, we want to proceed, especially if this is the compare command, where we want to
        // report actual kABI differences.
        let message = {
            let files = load_context.files.lock().unwrap();
            let path = &files[file_idx].path;
            let other_path = &files[other_file_idx].path;
            format!(
                "{}:{}: WARNING: Export '{}' is duplicate, previous occurrence found in '{}'",
                path.display(),
                line_idx + 1,
                type_name,
                other_path.display()
            )
        };
        let mut warnings = load_context.warnings.lock().unwrap();
        writeln!(warnings, "{}", message)
            .map_io_err("Failed to write a duplicate-export warning")?;
        Ok(())
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
                    return Err(Error::new_parse(format!(
                        "{}:{}: Type '{}' is not known",
                        path.display(),
                        from_line_idx + 1,
                        type_name
                    )));
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
        let mut file_indices = (0..self.files.len()).collect::<Vec<_>>();
        file_indices.sort_by_key(|&i| &self.files[i].path);

        // Process the sorted files and add their types to the output.
        let mut add_separator = false;
        for i in file_indices {
            let symfile = &self.files[i];

            // Sort all types in the file.
            let mut sorted_types = self.files[i].records.iter().collect::<Vec<_>>();
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

        burst::run_jobs(
            |work_idx| {
                let symfile = &self.files[work_idx];

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
            self.files.len(),
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
        file: &'a SymtypesFile,
        other_file: &'a SymtypesFile,
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
        let tokens = &**file.records.get(name).unwrap();
        let other_tokens = &**other_file.records.get(name).unwrap();

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
                        file,
                        other_file,
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
                                file,
                                other_file,
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

    /// Compares the symbols in `self` and `other_symtypes`.
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

    /// Compares the symbols in `self` and `other_symtypes`.
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
                let (name, file_idx) = works[work_idx];

                let file = &self.files[*file_idx];
                if let Some(other_file_idx) = other_symtypes.exports.get(name) {
                    let other_file = &other_symtypes.files[*other_file_idx];
                    let mut processed = CompareFileTypes::new();
                    Self::compare_types(file, other_file, name, name, &changes, &mut processed);
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
                    write_type_diff(name, tokens, other_tokens, writer.by_ref())?;
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

/// Reads words from a given iterator and converts them to `Tokens`.
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
        Error::new_parse(format!(
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
fn pretty_format_type(type_name: &str, tokens: &Tokens) -> Vec<String> {
    // Iterate over all tokens and produce the formatted output.
    let mut res = Vec::new();
    let mut indent: usize = 0;
    let comma_wraps = type_name.starts_with("e#");

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
    type_name: &str,
    tokens: &Tokens,
    other_tokens: &Tokens,
    writer: W,
) -> Result<(), Error> {
    let pretty = pretty_format_type(type_name, tokens);
    let other_pretty = pretty_format_type(type_name, other_tokens);
    unified_diff(&pretty, &other_pretty, writer)
}

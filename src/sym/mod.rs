// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::{debug, read_lines, Filter, MapIOErr, PathFile};
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::{HashMap, HashSet};
use std::io::{prelude::*, BufWriter};
use std::iter::zip;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};
use std::{fs, io, thread};

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
#[derive(Eq, PartialEq, Hash, Ord, PartialOrd)]
enum Token {
    TypeRef(String),
    Atom(String),
}

impl Token {
    /// Creates a new `Token::TypeRef`.
    fn new_typeref<S: Into<String>>(name: S) -> Self {
        Token::TypeRef(name.into())
    }

    /// Creates a new `Token::Atom`.
    fn new_atom<S: Into<String>>(name: S) -> Self {
        Token::Atom(name.into())
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

/// A mapping from a symbol name to an index in `SymFiles`, specifying in which file the symbol is
/// defined.
type Exports = HashMap<String, usize>;

/// A mapping from a type name to an index in `TypeVariants`, specifying its variant in a given
/// file.
type FileRecords = HashMap<String, usize>;

/// A representation of a single `.symtypes` file.
struct SymFile {
    path: PathBuf,
    records: FileRecords,
}

/// A collection of `.symtypes` files.
type SymFiles = Vec<SymFile>;

/// A representation of a kernel ABI, loaded from `.symtypes` files.
///
/// * The `types` collection stores all types and their variants.
/// * The `files` collection records types in individual `.symtypes` files. Each type uses an index
///   to reference its variant in `types`.
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
/// SymCorpus {
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
///     files: SymFiles[
///         SymFile {
///             path: PathBuf("test_a.symtypes"),
///             records: FileRecords {
///                 "s#foo": 0,
///                 "bar": 0,
///             }
///         },
///         SymFile {
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
#[derive(Default)]
pub struct SymCorpus {
    types: Types,
    exports: Exports,
    files: SymFiles,
}

/// A helper struct to provide synchronized access to `SymCorpus` data during parallel loading.
struct LoadContext<'a> {
    types: RwLock<&'a mut Types>,
    exports: Mutex<&'a mut Exports>,
    files: Mutex<&'a mut SymFiles>,
}

/// Type names to be present in the consolidated output, along with a mapping from their internal
/// symbol variant indices to the output variant indices.
type ConsolidateOutputTypes<'a> = HashMap<&'a str, HashMap<usize, usize>>;

/// Type names processed during consolidation for a specific file, providing for each type their
/// output variant index.
type ConsolidateFileTypes<'a> = HashMap<&'a str, usize>;

/// Changes between two corpuses, recording a tuple of each modified type's name, its old tokens and
/// its new tokens, along with a [`Vec`] of exported symbols affected by the change.
type CompareChangedTypes<'a> = HashMap<(&'a str, &'a Tokens, &'a Tokens), Vec<&'a str>>;

/// Type names processed during comparison for a specific file.
type CompareFileTypes<'a> = HashSet<&'a str>;

impl SymCorpus {
    /// Creates a new empty corpus.
    pub fn new() -> Self {
        Self {
            types: Types::new(),
            exports: Exports::new(),
            files: SymFiles::new(),
        }
    }

    /// Loads symtypes data from a given location.
    ///
    /// The `path` can point to a single `.symtypes` file or a directory. In the latter case, the
    /// function recursively collects all `.symtypes` in that directory and loads them.
    pub fn load<P: AsRef<Path>>(&mut self, path: P, num_workers: i32) -> Result<(), crate::Error> {
        let path = path.as_ref();

        // Determine if the input is a directory tree or a single symtypes file.
        let md = fs::metadata(path).map_err(|err| {
            crate::Error::new_io(&format!("Failed to query path '{}'", path.display()), err)
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

    /// Collects recursively all `.symtypes` files under the given root path and its subpath.
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
                &format!("Failed to read directory '{}'", path.display()),
                err,
            )
        })?;

        for maybe_entry in dir_iter {
            let entry = maybe_entry.map_err(|err| {
                crate::Error::new_io(
                    &format!("Failed to read directory '{}'", path.display()),
                    err,
                )
            })?;

            let entry_path = entry.path();

            let md = fs::symlink_metadata(&entry_path).map_err(|err| {
                crate::Error::new_io(
                    &format!("Failed to query path '{}'", entry_path.display()),
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

    /// Loads all specified `.symtypes` files.
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
                                &format!("Failed to open file '{}'", path.display()),
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
    /// The `path` should point to a `.symtypes` file name, indicating the origin of the data.
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
        debug!("Loading '{}'", path.display());

        let mut records = FileRecords::new();

        // Map each variant name/index that the type has in this specific .symtypes file to one
        // which it got assigned in the entire loaded corpus.
        let mut remap: HashMap<String, HashMap<String, usize>> = HashMap::new();

        // Read all content from the file.
        let lines = match read_lines(reader) {
            Ok(lines) => lines,
            Err(err) => return Err(crate::Error::new_io("Failed to read symtypes data", err)),
        };

        // Detect whether the input is a single or consolidated symtypes file.
        let mut is_consolidated = false;
        for line in &lines {
            if line.starts_with("F#") {
                is_consolidated = true;
                break;
            }
        }

        let file_idx = if !is_consolidated {
            // Record the file early to determine its file_idx.
            let symfile = SymFile {
                path: path.to_path_buf(),
                records: FileRecords::new(),
            };

            let mut files = load_context.files.lock().unwrap();
            files.push(symfile);
            files.len() - 1
        } else {
            usize::MAX
        };

        // Track names of all entries to detect duplicates.
        let mut all_names = HashSet::new();

        // Parse all declarations.
        let mut file_indices = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            // Obtain a name of the record.
            let mut words = line.split_ascii_whitespace();
            let name = words.next().ok_or_else(|| {
                crate::Error::new_parse(&format!(
                    "{}:{}: Expected a record name",
                    path.display(),
                    line_idx + 1
                ))
            })?;

            // Check if the record is a duplicate of another one.
            match all_names.get(name) {
                Some(_) => {
                    return Err(crate::Error::new_parse(&format!(
                        "{}:{}: Duplicate record '{}'",
                        path.display(),
                        line_idx + 1,
                        name,
                    )))
                }
                None => all_names.insert(name.to_string()),
            };

            // Check for a file declaration and remember its index. File declarations are processed
            // later after remapping of all symbol variants is known.
            if name.starts_with("F#") {
                file_indices.push(line_idx);
                continue;
            }

            // Handle a type/export record.

            // Turn the remaining words into tokens.
            let tokens = words_into_tokens(&mut words);

            // Parse the base name and any variant name/index, which is appended as a suffix after
            // the `@` character.
            let (base_name, orig_variant_name) = if is_consolidated {
                split_type_name(name)
            } else {
                (name, &name[name.len()..])
            };

            // Insert the type into the corpus.
            let variant_idx = Self::merge_type(base_name, tokens, load_context);

            if is_consolidated {
                // Record a mapping from the original variant name/index to the new one.
                remap
                    .entry(base_name.to_string()) // [1]
                    .or_default()
                    .insert(orig_variant_name.to_string(), variant_idx);
            } else {
                // Insert the record.
                records.insert(base_name.to_string(), variant_idx);
                Self::try_insert_export(base_name, file_idx, line_idx, load_context)?;
            }
        }

        // TODO Validate all references?

        if !is_consolidated {
            // Update the file records.
            let mut files = load_context.files.lock().unwrap();
            files[file_idx].records = records;
            return Ok(());
        }

        // Consolidated file needs more work.

        // Handle file declarations.
        for line_idx in file_indices {
            let mut words = lines[line_idx].split_ascii_whitespace();

            let record_name = words.next().unwrap();
            assert!(record_name.starts_with("F#"));
            let file_name = &record_name[2..];

            let file_idx = {
                let symfile = SymFile {
                    path: Path::new(file_name).to_path_buf(),
                    records: FileRecords::new(),
                };
                let mut files = load_context.files.lock().unwrap();
                files.push(symfile);
                files.len() - 1
            };

            let mut records = FileRecords::new();
            for type_name in words {
                // Parse the base name and variant name/index.
                let (base_name, orig_variant_name) = split_type_name(type_name);

                // Look up how the variant got remapped.
                let variant_idx = *remap
                    .get(base_name)
                    .and_then(|hash| hash.get(orig_variant_name))
                    .ok_or_else(|| {
                        crate::Error::new_parse(&format!(
                            "{}:{}: Type '{}' is not known",
                            path.display(),
                            line_idx + 1,
                            type_name
                        ))
                    })?;

                // Insert the record.
                records.insert(base_name.to_string(), variant_idx);
                Self::try_insert_export(base_name, file_idx, line_idx, load_context)?;
            }

            // Add implicit references, ones that were omitted by the F# declaration because only
            // one variant exists in the entire consolidated file.
            let walk_records: Vec<_> = records.iter().map(|(k, v)| (k.clone(), *v)).collect();
            for (name, variant_idx) in walk_records {
                let types = load_context.types.read().unwrap();
                Self::extrapolate_file_record(
                    path,
                    file_name,
                    &name,
                    variant_idx,
                    true,
                    *types,
                    &mut records,
                )?;
            }

            let mut files = load_context.files.lock().unwrap();
            files[file_idx].records = records;
        }

        Ok(())
    }

    /// Adds the given type definition to the corpus if not already present, and returns its variant
    /// index.
    fn merge_type(type_name: &str, tokens: Tokens, load_context: &LoadContext) -> usize {
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

    /// Checks if a specified `type_name` is an export and, if so, registers it with its `file_idx`
    /// in the `load_context.exports`.
    fn try_insert_export(
        type_name: &str,
        file_idx: usize,
        line_idx: usize,
        load_context: &LoadContext,
    ) -> Result<(), crate::Error> {
        if !is_export_name(type_name) {
            return Ok(());
        }

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
        Err(crate::Error::new_parse(&format!(
            "{}:{}: Export '{}' is duplicate, previous occurrence found in '{}'",
            path.display(),
            line_idx + 1,
            type_name,
            other_path.display()
        )))
    }

    /// Processes a single symbol in some file originated from an `F#` record and enhances the
    /// specified file records with the needed implicit types.
    ///
    /// This function is used when reading a consolidated input file and processing its `F#`
    /// records. Each `F#` record is in form `F#<filename> <type@variant>... <export>...`. It lists
    /// all types and exports in a given file but is allowed to omit any referenced types which have
    /// only one variant in the whole consolidated file. The purpose of this function is to find all
    /// such implicit references and add them to `records`.
    ///
    /// A caller of this function should pre-fill `records` with all explicit references given on
    /// the processed `F#` record and then call this function on each of the references. These root
    /// calls should be invoked with `is_explicit` set to `true`. The function then recursively adds
    /// all needed implicit types which are referenced from these roots.
    fn extrapolate_file_record(
        corpus_path: &Path,
        file_name: &str,
        name: &str,
        variant_idx: usize,
        is_explicit: bool,
        types: &Types,
        records: &mut FileRecords,
    ) -> Result<(), crate::Error> {
        if is_explicit {
            // All explicit symbols need to be added by the caller.
            assert!(records.get(name).is_some());
        } else {
            // A symbol can be implicit only if it has one variant.
            assert!(variant_idx == 0);

            // See if the symbol was already processed.
            if records.get(name).is_some() {
                return Ok(());
            }
            records.insert(name.to_string(), variant_idx); // [1]
        }

        // Obtain tokens for the selected variant and check it is correctly specified.
        let variants = types.get(name).unwrap();
        assert!(!variants.is_empty());
        if !is_explicit && variants.len() > 1 {
            return Err(crate::Error::new_parse(&format!(
                "{}: Type '{}' is implicitly referenced by file '{}' but has multiple variants in the corpus",
                corpus_path.display(),
                name,
                file_name,
            )));
        }
        let tokens = &variants[variant_idx];

        // Process recursively all types referenced by this symbol.
        for token in tokens {
            match token {
                Token::TypeRef(ref_name) => {
                    // Process the type. Note that passing variant_idx=0 is ok here:
                    // * If the type is explicitly specified in the parent F# record then it must be
                    //   already added in the records and the called function immediately returns.
                    // * If the type is implicit then it can have only one variant and so only
                    //   variant_idx=0 can be correct. The invoked function will check that no more
                    //   than one variant is actually present.
                    Self::extrapolate_file_record(
                        corpus_path,
                        file_name,
                        ref_name,
                        0,
                        false,
                        types,
                        records,
                    )?;
                }
                Token::Atom(_word) => {}
            }
        }

        Ok(())
    }

    /// Processes a single symbol specified in a given file and adds it to the consolidated output.
    ///
    /// The specified symbol is added to `output_types` and `processed_types`, if not already
    /// present, and all its type references get recursively processed in the same way.
    fn consolidate_type<'a>(
        &'a self,
        symfile: &SymFile,
        name: &'a str,
        output_types: &mut ConsolidateOutputTypes<'a>,
        processed_types: &mut ConsolidateFileTypes<'a>,
    ) {
        // See if the symbol was already processed.
        let processed_entry = match processed_types.entry(name) {
            Occupied(_) => return,
            Vacant(processed_entry) => processed_entry,
        };

        // Look up the internal variant index.
        let variant_idx = match symfile.records.get(name) {
            Some(&variant_idx) => variant_idx,
            None => panic!(
                "Type '{}' is not known in file '{}'",
                name,
                symfile.path.display()
            ),
        };

        // Determine the output variant index for the symbol.
        let remap_idx;
        match output_types.entry(name) {
            Occupied(mut active_entry) => {
                let remap = active_entry.get_mut();
                let remap_len = remap.len();
                match remap.entry(variant_idx) {
                    Occupied(remap_entry) => {
                        remap_idx = *remap_entry.get();
                    }
                    Vacant(remap_entry) => {
                        remap_idx = remap_len;
                        remap_entry.insert(remap_idx);
                    }
                }
            }
            Vacant(active_entry) => {
                remap_idx = 0;
                active_entry.insert(HashMap::from([(variant_idx, remap_idx)]));
            }
        };
        processed_entry.insert(remap_idx);

        // Process recursively all types that the symbol references.
        let variants = match self.types.get(name) {
            Some(variants) => variants,
            None => panic!("Type '{}' has a missing declaration", name),
        };

        for token in &variants[variant_idx] {
            match token {
                Token::TypeRef(ref_name) => {
                    self.consolidate_type(symfile, ref_name, output_types, processed_types)
                }
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
                        &format!("Failed to create file '{}'", path.display()),
                        err,
                    ))
                }
            }
        };

        self.write_consolidated_buffer(writer)
    }

    /// Writes the corpus in the consolidated form to the provided output stream.
    pub fn write_consolidated_buffer<W: Write>(&self, writer: W) -> Result<(), crate::Error> {
        let mut writer = BufWriter::new(writer);

        // Initialize output data. Variable output_types records all output symbols, file_types
        // provides per-file information.
        let mut output_types = ConsolidateOutputTypes::new();
        let mut file_types = vec![ConsolidateFileTypes::new(); self.files.len()];

        // Sort all files in the corpus by their path.
        let mut file_indices = (0..self.files.len()).collect::<Vec<_>>();
        file_indices.sort_by_key(|&i| &self.files[i].path);

        // Process the sorted files and add their needed types to the output.
        for &i in &file_indices {
            let symfile = &self.files[i];

            // Collect sorted exports in the file which are the roots for consolidation.
            let mut exports = Vec::new();
            for name in symfile.records.keys() {
                if is_export_name(name) {
                    exports.push(name.as_str());
                }
            }
            exports.sort();

            // Add the exported types and their needed types to the output.
            let mut processed_types = ConsolidateFileTypes::new();
            for name in &exports {
                self.consolidate_type(symfile, name, &mut output_types, &mut processed_types);
            }
            file_types[i] = processed_types;
        }

        // Go through all files and their output types. Check if a given type has only one variant
        // in the output and mark it as such.
        for file_types_item in &mut file_types {
            for (name, remap_idx) in file_types_item {
                let remap = output_types.get(name).unwrap();
                if remap.len() == 1 {
                    *remap_idx = usize::MAX;
                }
            }
        }

        // Sort all output types and write them to the specified file.
        let mut sorted_records = output_types.into_iter().collect::<Vec<_>>();
        sorted_records.sort_by_key(|(name, _remap)| (is_export_name(name), *name));

        let err_desc = "Failed to write a consolidated record";

        for (name, remap) in sorted_records {
            let variants = self.types.get(name).unwrap();
            let mut sorted_remap = remap
                .iter()
                .map(|(&variant_idx, &remap_idx)| (remap_idx, variant_idx))
                .collect::<Vec<_>>();
            sorted_remap.sort();

            let needs_suffix = sorted_remap.len() > 1;
            for (remap_idx, variant_idx) in sorted_remap {
                let tokens = &variants[variant_idx];

                if needs_suffix {
                    write!(writer, "{}@{}", name, remap_idx).map_io_err(err_desc)?;
                } else {
                    write!(writer, "{}", name).map_io_err(err_desc)?;
                }
                for token in tokens {
                    write!(writer, " {}", token.as_str()).map_io_err(err_desc)?;
                }
                writeln!(writer).map_io_err(err_desc)?;
            }
        }

        // Write file records.
        for &i in &file_indices {
            let symfile = &self.files[i];

            // TODO Sorting, make same as above.
            let mut sorted_types = file_types[i]
                .iter()
                .map(|(&name, &remap_idx)| (is_export_name(name), name, remap_idx))
                .collect::<Vec<_>>();
            sorted_types.sort();

            // Output the F# record in form `F#<filename> <type@variant>... <export>...`. Types with
            // only one variant in the entire consolidated file can be skipped because they can be
            // implicitly determined by a reader.
            write!(writer, "F#{}", symfile.path.display()).map_io_err(err_desc)?;
            for &(_, name, remap_idx) in &sorted_types {
                if remap_idx != usize::MAX {
                    write!(writer, " {}@{}", name, remap_idx).map_io_err(err_desc)?;
                } else if is_export_name(name) {
                    write!(writer, " {}", name).map_io_err(err_desc)?;
                }
            }
            writeln!(writer).map_io_err(err_desc)?;
        }
        Ok(())
    }

    /// Obtains tokens which describe a specified type name, in a given corpus and file.
    fn get_type_tokens<'a>(symtypes: &'a SymCorpus, file: &SymFile, name: &str) -> &'a Tokens {
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
        (corpus, file): (&'a SymCorpus, &'a SymFile),
        (other_corpus, other_file): (&'a SymCorpus, &'a SymFile),
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
        other_corpus: &SymCorpus,
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
                s.spawn(|| loop {
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

/// Splits the specified type name into a tuple of two string slices, with the first one being the
/// base name and the second one containing the variant name/index (or an empty string if no variant
/// was present).
fn split_type_name(type_name: &str) -> (&str, &str) {
    match type_name.rfind('@') {
        Some(i) => (&type_name[..i], &type_name[i + 1..]),
        None => (type_name, &type_name[type_name.len()..]),
    }
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
    crate::diff::unified(&pretty, &other_pretty, writer)
}

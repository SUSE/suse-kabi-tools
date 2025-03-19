// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::time::Instant;
use std::{env, io, process};
use suse_kabi_tools::sym::SymCorpus;
use suse_kabi_tools::{debug, init_debug_level};

/// An elapsed timer to measure time of some operation.
///
/// The time is measured between when the object is instantiated and when it is dropped. A message
/// with the elapsed time is output when the object is dropped.
enum Timing {
    Active { desc: String, start: Instant },
    Inactive,
}

impl Timing {
    fn new(do_timing: bool, desc: &str) -> Self {
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

/// Prints the global usage message on the standard output.
fn print_usage() {
    print!(concat!(
        "Usage: ksymtypes [OPTION...] COMMAND\n",
        "\n",
        "Options:\n",
        "  -d, --debug                   enable debug output\n",
        "  -h, --help                    display this help and exit\n",
        "  --version                     output version information and exit\n",
        "\n",
        "Commands:\n",
        "  consolidate                   consolidate symtypes into a single file\n",
        "  compare                       show differences between two symtypes corpuses\n",
    ));
}

/// Prints the version information on the standard output.
fn print_version() {
    println!("ksymtypes {}", env!("CARGO_PKG_VERSION"));
}

/// Prints the usage message for the `consolidate` command on the standard output.
fn print_consolidate_usage() {
    print!(concat!(
        "Usage: ksymtypes consolidate [OPTION...] PATH\n",
        "Consolidate symtypes into a single file.\n",
        "\n",
        "Options:\n",
        "  -h, --help                    display this help and exit\n",
        "  -j NUM, --jobs=NUM            use NUM workers to perform the operation\n",
        "  -o FILE, --output=FILE        write the result in FILE, instead of stdout\n",
    ));
}

/// Prints the usage message for the `compare` command on the standard output.
fn print_compare_usage() {
    print!(concat!(
        "Usage: ksymtypes compare [OPTION...] PATH PATH2\n",
        "Show differences between two symtypes corpuses.\n",
        "\n",
        "Options:\n",
        "  -h, --help                    display this help and exit\n",
        "  -j NUM, --jobs=NUM            use NUM workers to perform the operation\n",
    ));
}

/// Handles an option with a mandatory value.
///
/// When the `arg` matches the `short` or `long` variant, the function returns [`Ok(Some(String))`]
/// with the option value. Otherwise, [`Ok(None)`] is returned when the `arg` doesn't match, or
/// [`Err`] in case of an error.
fn handle_value_option<I: Iterator<Item = String>>(
    arg: &str,
    args: &mut I,
    short: &str,
    long: &str,
) -> Result<Option<String>, ()> {
    // Handle '-<short> <value>' and '--<long> <value>'.
    if arg == short || arg == long {
        match args.next() {
            Some(value) => return Ok(Some(value.to_string())),
            None => {
                eprintln!("Missing argument for '{}'", long);
                return Err(());
            }
        };
    }

    // Handle '-<short><value>'.
    if let Some(value) = arg.strip_prefix(short) {
        return Ok(Some(value.to_string()));
    }

    // Handle '--<long>=<value>'.
    if let Some(rem) = arg.strip_prefix(long) {
        if let Some(value) = rem.strip_prefix('=') {
            return Ok(Some(value.to_string()));
        }
    }

    Ok(None)
}

/// Handles the `-j`/`--jobs` option which specifies the number of workers to perform a given
/// operation simultaneously.
fn handle_jobs_option<I: Iterator<Item = String>>(
    arg: &str,
    args: &mut I,
) -> Result<Option<i32>, ()> {
    if let Some(value) = handle_value_option(arg, args, "-j", "--jobs")? {
        match value.parse::<i32>() {
            Ok(jobs) => {
                if jobs < 1 {
                    eprintln!("Invalid value for '{}': must be positive", arg);
                    return Err(());
                }
                return Ok(Some(jobs));
            }
            Err(err) => {
                eprintln!("Invalid value for '{}': {}", arg, err);
                return Err(());
            }
        };
    }

    Ok(None)
}

/// Handles the `consolidate` command which consolidates symtypes into a single file.
fn do_consolidate<I: IntoIterator<Item = String>>(do_timing: bool, args: I) -> Result<(), ()> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut num_workers = 1;
    let mut output = "-".to_string();
    let mut past_dash_dash = false;
    let mut maybe_path = None;

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_jobs_option(&arg, &mut args)? {
                num_workers = value;
                continue;
            }
            if let Some(value) = handle_value_option(&arg, &mut args, "-o", "--output")? {
                output = value;
                continue;
            }
            if arg == "-h" || arg == "--help" {
                print_consolidate_usage();
                return Ok(());
            }
            if arg == "--" {
                past_dash_dash = true;
                continue;
            }
            if arg.starts_with('-') || arg.starts_with("--") {
                eprintln!("Unrecognized consolidate option '{}'", arg);
                return Err(());
            }
        }

        if maybe_path.is_none() {
            maybe_path = Some(arg);
            continue;
        }
        eprintln!("Excess consolidate argument '{}' specified", arg);
        return Err(());
    }

    let path = maybe_path.ok_or_else(|| {
        eprintln!("The consolidate source is missing");
    })?;

    // Do the consolidation.
    let mut syms = SymCorpus::new();

    {
        let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path));

        if let Err(err) = syms.load(&path, num_workers) {
            eprintln!("Failed to read symtypes from '{}': {}", path, err);
            return Err(());
        }
    }

    {
        let _timing = Timing::new(
            do_timing,
            &format!("Writing consolidated symtypes to '{}'", output),
        );

        if let Err(err) = syms.write_consolidated(&output) {
            eprintln!(
                "Failed to write consolidated symtypes to '{}': {}",
                output, err
            );
            return Err(());
        }
    }

    Ok(())
}

/// Handles the `compare` command which shows differences between two symtypes corpuses.
fn do_compare<I: IntoIterator<Item = String>>(do_timing: bool, args: I) -> Result<(), ()> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut num_workers = 1;
    let mut past_dash_dash = false;
    let mut maybe_path = None;
    let mut maybe_path2 = None;

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_jobs_option(&arg, &mut args)? {
                num_workers = value;
                continue;
            }
            if arg == "-h" || arg == "--help" {
                print_compare_usage();
                return Ok(());
            }
            if arg == "--" {
                past_dash_dash = true;
                continue;
            }
            if arg.starts_with('-') || arg.starts_with("--") {
                eprintln!("Unrecognized compare option '{}'", arg);
                return Err(());
            }
        }

        if maybe_path.is_none() {
            maybe_path = Some(arg);
            continue;
        }
        if maybe_path2.is_none() {
            maybe_path2 = Some(arg);
            continue;
        }
        eprintln!("Excess compare argument '{}' specified", arg);
        return Err(());
    }

    let path = maybe_path.ok_or_else(|| {
        eprintln!("The first compare source is missing");
    })?;
    let path2 = maybe_path2.ok_or_else(|| {
        eprintln!("The second compare source is missing");
    })?;

    // Do the comparison.
    debug!("Compare '{}' and '{}'", path, path2);

    let syms = {
        let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path));

        let mut syms = SymCorpus::new();
        if let Err(err) = syms.load(&path, num_workers) {
            eprintln!("Failed to read symtypes from '{}': {}", path, err);
            return Err(());
        }
        syms
    };

    let syms2 = {
        let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path2));

        let mut syms2 = SymCorpus::new();
        if let Err(err) = syms2.load(&path2, num_workers) {
            eprintln!("Failed to read symtypes from '{}': {}", path2, err);
            return Err(());
        }
        syms2
    };

    {
        let _timing = Timing::new(do_timing, "Comparison");

        if let Err(err) = syms.compare_with(&syms2, io::stdout(), num_workers) {
            eprintln!(
                "Failed to compare symtypes from '{}' and '{}': {}",
                path, path2, err
            );
            return Err(());
        }
    }

    Ok(())
}

fn main() {
    let mut args = env::args();

    // Skip over the program name.
    match args.next() {
        Some(_) => {}
        None => {
            eprintln!("Unknown program name");
            process::exit(1);
        }
    };

    // Handle global options and stop at the command.
    let mut maybe_command = None;
    let mut do_timing = false;
    let mut debug_level = 0;
    for arg in args.by_ref() {
        if arg == "-d" || arg == "--debug" {
            debug_level += 1;
            continue;
        }
        if arg == "--timing" {
            do_timing = true;
            continue;
        }

        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        }
        if arg == "--version" {
            print_version();
            process::exit(0);
        }
        if arg.starts_with('-') || arg.starts_with("--") {
            eprintln!("Unrecognized global option '{}'", arg);
            process::exit(1);
        }
        maybe_command = Some(arg);
        break;
    }

    init_debug_level(debug_level);

    let command = match maybe_command {
        Some(command) => command,
        None => {
            eprintln!("No command specified");
            process::exit(1);
        }
    };

    // Process the specified command.
    let result = match command.as_str() {
        "consolidate" => do_consolidate(do_timing, args),
        "compare" => do_compare(do_timing, args),
        _ => {
            eprintln!("Unrecognized command '{}'", command);
            Err(())
        }
    };

    process::exit(if result.is_ok() { 0 } else { 1 });
}

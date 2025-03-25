// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::{env, io, process};
use suse_kabi_tools::cli::{handle_value_option, process_global_args};
use suse_kabi_tools::sym::SymCorpus;
use suse_kabi_tools::{debug, Filter, Timing};

/// Prints the global usage message on the standard output.

const USAGE_MSG: &str = concat!(
    "Usage: ksymtypes [OPTION...] COMMAND\n",
    "\n",
    "Options:\n",
    "  -d, --debug                   enable debug output\n",
    "  -h, --help                    display this help and exit\n",
    "  --version                     output version information and exit\n",
    "\n",
    "Commands:\n",
    "  consolidate                   consolidate symtypes into a single file\n",
    "  compare                       show differences between two symtypes corpuses\n"
);

const CONSOLIDATE_USAGE_MSG: &str = concat!(
    "Usage: ksymtypes consolidate [OPTION...] PATH\n",
    "Consolidate symtypes into a single file.\n",
    "\n",
    "Options:\n",
    "  -h, --help                    display this help and exit\n",
    "  -j NUM, --jobs=NUM            use NUM workers to perform the operation\n",
    "  -o FILE, --output=FILE        write the result in FILE, instead of stdout\n",
);

const COMPARE_USAGE_MSG: &str = concat!(
    "Usage: ksymtypes compare [OPTION...] PATH PATH2\n",
    "Show differences between two symtypes corpuses.\n",
    "\n",
    "Options:\n",
    "  -h, --help                    display this help and exit\n",
    "  -j NUM, --jobs=NUM            use NUM workers to perform the operation\n",
    "  -f FILE, --filter=FILE        consider only symbols matching patterns in FILE\n",
);

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
                print!("{}", CONSOLIDATE_USAGE_MSG);
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
    let mut maybe_filter_path = None;
    let mut past_dash_dash = false;
    let mut maybe_path = None;
    let mut maybe_path2 = None;

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_jobs_option(&arg, &mut args)? {
                num_workers = value;
                continue;
            }
            if let Some(value) = handle_value_option(&arg, &mut args, "-f", "--filter")? {
                maybe_filter_path = Some(value);
                continue;
            }
            if arg == "-h" || arg == "--help" {
                print!("{}", COMPARE_USAGE_MSG);
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

    let maybe_filter = match maybe_filter_path {
        Some(filter_path) => {
            let _timing = Timing::new(
                do_timing,
                &format!("Reading filters from '{}'", filter_path),
            );

            let mut filter = Filter::new();
            if let Err(err) = filter.load(&filter_path) {
                eprintln!("Failed to read filter from '{}': {}", filter_path, err);
                return Err(());
            }
            Some(filter)
        }
        None => None,
    };

    {
        let _timing = Timing::new(do_timing, "Comparison");

        if let Err(err) =
            syms.compare_with(&syms2, maybe_filter.as_ref(), io::stdout(), num_workers)
        {
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
    // Process global arguments.
    let mut args = env::args();
    let mut do_timing = false;

    let command = process_global_args(
        &mut args,
        USAGE_MSG,
        &format!("ksymtypes {}\n", env!("CARGO_PKG_VERSION")),
        &mut do_timing,
    );

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

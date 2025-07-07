// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::process::ExitCode;
use std::{env, io};
use suse_kabi_tools::cli::{handle_value_option, process_global_args};
use suse_kabi_tools::symtypes::SymtypesCorpus;
use suse_kabi_tools::text::Filter;
use suse_kabi_tools::{Error, Timing, debug};

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
    "  --filter-symbol-list=FILE     consider only symbols matching patterns in FILE\n",
);

/// Handles the `-j`/`--jobs` option which specifies the number of workers to perform a given
/// operation simultaneously.
fn handle_jobs_option<I: Iterator<Item = String>>(
    arg: &str,
    args: &mut I,
) -> Result<Option<i32>, Error> {
    if let Some(value) = handle_value_option(arg, args, "-j", "--jobs")? {
        match value.parse::<i32>() {
            Ok(jobs) => {
                if jobs < 1 {
                    return Err(Error::new_cli(format!(
                        "Invalid value for '{}': must be positive",
                        arg
                    )));
                }
                return Ok(Some(jobs));
            }
            Err(err) => {
                return Err(Error::new_cli(format!(
                    "Invalid value for '{}': {}",
                    arg, err
                )));
            }
        };
    }

    Ok(None)
}

/// Handles the `consolidate` command which consolidates symtypes into a single file.
fn do_consolidate<I: IntoIterator<Item = String>>(do_timing: bool, args: I) -> Result<(), Error> {
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
                return Err(Error::new_cli(format!(
                    "Unrecognized consolidate option '{}'",
                    arg
                )));
            }
        }

        if maybe_path.is_none() {
            maybe_path = Some(arg);
            continue;
        }
        return Err(Error::new_cli(format!(
            "Excess consolidate argument '{}' specified",
            arg
        )));
    }

    let path = maybe_path.ok_or_else(|| Error::new_cli("The consolidate source is missing"))?;

    // Do the consolidation.
    let symtypes = {
        let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path));

        let mut symtypes = SymtypesCorpus::new();
        symtypes
            .load(&path, io::stderr(), num_workers)
            .map_err(|err| {
                Error::new_context(format!("Failed to read symtypes from '{}'", path), err)
            })?;
        symtypes
    };

    {
        let _timing = Timing::new(
            do_timing,
            &format!("Writing consolidated symtypes to '{}'", output),
        );

        symtypes
            .write_consolidated(&output, num_workers)
            .map_err(|err| {
                Error::new_context(
                    format!("Failed to write consolidated symtypes to '{}'", output),
                    err,
                )
            })?;
    }

    Ok(())
}

/// Handles the `compare` command which shows differences between two symtypes corpuses.
fn do_compare<I: IntoIterator<Item = String>>(do_timing: bool, args: I) -> Result<(), Error> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut num_workers = 1;
    let mut maybe_symbol_filter_path = None;
    let mut past_dash_dash = false;
    let mut maybe_path = None;
    let mut maybe_path2 = None;

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_jobs_option(&arg, &mut args)? {
                num_workers = value;
                continue;
            }
            if let Some(value) = handle_value_option(&arg, &mut args, None, "--filter-symbol-list")?
            {
                maybe_symbol_filter_path = Some(value);
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
                return Err(Error::new_cli(format!(
                    "Unrecognized compare option '{}'",
                    arg
                )));
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
        return Err(Error::new_cli(format!(
            "Excess compare argument '{}' specified",
            arg
        )));
    }

    let path = maybe_path.ok_or_else(|| Error::new_cli("The first compare source is missing"))?;
    let path2 =
        maybe_path2.ok_or_else(|| Error::new_cli("The second compare source is missing"))?;

    // Do the comparison.
    debug!("Compare '{}' and '{}'", path, path2);

    let symtypes = {
        let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path));

        let mut symtypes = SymtypesCorpus::new();
        symtypes
            .load(&path, io::stderr(), num_workers)
            .map_err(|err| {
                Error::new_context(format!("Failed to read symtypes from '{}'", path), err)
            })?;
        symtypes
    };

    let symtypes2 = {
        let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path2));

        let mut symtypes2 = SymtypesCorpus::new();
        symtypes2
            .load(&path2, io::stderr(), num_workers)
            .map_err(|err| {
                Error::new_context(format!("Failed to read symtypes from '{}'", path2), err)
            })?;
        symtypes2
    };

    let maybe_symbol_filter = match maybe_symbol_filter_path {
        Some(symbol_filter_path) => {
            let _timing = Timing::new(
                do_timing,
                &format!("Reading symbol filters from '{}'", symbol_filter_path),
            );

            let mut symbol_filter = Filter::new();
            symbol_filter.load(&symbol_filter_path).map_err(|err| {
                Error::new_context(
                    format!(
                        "Failed to read symbol filters from '{}'",
                        symbol_filter_path
                    ),
                    err,
                )
            })?;
            Some(symbol_filter)
        }
        None => None,
    };

    {
        let _timing = Timing::new(do_timing, "Comparison");

        symtypes
            .compare_with(
                &symtypes2,
                maybe_symbol_filter.as_ref(),
                io::stdout(),
                num_workers,
            )
            .map_err(|err| {
                Error::new_context(
                    format!("Failed to compare symtypes from '{}' and '{}'", path, path2),
                    err,
                )
            })?;
    }

    Ok(())
}

fn main() -> ExitCode {
    // Process global arguments.
    let mut args = env::args();
    let mut do_timing = false;

    let result = process_global_args(
        &mut args,
        USAGE_MSG,
        &format!("ksymtypes {}\n", env!("CARGO_PKG_VERSION")),
        &mut do_timing,
    );
    let command = match result {
        Ok(Some(command)) => command,
        Ok(None) => return ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}", err);
            return ExitCode::FAILURE;
        }
    };

    // Process the specified command.
    let result = match command.as_str() {
        "consolidate" => do_consolidate(do_timing, args),
        "compare" => do_compare(do_timing, args),
        _ => Err(Error::new_cli(format!(
            "Unrecognized command '{}'",
            command
        ))),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}", err);
            ExitCode::FAILURE
        }
    }
}

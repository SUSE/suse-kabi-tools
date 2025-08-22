// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::process::ExitCode;
use std::{env, io, thread};
use suse_kabi_tools::burst::JobControl;
use suse_kabi_tools::cli::{handle_value_option, process_global_args};
use suse_kabi_tools::symtypes::{CompareFormat, SymtypesCorpus};
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
    "  split                         split a consolidated symtypes file into\n",
    "                                individual files\n",
    "  compare                       show differences between two symtypes corpuses\n"
);

const CONSOLIDATE_USAGE_MSG: &str = concat!(
    "Usage: ksymtypes consolidate [OPTION...] PATH\n",
    "Consolidate symtypes into a single file.\n",
    "\n",
    "Options:\n",
    "  -h, --help                    display this help and exit\n",
    "  -j NUM, --jobs=NUM            use NUM workers to perform the operation\n",
    "  -o FILE, --output=FILE        write the result in FILE\n",
);

const SPLIT_USAGE_MSG: &str = concat!(
    "Usage: ksymtypes split [OPTION...] PATH\n",
    "Split a consolidated symtypes file into individual files.\n",
    "\n",
    "Options:\n",
    "  -h, --help                    display this help and exit\n",
    "  -j NUM, --jobs=NUM            use NUM workers to perform the operation\n",
    "  -o DIR, --output=DIR          write the result to DIR\n",
);

const COMPARE_USAGE_MSG: &str = concat!(
    "Usage: ksymtypes compare [OPTION...] PATH PATH2\n",
    "Show differences between two symtypes corpuses.\n",
    "\n",
    "Options:\n",
    "  -h, --help                    display this help and exit\n",
    "  -j NUM, --jobs=NUM            use NUM workers to perform the operation\n",
    "  --filter-symbol-list=FILE     consider only symbols matching patterns in FILE\n",
    "  -f TYPE[:FILE], --format=TYPE[:FILE]\n",
    "                                change the output format to TYPE, or write the\n",
    "                                TYPE-formatted output to FILE\n",
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
fn do_consolidate<I: IntoIterator<Item = String>>(
    do_timing: bool,
    args: I,
) -> Result<ExitCode, Error> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut num_workers = 1;
    let mut maybe_output = None;
    let mut past_dash_dash = false;
    let mut maybe_path = None;

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_jobs_option(&arg, &mut args)? {
                num_workers = value;
                continue;
            }
            if let Some(value) = handle_value_option(&arg, &mut args, "-o", "--output")? {
                maybe_output = Some(value);
                continue;
            }
            if arg == "-h" || arg == "--help" {
                print!("{}", CONSOLIDATE_USAGE_MSG);
                return Ok(ExitCode::from(0));
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

    let output = maybe_output.ok_or_else(|| Error::new_cli("The consolidate output is missing"))?;
    let path = maybe_path.ok_or_else(|| Error::new_cli("The consolidate source is missing"))?;

    // Do the consolidation.
    let symtypes = {
        let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path));

        let mut symtypes = SymtypesCorpus::new();
        symtypes
            .load_split(
                &path,
                io::stderr(),
                &mut JobControl::new_simple(num_workers),
            )
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

        symtypes.write_consolidated(&output).map_err(|err| {
            Error::new_context(
                format!("Failed to write consolidated symtypes to '{}'", output),
                err,
            )
        })?;
    }

    Ok(ExitCode::from(0))
}

/// Handles the `split` command which splits a consolidated symtypes file into individual files.
fn do_split<I: IntoIterator<Item = String>>(do_timing: bool, args: I) -> Result<ExitCode, Error> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut num_workers = 1;
    let mut maybe_output = None;
    let mut past_dash_dash = false;
    let mut maybe_path = None;

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_jobs_option(&arg, &mut args)? {
                num_workers = value;
                continue;
            }
            if let Some(value) = handle_value_option(&arg, &mut args, "-o", "--output")? {
                maybe_output = Some(value);
                continue;
            }
            if arg == "-h" || arg == "--help" {
                print!("{}", SPLIT_USAGE_MSG);
                return Ok(ExitCode::from(0));
            }
            if arg == "--" {
                past_dash_dash = true;
                continue;
            }
            if arg.starts_with('-') || arg.starts_with("--") {
                return Err(Error::new_cli(format!(
                    "Unrecognized split option '{}'",
                    arg
                )));
            }
        }

        if maybe_path.is_none() {
            maybe_path = Some(arg);
            continue;
        }
        return Err(Error::new_cli(format!(
            "Excess split argument '{}' specified",
            arg
        )));
    }

    let output = maybe_output.ok_or_else(|| Error::new_cli("The split output is missing"))?;
    let path = maybe_path.ok_or_else(|| Error::new_cli("The split source is missing"))?;

    // Do the split.
    let symtypes = {
        let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path));

        let mut symtypes = SymtypesCorpus::new();
        symtypes
            .load_consolidated(
                &path,
                io::stderr(),
                &mut JobControl::new_simple(num_workers),
            )
            .map_err(|err| {
                Error::new_context(format!("Failed to read symtypes from '{}'", path), err)
            })?;
        symtypes
    };

    {
        let _timing = Timing::new(
            do_timing,
            &format!("Writing split symtypes to '{}'", output),
        );

        symtypes
            .write_split(&output, &mut JobControl::new_simple(num_workers))
            .map_err(|err| {
                Error::new_context(
                    format!("Failed to write split symtypes to '{}'", output),
                    err,
                )
            })?;
    }

    Ok(ExitCode::from(0))
}

/// Handles the `compare` command which shows differences between two symtypes corpuses.
fn do_compare<I: IntoIterator<Item = String>>(do_timing: bool, args: I) -> Result<ExitCode, Error> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut num_workers = 1;
    let mut maybe_symbol_filter_path = None;
    let mut writers_conf = vec![(CompareFormat::Pretty, "-".to_string())];
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
            if let Some(value) = handle_value_option(&arg, &mut args, "-f", "--format")? {
                match value.split_once(':') {
                    Some((format, path)) => {
                        writers_conf.push((CompareFormat::try_from_str(format)?, path.to_string()))
                    }
                    None => writers_conf[0].0 = CompareFormat::try_from_str(&value)?,
                }
                continue;
            }
            if arg == "-h" || arg == "--help" {
                print!("{}", COMPARE_USAGE_MSG);
                return Ok(ExitCode::from(0));
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

    let job_control_rc = JobControl::new(num_workers);
    let job_slots = JobControl::new_slots(&job_control_rc, 1);
    let job_slots2 = JobControl::new_slots(&job_control_rc, if num_workers > 1 { 1 } else { 0 });

    let (symtypes, symtypes2) = thread::scope(|scope| {
        let read_thread = scope.spawn(|| {
            let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path));

            let mut job_slots = job_slots;
            job_slots.ensure_one_reserved();

            let mut symtypes = SymtypesCorpus::new();
            symtypes
                .load(&path, io::stderr(), &mut job_slots)
                .map_err(|err| {
                    Error::new_context(format!("Failed to read symtypes from '{}'", path), err)
                })?;
            Ok(symtypes)
        });

        let read_thread2 = scope.spawn(|| {
            let _timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path2));

            let mut job_slots2 = job_slots2;
            job_slots2.ensure_one_reserved();

            let mut symtypes2 = SymtypesCorpus::new();
            symtypes2
                .load(&path2, io::stderr(), &mut job_slots2)
                .map_err(|err| {
                    Error::new_context(format!("Failed to read symtypes from '{}'", path2), err)
                })?;
            Ok(symtypes2)
        });

        let symtypes = read_thread.join().unwrap()?;
        let symtypes2 = read_thread2.join().unwrap()?;

        Ok((symtypes, symtypes2))
    })?;

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

    let is_equal = {
        let _timing = Timing::new(do_timing, "Comparison");

        symtypes
            .compare_with(
                &symtypes2,
                maybe_symbol_filter.as_ref(),
                &writers_conf[..],
                num_workers,
            )
            .map_err(|err| {
                Error::new_context(
                    format!("Failed to compare symtypes from '{}' and '{}'", path, path2),
                    err,
                )
            })?
    };

    Ok(ExitCode::from(if is_equal { 0 } else { 1 }))
}

fn main() -> ExitCode {
    // Process global arguments.
    let mut args = env::args();
    let mut do_timing = false;

    let result = process_global_args(
        &mut args,
        USAGE_MSG,
        &format!("ksymtypes {}\n", env!("SUSE_KABI_TOOLS_VERSION")),
        &mut do_timing,
    );
    let command = match result {
        Ok(Some(command)) => command,
        Ok(None) => return ExitCode::from(0),
        Err(err) => {
            eprintln!("{}", err);
            return ExitCode::from(2);
        }
    };

    // Process the specified command.
    let result = match command.as_str() {
        "consolidate" => do_consolidate(do_timing, args),
        "split" => do_split(do_timing, args),
        "compare" => do_compare(do_timing, args),
        _ => Err(Error::new_cli(format!(
            "Unrecognized command '{}'",
            command
        ))),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{}", err);
            ExitCode::from(2)
        }
    }
}

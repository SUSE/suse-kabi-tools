// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::{init_debug_level, Error};

/// Handles a command-line option with a mandatory value.
///
/// When the `arg` matches the `short` or `long` variant, the function returns [`Ok(Some(String))`]
/// with the option value. Otherwise, [`Ok(None)`] is returned when the `arg` doesn't match, or
/// [`Err`] in case of an error.
pub fn handle_value_option<I: Iterator<Item = String>>(
    arg: &str,
    args: &mut I,
    short: &str,
    long: &str,
) -> Result<Option<String>, Error> {
    // Handle '-<short> <value>' and '--<long> <value>'.
    if arg == short || arg == long {
        match args.next() {
            Some(value) => return Ok(Some(value.to_string())),
            None => {
                return Err(Error::new_cli(format!("Missing argument for '{}'", long)));
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

/// Processes command-line options, stopping at the command name.
///
/// Returns [`Ok`] containing [`Some`] with the command name, or [`Ok`] containing [`None`] if the
/// function handles an option directly (such as `--help`), or [`Err`] on error.
pub fn process_global_args<I: Iterator<Item = String>>(
    args: &mut I,
    usage_msg: &str,
    version_msg: &str,
    do_timing: &mut bool,
) -> Result<Option<String>, Error> {
    // Skip over the program name.
    match args.next() {
        Some(_) => {}
        None => return Err(Error::new_cli("Unknown program name")),
    };

    // Handle global options and stop at the command.
    let mut maybe_command = None;
    let mut debug_level = 0;
    for arg in args.by_ref() {
        if arg == "-d" || arg == "--debug" {
            debug_level += 1;
            continue;
        }
        if arg == "--timing" {
            *do_timing = true;
            continue;
        }

        if arg == "-h" || arg == "--help" {
            print!("{}", usage_msg);
            return Ok(None);
        }
        if arg == "--version" {
            print!("{}", version_msg);
            return Ok(None);
        }
        if arg.starts_with('-') || arg.starts_with("--") {
            return Err(Error::new_cli(format!(
                "Unrecognized global option '{}'",
                arg
            )));
        }
        maybe_command = Some(arg);
        break;
    }

    init_debug_level(debug_level);

    match maybe_command {
        Some(command) => Ok(Some(command)),
        None => Err(Error::new_cli("No command specified")),
    }
}

// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::{env, io, process};
use suse_kabi_tools::cli::{handle_value_option, process_global_args};
use suse_kabi_tools::rules::Rules;
use suse_kabi_tools::symvers::Symvers;
use suse_kabi_tools::{debug, Error, Timing};

const USAGE_MSG: &str = concat!(
    "Usage: ksymvers [OPTION...] COMMAND\n",
    "\n",
    "Options:\n",
    "  -d, --debug                   enable debug output\n",
    "  -h, --help                    display this help and exit\n",
    "  --version                     output version information and exit\n",
    "\n",
    "Commands:\n",
    "  compare                       show differences between two symvers files\n",
);

const COMPARE_USAGE_MSG: &str = concat!(
    "Usage: ksymvers compare [OPTION...] PATH PATH2\n",
    "Show differences between two symvers files.\n",
    "\n",
    "Options:\n",
    "  -h, --help                    display this help and exit\n",
    "  -r FILE, --rules=FILE         load severity rules from FILE\n",
);

/// Handles the `compare` command which shows differences between two symvers files.
fn do_compare<I: IntoIterator<Item = String>>(do_timing: bool, args: I) -> Result<(), Error> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut maybe_rules_path = None;
    let mut past_dash_dash = false;
    let mut maybe_path = None;
    let mut maybe_path2 = None;

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_value_option(&arg, &mut args, "-r", "--rules")? {
                maybe_rules_path = Some(value);
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

    let syms = {
        let _timing = Timing::new(do_timing, &format!("Reading symvers from '{}'", path));

        let mut syms = Symvers::new();
        syms.load(&path).map_err(|err| {
            Error::new_context(format!("Failed to read symvers from '{}'", path), err)
        })?;
        syms
    };

    let syms2 = {
        let _timing = Timing::new(do_timing, &format!("Reading symvers from '{}'", path2));

        let mut syms2 = Symvers::new();
        syms2.load(&path2).map_err(|err| {
            Error::new_context(format!("Failed to read symvers from '{}'", path2), err)
        })?;
        syms2
    };

    let maybe_rules = match maybe_rules_path {
        Some(rules_path) => {
            let _timing = Timing::new(
                do_timing,
                &format!("Reading severity rules from '{}'", rules_path),
            );

            let mut rules = Rules::new();
            rules.load(&rules_path).map_err(|err| {
                Error::new_context(
                    format!("Failed to read severity rules from '{}'", rules_path),
                    err,
                )
            })?;
            Some(rules)
        }
        None => None,
    };

    {
        let _timing = Timing::new(do_timing, "Comparison");

        syms.compare_with(&syms2, maybe_rules.as_ref(), io::stdout())
            .map_err(|err| {
                Error::new_context(
                    format!("Failed to compare symvers from '{}' and '{}'", path, path2),
                    err,
                )
            })?;
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
        &format!("ksymvers {}\n", env!("CARGO_PKG_VERSION")),
        &mut do_timing,
    );

    // Process the specified command.
    let result = match command.as_str() {
        "compare" => do_compare(do_timing, args),
        _ => Err(Error::new_cli(format!(
            "Unrecognized command '{}'",
            command
        ))),
    };

    match result {
        Ok(()) => process::exit(0),
        Err(err) => {
            eprintln!("{}", err);
            process::exit(1);
        }
    }
}

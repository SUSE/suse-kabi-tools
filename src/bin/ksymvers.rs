// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::env;
use std::process::ExitCode;
use suse_kabi_tools::cli::{handle_value_option, process_global_args};
use suse_kabi_tools::rules::Rules;
use suse_kabi_tools::symvers::{CompareFormat, SymversCorpus};
use suse_kabi_tools::{Error, Timing, debug};

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
    "  -f TYPE[:FILE], --format=TYPE[:FILE]\n",
    "                                change the output format to TYPE, or write the\n",
    "                                TYPE-formatted output to FILE\n",
);

/// Handles the `compare` command which shows differences between two symvers files.
fn do_compare<I: IntoIterator<Item = String>>(do_timing: bool, args: I) -> Result<bool, Error> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut maybe_rules_path = None;
    let mut writers_conf = vec![(CompareFormat::Pretty, "-".to_string())];
    let mut past_dash_dash = false;
    let mut maybe_path = None;
    let mut maybe_path2 = None;

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_value_option(&arg, &mut args, "-r", "--rules")? {
                maybe_rules_path = Some(value);
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
                return Ok(true);
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

    let symvers = {
        let _timing = Timing::new(do_timing, &format!("Reading symvers from '{}'", path));

        let mut symvers = SymversCorpus::new();
        symvers.load(&path).map_err(|err| {
            Error::new_context(format!("Failed to read symvers from '{}'", path), err)
        })?;
        symvers
    };

    let symvers2 = {
        let _timing = Timing::new(do_timing, &format!("Reading symvers from '{}'", path2));

        let mut symvers2 = SymversCorpus::new();
        symvers2.load(&path2).map_err(|err| {
            Error::new_context(format!("Failed to read symvers from '{}'", path2), err)
        })?;
        symvers2
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

    let changed = {
        let _timing = Timing::new(do_timing, "Comparison");

        symvers
            .compare_with(&symvers2, maybe_rules.as_ref(), &writers_conf[..])
            .map_err(|err| {
                Error::new_context(
                    format!("Failed to compare symvers from '{}' and '{}'", path, path2),
                    err,
                )
            })?
    };

    Ok(changed)
}

fn main() -> ExitCode {
    // Process global arguments.
    let mut args = env::args();
    let mut do_timing = false;

    let result = process_global_args(
        &mut args,
        USAGE_MSG,
        &format!("ksymvers {}\n", env!("CARGO_PKG_VERSION")),
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
        "compare" => do_compare(do_timing, args).map(|is_equal| {
            if is_equal {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }),
        _ => Err(Error::new_cli(format!(
            "Unrecognized command '{}'",
            command
        ))),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{}", err);
            ExitCode::FAILURE
        }
    }
}

// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use std::process::ExitCode;
use std::{env, io};
use suse_kabi_tools::cli::{handle_value_option, process_global_args};
use suse_kabi_tools::rules::{Rules, UsedRules};
use suse_kabi_tools::symvers::{CompareFormat, SymversCorpus};
use suse_kabi_tools::text::Filter;
use suse_kabi_tools::{Error, Timing};

const USAGE_MSG: &str = concat!(
    "Usage: ksymvers [OPTION]... COMMAND ...\n",
    "\n",
    "Options:\n",
    "  -d, --debug                   enable debug output\n",
    "  -h, --help                    display this help and exit\n",
    "  --version                     output version information and exit\n",
    "\n",
    "Commands:\n",
    "  compare                       show differences between two symvers files\n",
    "  unused-rules                  detect unused severity rules\n",
    "\n",
    "See 'ksymvers COMMAND --help' for more information on a specific command.\n",
);

const COMPARE_USAGE_MSG: &str = concat!(
    "Usage: ksymvers compare [OPTION]... FILE FILE2\n",
    "\n",
    "Show differences between two symvers files.\n",
    "\n",
    "Options:\n",
    "  -h, --help                    display this help and exit\n",
    "  --filter-symbol-list=FILE     consider only symbols matching patterns in FILE\n",
    "  -r FILE, --rules=FILE         load severity rules from FILE\n",
    "  -f TYPE[:FILE], --format=TYPE[:FILE]\n",
    "                                change the output format to TYPE, or write the\n",
    "                                TYPE-formatted output to FILE\n",
);

const UNUSED_RULES_USAGE_MSG: &str = concat!(
    "Usage: ksymvers unused-rules [OPTION]... FILE...\n",
    "\n",
    "Detect severity rules not matching any records in the specified symvers files.\n",
    "\n",
    "Options:\n",
    "  -h, --help                    display this help and exit\n",
    "  -r FILE, --rules=FILE         load severity rules from FILE\n",
);

/// Handles the `compare` command which shows differences between two symvers files.
fn do_compare<I: IntoIterator<Item = String>>(do_timing: bool, args: I) -> Result<ExitCode, Error> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut maybe_symbol_filter_path = None;
    let mut maybe_rules_path = None;
    let mut writers_conf = vec![(CompareFormat::Pretty, "-".to_string())];
    let mut past_dash_dash = false;
    let mut maybe_path = None;
    let mut maybe_path2 = None;

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_value_option(&arg, &mut args, None, "--filter-symbol-list")?
            {
                maybe_symbol_filter_path = Some(value);
                continue;
            }
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
                return Ok(ExitCode::from(0));
            }
            if arg == "--" {
                past_dash_dash = true;
                continue;
            }
            if arg.starts_with('-') {
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

    let is_equal = {
        let _timing = Timing::new(do_timing, "Comparison");

        symvers
            .compare_with(
                &symvers2,
                maybe_symbol_filter.as_ref(),
                maybe_rules.as_ref(),
                &writers_conf[..],
            )
            .map_err(|err| {
                Error::new_context(
                    format!("Failed to compare symvers from '{}' and '{}'", path, path2),
                    err,
                )
            })?
    };

    Ok(ExitCode::from(if is_equal { 0 } else { 1 }))
}

/// Handles the `unused-rules` command which detects unused severity rules.
fn do_unused_rules<I: IntoIterator<Item = String>>(
    do_timing: bool,
    args: I,
) -> Result<ExitCode, Error> {
    // Parse specific command options.
    let mut args = args.into_iter();
    let mut maybe_rules_path = None;
    let mut past_dash_dash = false;
    let mut paths = Vec::new();

    while let Some(arg) = args.next() {
        if !past_dash_dash {
            if let Some(value) = handle_value_option(&arg, &mut args, "-r", "--rules")? {
                maybe_rules_path = Some(value);
                continue;
            }
            if arg == "-h" || arg == "--help" {
                print!("{}", UNUSED_RULES_USAGE_MSG);
                return Ok(ExitCode::from(0));
            }
            if arg == "--" {
                past_dash_dash = true;
                continue;
            }
            if arg.starts_with('-') {
                return Err(Error::new_cli(format!(
                    "Unrecognized unused-rules option '{}'",
                    arg
                )));
            }
        }

        paths.push(arg);
    }

    let rules_path = maybe_rules_path.ok_or_else(|| Error::new_cli("The rules file is missing"))?;
    if paths.is_empty() {
        return Err(Error::new_cli("No symvers file is specified"));
    }

    let rules = {
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
        rules
    };

    let mut used_rules = UsedRules::new();
    for path in paths {
        let symvers = {
            let _timing = Timing::new(do_timing, &format!("Reading symvers from '{}'", path));

            let mut symvers = SymversCorpus::new();
            symvers.load(&path).map_err(|err| {
                Error::new_context(format!("Failed to read symvers from '{}'", path), err)
            })?;
            symvers
        };

        let _timing = Timing::new(do_timing, &format!("Matching records in '{}'", path));
        symvers.mark_used_rules(&rules, &mut used_rules);
    }

    {
        let _timing = Timing::new(do_timing, "Reporting unused rules");

        rules
            .write_unused_rules_buffer(&used_rules, io::stdout())
            .map_err(|err| {
                Error::new_context(
                    format!("Failed to report unused rules in '{}'", rules_path),
                    err,
                )
            })?;
    }

    Ok(ExitCode::from(0))
}

fn main() -> ExitCode {
    // Process global arguments.
    let mut args = env::args();
    let mut do_timing = false;

    let result = process_global_args(
        &mut args,
        USAGE_MSG,
        &format!("ksymvers {}\n", env!("SUSE_KABI_TOOLS_VERSION")),
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
        "compare" => do_compare(do_timing, args),
        "unused-rules" => do_unused_rules(do_timing, args),
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

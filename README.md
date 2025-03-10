# suse-kabi-tools

## Overview

suse-kabi-tools is a set of Application Binary Interface (ABI) tools for the Linux kernel.

The project currently contains the following tools:

* ksymtypes &ndash; a tool to work with symtypes files which are produced by [genksyms][genksyms]
  during the Linux kernel build. It allows to consolidate multiple symtypes files into a single file
  and to compare symtypes data. For details, see the manual pages [ksymtypes(1)][ksymtypes_1] and
  [ksymtypes(5)][ksymtypes_5].
* suse-kabi &ndash; a wrapper used by RPM when building SUSE kernels

## Installation

TODO Packages are available in OBS.

To build the project locally, install a Rust toolchain and run `cargo build`.

## License

This project is released under the terms of [the GPLv2 License](COPYING).

[genksyms]: https://github.com/torvalds/linux/tree/master/scripts/genksyms
[ksymtypes_1]: https://suse.github.io/suse-kabi-tools/ksymtypes.1.html
[ksymtypes_5]: https://suse.github.io/suse-kabi-tools/ksymtypes.5.html

# suse-kabi-tools

## Overview

suse-kabi-tools is a set of Application Binary Interface (ABI) tools for the Linux kernel.

The project contains the following utilities:

* ksymtypes &ndash; a tool to work with symtypes files, which are produced by
  [genksyms][genksyms] during the Linux kernel build. It allows you to consolidate multiple symtypes
  files into a single file and to compare symtypes data.
* ksymvers &ndash; a tool to work with symvers files, which are produced by [modpost][modpost]
  during the Linux kernel build. It allows you to compare symvers data, taking into account specific
  severity rules.

The tools aim to provide fast and detailed kABI comparison. The most time-consuming operations can
utilize multiple threads running in parallel.

The project is implemented in Rust. The code depends only on the standard library, which avoids
bloating the build and keeps project maintenance low.

Manual pages: [ksymtypes(1)][ksymtypes_1], [ksymvers(1)][ksymvers_1],
[suse-kabi-tools(5)][suse_kabi_tools_5].

## Installation

Ready-to-install packages for (open)SUSE distributions are available in [the Kernel:tools
project][kernel_tools] in the openSUSE Build Service.

To build the project locally, install a Rust toolchain and run `cargo build`.

## License

This project is released under the terms of [the GPLv2 license](COPYING).

[genksyms]: https://github.com/torvalds/linux/tree/master/scripts/genksyms
[modpost]: https://github.com/torvalds/linux/tree/master/scripts/mod
[ksymtypes_1]: https://suse.github.io/suse-kabi-tools/ksymtypes.1.html
[ksymvers_1]: https://suse.github.io/suse-kabi-tools/ksymvers.1.html
[suse_kabi_tools_5]: https://suse.github.io/suse-kabi-tools/suse-kabi-tools.5.html
[kernel_tools]: https://build.opensuse.org/package/show/Kernel:tools/suse-kabi-tools

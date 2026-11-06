**Not a version of ascot**

The pre-built declaration files `ascot analyze --iris <image>:<tag>[:<namespace>]` downloads into `~/.ascot` on first use, verified against the `.sha256` beside each, one per namespace of each Community release:

- IRIS Community: `%SYS`
- IRIS for Health Community: `%SYS`, `HSLIB` and `HSSYS`

The `Declarations` workflow regenerates the files and replaces them in place; the tag stays.

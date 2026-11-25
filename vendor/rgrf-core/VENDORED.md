# rgrf-core (vendored)

A copy of the `rgrf-core` crate of [RustyGRF](https://github.com/kxdrsrt/RustyGRF) (MIT OR Apache-2.0, same author),
revision `97245f754a65259d28320cd918feef50ca55a7e4`, version 0.2.3. It is here so that this project does not depend on RustyGRF: no git dependency, no
`rgrf` program, no network access to build.

- Changed: `Cargo.toml` (no workspace inheritance, lib tests off), `clientcrypt` left out (it embeds a third-party DLL).
- Not changed: everything in `src/`. Do not edit it here; fix it in RustyGRF and run `tools/vendor-rgrf-core.sh` again.
- The upstream tests (and the test data they need) stay in RustyGRF.

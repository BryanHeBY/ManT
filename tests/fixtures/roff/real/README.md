# Fixed real-man roff fixtures

These compressed files are verbatim roff inputs used to test the native
libmandoc path without consulting the host's installed manual database. They
replace the former renderer-specific HTML fixtures.

| File | Upstream package | Packaged version | SHA-256 |
| --- | --- | --- | --- |
| `ls.1.gz` | GNU coreutils | 9.11-2 | `091e614c945887862980212abe697c63b946fbb4d189c741ad47c5dd71bd4ea0` |
| `git.1.gz` | Git | 2.55.0-1 | `8b58cbf77d1eb0ca9efcea2a98790574dcf3c2f76d02ce08531af1e931a926ed` |
| `gcc.1.gz` | GCC | 16.1.1+r346+g4e03491b401d-4 | `8a0bbfaaa5b05a8fcefc6d4741530d09abcfd95b26f8947e9aecbce68cb75b23` |
| `clang.1.gz` | LLVM Clang | 22.1.8-1 | `313398b1f95b070d7a807ea8cc2d28403b0e25159960b9fa9ce90d820bff5bed` |
| `tar.1.gz` | GNU tar | 1.35-2 | `dfeee239e4bbed1d271c0902c0fce79e5844c4d4778deae3e8d9c9995341c726` |

The pages retain their upstream notices. They remain governed by their
respective upstream documentation licences and are test data, not part of
Mant's public document contract. When replacing a fixture, update its version,
hash, and the topology assertions together in one commit.

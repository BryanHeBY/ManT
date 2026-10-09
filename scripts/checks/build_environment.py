"""Small, scoped Cargo environment for one-shot verification subprocesses."""

import os


def verification_environment(base=None):
    environment = dict(os.environ if base is None else base)
    environment["CARGO_INCREMENTAL"] = "0"
    environment.setdefault("CARGO_PROFILE_DEV_DEBUG", "line-tables-only")
    environment.setdefault("CARGO_PROFILE_TEST_DEBUG", "line-tables-only")
    return environment

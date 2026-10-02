"""Authenticate finite oracle recipes before collecting or comparing output.

The compatibility recorder predates the replay collectors and explicitly
sets -Ios.  Both recipes are real evidence: never repair footer bytes or try
recipes until an output matches.  Aliases may share source bytes while their
recorded native witnesses require different invocations.
"""

from functools import lru_cache
import gzip
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
PROFILES = ("ascii", "utf8", "html", "tree", "lint")
DEFAULT = "native-default"
RECORDER = "compatibility-recorder"
CACHE_SCHEMA = "mant.pristine-recipe-cache/v2"
ESCAPE_POLICIES = ROOT / "crates/mant-codec/src/mandoc/roff_escape/fixtures/escape_policies.json"
RECOVERY_CARDS = ROOT / "tests/fixtures/roff/rule_boundaries/source_recovery_cards.json.gz"


def digest(value):
    return hashlib.sha256(value).hexdigest()


def environment():
    return {"PATH": "/usr/bin:/bin", "LC_ALL": "C.UTF-8", "TZ": "UTC"}


def arguments(profile, recipe=DEFAULT, width=78):
    if profile not in PROFILES or recipe not in (DEFAULT, RECORDER):
        raise ValueError("unknown pristine profile recipe")
    if type(width) is not int or width <= 0:
        raise ValueError("positive integer oracle width required")
    result = ["-T" + profile]
    if recipe == RECORDER or profile in ("ascii", "utf8"):
        result.append(f"-Owidth={width}")
    if recipe == RECORDER:
        result.append("-Ios=Historical Oracle Footer")
    return result


def descriptor(recipe):
    return {"name": recipe, "stdin": "exact source bytes encoded as UTF-8",
            "environment": environment(),
            "profiles": {profile: arguments(profile, recipe) for profile in PROFILES},
            "timeoutSeconds": 15 if recipe == RECORDER else 30}


def recipe_hash(recipe):
    return digest(json.dumps(descriptor(recipe), sort_keys=True, separators=(",", ":")).encode())


@lru_cache(maxsize=1)
def recorder_receipts():
    # Only existing permanent, fully bound raw receipts establish which
    # retained cards came from record_profiles().  All other retained cards
    # retain the original default collector recipe.  No product is consulted.
    policies = json.loads(ESCAPE_POLICIES.read_bytes())["policies"]
    keys = ("source_sha256", "oracle_identity", "oracle_sha256",
            "native_utf8_sha256", "native_tree_sha256")
    witnesses = {tuple(policy[key] for key in keys) for policy in policies}
    cards = json.loads(gzip.decompress(RECOVERY_CARDS.read_bytes()))
    result = {}
    for card in cards:
        if tuple(card[key] for key in keys) in witnesses:
            if card["id"] in result:
                raise ValueError("duplicate recorder receipt identity")
            result[card["id"]] = card["source_sha256"]
    return result


def for_case(case):
    if case.get("cohort") == "escape":
        # escape_rule_cases originates in the compatibility recorder, whose
        # complete argv includes -Owidth for every profile and explicit -Ios.
        return RECORDER
    receipt = recorder_receipts().get(case.get("id"))
    if receipt is not None:
        if (case.get("cohort") != "retained-review"
                or case.get("source_sha256") != receipt
                or digest(case["source"].encode()) != receipt):
            raise ValueError("recorder receipt source or cohort changed")
        return RECORDER
    return DEFAULT


def key(case):
    return case["source_sha256"], recipe_hash(for_case(case))


def catalog():
    return {recipe_hash(name): descriptor(name) for name in (DEFAULT, RECORDER)}


def receipt_binding():
    return {str(path.relative_to(ROOT)): digest(path.read_bytes())
            for path in (ESCAPE_POLICIES, RECOVERY_CARDS)}


def cache_binding():
    return {"cacheSchema": CACHE_SCHEMA, "recipeCatalog": catalog(),
            "receiptBinding": receipt_binding()}


def validate_manifest(manifest, identity, reference_sha256=None):
    if (manifest.get("identity") != identity
            or manifest.get("expectations_from_product") is not False
            or reference_sha256 is not None
            and manifest.get("reference_sha256") != reference_sha256):
        raise ValueError("oracle cache does not bind the active pristine reference")
    reference_path = manifest.get("reference_path")
    if (not isinstance(reference_path, str) or not Path(reference_path).is_absolute()
            or str(Path(reference_path)) != reference_path):
        raise ValueError("oracle cache lacks its original reference invocation path")
    if any(manifest.get(name) != value for name, value in cache_binding().items()):
        raise ValueError("oracle cache profile recipe is missing or changed; recollect in a fresh directory")


def validate_record(record, reference_path):
    validate_streams(record.get("source_sha256"), record)
    name = record.get("recipe")
    if name not in (DEFAULT, RECORDER) or record.get("recipeSha256") != recipe_hash(name):
        raise ValueError("oracle record recipe binding changed")
    for profile in PROFILES:
        raw = record.get(profile, {})
        if any(type(raw.get(flag)) is not bool for flag in ("timeout", "utf8_valid")):
            raise ValueError("missing oracle process flags: " + profile)
        argv = raw.get("argv", [])
        if (raw.get("arguments") != arguments(profile, name)
                or not argv or argv[0] != reference_path
                or argv[1:] != arguments(profile, name)
                or raw.get("environment") != environment()
                or raw.get("stdinSha256") != record.get("source_sha256")
                or raw.get("timeoutSeconds") != descriptor(name)["timeoutSeconds"]):
            raise ValueError("oracle profile invocation binding changed: " + profile)
    return record["source_sha256"], record["recipeSha256"]


def validate_streams(source_sha256, oracle):
    """Check actual transport bytes, not a cache's claimed hash strings."""
    if oracle.get("source_sha256") != source_sha256:
        raise ValueError("oracle source binding changed")
    for name in PROFILES:
        profile = oracle.get(name)
        if not isinstance(profile, dict):
            raise ValueError("missing oracle profile: " + name)
        for flag in ("timeout", "utf8_valid"):
            if flag in profile and type(profile[flag]) is not bool:
                raise ValueError("invalid oracle process flag: " + name + "." + flag)
        code = profile.get("code")
        timed_out = profile.get("timeout", False)
        if ((timed_out and code is not None)
                or (not timed_out and type(code) is not int)):
            raise ValueError("invalid oracle process status: " + name)
        utf8_valid = True
        for stream in ("stdout", "stderr"):
            text = profile.get(stream)
            if not isinstance(text, str):
                raise ValueError("missing oracle stream: " + name + "." + stream)
            try:
                raw = (bytes.fromhex(profile[stream + "_bytes_hex"])
                       if stream + "_bytes_hex" in profile else text.encode())
            except (ValueError, TypeError) as error:
                raise ValueError("invalid oracle raw bytes: " + name) from error
            if digest(raw) != profile.get(stream + "_sha256"):
                raise ValueError("oracle raw hash changed: " + name + "." + stream)
            if raw.decode("utf-8", errors="replace") != text:
                raise ValueError("oracle text and raw bytes disagree: " + name)
            try:
                raw.decode("utf-8")
            except UnicodeDecodeError:
                utf8_valid = False
        if profile.get("utf8_valid", True) != utf8_valid:
            raise ValueError("oracle UTF-8 validity flag changed: " + name)
    return oracle


def completed(profile, maximum_code=None):
    """Qualify actual execution, retaining rejected/partial runs as evidence."""
    code = profile.get("code")
    return (type(code) is int and code >= 0
            and (maximum_code is None or code <= maximum_code)
            and profile.get("timeout", False) is False
            and profile.get("utf8_valid", True) is True)

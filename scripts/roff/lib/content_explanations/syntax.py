"""Shared conservative lexical guards for source proofs."""
from __future__ import annotations
import re

_MDOC_REQUEST = re.compile(r"^[.']([A-Za-z][A-Za-z0-9]*)(?:[ \t]+(.*))?$")
_DYNAMIC_ESCAPE = re.compile(r"\\(?:\*|n|g|V|\$)")
_AUDIT_DYNAMIC_REQUESTS = {
    "als", "am", "cc", "c2", "de", "di", "ds", "ec", "el", "ie", "if", "ig",
    "mso", "nr", "rm", "rn", "so", "soquiet", "tr", "wh",
}

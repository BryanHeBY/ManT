"""Bind logical section members to their independently observed source branch."""

from collections import Counter
import json

from .manifest import jsonl, row_bytes
from .oracle import atomic_output
from .section_observations import transparent


def entry_admission(template, witness):
    """A template is not a claim that parser normalization kept its shape."""
    if not witness:
        return "uncovered"
    prior = witness["previous"]
    raw = witness["rawBodyChildren"]
    if template == "none":
        matched = prior is None
    elif template in ("empty-peer", "transparent-peer"):
        same_macro = prior is not None and prior["label"] == witness["node"]["label"]
        matched = same_macro and (
            not raw if template == "empty-peer" else bool(raw) and all(map(transparent, raw)))
    else:
        # Top-level macros see a block predecessor, subsections may see
        # the word itself. Macro operands can be inside ELEM nodes.
        admitted = [*raw, *witness.get("predecessorChildren", ())] if prior else []
        labels = {node["label"] for node in admitted}
        if prior:
            labels.add(prior["label"])
        matched = {
            "visible": "BEFOREMARK" in labels,
            "transparent": (prior is None or bool(raw) and all(map(transparent, raw))),
            "empty-word": not {"BEFOREMARK", "\\&"}.intersection(labels)
            and bool({"\\fR", "B", "No"}.intersection(labels)),
            "invisible-word": "\\&" in labels,
        }[template]
    return "observed" if matched else "normalized-or-other-branch"


def section_ledger(directory):
    """Write every logical member, including aliases and uncovered sources."""
    manifest = json.loads((directory / "manifest.json").read_bytes())
    if not manifest["cohorts"] or not all(key.startswith("SEC-") for key in manifest["cohorts"]):
        raise ValueError("section-ledger requires a --sections manifest")
    product = directory / "product" / "candidate"
    binding = json.loads((product / "binding.json").read_bytes())
    sources = {row["sourceSha256"]: row["aliases"]
               for row in jsonl(directory / "sources.jsonl.gz")}
    observed = {}
    for path in sorted(product.glob("*.jsonl.gz")):
        for row in jsonl(path):
            verdict = row["verdict"]
            if "nativeSection" in verdict:
                observed[row["sourceSha256"]] = (
                    path.name, verdict["nativeStatuses"], verdict["nativeSection"])
    counts = Counter()
    with atomic_output(directory / "section-members.jsonl.gz") as output:
        for member in jsonl(directory / "members.jsonl.gz"):
            key = member["sourceSha256"]
            checkpoint, statuses, receipt = observed.get(key, (None, {}, {}))
            admission = (entry_admission(member["coordinates"][1], receipt.get("witness"))
                         if member["cohort"] == "SEC-entry" else "control")
            counts[admission] += 1
            output.write(row_bytes(dict(
                member, aliases=sources[key], templateAdmission=admission,
                nativeStatus=statuses, checkpoint=checkpoint,
                route="roff", evidence="registered pristine native profiles",
                receipt=receipt or {"state": "uncovered"})))
    summary = dict(members=sum(counts.values()), observedSources=len(observed),
                   admission=dict(counts), binding=binding)
    (directory / "section-members.json").write_bytes(row_bytes(summary))
    return summary

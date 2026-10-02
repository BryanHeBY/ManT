"""Independent, source-bound axes for the full acceptance replay.

Policy records authorize a particular display dimension of one exact source.
They never grant a family-wide waiver or manufacture missing consumer tests.
This is a projection comparator, not a second native execution simulator.
"""

import hashlib
import json
from html.parser import HTMLParser
from pathlib import Path
import re


POLICIES = Path(__file__).with_name("acceptance") / "presentation-policies.jsonl"


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


def load_policies(path=POLICIES):
    policies = {}
    if path.exists():
        for line in path.read_text().splitlines():
            record = json.loads(line)
            if record["id"] in policies:
                raise ValueError(f"duplicate acceptance policy: {record['id']}")
            policies[record["id"]] = record
    return policies


def qualified_policy(one, oracle, binding, policies):
    policy = policies.get(one["id"])
    if policy is None:
        return {}, None
    checks = {
        "source_sha256": one["source_sha256"],
        "oracle_identity": binding.get("identity"),
        "oracle_sha256": binding.get("reference_sha256"),
        "native_utf8_sha256": digest(oracle.get("utf8", {}).get("stdout", "")),
        "native_tree_sha256": digest(oracle.get("tree", {}).get("stdout", "")),
    }
    mismatches = [key for key, value in checks.items() if policy.get(key) != value]
    if "source" in one and digest(one["source"]) != one["source_sha256"]:
        mismatches.append("complete_source")
    if mismatches:
        return {}, "policy binding changed: " + ", ".join(mismatches)
    return policy, None


def row_projection(rows, policy):
    """Normalize only independently approved generated display dimensions."""
    rows = list(rows)
    if policy.get("indent") == "responsive":
        rows = [row.lstrip(" ") for row in rows]
    else:
        margins = [len(row) - len(row.lstrip(" ")) for row in rows if row.strip(" ")]
        margin = min(margins, default=0)
        rows = [row[margin:] if row.strip(" ") else row for row in rows]
    if policy.get("tab") == "device-expansion":
        rows = [row.replace("\t", " ") for row in rows]
    if policy.get("padding") == "responsive":
        rows = [re.sub(r" +", " ", row).rstrip(" ") for row in rows]
    if policy.get("conservative_gap"):
        # Only this exact seam permits an additional ordinary separator.
        # A HEAD-internal D/AFTER boundary is never covered by this policy.
        seam = policy["conservative_gap"]
        locations = seam_locations(rows, seam["before"])
        occurrence = seam["occurrence"]
        if len(locations) == seam["occurrences"] and occurrence < len(locations):
            row, column, scalar_boundary = locations[occurrence]
            if scalar_boundary == seam["scalar_boundary"]:
                rows[row] = rows[row][:column].rstrip(" ") + rows[row][column:]
    return rows


def content_scalars(rows):
    # Ordinary generated spacing is compared separately. NBSP, fixed Unicode
    # blanks and all nonprinting scalars remain observable content here.
    return "".join(char for row in rows for char in row if char not in " \t")


def seam_locations(rows, unit):
    """Locate ordered unit occurrences by physical row and scalar boundary."""
    locations = []
    prefix = 0
    for row_index, row in enumerate(rows):
        start = 0
        while (column := row.find(unit, start)) >= 0:
            locations.append((row_index, column, prefix + len(content_scalars([row[:column]]))))
            start = column + len(unit)
        prefix += len(content_scalars([row]))
    return locations


def tab_sites(rows):
    """Literal Tab owners: physical row and ordered adjacent-scalar boundary."""
    sites = []
    scalar_boundary = 0
    for row_index, row in enumerate(rows):
        for character in row:
            if character == "\t":
                sites.append({"row": row_index, "scalar_boundary": scalar_boundary})
            elif character != " ":
                scalar_boundary += 1
    return sites


def separator_relations(rows):
    """Record ordinary word boundaries by adjacent scalar occurrence index."""
    text = "\n".join(rows)
    count = 0
    blank = False
    relations = []
    for char in text:
        if char in " \t\n":
            blank = True
        else:
            if count:
                relations.append("word-boundary" if blank else "joined")
            count += 1
            blank = False
    return relations


class Hrefs(HTMLParser):
    def __init__(self):
        super().__init__()
        self.targets = []

    def handle_starttag(self, tag, attrs):
        if tag == "a":
            href = dict(attrs).get("href", "")
            if href.startswith(("http://", "https://", "ftp://", "mailto:")):
                self.targets.append(href)


def native_external_targets(html):
    parser = Hrefs()
    parser.feed(html)
    return parser.targets


def product_external_targets(bundle):
    targets = []

    def visit(value):
        if isinstance(value, list):
            for item in value:
                visit(item)
        elif isinstance(value, dict):
            if value.get("type") == "link":
                target = value.get("target", {})
                if target.get("kind") == "external":
                    targets.append(target.get("uri", ""))
                elif target.get("kind") == "email":
                    # mdoc_html.c::mdoc_mt_pre and man_html.c::man_UR_pre
                    # encode email navigation as mailto hrefs. The typed IR
                    # keeps the address separately; compare that same href,
                    # retaining order and every authored occurrence.
                    targets.append("mailto:" + target.get("address", ""))
            # Visible children and typed destinations are independent.
            # Count every authored occurrence without traversing target data.
            for key, item in value.items():
                if key != "target":
                    visit(item)

    visit(bundle.get("document", {}).get("sections", []))
    return targets


def compare_axes(native, product, policy=None):
    policy = policy or {}
    if native.get("status") != "asserted" or product.get("status") != "asserted":
        return {"content": "uncovered", "separators": "uncovered",
                "rows": "uncovered", "indent": "uncovered"}
    expected = row_projection(native["rows"], policy)
    actual = row_projection(product["rows"], policy)
    content = content_scalars(expected) == content_scalars(actual)
    # All row edges and runs of completed blank rows are preserved. This
    # catches dropped/added leading, internal and trailing physical rows.
    rows = ([content_scalars([row]) for row in expected]
            == [content_scalars([row]) for row in actual])
    separators = separator_relations(expected) == separator_relations(actual)
    # Geometry is separately asserted only under the common-margin policy.
    # The original replay never had an independent relative-origin oracle.
    # Its old `indent_equal` was entire-row lstrip equality. Do not promote
    # that screening field into a new claim that all native device origins
    # are mandatory IR geometry. Cards must qualify this dimension first.
    indent = {"responsive": "allowed-responsive",
              "omit-common-margin": expected == actual}.get(
                  policy.get("indent"), "uncovered")
    report = {"content": content, "separators": separators, "rows": rows,
              "indent": indent}
    if "literal_tab_sites" in policy:
        report["literal-tab-owner"] = tab_sites(product["rows"]) == policy["literal_tab_sites"]
    if "conservative_gap" in policy:
        seam = policy["conservative_gap"]
        locations = seam_locations(product["rows"], seam["before"])
        occurrence = seam["occurrence"]
        report["conservative-seam-owner"] = (len(locations) == seam["occurrences"]
            and occurrence < len(locations)
            and locations[occurrence][2] == seam["scalar_boundary"])
    return report


UNQUALIFIED_ADMISSIONS = frozenset((
    "not-collected", "generator-defect", "generator-scope-gap", "generator-invalid",
))


def qualified_axes(admission, report):
    """Keep partial oracle observations separate from usable product gold."""
    return ({axis: "uncovered" for axis in report}
            if admission in UNQUALIFIED_ADMISSIONS else report)


def verdict(admission, report, execution, uncovered):
    """Diagnostics classify input; independent assertion failures decide fate."""
    if admission in UNQUALIFIED_ADMISSIONS:
        return "review", []
    failing = [axis for axis, value in report.items() if value is False]
    if execution.get("product_error"):
        failing.append("product-error")
    if execution.get("ansi_parity") is False:
        failing.append("ansi-parity")
    if execution.get("marker_leak"):
        failing.append("marker-leak")
    if failing:
        return "fail", failing
    if uncovered or any(value == "uncovered" for value in report.values()):
        return "review", []
    return "pass", []

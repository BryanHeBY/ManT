"""Reproduce exact readable-recovery cards from pristine consumed extents.

The only accepted library is the formal pristine build already hash-bound by
these cards. Compile a tiny probe in a scratch directory, never in vendor.
No candidate binary/output is accepted. Native extent and a fresh five-profile
witness render determine the readable rows under the published recovery rule.
"""

import argparse
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

from scripts.roff.fixtures import replay_rule_boundaries as replay
from scripts.roff.fixtures.acceptance_regions import select_native_region
from scripts.roff.fixtures.generate_roff_compatibility_fixtures import record_profiles
from scripts.roff.fixtures.roff_fixture_reference import verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'tests/fixtures/roff/rule_boundaries/source_recovery_cards.json.gz'
WITNESS = 'MANTFALLBACKWITNESS'
PROBE = '''#include <stdio.h>
extern int roff_escape(const char *, int, int, int *, int *, int *, int *, int *);
int main(int argc, char **argv) {
 if(argc != 3) return 2;
 int offset=0; sscanf(argv[2], "%d", &offset);
 int esc,name,arg,endarg,end;
 int kind=roff_escape(argv[1],0,offset,&esc,&name,&arg,&endarg,&end);
 printf("%d %d %d %d %d %d\\n",kind,esc,name,arg,endarg,end);
 return 0;
}
'''


def digest(data):
    return hashlib.sha256(data).hexdigest()


def derive_input(case, extent):
    """Replace only this finite corpus's consumed source with a witness.

    The incoming extent must be acquired from the pristine probe. This helper
    does not scan/guess escape completion or emulate formatter execution.
    """
    if case['family'] != 'argument-boundary':
        raise ValueError('recovery proof requires argument-boundary source')
    metadata = case['metadata']
    outer = metadata['outer']
    lines = [line for line in case['source'].splitlines() if line.startswith('.No "')]
    if len(lines) != 1 or not lines[0].endswith('"'):
        raise ValueError('recovery proof requires one quoted No carrier')
    line = lines[0]
    word = line[5:-1]
    if outer not in ('C', 'N') or not word.startswith('A\\' + outer):
        raise ValueError('recovery proof requires the fixed visible A prefix')
    if extent['escape'] != 1 or extent['name'] != 2:
        raise ValueError('native extent changed the owning escape')
    start, endarg, end = (extent[key] for key in ('argument', 'end_argument', 'end'))
    if not (3 <= start <= len(word) and 3 <= end <= len(word)
            and start <= endarg <= len(word)):
        raise ValueError('native extent outside this source word')
    if outer == 'C':
        if metadata['end'] != 'complete' or end != len(word) - 1 or endarg <= start:
            raise ValueError('descriptor recovery requires a complete nonempty name')
        fallback = "\\C'" + word[start:endarg] + "'"
    else:
        operand = word[start:endarg]
        if operand.isascii() and operand.isdigit() and 0 <= int(operand) <= 255:
            raise ValueError('supported numbered glyph cannot use source recovery')
        fallback = word[1:end]
    source = case['source'].replace(line, '.No "' + word[:1] + WITNESS + word[end:] + '"', 1)
    return fallback, source


def reconstruct_rows(profiles, source, fallback):
    region = select_native_region(profiles['utf8']['stdout'], profiles['tree']['stdout'])
    if region['status'] != 'asserted' or sum(row.count(WITNESS) for row in region['rows']) != 1:
        raise ValueError('native recovery witness region is not qualified')
    if WITNESS not in source:
        raise ValueError('missing authored witness')
    return [row.lstrip(' ').replace(WITNESS, fallback) for row in region['rows']]


def reproduce(evidence, library, destination):
    original = json.loads(gzip.decompress(FIXTURE.read_bytes()))
    selected = [card for card in original if 'pristine_extent' in card.get('proof', {})]
    # This recipe is intentionally finite; integrity/source-fragment cards use
    # their own rule and are not admitted through an escape family wildcard.
    if len(selected) != 94:
        raise ValueError('retained recovery source inventory changed')
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registration = verified_reference(ROOT, reference)
    binding = dict(identity=registration['identity'], reference_sha256=digest(reference.read_bytes()))
    generated = {case['id']: case for case in replay.assemble(['retained-review'])}
    cases = [generated[card['id']] for card in selected]
    _, _, cache = replay.validated_cache(evidence, binding, cases)
    library_hash = digest(library.read_bytes())
    if any(card['proof']['library_sha256'] != library_hash for card in selected):
        raise ValueError('library is not the bound formal pristine build')
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='mant-recovery-probe-') as scratch:
        path = Path(scratch)
        (path / 'pristine_extent.c').write_text(PROBE)
        executable = path / 'pristine_extent'
        subprocess.run(['cc', '-O2', str(path / 'pristine_extent.c'), str(library),
                        '-lm', '-o', str(executable)], check=True, capture_output=True)
        probe_hash = digest(executable.read_bytes())
        if any(card['proof']['probe_sha256'] != probe_hash for card in selected):
            raise ValueError('pristine probe build identity changed')
        for card, case in zip(selected, cases, strict=True):
            oracle = replay.transport.oracle_record(cache, case)
            replay.validate_oracle_record(case, oracle)
            expected_binding = dict(source_sha256=case['source_sha256'],
                                    oracle_identity=binding['identity'],
                                    oracle_sha256=binding['reference_sha256'],
                                    native_utf8_sha256=oracle['utf8']['stdout_sha256'],
                                    native_tree_sha256=oracle['tree']['stdout_sha256'])
            if any(card[key] != value for key, value in expected_binding.items()):
                raise ValueError('source/reference binding changed: ' + card['id'])
            word = next(line[5:-1] for line in case['source'].splitlines() if line.startswith('.No "'))
            values = list(map(int, subprocess.check_output([str(executable), word, '1'], timeout=10).split()))
            extent = dict(zip(('kind', 'escape', 'name', 'argument', 'end_argument', 'end'), values, strict=True))
            fallback, shadow = derive_input(case, extent)
            profiles = record_profiles('retained-fallback-shadow', case['id'], shadow)
            rows = reconstruct_rows(profiles, shadow, fallback)
            proof = dict(pristine_extent=extent, fallback=fallback,
                         shadow_source_sha256=digest(shadow.encode()),
                         probe_sha256=probe_hash, library_sha256=library_hash)
            if proof != card['proof'] or rows != card['expected_product_rows']:
                raise ValueError('independent recovery derivation changed: ' + card['id'])
            raw = dict(id=card['id'], binding=expected_binding, proof=proof,
                       shadow_source=shadow, shadow_profiles=profiles,
                       expected_product_rows=rows, candidate_output_used=False)
            (destination / (card['id'] + '.json')).write_text(json.dumps(raw, ensure_ascii=False, indent=2) + '\n')
    return len(selected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--oracle-evidence', required=True, type=Path)
    parser.add_argument('--pristine-library', required=True, type=Path)
    parser.add_argument('--evidence', required=True, type=Path)
    args = parser.parse_args()
    count = reproduce(args.oracle_evidence, args.pristine_library.resolve(), args.evidence)
    print(f'{count} exact recovery cards independently reproduced; fixture unchanged')


if __name__ == '__main__':
    main()

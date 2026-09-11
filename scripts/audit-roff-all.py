#!/usr/bin/env python3
"""Manifest-bound orchestration of eight existing roff audit dimensions.

No builds, network, CSV mutation, acceptance promotion, or replacement oracle.
Use trusted local sources: subprocess resource limits are not a security sandbox.
New content/source-geometry censuses remain separate, source-bound evidence.
"""
from __future__ import annotations

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import re
import shutil
import subprocess
import sys

from roff_audit_common import run_bounded_profile_batch
from roff_reference import reference_environment, run_renderer

ROOT = Path(__file__).resolve().parents[1]
PROFILES = ('structure', 'projection', 'targets', 'semantics')
EXAMPLES = dict(zip(PROFILES, ('roff_structure_profile', 'roff_projection_profile',
                             'roff_target_profile', 'roff_semantic_profile')))
DIMENSIONS = (*PROFILES, 'fidelity-mandoc', 'layout-mandoc', 'fidelity-groff', 'layout-groff')


def module(name):
    key = 'manifest_audit_' + name.replace('-', '_')
    spec = importlib.util.spec_from_file_location(key, ROOT / 'scripts' / (name + '.py'))
    loaded = importlib.util.module_from_spec(spec)
    sys.modules[key] = loaded
    spec.loader.exec_module(loaded)
    return loaded


# Reuse bounded source/manifest transport, not its content or geometry oracle.
SOURCES = module('audit-roff-rendering')
FIDELITY = module('audit-roff-fidelity')
LAYOUT = module('audit-roff-layout')
LEGACY = {name: module('audit-roff-' + name) for name in PROFILES}


def stamp():
    return datetime.now(timezone.utc).isoformat()


def positive(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError('must be finite and positive')
    return number


def arguments(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--mant', required=True, type=Path)
    parser.add_argument('--mandoc', required=True, type=Path)
    parser.add_argument('--groff', required=True, type=Path)
    parser.add_argument('--profiler-dir', required=True, type=Path)
    parser.add_argument('--workers', type=int, default=2)
    parser.add_argument('--batch-size', type=int, default=16)
    parser.add_argument('--timeout', type=positive, default=20)
    parser.add_argument('--batch-timeout', type=positive, default=120)
    parser.add_argument('--max-result-bytes', type=int, default=2 * 1024**3)
    parser.add_argument('--plan', action='store_true', help='validate and print plan; do not run audits')
    args = parser.parse_args(argv)
    if not 1 <= args.workers <= 16 or not 1 <= args.batch_size <= 64 or args.max_result_bytes < 1024:
        parser.error('workers 1..16, batch-size 1..64 and max-result-bytes >=1024 required')
    args.output = args.output.resolve()
    if not args.output.is_relative_to(ROOT / 'target') or args.output == ROOT / 'target':
        parser.error('--output must be a new descendant of the repository target directory')
    args.max_pages = None  # The shared immutable manifest reader never samples.
    return args


def hierarchy(path):
    """Exact leaf ownership; no suffix-based exclusion of explicit inputs."""
    section = FIDELITY.manual_section(path)
    if section:
        for parent in path.parents:
            if parent.name.startswith('man') and parent.name[3:] and section.startswith(parent.name[3:]):
                return parent.parent
    return path.parent


def identity(path):
    path = Path(path).absolute()
    return {'path': str(path), 'sha256': SOURCES.file_hash(path)}


def git(*args):
    result = subprocess.run(['git', *args], cwd=ROOT, capture_output=True, text=True, check=True)
    return result.stdout.strip()


def plan(args):
    inputs = [(Path(path), rows) for path, rows in SOURCES.census_inputs(args)]
    binaries = {'mant': identity(args.mant), 'mandoc': identity(args.mandoc), 'groff': identity(args.groff)}
    for name in PROFILES:
        binaries[name] = identity(args.profiler_dir / EXAMPLES[name])
    # groff launches these backends; record them independently of host man(1).
    for name in ('troff', 'preconv', 'tbl', 'eqn', 'grotty'):
        candidate = args.groff.absolute().parent / name
        resolved = candidate if candidate.is_file() else shutil.which(name)
        if not resolved:
            raise ValueError(f'missing groff backend {name}')
        binaries['groff-' + name] = identity(resolved)
    rules = [Path(__file__), ROOT / 'scripts/roff_audit_common.py', ROOT / 'scripts/roff_reference.py',
             *[ROOT / 'scripts' / ('audit-roff-' + name + '.py') for name in (*PROFILES, 'fidelity', 'layout', 'rendering')]]
    # The source helper imports these modules. Freeze them too; do not invoke
    # their oracles or imply their full-corpus coverage in this runner.
    rules += [ROOT / 'scripts' / name for name in ('roff_content_compare.py', 'roff_layout_geometry.py',
        'roff_rendering_frame.py', 'roff_content_explanations.py', 'roff_review_queue.py')]
    report = {'schema': 'mant.roff-all-audit/v1', 'status': 'planned', 'started': stamp(),
              'manifestSha256': args._manifest_sha256, 'producerCommit': git('rev-parse', 'HEAD'),
              'producerGitStatus': git('status', '--porcelain'), 'binaries': binaries,
              'producerAttribution': 'Working tree at run start; binary hashes are authoritative identities, not independent source-to-build attestations.',
              'rules': {str(p.relative_to(ROOT)): SOURCES.file_hash(p) for p in rules},
              'physicalPages': len(inputs), 'logicalPages': sum(len(rows) for _, rows in inputs),
              'dimensions': list(DIMENSIONS), 'parameters': {key: getattr(args, key) for key in
                  ('workers', 'batch_size', 'timeout', 'batch_timeout', 'max_result_bytes')},
              'commands': {'mandoc': ['mandoc', '-T', 'utf8', '-O', 'width=200'],
                           'groff': ['groff', '-Kutf8', '-Tutf8', '-t', '-e', '-mandoc', '-rLL=200n', '-rLT=200n', '-I', 'EXACT_ROOT'],
                           'profiles': ['PROFILE', '< JSONL(id,path,root)']},
              'limitations': ['Legacy candidate rules are unchanged; clean is not human acceptance.',
                  'This runner does not cover the separate new content/source-geometry dimensions.',
                  'Source hashes bind manifest leaves; include dependencies outside the manifest are not fully inventoried.',
                  'External-context sources retain explicit partial coverage even after successful rendering.',
                  'Profile modes differ by existing contract: indexed source for structure/projection, same native session for targets, production standalone for semantics.',
                  'groff macro/font data are host resources, not independently attested by executable hashes.',
                  'Local trusted-source POSIX audit only; no cross-platform or security-sandbox claim.',
                  'Finding details are bounded; truncation is recorded, never interpreted as clean.']}
    return inputs, report


def failed(execution, detail, status='uncovered'):
    return {'execution': execution, 'status': status, 'coverage': 'uncovered', 'detail': str(detail)[:4096]}


def bounded_finding(finding):
    """Bound retention without changing a candidate's original status/count."""
    truncations = []
    def compact(value, path=''):
        if isinstance(value, list):
            if len(value) > 16:
                truncations.append({'path': path, 'originalLength': len(value), 'retained': 16})
            return [compact(v, path + f'/{i}') for i, v in enumerate(value[:16])]
        if isinstance(value, dict):
            return {k: compact(v, path + '/' + k) for k, v in value.items()}
        if isinstance(value, str) and len(value) > 2048:
            truncations.append({'path': path, 'originalLength': len(value), 'retained': 2048})
            return value[:2048]
        return value
    return {'finding': compact(finding), 'retentionTruncations': truncations}


def execution_status(code, error):
    if code == 0:
        return 'success'
    if code in (124, -24, -9) or (code == 125 and any(word in error for word in ('exceeds', 'budget', 'memory'))):
        return 'budget'
    return 'error'


def render(command, args, environment, source=None):
    code, output, error = run_renderer(command, args.timeout, environment, source, binary_output=True)
    observation = {'command': command, 'exitCode': code,
                   'stdoutSha256': hashlib.sha256(output).hexdigest(),
                   'stderrTextSha256': hashlib.sha256(error.encode()).hexdigest()}
    if code:
        return None, observation, failed(execution_status(code, error), error or f'exit {code}')
    try:
        return output.decode('utf-8'), observation, None
    except UnicodeError:
        return None, observation, failed('uncovered', 'successful renderer emitted invalid UTF-8')


def render_dimensions(path, source, args, external):
    result, observations = {}, {}
    root = hierarchy(path)
    env = reference_environment()
    env['PATH'] = str(args.groff.absolute().parent) + ':' + env.get('PATH', '')
    env['MANT_MANPATH'] = env['MANPATH'] = str(root)
    # Reuse the exact legacy standalone/indexed command policy. Its source read
    # is replaced by already validated source through the optional shared API.
    command, _ = FIDELITY.mant_render_command(path, [root], args.mant, source=source)
    mine, observations['mant'], failure = render(command, args, env)
    if failure:
        return {name: failure for name in DIMENSIONS if name.startswith(('fidelity-', 'layout-'))}, observations
    for kind, binary in [('mandoc', args.mandoc), ('groff', args.groff)]:
        if kind == 'mandoc':
            try:
                command, data, reason = FIDELITY.reference_render_command(path, str(binary), kind, root, source,
                    source_reader=lambda p: SOURCES.source_bytes(p)[0])
            except Exception as error:
                category = 'budget' if 'exceeds' in str(error) else 'error'
                result['fidelity-' + kind] = result['layout-' + kind] = failed(category, error)
                continue
        else:
            command = [str(binary), '-Kutf8', '-Tutf8', '-t', '-e', '-mandoc', '-rLL=200n', '-rLT=200n', '-I', str(root)]
            data, reason = source, None
        if command is None:
            value = failed('uncovered', reason)
            result['fidelity-' + kind] = result['layout-' + kind] = value
            continue
        reference, observations[kind], failure = render(command, args, env, data)
        if failure:
            result['fidelity-' + kind] = result['layout-' + kind] = failure
            continue
        # Run the unchanged Python oracle under the same process resource
        # boundary too. A successful formatter does not bound comparison cost.
        payload = json.dumps({'label': str(path), 'source': source.decode('utf-8'),
                              'reference': reference, 'mant': mine, 'kind': kind}).encode()
        code, output, error = run_renderer([sys.executable, str(Path(__file__).resolve()), '--compare-worker'],
            args.timeout, env, payload, binary_output=True)
        if code:
            result['fidelity-' + kind] = result['layout-' + kind] = failed(execution_status(code, error), error)
            continue
        try:
            finding = json.loads(output)
            if not isinstance(finding, dict) or finding.get('status') not in ('clean', 'review', 'hard-failure') or 'layout' not in finding:
                raise ValueError('invalid comparison worker response')
        except (ValueError, UnicodeError) as error:
            result['fidelity-' + kind] = result['layout-' + kind] = failed('error', error)
            continue
        coverage = 'partial-external-context' if external else 'legacy-dimensions-covered'
        result['fidelity-' + kind] = {'execution': 'success', 'status': finding['status'],
                                     'coverage': coverage, **bounded_finding(finding)}
        layout = finding['layout']
        if not LAYOUT.valid_layout(layout):
            result['layout-' + kind] = failed('uncovered', 'legacy fidelity did not produce a valid layout observation')
        else:
            result['layout-' + kind] = {'execution': 'success',
                'status': 'review' if layout['candidates'] else 'clean', 'coverage': coverage,
                **bounded_finding(layout)}
    return result, observations


def inspect_source(item, args):
    path, identities = item
    record = {'sourcePath': str(path), 'identities': identities, 'dimensions': {}}
    try:
        source, transport = SOURCES.source_bytes(path)
        record.update(sourceSha256=hashlib.sha256(source).hexdigest(), transportSha256=transport,
                      sourceBytes=len(source), externalContext=bool(SOURCES.EXTERNAL.search(source)))
        record['historicalHashMatches'] = [r.get('historicalSha256') == record['sourceSha256'] for r in identities]
        source.decode('utf-8')
        record['dimensions'], record['renderers'] = render_dimensions(path, source, args, record['externalContext'])
    except UnicodeError:
        record['dimensions'] = {name: failed('uncovered', 'source is not valid UTF-8') for name in DIMENSIONS}
    except Exception as error:
        category = 'budget' if 'exceeds' in str(error) else 'error'
        record['dimensions'] = {name: failed(category, error) for name in DIMENSIONS}
    return record


def profile_dimension(name, records, args):
    selected = [r for r in records if name not in r['dimensions']]
    paths = [Path(r['sourcePath']) for r in selected]
    if not paths:
        return
    transport = {}
    def run(binary, requests, timeout):
        responses = run_bounded_profile_batch(binary, requests, timeout)
        for key, response in responses.items():
            transport[key] = response
        return responses
    try:
        findings = LEGACY[name].profile_findings(paths, [], profiler=args.profiler_dir / EXAMPLES[name],
            timeout=args.batch_timeout, exact_roots={p: hierarchy(p) for p in paths}, batch_runner=run)
        by_path = {}
        for finding in findings:
            if finding.path in by_path or finding.path not in {str(p) for p in paths}:
                raise ValueError('legacy interpreter returned an unknown or duplicate page')
            by_path[finding.path] = finding
        for row in selected:
            response = transport.get(hashlib.sha256(row['sourcePath'].encode()).hexdigest(), {})
            finding = by_path.get(row['sourcePath'])
            if response.get('_execution'):
                value = failed(response['_execution'], response['error'])
            elif finding is None:
                value = failed('error', 'legacy interpreter omitted the selected page')
            else:
                value = {'execution': 'error' if finding.status == 'hard-failure' else 'success',
                         'status': finding.status,
                         'coverage': 'uncovered' if finding.status == 'hard-failure' else
                             'partial-external-context' if row['externalContext'] else 'legacy-dimensions-covered',
                         **bounded_finding(asdict(finding))}
            row['dimensions'][name] = value
    except Exception as error:
        for row in selected:
            row['dimensions'][name] = failed('error', f'{name} interpretation failed: {error}')


def execute(args, inputs, report):
    args.output.mkdir(parents=True, exist_ok=False)
    result_path = args.output / 'results.jsonl'
    report['status'] = 'running'
    counts = {name: Counter() for name in DIMENSIONS}
    physical, logical, written = 0, 0, 0
    snapshots = {}
    inventory = []
    try:
        with result_path.open('w', encoding='utf-8') as output, ThreadPoolExecutor(max_workers=args.workers) as pool:
            for offset in range(0, len(inputs), args.batch_size):
                records = list(pool.map(lambda item: inspect_source(item, args), inputs[offset:offset + args.batch_size]))
                for name in PROFILES:
                    profile_dimension(name, records, args)
                for row in records:
                    path = Path(row['sourcePath'])
                    if row.get('transportSha256'):
                        snapshots[path] = row['transportSha256']
                        if SOURCES.file_hash(path) != row['transportSha256']:
                            row['dimensions'] = {name: failed('uncovered', 'source changed during auditing') for name in DIMENSIONS}
                    assert set(row['dimensions']) == set(DIMENSIONS)
                    encoded = json.dumps(row, ensure_ascii=False, separators=(',', ':')) + '\n'
                    written += len(encoded.encode())
                    if written > args.max_result_bytes:
                        raise ValueError('result retention byte budget exhausted; remaining pages uncovered')
                    output.write(encoded)
                    physical += 1; logical += len(row['identities'])
                    inventory.extend([item['id'], row.get('sourceSha256'), row.get('transportSha256')]
                                     for item in row['identities'])
                    for name, value in row['dimensions'].items():
                        for field in ('execution', 'status', 'coverage'):
                            counts[name][field + ':' + value[field]] += len(row['identities'])
                output.flush()
                if offset % (args.batch_size * 8) == 0:
                    print(f'{physical}/{len(inputs)} physical pages', flush=True)
        report['status'] = 'completed'
    except Exception as error:
        report.update(status='incomplete', error=str(error)[:4096])
    finally:
        def unchanged(path, digest):
            try:
                return SOURCES.file_hash(path) == digest
            except (OSError, ValueError):
                return False
        missing = report['logicalPages'] - logical
        for value in counts.values():
            if missing:
                value.update({'execution:uncovered': missing, 'status:uncovered': missing, 'coverage:uncovered': missing})
        inventory.sort()
        encoded_inventory = ''.join(json.dumps(row, ensure_ascii=False, separators=(',', ':')) + '\n' for row in inventory).encode()
        report.update(finished=stamp(), processedPhysicalPages=physical, processedLogicalPages=logical,
            unprocessedLogicalPages=missing,
            actualSourceInventorySha256=hashlib.sha256(encoded_inventory).hexdigest(),
            inventoryEncoding='Sorted [logical ID, decoded SHA256 or null, transport SHA256 or null] arrays, compact UTF-8 JSON plus LF per processed identity.',
            dimensionCounts={key: dict(value) for key, value in counts.items()},
            resultsSha256=SOURCES.file_hash(result_path),
            binariesUnchanged=all(unchanged(Path(value['path']), value['sha256']) for value in report['binaries'].values()),
            rulesUnchanged=all(unchanged(ROOT / path, digest) for path, digest in report['rules'].items()),
            manifestUnchanged=unchanged(args.manifest, report['manifestSha256']),
            sourcesUnchanged=all(unchanged(path, digest) for path, digest in snapshots.items()),
            sourceHashesChecked=len(snapshots), reviewPending=True)
        report['evidenceStable'] = all(report[key] for key in ('binariesUnchanged', 'rulesUnchanged', 'manifestUnchanged', 'sourcesUnchanged'))
        report['coverageComplete'] = (report['status'] == 'completed' and report['evidenceStable'] and
            all(value.get('coverage:legacy-dimensions-covered', 0) == logical for value in counts.values()))
        (args.output / 'summary.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    return int(report['status'] != 'completed' or not report['evidenceStable'] or any(
        value.get('execution:error', 0) or value.get('execution:budget', 0) or value.get('status:hard-failure', 0)
        for value in counts.values()))


def main(argv=None):
    args = arguments(argv)
    try:
        inputs, report = plan(args)
        if args.plan:
            print(json.dumps(report, ensure_ascii=False, indent=2))
            return 0
        return execute(args, inputs, report)
    except Exception as error:
        print(f'audit orchestration failed: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    if sys.argv[1:] == ['--compare-worker']:
        payload = json.load(sys.stdin)
        artifact = FIDELITY.compare_rendered(payload['label'], payload['source'].encode(),
            payload['reference'], payload['mant'], payload['kind'])
        print(json.dumps(asdict(artifact.finding), ensure_ascii=False))
    else:
        raise SystemExit(main())
